use crate::board::r#move::BoardMove;
use crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumAction;
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::game_editor::DesktopGameEditorContext;
use crate::game::enums::GameMoveId;
use crate::presentation::analysis::annotations::{
    PresentationAnalysisAnnotations, PresentationAnalysisAnnotationsEnumDestination,
    PresentationAnalysisAnnotationsEnumEffect, PresentationAnalysisAnnotationsEnumGuardChoice,
};
use crate::presentation::analysis::editor_intent::PresentationAnalysisEditorError;
use crate::presentation::analysis::editor_intent::tests::{
    PresentationAnalysisEditorTestFixture, PresentationAnalysisEditorTestResult,
};
use crate::storage::document_store::{StorageDocumentError, StorageDocumentStore};

struct PresentationAnalysisAnnotationsReviewState {
    context: Option<DesktopGameEditorContext>,
    notes: Option<String>,
    has_changes: bool,
    has_guard: bool,
    is_conflicted: bool,
    has_pending_intent: bool,
}

impl PresentationAnalysisAnnotationsReviewState {
    fn capture(panel: &PresentationAnalysisAnnotations) -> Self {
        Self {
            context: panel.context(),
            notes: panel.notes().map(str::to_owned),
            has_changes: panel.has_notes_changes(),
            has_guard: panel.has_guard(),
            is_conflicted: panel.is_conflicted(),
            has_pending_intent: panel.pending_intent().is_some(),
        }
    }
}

struct PresentationAnalysisAnnotationsReviewObservation {
    expected_a: DesktopGameEditorContext,
    expected_b: DesktopGameEditorContext,
    both_selected_at_root: bool,
    same_fen: bool,
    movement_is_also_legal_in_b: bool,
    node: GameMoveId,
    movement: BoardMove,
    switch: DesktopApplicationReceipt,
    queued_without_emission: bool,
    guarded: PresentationAnalysisAnnotationsReviewState,
    before_discard: PresentationAnalysisAnnotationsReviewState,
    resolution:
        Result<Option<PresentationAnalysisAnnotationsEnumEffect>, PresentationAnalysisEditorError>,
    after_discard: PresentationAnalysisAnnotationsReviewState,
    publications_are_charged: bool,
}

