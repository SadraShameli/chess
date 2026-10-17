use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumPending,
    DesktopAnnotationCommandsEnumScope,
};
use crate::desktop::game_editor::DesktopGameEditorTreePage;
use crate::game::annotations::GameAnnotationsEnumVariationRequest;
use crate::presentation::analysis::editor_intent::PresentationAnalysisEditorError;
use crate::presentation::analysis::editor_intent::tests::{
    PresentationAnalysisEditorTestFixture, PresentationAnalysisEditorTestResult,
};
use crate::presentation::analysis::variation_tree::PresentationAnalysisVariationTree;
use bevy_egui::egui;
use egui_kittest::{Harness, kittest::Queryable};

#[tokio::test]
async fn deleted_subtree_scope_is_paged_from_the_retained_owner_proof()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let mut text = "1. e4 (".to_owned();
    for turn in 0..30 {
        text.push_str(&format!(
            "{}. Nf3 Nf6 {}. Ng1 Ng8 ",
            turn * 2 + 1,
            turn * 2 + 2
        ));
    }
    text.push_str(") *");
    let mut owner = fixture.open(100, &text).await?;
    let mut panel = PresentationAnalysisVariationTree::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = owner.snapshot()?.context();
    let receipt = fixture.receipt()?;
    let intent = panel
        .preview_variation(
            receipt.request_id(),
            GameAnnotationsEnumVariationRequest::Delete {
                root: PresentationAnalysisEditorTestFixture::node(3)?,
            },
        )?
        .ok_or("missing preview")?;
    panel.attach_receipt(&intent, receipt.clone())?;
    let result = fixture.execute(&mut owner, intent, &receipt).await?;
    panel.observe_result(&receipt, expected, &result)?;
    assert_eq!(
        panel.preview().ok_or("missing preview")?.deleted_count(),
        120
    );
    let mut nodes = std::collections::HashSet::new();
    for (next, length) in [(false, 100), (true, 20)] {
        let receipt = fixture.receipt()?;
        let intent = panel
            .request_scope(
                receipt.request_id(),
                DesktopAnnotationCommandsEnumScope::DeletedNodes,
                next,
            )?
            .ok_or("missing scope")?;
        panel.attach_receipt(&intent, receipt.clone())?;
        let result = fixture.execute(&mut owner, intent, &receipt).await?;
        panel.observe_result(&receipt, expected, &result)?;
        let page = panel.scope_page().ok_or("missing scope page")?;
        assert_eq!(page.nodes().len(), length);
        assert_eq!(page.total(), 120);
        for node in page.nodes() {
            assert!(nodes.insert(*node));
        }
    }
    assert_eq!(nodes.len(), 120);
    assert!(
        panel
            .scope_page()
            .ok_or("missing scope page")?
            .next()
            .is_none()
    );
    drop((panel, owner));
    fixture.close().await
}

#[tokio::test]
async fn native_variation_confirmation_emits_one_exact_token_across_repeated_passes()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let mut owner = fixture.open(100, "1. e4 (1. d4 d5) e5 *").await?;
    let mut panel = PresentationAnalysisVariationTree::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = owner.snapshot()?.context();
    let receipt = fixture.receipt()?;
    let intent = panel
        .preview_variation(
            receipt.request_id(),
            GameAnnotationsEnumVariationRequest::Promote {
                tip: PresentationAnalysisEditorTestFixture::node(4)?,
            },
        )?
        .ok_or("missing preview")?;
    panel.attach_receipt(&intent, receipt.clone())?;
    let result = fixture.execute(&mut owner, intent, &receipt).await?;
    panel.observe_result(&receipt, expected, &result)?;
    let token = panel.preview().ok_or("missing preview")?.token();
    let request = PresentationAnalysisEditorTestFixture::request(2001)?;
    let mut harness = Harness::builder().with_size(egui::vec2(1280.0, 1000.0)).build_ui_state(move |ui, state: &mut (PresentationAnalysisVariationTree, Vec<crate::presentation::analysis::variation_tree::PresentationAnalysisVariationTreeEnumEffect>, Vec<String>, bool)| {
        if !state.3 {
            crate::presentation::theme::tokens::PresentationThemeTokens::apply(ui.ctx(), crate::desktop::preferences::DesktopPreferencesEnumTheme::Dark);
            state.3 = true;
            ui.ctx().request_repaint();
            return;
        }
        match state.0.show(ui, request) {
            Ok(Some(effect)) => { state.1.push(effect); ui.ctx().request_discard("Exercise repeated variation passes"); }
            Ok(None) => {}
            Err(error) => state.2.push(error.to_string()),
        }
    }, (panel, Vec::new(), Vec::new(), false));
    harness.run_ok().ok_or("preview did not settle")?;
    harness.get_by_label("Confirm Variation Change").click();
    harness.run_ok().ok_or("confirmation did not settle")?;
    harness.run_steps(5);
    assert!(harness.state().2.is_empty());
    assert_eq!(harness.state().1.len(), 1);
    let crate::presentation::analysis::variation_tree::PresentationAnalysisVariationTreeEnumEffect::Command(intent) = &harness.state().1[0] else { return Err("wrong effect".into()); };
    assert_eq!(intent.expected(), expected);
    assert!(
        matches!(intent.action(), DesktopAnnotationCommandsEnumAction::ConfirmVariation { token: actual } if *actual == token)
    );
    drop((harness, owner));
    fixture.close().await
}

