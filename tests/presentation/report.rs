use bevy_egui::egui;
use egui_kittest::{Harness, kittest::Queryable};
use uuid::Uuid;

use crate::board::enums::BoardEnumColor;
use crate::desktop::report_commands::{
    DesktopReportAccuracy, DesktopReportEnumEvaluation, DesktopReportFinding, DesktopReportView,
};
use crate::game::enums::{GameId, GameMoveId};
use crate::presentation::cursor::PresentationStudyCursor;
use crate::presentation::report::controller::{
    PresentationReportController, PresentationReportIntent,
};
use crate::presentation::report::view::PresentationReportView;
use crate::report::job::{ReportJobEnumFreshness, ReportJobEnumState, ReportJobId};
use crate::report::scoring::{
    ReportScoringEnumCoverage, ReportScoringEnumLabel, ReportScoringEnumPolicyVersion,
};

type PresentationReportTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct PresentationReportFixture;

impl PresentationReportFixture {
    fn node(value: u128) -> PresentationReportTestResult<GameMoveId> {
        Ok(Uuid::from_u128(value).to_string().parse()?)
    }

    fn view(
        state: ReportJobEnumState,
        freshness: ReportJobEnumFreshness,
        coverage: ReportScoringEnumCoverage,
    ) -> PresentationReportTestResult<DesktopReportView> {
        let finding = |index: u32,
                       uci: &str,
                       label: Option<ReportScoringEnumLabel>,
                       forced: bool,
                       evaluation: Option<DesktopReportEnumEvaluation>,
                       node: u128|
         -> PresentationReportTestResult<DesktopReportFinding> {
            Ok(DesktopReportFinding {
                index,
                played_uci: uci.to_owned(),
                label,
                forced,
                missed_mate: false,
                loss: None,
                after_evaluation: evaluation,
                before_node: Self::node(node - 1)?,
                after_node: Self::node(node)?,
            })
        };
        Ok(DesktopReportView {
            id: ReportJobId::try_from(Uuid::from_u128(9))?,
            game: Uuid::from_u128(1).to_string().parse::<GameId>()?,
            state,
            freshness,
            completed: 3,
            total: 4,
            policy: ReportScoringEnumPolicyVersion::ExpectedScoreV1,
            coverage,
            accuracy: [
                DesktopReportAccuracy {
                    color: BoardEnumColor::White,
                    display: Some("91.5".to_owned()),
                    scored_moves: 2,
                    forced_moves: 0,
                    unscored_moves: 0,
                },
                DesktopReportAccuracy {
                    color: BoardEnumColor::Black,
                    display: None,
                    scored_moves: 0,
                    forced_moves: 1,
                    unscored_moves: 0,
                },
            ],
            findings: vec![
                finding(
                    0,
                    "e2e4",
                    Some(ReportScoringEnumLabel::Acceptable),
                    false,
                    Some(DesktopReportEnumEvaluation::Centipawns(30)),
                    11,
                )?,
                finding(1, "e7e5", None, true, None, 12)?,
                finding(
                    2,
                    "d1h5",
                    Some(ReportScoringEnumLabel::Blunder),
                    false,
                    Some(DesktopReportEnumEvaluation::Centipawns(-450)),
                    13,
                )?,
            ],
        })
    }
}

struct PresentationReportMounted {
    controller: PresentationReportController,
    cursor: PresentationStudyCursor,
    intents: Vec<PresentationReportIntent>,
}

impl PresentationReportMounted {
    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if let Some(intent) = PresentationReportView::show(ui, &state.controller, &mut state.cursor)
        {
            state.intents.push(intent);
            ui.ctx().request_discard("Exercise report intents once");
        }
    }

    fn mount(
        view: DesktopReportView,
        preview: bool,
    ) -> Harness<'static, Self> {
        let mut controller = PresentationReportController::new();
        controller.observe(view);
        controller.set_replacement_preview(preview);
        Harness::builder()
            .with_size(egui::vec2(900.0, 1400.0))
            .build_ui_state(
                Self::show,
                Self {
                    controller,
                    cursor: PresentationStudyCursor::new(),
                    intents: Vec::new(),
                },
            )
    }
}

#[test]
fn findings_labels_missing_data_and_selection_continue_across_the_shared_cursor()
-> PresentationReportTestResult {
    let view = PresentationReportFixture::view(
        ReportJobEnumState::Running,
        ReportJobEnumFreshness::Current,
        ReportScoringEnumCoverage::Partial,
    )?;
    let mut harness = PresentationReportMounted::mount(view, false);
    harness.run_ok().ok_or("Report did not mount")?;
    harness.get_by_label("Report status: Analyzing");
    harness.get_by_label("Analyzed positions: 3 of 4");
    harness.get_by_label("Partial report: accuracy covers analyzed moves only");
    harness.get_by_label(
        "White accuracy: 91.5% (2 scored, 0 forced, 0 not scored)",
    );
    harness.get_by_label(
        "Black accuracy: unavailable (0 scored, 1 forced, 0 not scored)",
    );
    harness.get_by_label("Move 1: e2e4 Acceptable");
    harness.get_by_label("Move 2: e7e5 Forced");
    harness.get_by_label("Move 3: d1h5 Blunder");
    assert_eq!(harness.state().cursor.selected(), None);
    harness.get_by_label("Move 3: d1h5 Blunder").click_accesskit();
    harness.run_ok().ok_or("Selection did not settle")?;
    assert_eq!(
        harness.state().cursor.selected(),
        Some(PresentationReportFixture::node(13)?)
    );
    Ok(())
}

#[test]
fn outdated_reports_are_labeled_and_can_only_be_reanalyzed_through_a_confirmed_preview()
-> PresentationReportTestResult {
    let view = PresentationReportFixture::view(
        ReportJobEnumState::Completed,
        ReportJobEnumFreshness::Outdated,
        ReportScoringEnumCoverage::Complete,
    )?;
    let id = view.id;
    let mut harness = PresentationReportMounted::mount(view.clone(), false);
    harness.run_ok().ok_or("Report did not mount")?;
    harness.get_by_label("Outdated: the game line changed after this report");
    harness.get_by_label("Reanalyze Game").click_accesskit();
    harness.run_ok().ok_or("Reanalysis did not settle")?;
    assert_eq!(
        harness.state().intents,
        vec![PresentationReportIntent::PreviewReanalysis(id)]
    );
    let mut preview = PresentationReportMounted::mount(view, true);
    preview.run_ok().ok_or("Preview did not mount")?;
    preview.get_by_label("Replace this report?");
    assert!(preview.state().intents.is_empty());
    preview.get_by_label("Confirm Replacement").click_accesskit();
    preview.run_ok().ok_or("Confirmation did not settle")?;
    assert_eq!(
        preview.state().intents,
        vec![PresentationReportIntent::ConfirmReplacement(id)]
    );
    Ok(())
}
