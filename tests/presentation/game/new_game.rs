use bevy_egui::egui;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use strum::IntoEnumIterator;

use chess::board::enums::BoardEnumColor;
use chess::clock::stage::{ClockStageEnumBonus, ClockStageEnumQuota};
use chess::game::configuration::{
    GameConfiguration, GameConfigurationEnumPreset, GameConfigurationEnumStart,
};
use chess::game::enums::GameEnumSeat;
use chess::presentation::game::configuration::{
    PresentationGameConfigurationEnumBonus, PresentationGameConfigurationEnumClockFormat,
    PresentationGameConfigurationError, PresentationGameConfigurationStage,
};
use chess::presentation::game::new_game::PresentationGameNewGame;
use chess::presentation::theme::font::PresentationThemeFont;

type PresentationGameNewGameTestResult = Result<(), Box<dyn std::error::Error>>;

struct PresentationGameNewGameFixture {
    needs_fonts: bool,
    form: PresentationGameNewGame,
    submissions: Vec<GameConfiguration>,
}

impl PresentationGameNewGameFixture {
    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if state.needs_fonts {
            ui.ctx().set_fonts(PresentationThemeFont::definitions());
            ui.ctx().request_repaint();
            state.needs_fonts = false;
            return;
        }
        if let Some(configuration) = state.form.show(ui) {
            state.submissions.push(configuration);
            ui.ctx().request_discard("Verify repeated Start passes");
        }
    }

    fn harness() -> Result<Harness<'static, Self>, Box<dyn std::error::Error>> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(
                Self::show,
                Self {
                    needs_fonts: true,
                    form: PresentationGameNewGame::new(),
                    submissions: Vec::new(),
                },
            );
        harness.run_ok().ok_or("New Game did not settle")?;
        Ok(harness)
    }
}

#[test]
fn explicit_start_submits_once_and_repeated_frames_do_not_start_or_randomize_games()
-> PresentationGameNewGameTestResult {
    let mut harness = PresentationGameNewGameFixture::harness()?;
    harness.run_steps(8);
    assert!(harness.state().submissions.is_empty());
    harness.get_by_label("Chess960 / FRC").click_accesskit();
    harness.run_ok().ok_or("FRC selection did not settle")?;
    harness.run_steps(8);
    assert!(harness.state().submissions.is_empty());
    harness.get_by_label("Start Game").click_accesskit();
    harness.run_ok().ok_or("Start action did not settle")?;
    harness.run_steps(8);
    assert_eq!(harness.state().submissions.len(), 1);
    let submitted = harness
        .state()
        .submissions
        .first()
        .ok_or("missing configuration")?;
    assert!(matches!(
        submitted.start(),
        GameConfigurationEnumStart::Chess960 { .. }
    ));
    Ok(())
}

#[test]
fn accessible_opponent_and_side_controls_produce_validated_seats()
-> PresentationGameNewGameTestResult {
    let mut harness = PresentationGameNewGameFixture::harness()?;
    harness.get_by_label("Play as Black").click_accesskit();
    harness.run_ok().ok_or("Black selection did not settle")?;
    assert_eq!(
        harness
            .state()
            .form
            .configuration()?
            .seats()?
            .seat(BoardEnumColor::Black),
        GameEnumSeat::Human
    );
    harness.get_by_label("Local two-player").click_accesskit();
    harness.run_ok().ok_or("Local selection did not settle")?;
    let seats = harness.state().form.configuration()?.seats()?;
    assert_eq!(seats.seat(BoardEnumColor::White), GameEnumSeat::Human);
    assert_eq!(seats.seat(BoardEnumColor::Black), GameEnumSeat::Human);
    Ok(())
}