#[tokio::test]
async fn canonical_tree_cursor_pages_are_bounded_and_returned_in_source_order()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let mut text = String::new();
    for turn in 0..60 {
        text.push_str(&format!(
            "{}. Nf3 Nf6 {}. Ng1 Ng8 ",
            turn * 2 + 1,
            turn * 2 + 2
        ));
    }
    text.push('*');
    let mut owner = fixture.open(100, &text).await?;
    let mut panel = PresentationAnalysisVariationTree::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = owner.snapshot()?.context();
    let mut visited = std::collections::HashSet::new();
    for next in [false, true, true] {
        let receipt = fixture.receipt()?;
        let intent = panel
            .request_tree(receipt.request_id(), next)?
            .ok_or("missing page request")?;
        panel.attach_receipt(&intent, receipt.clone())?;
        let result = fixture.execute(&mut owner, intent, &receipt).await?;
        panel.observe_result(&receipt, expected, &result)?;
        let page = panel.tree_page().ok_or("missing page")?;
        assert!(page.nodes().len() <= DesktopGameEditorTreePage::MAX_ROWS);
        for node in page.nodes() {
            assert!(visited.insert(node.id()));
        }
    }
    assert_eq!(visited.len(), 241);
    assert!(
        panel
            .tree_page()
            .ok_or("missing page")?
            .next_cursor()
            .is_none()
    );
    assert!(matches!(
        panel.request_tree(PresentationAnalysisEditorTestFixture::request(2001)?, true),
        Err(PresentationAnalysisEditorError::EndOfPages)
    ));
    drop((panel, owner));
    fixture.close().await
}

#[tokio::test]
async fn stale_tree_reply_and_confirmations_are_refused_until_the_source_is_ready()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let mut owner = fixture.open(100, "1. e4 *").await?;
    let foreign = fixture.open(101, "1. d4 *").await?;
    let mut panel = PresentationAnalysisVariationTree::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = owner.snapshot()?.context();
    let receipt = fixture.receipt()?;
    let intent = panel
        .request_tree(receipt.request_id(), false)?
        .ok_or("missing request")?;
    panel.attach_receipt(&intent, receipt.clone())?;
    let result = fixture.execute(&mut owner, intent, &receipt).await?;
    panel.synchronize(foreign.snapshot()?, None);
    assert!(matches!(
        panel.observe_result(&receipt, expected, &result),
        Err(PresentationAnalysisEditorError::StalePage)
    ));
    assert!(panel.tree_page().is_none());
    drop((panel, owner, foreign));
    fixture.close().await
}

#[tokio::test]
async fn board_move_intent_preserves_the_authoritative_typed_move_and_selected_context()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let owner = fixture.open(100, "1. e4 *").await?;
    let mut panel = PresentationAnalysisVariationTree::new();
    panel.synchronize(owner.snapshot()?, None);
    let context = owner.snapshot()?.context();
    let origin = "d2".parse()?;
    let destination = "d4".parse()?;
    let movement = owner
        .snapshot()?
        .board()
        .legal_moves()
        .iter()
        .find(|candidate| {
            candidate.origin() == Some(origin) && candidate.destination() == destination
        })
        .ok_or("missing authoritative board move")?
        .movement();
    let node = PresentationAnalysisEditorTestFixture::node(1001)?;
    let request = PresentationAnalysisEditorTestFixture::request(2001)?;
    let intent = panel
        .append_study_move(request, context, node, movement)?
        .ok_or("missing branch intent")?;
    assert_eq!(intent.expected(), context);
    assert!(
        matches!(intent.action(), DesktopAnnotationCommandsEnumAction::AppendStudyBranch { node: actual, movement: actual_move } if *actual == node && *actual_move == movement)
    );
    assert!(
        panel
            .append_study_move(request, context, node, movement)?
            .is_none()
    );
    assert_eq!(owner.snapshot()?.context(), context);
    drop((panel, owner));
    fixture.close().await
}

