use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumResult,
};
use crate::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationName,
    GameLibraryOrganizationRequest, GameLibraryOrganizationTimestamp,
};
use crate::storage::library_metadata::{
    StorageLibraryMetadataEnumRequest, StorageLibraryMetadataEnumResponse,
};
use crate::storage::library_query::{StorageLibraryQuery, StorageLibraryQueryOptions};
use crate::storage::library_repository::StorageLibraryRepository;

#[tokio::test]
async fn query_cursor_preserves_page_membership_and_rejects_catalog_changes()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    for id in 100..=200 {
        fixture.seed(id, true).await?;
    }
    let DesktopLibraryServiceEnumResult::Query(first) = fixture
        .execute(DesktopLibraryServiceEnumAction::Query {
            options: StorageLibraryQueryOptions::default(),
            cursor: None,
        })
        .await?
    else {
        return Err("missing first query page".into());
    };
    assert_eq!(first.entries().len(), StorageLibraryQuery::PAGE_SIZE);
    let cursor = first
        .next_cursor()
        .ok_or("missing page continuation")?
        .clone();
    let DesktopLibraryServiceEnumResult::Query(second) = fixture
        .execute(DesktopLibraryServiceEnumAction::Query {
            options: StorageLibraryQueryOptions::default(),
            cursor: Some(cursor.clone()),
        })
        .await?
    else {
        return Err("missing second query page".into());
    };
    assert_eq!(second.entries().len(), 1);
    assert!(second.next_cursor().is_none());
    assert!(
        !first
            .entries()
            .iter()
            .any(|entry| entry.id() == second.entries()[0].id())
    );
    fixture.seed(201, true).await?;
    assert!(
        fixture
            .execute(DesktopLibraryServiceEnumAction::Query {
                options: StorageLibraryQueryOptions::default(),
                cursor: Some(cursor)
            })
            .await
            .is_err()
    );
    drop((first, second));
    fixture.close().await
}

#[tokio::test]
async fn metadata_cursor_returns_real_catalog_rows_across_pages() -> DesktopLibraryServiceTestResult
{
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let repository = StorageLibraryRepository::new(fixture.store.client(), &fixture.executor);
    for value in 1..=101_u128 {
        repository
            .organize(GameLibraryOrganizationRequest::new(
                GameLibraryOrganizationTimestamp::new(10)?,
                GameLibraryOrganizationEnumMutation::CreateTag(
                    uuid::Uuid::from_u128(value).to_string().parse()?,
                    GameLibraryOrganizationName::new(format!("Tag {value}"))?,
                ),
            )?)
            .await?;
    }
    let DesktopLibraryServiceEnumResult::Metadata(StorageLibraryMetadataEnumResponse::Tags(first)) =
        fixture
            .execute(DesktopLibraryServiceEnumAction::Metadata(
                StorageLibraryMetadataEnumRequest::Tags { cursor: None },
            ))
            .await?
    else {
        return Err("missing first metadata page".into());
    };
    assert_eq!(
        first.entries().len(),
        StorageLibraryMetadataEnumRequest::PAGE_SIZE
    );
    let cursor = *first.next_cursor().ok_or("missing metadata continuation")?;
    let DesktopLibraryServiceEnumResult::Metadata(StorageLibraryMetadataEnumResponse::Tags(second)) =
        fixture
            .execute(DesktopLibraryServiceEnumAction::Metadata(
                StorageLibraryMetadataEnumRequest::Tags {
                    cursor: Some(cursor),
                },
            ))
            .await?
    else {
        return Err("missing second metadata page".into());
    };
    assert_eq!(second.entries().len(), 1);
    assert_eq!(second.catalog_revision(), first.catalog_revision());
    assert!(second.next_cursor().is_none());
    assert!(
        !first
            .entries()
            .iter()
            .any(|entry| entry.id() == second.entries()[0].id())
    );
    drop((first, second));
    fixture.close().await
}

#[tokio::test]
async fn organize_action_applies_through_the_service_and_reports_the_organization_revision()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let action = DesktopLibraryServiceEnumAction::Organize(
        GameLibraryOrganizationEnumMutation::CreateTag(
            uuid::Uuid::from_u128(7).to_string().parse()?,
            GameLibraryOrganizationName::new("Endgames".to_owned())?,
        ),
    );
    assert_eq!(action, action.clone());
    let result = fixture.execute(action).await?;
    let DesktopLibraryServiceEnumResult::Organized(receipt) = &result else {
        return Err("missing organization receipt".into());
    };
    assert!(receipt.is_changed());
    assert_eq!(
        result.revision().map(|revision| revision.value()),
        Some(receipt.revision().value())
    );
    let DesktopLibraryServiceEnumResult::Metadata(StorageLibraryMetadataEnumResponse::Tags(tags)) =
        fixture
            .execute(DesktopLibraryServiceEnumAction::Metadata(
                StorageLibraryMetadataEnumRequest::Tags { cursor: None },
            ))
            .await?
    else {
        return Err("missing tags".into());
    };
    assert_eq!(tags.entries().len(), 1);
    drop(tags);
    fixture.close().await
}

struct DesktopLibraryServiceTrashLifecycle(crate::game::storage_repository::GameStorageRevision);

impl crate::desktop::library_commands::DesktopLibraryCommandsLifecycleProvider
    for DesktopLibraryServiceTrashLifecycle
{
    type Error = std::convert::Infallible;

    fn closed_revision(
        &self,
        _game: crate::game::enums::GameId,
    ) -> Result<Option<crate::game::storage_repository::GameStorageRevision>, Self::Error> {
        Ok(Some(self.0))
    }
}

