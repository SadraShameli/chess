use std::collections::HashSet;

use bevy_egui::egui;
use egui_kittest::{Harness, kittest::Queryable};

use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::game_editor::DesktopGameEditorTreePage;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::preferences::DesktopPreferencesEnumTheme;
use crate::presentation::analysis::editor_intent::PresentationAnalysisEditorError;
use crate::presentation::analysis::editor_intent::tests::{
    PresentationAnalysisEditorTestFixture, PresentationAnalysisEditorTestResult,
};
use crate::presentation::analysis::variation_tree::{
    PresentationAnalysisVariationTree, PresentationAnalysisVariationTreeEnumEffect,
};
use crate::presentation::theme::tokens::PresentationThemeTokens;

struct PresentationAnalysisEditorHistoryReviewUi {
    panel: PresentationAnalysisVariationTree,
    request: DesktopRequestId,
    effects: Vec<PresentationAnalysisVariationTreeEnumEffect>,
    failures: Vec<PresentationAnalysisEditorError>,
    needs_theme: bool,
}

impl PresentationAnalysisEditorHistoryReviewUi {
    fn harness(
        panel: PresentationAnalysisVariationTree,
        request: DesktopRequestId,
    ) -> Harness<'static, Self> {
        Harness::builder()
            .with_size(egui::vec2(1280.0, 1000.0))
            .build_ui_state(
                Self::show,
                Self {
                    panel,
                    request,
                    effects: Vec::new(),
                    failures: Vec::new(),
                    needs_theme: true,
                },
            )
    }

    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if state.needs_theme {
            PresentationThemeTokens::apply(ui.ctx(), DesktopPreferencesEnumTheme::Dark);
            state.needs_theme = false;
            ui.ctx().request_repaint();
            return;
        }
        match state.panel.show(ui, state.request) {
            Ok(Some(effect)) => {
                state.effects.push(effect);
                ui.ctx()
                    .request_discard("Exercise repeated tree request passes");
            }
            Ok(None) => {}
            Err(error) => state.failures.push(error),
        }
    }

    fn settle(harness: &mut Harness<'_, Self>) -> PresentationAnalysisEditorTestResult {
        harness.run_ok().ok_or("tree review did not settle")?;
        harness.run_steps(3);
        Ok(())
    }

    fn check_duplicate(
        &mut self,
        request: DesktopRequestId,
    ) -> PresentationAnalysisEditorTestResult {
        if self.panel.request_tree(request, false)?.is_some() {
            return Err("repeated tree input emitted a second intent".into());
        }
        Ok(())
    }
}

#[derive(Debug)]
struct PresentationAnalysisEditorHistoryReviewObservation {
    completed: usize,
    stopped_at: Option<usize>,
    emitted: usize,
    failures: Vec<PresentationAnalysisEditorError>,
}

#[tokio::test]
async fn one_tree_panel_remains_usable_after_256_completed_canonical_page_requests()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let observation: PresentationAnalysisEditorTestResult<_> = async {
        let mut owner = fixture.open(4200, "1. e4 e5 2. Nf3 Nc6 *").await?;
        let expected = owner.snapshot()?.context();
        let foreign = fixture.receipt()?;
        let mut panel = PresentationAnalysisVariationTree::new();
        panel.synchronize(owner.snapshot()?, None);
        let mut harness =
            PresentationAnalysisEditorHistoryReviewUi::harness(panel, foreign.request_id());
        PresentationAnalysisEditorHistoryReviewUi::settle(&mut harness)?;
        let mut requests = HashSet::new();
        let mut previous = None;
        let mut completed = 0;
        for ordinal in 1..=257 {
            let receipt = fixture.completed_receipt()?;
            if !requests.insert(receipt.request_id()) {
                return Err("application reused a request identity".into());
            }
            harness.state_mut().request = receipt.request_id();
            harness.get_by_label("Load Moves").click();
            PresentationAnalysisEditorHistoryReviewUi::settle(&mut harness)?;
            let effects = std::mem::take(&mut harness.state_mut().effects);
            let failures = std::mem::take(&mut harness.state_mut().failures);
            if effects.len() != 1 || !failures.is_empty() {
                return Ok(PresentationAnalysisEditorHistoryReviewObservation {
                    completed,
                    stopped_at: Some(ordinal),
                    emitted: effects.len(),
                    failures,
                });
            }
            let Some(PresentationAnalysisVariationTreeEnumEffect::Command(intent)) =
                effects.into_iter().next()
            else {
                return Err("Load Moves emitted a non-command effect".into());
            };
            if intent.request_id() != receipt.request_id()
                || intent.expected() != expected
                || !matches!(
                    intent.action(),
                    DesktopAnnotationCommandsEnumAction::TreePage { cursor: None }
                )
            {
                return Err(
                    "tree input lost its admitted identity, full context, or action".into(),
                );
            }
            let state = harness.state_mut();
            state.panel.attach_receipt(&intent, receipt.clone())?;
            state.check_duplicate(receipt.request_id())?;
            if let Some(previous) = previous {
                state.check_duplicate(previous)?;
            }
            if state.panel.pending_intent().is_none_or(|pending| {
                pending.request_id() != receipt.request_id() || pending.expected() != expected
            }) {
                return Err("duplicate input replaced the pending tree request".into());
            }
            let result = fixture.execute(&mut owner, intent, &receipt).await?;
            let DesktopAnnotationCommandsEnumResult::Tree(source_page) = &result else {
                return Err("canonical owner did not return a tree page".into());
            };
            if source_page.context() != expected
                || source_page.nodes().is_empty()
                || source_page.nodes().len() > DesktopGameEditorTreePage::MAX_ROWS
                || owner.snapshot()?.context() != expected
            {
                return Err("canonical tree page lost its context or bounded rows".into());
            }
            let state = harness.state_mut();
            if !matches!(
                state.panel.observe_result(&foreign, expected, &result),
                Err(PresentationAnalysisEditorError::ForeignReceipt)
            ) || state.panel.pending_intent().is_none()
            {
                return Err("foreign receipt consumed a canonical tree result".into());
            }
            state.panel.observe_result(&receipt, expected, &result)?;
            if state.panel.tree_page() != Some(source_page)
                || state.panel.context() != Some(expected)
                || state.panel.pending_intent().is_some()
            {
                return Err("correlated tree result was not installed exactly once".into());
            }
            state.check_duplicate(receipt.request_id())?;
            PresentationAnalysisEditorHistoryReviewUi::settle(&mut harness)?;
            if !harness.state().effects.is_empty() || !harness.state().failures.is_empty() {
                return Err("completed tree input emitted again during repeated frames".into());
            }
            previous = Some(receipt.request_id());
            completed += 1;
        }
        Ok(PresentationAnalysisEditorHistoryReviewObservation {
            completed,
            stopped_at: None,
            emitted: 0,
            failures: Vec::new(),
        })
    }
    .await;
    fixture.close().await?;
    let observation = observation?;
    assert!(
        observation.completed >= 256,
        "the first 256 canonical tree requests did not complete: {observation:?}"
    );
    assert_eq!(
        observation.completed, 257,
        "a new input must remain usable after 256 completed requests: stopped at {:?}, emitted {}, failures {:?}",
        observation.stopped_at, observation.emitted, observation.failures
    );
    Ok(())
}