#[tokio::test]
async fn discard_never_rebinds_a_guarded_board_move_to_another_game_with_the_same_fen()
-> PresentationAnalysisEditorTestResult {
    let fixture = PresentationAnalysisEditorTestFixture::new().await?;
    let observation: PresentationAnalysisEditorTestResult<_> = async {
        let owner_a = fixture.open(4100, "{game A} *").await?;
        let owner_b = fixture.open(4101, "{game B} *").await?;
        let publication_a = owner_a.snapshot()?;
        let publication_b = owner_b.snapshot()?;
        let expected_a = publication_a.context();
        let expected_b = publication_b.context();
        let movement = publication_a
            .board()
            .legal_moves()
            .first()
            .ok_or("missing authoritative root move")?
            .movement();
        let node = PresentationAnalysisEditorTestFixture::node(4102)?;
        let switch = fixture.receipt()?;
        let discard = fixture.receipt()?;
        let mut panel = PresentationAnalysisAnnotations::new();
        panel.synchronize(publication_a, None);
        panel.set_notes(expected_a, "retain game A draft".to_owned())?;
        let queued = panel.request_switch(
            switch.request_id(),
            PresentationAnalysisAnnotationsEnumDestination::AppendStudyBranch { node, movement },
        )?;
        let guarded = PresentationAnalysisAnnotationsReviewState::capture(&panel);
        panel.synchronize(publication_b, None);
        let before_discard = PresentationAnalysisAnnotationsReviewState::capture(&panel);
        let resolution = panel.resolve_guard(
            PresentationAnalysisAnnotationsEnumGuardChoice::Discard,
            discard.request_id(),
        );
        let after_discard = PresentationAnalysisAnnotationsReviewState::capture(&panel);
        let charged = fixture
            .store
            .client()
            .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET);
        let publications_are_charged = matches!(charged, Err(StorageDocumentError::Saturated));
        drop(charged);
        Ok(PresentationAnalysisAnnotationsReviewObservation {
            expected_a,
            expected_b,
            both_selected_at_root: publication_a.root() == expected_a.selected()
                && publication_b.root() == expected_b.selected(),
            same_fen: publication_a.board().fen() == publication_b.board().fen(),
            movement_is_also_legal_in_b: publication_b
                .board()
                .legal_moves()
                .iter()
                .any(|candidate| candidate.movement() == movement),
            node,
            movement,
            switch,
            queued_without_emission: queued.is_none(),
            guarded,
            before_discard,
            resolution,
            after_discard,
            publications_are_charged,
        })
    }
    .await;
    let released = fixture
        .store
        .client()
        .reserve_library_memory(StorageDocumentStore::MEMORY_BUDGET);
    let all_publication_memory_released = released.is_ok();
    drop(released);
    fixture.close().await?;
    let PresentationAnalysisAnnotationsReviewObservation {
        expected_a,
        expected_b,
        both_selected_at_root,
        same_fen,
        movement_is_also_legal_in_b,
        node,
        movement,
        switch,
        queued_without_emission,
        guarded,
        before_discard,
        resolution,
        after_discard,
        publications_are_charged,
    } = observation?;
    assert!(publications_are_charged);
    assert!(all_publication_memory_released);
    assert!(both_selected_at_root);
    assert!(same_fen);
    assert!(movement_is_also_legal_in_b);
    assert_ne!(expected_a.game(), expected_b.game());
    assert_ne!(expected_a.epoch(), expected_b.epoch());
    assert!(queued_without_emission);
    assert!(guarded.has_guard);
    assert!(guarded.has_changes);
    assert!(!guarded.is_conflicted);
    assert_eq!(before_discard.context, Some(expected_a));
    assert_eq!(before_discard.notes.as_deref(), Some("retain game A draft"));
    assert!(before_discard.has_changes);
    assert!(before_discard.is_conflicted);
    assert!(!before_discard.has_pending_intent);
    assert!(!after_discard.has_pending_intent);
    match &resolution {
        Err(error) => {
            assert!(matches!(
                error,
                PresentationAnalysisEditorError::Conflict
                    | PresentationAnalysisEditorError::UnexpectedReply
            ));
            assert_eq!(after_discard.context, Some(expected_a));
            assert_eq!(after_discard.notes, before_discard.notes);
            assert!(after_discard.has_changes);
            assert!(after_discard.is_conflicted);
        }
        Ok(None) => assert!(!after_discard.has_guard),
        Ok(Some(PresentationAnalysisAnnotationsEnumEffect::Command(_))) => {
            assert!(!after_discard.has_guard);
        }
        Ok(Some(PresentationAnalysisAnnotationsEnumEffect::Leave { .. })) => {
            return Err("board move guard emitted an unrelated leave effect".into());
        }
    }
    if after_discard.has_guard {
        assert!(before_discard.has_guard);
        assert_eq!(after_discard.context, Some(expected_a));
        assert_eq!(after_discard.notes, before_discard.notes);
    } else if after_discard.context == Some(expected_b) {
        assert_eq!(after_discard.notes.as_deref(), Some("game B"));
        assert!(!after_discard.has_changes);
        assert!(!after_discard.is_conflicted);
    } else {
        assert_eq!(after_discard.context, Some(expected_a));
    }
    if let Ok(Some(PresentationAnalysisAnnotationsEnumEffect::Command(intent))) = resolution {
        assert_eq!(intent.request_id(), switch.request_id());
        assert!(matches!(
            intent.action(),
            DesktopAnnotationCommandsEnumAction::AppendStudyBranch {
                node: retained_node,
                movement: retained_move,
            } if *retained_node == node && *retained_move == movement
        ));
        let command = intent.admit(switch.clone())?;
        assert!(command.receipt().shares_outcome(&switch));
        assert_eq!(
            command.expected(),
            expected_a,
            "Discard rebound game A's queued board move to game B despite distinct game and source epoch"
        );
    }
    Ok(())
}
