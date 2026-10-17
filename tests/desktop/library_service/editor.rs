use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumResult,
    DesktopAnnotationCommandsEnumScope,
};
use crate::desktop::game_editor::{
    DesktopGameEditor, DesktopGameEditorBinding, DesktopGameEditorEpoch,
};
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumResult,
    DesktopLibraryServiceOperation,
};
use crate::game::annotations::GameAnnotationsEnumVariationRequest;
use crate::game::enums::GameRevision;
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::library_repository::StorageLibraryRepository;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn absent_open_rejects_trash_for_both_content_kinds_before_creating_any_owner()
-> DesktopLibraryServiceTestResult {
    for notation in [true, false] {
        let mut fixture = DesktopLibraryServiceFixture::new().await?;
        let revision = fixture.seed(100, notation).await?;
        let game = DesktopLibraryServiceFixture::id(100)?;
        let repository = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor);
        let plan = repository
            .preview_destruction(
                crate::storage::trash::StorageTrashPreviewRequest::MoveToTrash(game),
            )
            .await?;
        let request = crate::storage::trash::StorageTrashConfirmedRequest::new(
            plan.approve(),
            crate::game::library_organization::GameLibraryOrganizationTimestamp::new(20)?,
            Some(crate::storage::trash::StorageTrashClosedGameEvidence::new(
                game, revision,
            )),
        )?;
        let receipt = repository.begin_destruction(request)?.wait().await?;
        assert!(matches!(receipt, crate::storage::library_repository::StorageLibraryEnumMutationReceipt::DestructionApplied(_)));
        assert!(fixture.open(100, revision).await.is_err());
        assert_eq!(fixture.factories.load(Ordering::SeqCst), 0);
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn notation_clock_annotations_and_raw_warnings_survive_open_edit_and_durable_save()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let before = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(DesktopLibraryServiceFixture::id(100)?)
        .await?
        .ok_or("missing notation")?;
    let crate::storage::library_record::StorageLibraryEnumContent::Notation(notation) =
        before.record().content()
    else {
        return Err("expected notation content".into());
    };
    let warnings = notation.warnings().to_vec();
    let clocks = notation.clocks().to_vec();
    assert!(!warnings.is_empty());
    assert!(!clocks.is_empty());
    let selected = clocks[0].node();
    drop(before);
    let opened = fixture.open(100, revision).await?;
    let DesktopLibraryServiceEnumResult::Editor(DesktopAnnotationCommandsEnumResult::Snapshot(
        navigated,
    )) = fixture
        .execute(DesktopLibraryServiceEnumAction::Editor {
            expected: opened.context(),
            action: DesktopAnnotationCommandsEnumAction::Navigate { selected },
        })
        .await?
    else {
        return Err("missing selected clock node".into());
    };
    let crate::desktop::game_editor::DesktopGameEditorSnapshotEnumState::Notation {
        time_control,
        selected_clock_annotation,
        ..
    } = navigated.state()
    else {
        return Err("missing clockless notation state".into());
    };
    assert_eq!(
        time_control.as_ref().map(|value| value.as_str()),
        Some("600+5")
    );
    assert_eq!(*selected_clock_annotation, Some(clocks[0].remaining()));
    let DesktopLibraryServiceEnumResult::Saved { snapshot, .. } = fixture
        .execute(DesktopLibraryServiceEnumAction::Editor {
            expected: navigated.context(),
            action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                notes: "annotated clock node".to_owned(),
                nags: Vec::new(),
            },
        })
        .await?
    else {
        return Err("missing durable annotation".into());
    };
    let after = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(opened.context().game())
        .await?
        .ok_or("missing saved notation")?;
    let crate::storage::library_record::StorageLibraryEnumContent::Notation(notation) =
        after.record().content()
    else {
        return Err("expected saved notation".into());
    };
    assert_eq!(notation.warnings(), warnings);
    assert_eq!(notation.clocks(), clocks);
    drop((opened, navigated, snapshot, after));
    fixture.close().await
}

#[tokio::test]
async fn every_context_binding_dimension_is_checked_before_source_access()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let opened = fixture.open(100, revision).await?;
    let loaded = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor)
        .load(opened.context().game())
        .await?
        .ok_or("missing notation source")?;
    let context = opened.context();
    for binding in [
        DesktopGameEditorBinding::new(
            GameStorageRevision::new(revision.value() + 1),
            context.epoch(),
            context.generation(),
        ),
        DesktopGameEditorBinding::new(
            revision,
            DesktopGameEditorEpoch::from_request_id(DesktopRequestId::new()),
            context.generation(),
        ),
        DesktopGameEditorBinding::new(revision, context.epoch(), context.generation().next()?),
    ] {
        let foreign = DesktopGameEditor::notation(
            loaded.record(),
            GameRevision::ZERO,
            context.selected(),
            binding,
        )?
        .context();
        assert!(
            fixture
                .execute(DesktopLibraryServiceEnumAction::Editor {
                    expected: foreign,
                    action: DesktopAnnotationCommandsEnumAction::Snapshot
                })
                .await
                .is_err()
        );
    }
    assert_eq!(fixture.wall.calls.load(Ordering::SeqCst), 0);
    drop((loaded, opened));
    fixture.close().await
}