#[tokio::test]
async fn trash_actions_preview_apply_and_restore_a_closed_game_through_the_service()
-> DesktopLibraryServiceTestResult {
    use crate::desktop::library_commands::DesktopLibraryCommandsConfirmation;
    use crate::game::library_organization::GameLibraryOrganizationGameTarget;
    use crate::storage::trash::{
        StorageTrashEnumAppliedAction, StorageTrashPreviewRequest,
    };
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(800, true).await?;
    let game = DesktopLibraryServiceFixture::id(800)?;
    let DesktopLibraryServiceEnumResult::TrashPlan(plan) = fixture
        .execute(DesktopLibraryServiceEnumAction::PreviewTrash(
            StorageTrashPreviewRequest::MoveToTrash(game),
        ))
        .await?
    else {
        return Err("missing trash plan".into());
    };
    assert_eq!(plan.content_revision(), Some(revision));
    let organization = plan.organization_revision();
    let confirmed = DesktopLibraryCommandsConfirmation::confirm(plan).prepare(
        &DesktopLibraryServiceTrashLifecycle(revision),
        GameLibraryOrganizationTimestamp::new(20)?,
    )?;
    let action = DesktopLibraryServiceEnumAction::ApplyTrash(confirmed);
    assert_eq!(action, action.clone());
    let applied = fixture.execute(action).await?;
    let DesktopLibraryServiceEnumResult::Trashed(receipt) = &applied else {
        return Err("missing trash receipt".into());
    };
    assert_eq!(receipt.action(), StorageTrashEnumAppliedAction::Trashed(game));
    assert_eq!(
        applied.revision().map(|revision| revision.value()),
        Some(receipt.catalog_revision().value())
    );
    let restored = fixture
        .execute(DesktopLibraryServiceEnumAction::Restore(
            GameLibraryOrganizationGameTarget::new(game, organization.next()?),
        ))
        .await?;
    let DesktopLibraryServiceEnumResult::Restored(receipt) = restored else {
        return Err("missing restore receipt".into());
    };
    assert_eq!(receipt.action(), StorageTrashEnumAppliedAction::Restored(game));
    fixture.close().await
}

#[tokio::test]
async fn remove_folder_action_applies_a_previewed_plan_with_a_service_timestamp()
-> DesktopLibraryServiceTestResult {
    use crate::storage::trash::{StorageTrashEnumAppliedAction, StorageTrashPreviewRequest};
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let folder: crate::game::library_organization::GameLibraryOrganizationFolderId =
        uuid::Uuid::from_u128(31).to_string().parse()?;
    fixture
        .execute(DesktopLibraryServiceEnumAction::Organize(
            GameLibraryOrganizationEnumMutation::CreateFolder(
                folder,
                None,
                GameLibraryOrganizationName::new("Openings".to_owned())?,
            ),
        ))
        .await?;
    let DesktopLibraryServiceEnumResult::TrashPlan(plan) = fixture
        .execute(DesktopLibraryServiceEnumAction::PreviewTrash(
            StorageTrashPreviewRequest::RemoveFolder(folder),
        ))
        .await?
    else {
        return Err("missing folder plan".into());
    };
    assert_eq!(plan.affected_folder_count(), 1);
    let DesktopLibraryServiceEnumResult::Trashed(receipt) = fixture
        .execute(DesktopLibraryServiceEnumAction::RemoveFolder(plan))
        .await?
    else {
        return Err("missing removal receipt".into());
    };
    assert_eq!(
        receipt.action(),
        StorageTrashEnumAppliedAction::RemovedFolder(folder)
    );
    fixture.close().await
}

#[tokio::test]
async fn confirm_game_trash_uses_the_registry_close_certificate_for_move_and_delete()
-> DesktopLibraryServiceTestResult {
    use crate::storage::trash::{StorageTrashEnumAppliedAction, StorageTrashPreviewRequest};
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    fixture.seed(950, true).await?;
    fixture.seed(951, true).await?;
    let moved = DesktopLibraryServiceFixture::id(950)?;
    let deleted = DesktopLibraryServiceFixture::id(951)?;
    let DesktopLibraryServiceEnumResult::TrashPlan(plan) = fixture
        .execute(DesktopLibraryServiceEnumAction::PreviewTrash(
            StorageTrashPreviewRequest::MoveToTrash(moved),
        ))
        .await?
    else {
        return Err("missing move plan".into());
    };
    let DesktopLibraryServiceEnumResult::Trashed(receipt) = fixture
        .execute(DesktopLibraryServiceEnumAction::ConfirmGameTrash(plan))
        .await?
    else {
        return Err("missing move receipt".into());
    };
    assert_eq!(receipt.action(), StorageTrashEnumAppliedAction::Trashed(moved));
    let DesktopLibraryServiceEnumResult::TrashPlan(plan) = fixture
        .execute(DesktopLibraryServiceEnumAction::PreviewTrash(
            StorageTrashPreviewRequest::MoveToTrash(deleted),
        ))
        .await?
    else {
        return Err("missing second move plan".into());
    };
    fixture
        .execute(DesktopLibraryServiceEnumAction::ConfirmGameTrash(plan))
        .await?;
    let DesktopLibraryServiceEnumResult::TrashPlan(plan) = fixture
        .execute(DesktopLibraryServiceEnumAction::PreviewTrash(
            StorageTrashPreviewRequest::PermanentlyDelete(deleted),
        ))
        .await?
    else {
        return Err("missing delete plan".into());
    };
    let DesktopLibraryServiceEnumResult::Trashed(receipt) = fixture
        .execute(DesktopLibraryServiceEnumAction::ConfirmGameTrash(plan))
        .await?
    else {
        return Err("missing delete receipt".into());
    };
    assert_eq!(receipt.action(), StorageTrashEnumAppliedAction::Deleted(deleted));
    fixture.close().await
}