#[tokio::test]
async fn tree_and_children_use_correlated_canonical_pages_and_clear_on_context_change()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let mut owner = fixture.open(100, "1. e4 (1. d4 d5 2. c4) e5 *").await?;
    let mut panel = PresentationAnalysisVariationTree::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = owner.snapshot()?.context();
    let receipt = fixture.receipt()?;
    let intent = panel
        .request_tree(receipt.request_id(), false)?
        .ok_or("missing tree request")?;
    assert!(matches!(
        intent.action(),
        DesktopAnnotationCommandsEnumAction::TreePage { cursor: None }
    ));
    panel.attach_receipt(&intent, receipt.clone())?;
    let result = fixture.execute(&mut owner, intent, &receipt).await?;
    let foreign = fixture.receipt()?;
    assert!(matches!(
        panel.observe_result(&foreign, expected, &result),
        Err(PresentationAnalysisEditorError::ForeignReceipt)
    ));
    panel.observe_result(&receipt, expected, &result)?;
    assert!(
        panel.tree_page().ok_or("missing tree page")?.nodes().len()
            <= DesktopGameEditorTreePage::MAX_ROWS
    );
    let receipt = fixture.receipt()?;
    let parent = owner.snapshot()?.root();
    let intent = panel
        .request_children(receipt.request_id(), parent, false)?
        .ok_or("missing children request")?;
    panel.attach_receipt(&intent, receipt.clone())?;
    let result = fixture.execute(&mut owner, intent, &receipt).await?;
    panel.observe_result(&receipt, expected, &result)?;
    let children = panel.children_page().ok_or("missing children page")?;
    assert_eq!(children.parent(), parent);
    assert_eq!(children.nodes().len(), 2);
    assert_eq!(children.nodes()[0].san(), Some("e4"));
    assert_eq!(children.nodes()[1].san(), Some("d4"));
    panel.synchronize(
        owner.snapshot()?,
        Some(DesktopAnnotationCommandsEnumPending::Unresolved),
    );
    assert!(matches!(
        panel.request_tree(PresentationAnalysisEditorTestFixture::request(2001)?, false),
        Err(PresentationAnalysisEditorError::SourcePending)
    ));
    drop((panel, owner));
    fixture.close().await
}

#[tokio::test]
async fn confirmation_keeps_actual_owner_proof_token_and_bounded_scope()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let mut owner = fixture.open(100, "1. e4 (1. d4 d5 2. c4) e5 *").await?;
    let mut panel = PresentationAnalysisVariationTree::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = owner.snapshot()?.context();
    let request = GameAnnotationsEnumVariationRequest::Delete {
        root: PresentationAnalysisEditorTestFixture::node(3)?,
    };
    let receipt = fixture.receipt()?;
    let intent = panel
        .preview_variation(receipt.request_id(), request)?
        .ok_or("missing preview request")?;
    panel.attach_receipt(&intent, receipt.clone())?;
    let result = fixture.execute(&mut owner, intent, &receipt).await?;
    panel.observe_result(&receipt, expected, &result)?;
    let preview = panel.preview().ok_or("missing preview")?;
    let token = preview.token();
    assert_eq!(preview.request(), request);
    assert_eq!(preview.deleted_count(), 3);
    let scope_receipt = fixture.receipt()?;
    let scope = panel
        .request_scope(
            scope_receipt.request_id(),
            DesktopAnnotationCommandsEnumScope::DeletedNodes,
            false,
        )?
        .ok_or("missing scope request")?;
    panel.attach_receipt(&scope, scope_receipt.clone())?;
    let result = fixture.execute(&mut owner, scope, &scope_receipt).await?;
    panel.observe_result(&scope_receipt, expected, &result)?;
    assert_eq!(panel.scope_page().ok_or("missing scope")?.nodes().len(), 3);
    let confirmation = panel
        .confirm_variation(PresentationAnalysisEditorTestFixture::request(2001)?)?
        .ok_or("missing confirmation")?;
    assert_eq!(confirmation.expected(), expected);
    assert!(
        matches!(confirmation.action(), DesktopAnnotationCommandsEnumAction::ConfirmVariation { token: retained } if *retained == token)
    );
    assert!(
        panel
            .confirm_variation(confirmation.request_id())?
            .is_none()
    );
    drop((panel, owner));
    fixture.close().await
}

#[tokio::test]
async fn independent_source_changes_invalidate_proof_and_pending_results()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let mut owner = fixture.open(100, "1. e4 (1. d4 d5) e5 *").await?;
    let mut panel = PresentationAnalysisVariationTree::new();
    panel.synchronize(owner.snapshot()?, None);
    let expected = owner.snapshot()?.context();
    let receipt = fixture.receipt()?;
    let intent = panel
        .preview_variation(
            receipt.request_id(),
            GameAnnotationsEnumVariationRequest::Promote {
                tip: PresentationAnalysisEditorTestFixture::node(4)?,
            },
        )?
        .ok_or("missing preview")?;
    panel.attach_receipt(&intent, receipt.clone())?;
    let result = fixture.execute(&mut owner, intent, &receipt).await?;
    panel.observe_result(&receipt, expected, &result)?;
    assert!(panel.preview().is_some());
    let foreign = fixture.open(101, "1. e4 *").await?;
    panel.synchronize(foreign.snapshot()?, None);
    assert!(panel.preview().is_none());
    assert!(matches!(
        panel.confirm_variation(PresentationAnalysisEditorTestFixture::request(2001)?),
        Err(PresentationAnalysisEditorError::MissingPreview)
    ));
    assert!(panel.observe_result(&receipt, expected, &result).is_err());
    drop((panel, owner, foreign));
    fixture.close().await
}

#[path = "editor_history_review.rs"]
mod editor_history_review;