#[tokio::test]
async fn canonical_variation_proof_scopes_and_confirmation_survive_service_forwarding()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let opened = fixture.open(100, revision).await?;
    let DesktopLibraryServiceEnumResult::Editor(DesktopAnnotationCommandsEnumResult::Tree(tree)) =
        fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: opened.context(),
                action: DesktopAnnotationCommandsEnumAction::TreePage { cursor: None },
            })
            .await?
    else {
        return Err("missing canonical tree".into());
    };
    let tip = tree
        .nodes()
        .iter()
        .find(|node| node.san() == Some("d5"))
        .ok_or("missing branch tip")?
        .id();
    let DesktopLibraryServiceEnumResult::Editor(DesktopAnnotationCommandsEnumResult::Children(
        children,
    )) = fixture
        .execute(DesktopLibraryServiceEnumAction::Editor {
            expected: opened.context(),
            action: DesktopAnnotationCommandsEnumAction::ChildrenPage {
                parent: opened.root(),
                cursor: None,
            },
        })
        .await?
    else {
        return Err("missing canonical children".into());
    };
    assert_eq!(children.nodes().len(), 2);
    let DesktopLibraryServiceEnumResult::Editor(DesktopAnnotationCommandsEnumResult::Variation(
        preview,
    )) = fixture
        .execute(DesktopLibraryServiceEnumAction::Editor {
            expected: opened.context(),
            action: DesktopAnnotationCommandsEnumAction::PreviewVariation(
                GameAnnotationsEnumVariationRequest::Promote { tip },
            ),
        })
        .await?
    else {
        return Err("missing retained proof".into());
    };
    assert!(
        fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: opened.context(),
                action: DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                    token: DesktopRequestId::new()
                }
            })
            .await
            .is_err()
    );
    let DesktopLibraryServiceEnumResult::Editor(DesktopAnnotationCommandsEnumResult::Scope(scope)) =
        fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: preview.context(),
                action: DesktopAnnotationCommandsEnumAction::VariationScope {
                    token: preview.token(),
                    scope: DesktopAnnotationCommandsEnumScope::SelectedPath,
                    offset: 0,
                },
            })
            .await?
    else {
        return Err("missing bound proof scope".into());
    };
    assert_eq!(scope.total(), preview.selected_path_count());
    let DesktopLibraryServiceEnumResult::Saved { snapshot, .. } = fixture
        .execute(DesktopLibraryServiceEnumAction::Editor {
            expected: preview.context(),
            action: DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                token: preview.token(),
            },
        })
        .await?
    else {
        return Err("missing confirmed durable tree".into());
    };
    assert_eq!(snapshot.mainline_tip(), tip);
    assert!(
        fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: snapshot.context(),
                action: DesktopAnnotationCommandsEnumAction::VariationScope {
                    token: preview.token(),
                    scope: DesktopAnnotationCommandsEnumScope::SelectedPath,
                    offset: 0
                }
            })
            .await
            .is_err()
    );
    assert_eq!(fixture.wall.calls.load(Ordering::SeqCst), 1);
    drop((opened, tree, children, preview, scope, snapshot));
    fixture.close().await
}

#[tokio::test]
async fn navigation_keeps_content_utc_and_read_admission_separate_from_game_mutation()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, true).await?;
    let opened = fixture.open(100, revision).await?;
    let DesktopLibraryServiceEnumResult::Editor(DesktopAnnotationCommandsEnumResult::Children(
        children,
    )) = fixture
        .execute(DesktopLibraryServiceEnumAction::Editor {
            expected: opened.context(),
            action: DesktopAnnotationCommandsEnumAction::ChildrenPage {
                parent: opened.root(),
                cursor: None,
            },
        })
        .await?
    else {
        return Err("missing children".into());
    };
    let selected = children.nodes().first().ok_or("missing first move")?.id();
    let read = DesktopLibraryServiceOperation::new(
        fixture.workspace,
        DesktopLibraryServiceEnumAction::Editor {
            expected: opened.context(),
            action: DesktopAnnotationCommandsEnumAction::Snapshot,
        },
    );
    let navigate = DesktopLibraryServiceOperation::new(
        fixture.workspace,
        DesktopLibraryServiceEnumAction::Editor {
            expected: opened.context(),
            action: DesktopAnnotationCommandsEnumAction::Navigate { selected },
        },
    );
    assert!(read.mutation_entity().is_none());
    assert_eq!(
        navigate.mutation_entity(),
        Some(crate::desktop::messages::DesktopMessagesEntityId::for_game(
            opened.context().game()
        ))
    );
    let DesktopLibraryServiceEnumResult::Editor(DesktopAnnotationCommandsEnumResult::Snapshot(
        snapshot,
    )) = fixture.execute(navigate.into_action()).await?
    else {
        return Err("missing navigation snapshot".into());
    };
    assert_eq!(snapshot.context().selected(), selected);
    assert_eq!(snapshot.context().source(), opened.context().source());
    assert_ne!(
        snapshot.context().generation(),
        opened.context().generation()
    );
    assert_eq!(snapshot.context().storage_revision(), revision);
    assert_eq!(fixture.wall.calls.load(Ordering::SeqCst), 0);
    drop((opened, children, snapshot));
    fixture.close().await
}