#[test]
fn zero_engine_budget_disables_start_without_losing_edits() -> PresentationGameNewGameTestResult {
    let mut harness = PresentationGameNewGameFixture::harness()?;
    harness
        .state_mut()
        .form
        .draft_mut()
        .set_untimed_move_millis(0);
    harness.run_ok().ok_or("Budget validation did not settle")?;
    assert!(harness.state().form.configuration().is_err());
    assert!(
        harness
            .get_by_label("Start Game")
            .accesskit_node()
            .is_disabled()
    );
    assert!(harness.state().submissions.is_empty());
    harness
        .state_mut()
        .form
        .draft_mut()
        .set_untimed_move_millis(2500);
    harness.run_ok().ok_or("Budget recovery did not settle")?;
    assert_eq!(
        harness
            .state()
            .form
            .configuration()?
            .options()
            .engine_search()
            .untimed_move_time()
            .as_millis(),
        2500
    );
    Ok(())
}

#[test]
fn custom_formats_keep_delay_increment_stage_and_cycle_semantics_distinct()
-> PresentationGameNewGameTestResult {
    for format in PresentationGameConfigurationEnumClockFormat::iter() {
        let mut form = PresentationGameNewGame::new();
        let stages = if format == PresentationGameConfigurationEnumClockFormat::Staged {
            vec![
                PresentationGameConfigurationStage::new(
                    90_000,
                    2000,
                    40,
                    PresentationGameConfigurationEnumBonus::Fischer,
                ),
                PresentationGameConfigurationStage::new(
                    30_000,
                    3000,
                    20,
                    PresentationGameConfigurationEnumBonus::Bronstein,
                ),
            ]
        } else {
            vec![PresentationGameConfigurationStage::new(
                60_000,
                2000,
                20,
                PresentationGameConfigurationEnumBonus::None,
            )]
        };
        form.draft_mut().set_custom_clock(format, stages);
        let configuration = form.configuration()?;
        let stage = configuration
            .time_control()
            .stages()
            .first()
            .ok_or("missing custom stage")?;
        match format {
            PresentationGameConfigurationEnumClockFormat::SuddenDeath => {
                assert_eq!(stage.bonus(), ClockStageEnumBonus::None)
            }
            PresentationGameConfigurationEnumClockFormat::Fischer => {
                assert_eq!(stage.bonus().increment().as_millis(), 2000)
            }
            PresentationGameConfigurationEnumClockFormat::SimpleDelay
            | PresentationGameConfigurationEnumClockFormat::Bronstein => {
                assert_eq!(stage.bonus().delay().as_millis(), 2000);
                assert_eq!(stage.bonus().increment().as_millis(), 0);
            }
            PresentationGameConfigurationEnumClockFormat::Staged => {
                assert!(matches!(stage.quota(), ClockStageEnumQuota::Moves(_)));
                assert_eq!(configuration.time_control().stages().len(), 2);
                assert!(matches!(
                    configuration
                        .time_control()
                        .stages()
                        .last()
                        .ok_or("missing final stage")?
                        .quota(),
                    ClockStageEnumQuota::Unlimited
                ));
            }
            PresentationGameConfigurationEnumClockFormat::Cyclic => {
                assert!(matches!(stage.quota(), ClockStageEnumQuota::Cyclic(_)))
            }
        }
    }
    Ok(())
}

#[test]
fn invalid_stage_quotas_and_overflow_are_typed_and_presets_recover_the_form()
-> PresentationGameNewGameTestResult {
    let mut form = PresentationGameNewGame::new();
    form.draft_mut().set_custom_clock(
        PresentationGameConfigurationEnumClockFormat::Cyclic,
        vec![PresentationGameConfigurationStage::new(
            60_000,
            0,
            0,
            PresentationGameConfigurationEnumBonus::None,
        )],
    );
    assert!(matches!(
        form.configuration(),
        Err(PresentationGameConfigurationError::EmptyQuota)
    ));
    form.draft_mut().set_custom_clock(
        PresentationGameConfigurationEnumClockFormat::Fischer,
        vec![PresentationGameConfigurationStage::new(
            u64::MAX,
            0,
            1,
            PresentationGameConfigurationEnumBonus::None,
        )],
    );
    assert!(form.configuration().is_err());
    form.draft_mut()
        .select_preset(GameConfigurationEnumPreset::TenPlusFive);
    assert_eq!(
        form.configuration()?.time_control(),
        &chess::clock::time_control::ClockTimeControl::initial_default()
    );
    Ok(())
}

