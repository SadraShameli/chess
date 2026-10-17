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
    let request = GameLibraryOrganizationRequest::new(
        GameLibraryOrganizationTimestamp::new(10)?,
        GameLibraryOrganizationEnumMutation::CreateTag(
            uuid::Uuid::from_u128(7).to_string().parse()?,
            GameLibraryOrganizationName::new("Endgames".to_owned())?,
        ),
    )?;
    let action = DesktopLibraryServiceEnumAction::Organize(request);
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
