use bevy_egui::egui;
use egui_kittest::{Harness, kittest::Queryable};

use crate::board::position::BoardPosition;
use crate::desktop::opening_commands::{
    DesktopOpeningContinuation, DesktopOpeningExplorerView, DesktopOpeningOutcomes,
    DesktopOpeningRating, DesktopOpeningRatio, DesktopOpeningSample,
};
use crate::presentation::cursor::PresentationStudyCursor;
use crate::presentation::opening::controller::{
    PresentationOpeningController, PresentationOpeningEnumOrigin, PresentationOpeningEnumTab,
    PresentationOpeningIntent,
};
use crate::presentation::opening::explorer::PresentationOpeningExplorer;

type PresentationOpeningTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PresentationOpeningMounted {
    controller: PresentationOpeningController,
    cursor: PresentationStudyCursor,
    intents: Vec<PresentationOpeningIntent>,
}

impl PresentationOpeningMounted {
    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if let Some(intent) =
            PresentationOpeningExplorer::show_tabs(ui, &mut state.controller, state.cursor)
        {
            state.intents.push(intent);
            ui.ctx().request_discard("Exercise opening intents once");
        }
    }
}

fn view() -> PresentationOpeningTestResult<DesktopOpeningExplorerView> {
    let position = BoardPosition::standard();
    let movement = position.parse_uci("e2e4")?;
    Ok(DesktopOpeningExplorerView {
        games: 10,
        next_move_games: 9,
        ending_games: 1,
        partial_date_matches: 2,
        potential_duplicate_pairs: 0,
        outcomes: DesktopOpeningOutcomes {
            white: 5,
            draws: 2,
            black: 1,
            unknown: 2,
        },
        white_rating: Some(DesktopOpeningRating {
            sum: 4_400,
            samples: 2,
        }),
        black_rating: None,
        continuations: vec![DesktopOpeningContinuation {
            movement,
            uci: "e2e4".to_owned(),
            games: 9,
            frequency: DesktopOpeningRatio {
                numerator: 9,
                denominator: 9,
            },
            outcomes: DesktopOpeningOutcomes {
                white: 5,
                draws: 2,
                black: 1,
                unknown: 1,
            },
            white_rating: None,
            black_rating: None,
        }],
        samples: vec![DesktopOpeningSample {
            game: 7,
            library: false,
            sources: 1,
            white: Some("Alice".to_owned()),
            black: None,
            white_rating: Some(2200),
            black_rating: None,
            partial_date: true,
        }],
        next_sample_offset: Some(50),
    })
}

fn mount(
    controller: PresentationOpeningController,
) -> Harness<'static, PresentationOpeningMounted> {
    Harness::builder()
        .with_size(egui::vec2(900.0, 1400.0))
        .build_ui_state(
            PresentationOpeningMounted::show,
            PresentationOpeningMounted {
                controller,
                cursor: PresentationStudyCursor::new(),
                intents: Vec::new(),
            },
        )
}

#[test]
fn explorer_shows_available_statistics_and_marks_missing_data_unavailable()
-> PresentationOpeningTestResult {
    let mut controller = PresentationOpeningController::new();
    controller.observe_explorer(view()?);
    let mut harness = mount(controller);
    harness.run_ok().ok_or("Explorer did not mount")?;
    harness.get_by_label("Games at this position: 10");
    harness.get_by_label("White wins: 5 of 8");
    harness.get_by_label("Unknown results: 2");
    harness.get_by_label("Average White rating: 2200");
    harness.get_by_label("Average Black rating: unavailable");
    harness.get_by_label("Includes 2 games with partial dates");
    harness.get_by_label("Game 7: Alice vs unknown");
    harness
        .get_by_label("e2e4: 9 games, 9 of 9 next moves")
        .click_accesskit();
    harness.run_ok().ok_or("Continuation did not settle")?;
    assert!(matches!(
        harness.state().intents.as_slice(),
        [PresentationOpeningIntent::FollowContinuation {
            origin: PresentationOpeningEnumOrigin::Explorer,
            ..
        }]
    ));
    Ok(())
}

#[test]
fn book_tab_is_study_only_and_keeps_the_shared_position_when_switching()
-> PresentationOpeningTestResult {
    let mut controller = PresentationOpeningController::new();
    controller.observe_explorer(view()?);
    let mut harness = mount(controller);
    harness.run_ok().ok_or("Explorer did not mount")?;
    harness.get_by_label("Book").click_accesskit();
    harness.run_ok().ok_or("Book tab did not settle")?;
    assert_eq!(
        harness.state().controller.tab(),
        PresentationOpeningEnumTab::Book
    );
    harness.get_by_label("Study only. Book moves are never played for an opponent");
    harness.get_by_label("No reference book is available for this position");
    harness.get_by_label("No position is selected. The starting position is shown");
    Ok(())
}