#[test]
fn all_presets_are_accessible_and_custom_editor_positions_require_explicit_start()
-> PresentationGameNewGameTestResult {
    let mut harness = PresentationGameNewGameFixture::harness()?;
    for preset in GameConfigurationEnumPreset::iter() {
        harness.get_by_label(preset.label()).click_accesskit();
        harness.run_ok().ok_or("Preset selection did not settle")?;
        assert_eq!(
            harness.state().form.configuration()?.time_control(),
            &preset.time_control()?
        );
        assert!(harness.state().submissions.is_empty());
    }
    let position = chess::board::position::BoardPosition::from_fen(
        "4k3/8/8/8/8/8/P7/4K3 w - - 0 1",
        chess::board::enums::BoardEnumVariant::Standard,
    )?;
    harness.state_mut().form = PresentationGameNewGame::from_position(position.clone());
    harness.run_ok().ok_or("Custom position did not settle")?;
    assert_eq!(
        harness.state().form.configuration()?.start(),
        &GameConfigurationEnumStart::Custom(position)
    );
    assert!(harness.state().submissions.is_empty());
    harness.get_by_label("Start Game").click_accesskit();
    harness.run_ok().ok_or("Custom Start did not settle")?;
    assert_eq!(harness.state().submissions.len(), 1);
    Ok(())
}

#[test]
fn game_options_and_staged_final_cycles_remain_explicit_and_independent()
-> PresentationGameNewGameTestResult {
    let mut harness = PresentationGameNewGameFixture::harness()?;
    harness
        .get_by_label("Enable one queued premove")
        .click_accesskit();
    harness.run_ok().ok_or("Premove selection did not settle")?;
    assert!(
        harness
            .state()
            .form
            .configuration()?
            .options()
            .is_premove_enabled()
    );
    harness.get_by_label("Local two-player").click_accesskit();
    harness.run_ok().ok_or("Local selection did not settle")?;
    assert!(
        harness
            .state()
            .form
            .configuration()?
            .options()
            .requires_local_takeback_consent()
    );
    harness
        .get_by_label("Require the other player's confirmation for takebacks")
        .click_accesskit();
    harness
        .get_by_label("Auto-flip after each move")
        .click_accesskit();
    harness.run_ok().ok_or("Local options did not settle")?;
    let options = harness.state().form.configuration()?.options();
    assert!(!options.requires_local_takeback_consent());
    assert!(options.is_auto_flip_enabled());

    let draft = harness.state_mut().form.draft_mut();
    draft.set_custom_clock(
        PresentationGameConfigurationEnumClockFormat::Staged,
        vec![
            PresentationGameConfigurationStage::new(
                90_000,
                2000,
                40,
                PresentationGameConfigurationEnumBonus::Fischer,
            ),
            PresentationGameConfigurationStage::new(
                30_000,
                3000,
                20,
                PresentationGameConfigurationEnumBonus::Bronstein,
            ),
        ],
    );
    draft.set_final_quota(chess::presentation::game::configuration::PresentationGameConfigurationEnumFinalQuota::Cyclic);
    let configuration = draft.validate()?;
    let final_stage = configuration
        .time_control()
        .stages()
        .last()
        .ok_or("missing final cycle")?;
    assert!(matches!(final_stage.quota(), ClockStageEnumQuota::Cyclic(moves) if moves.get() == 20));
    assert_eq!(final_stage.bonus().increment().as_millis(), 0);
    assert_eq!(final_stage.bonus().delay().as_millis(), 3000);
    Ok(())
}
