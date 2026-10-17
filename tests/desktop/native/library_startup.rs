use std::future::{Future, poll_fn};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use chess::clock::duration::ClockInstant;
use chess::desktop::native::library_startup::{
    DesktopNativeLibraryStartup, DesktopNativeLibraryStartupEnumIssueCause,
    DesktopNativeLibraryStartupEnumReadiness, DesktopNativeLibraryStartupEnumStage,
};
use chess::game::enums::GameId;
use chess::game::storage_repository::GameStorageRevision;
use chess::storage::game_document::StorageGameDocument;
use chess::storage::library_inventory::{
    StorageLibraryInventoryCursor, StorageLibraryInventoryRepository,
};
use chess::storage::library_query::StorageLibraryQuery;
use chess::storage::library_repository::StorageLibraryRepository;
use tokio::time::Instant;

use crate::desktop_game_registry_fixture::{
    DesktopGameRegistryFixtureProof, DesktopGameRegistryFixtureRuntime,
    DesktopGameRegistryTestResult,
};
use crate::desktop_native_game_service::{
    DesktopNativeGameServiceFixture, DesktopNativeGameServiceFixtureEnumDocument,
};

struct DesktopNativeLibraryStartupFixture;

impl DesktopNativeLibraryStartupFixture {
    fn id(value: u128) -> Result<GameId, Box<dyn std::error::Error>> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn played(value: u128) -> Result<String, Box<dyn std::error::Error>> {
        let session = DesktopNativeGameServiceFixture::played(
            Self::id(value)?,
            uuid::Uuid::from_u128(50_000).to_string().parse()?,
            uuid::Uuid::from_u128(50_001).to_string().parse()?,
            "e2e4",
        )?;
        let body =
            StorageGameDocument::capture(&session, ClockInstant::from_millis(2_000)?)?.json()?;
        let legacy = body
            .replacen(
                "\"schema_version\":3,\"kind\":\"played\"",
                "\"schema_version\":2",
                1,
            )
            .replacen(
                &format!("\"study_mainline_tip\":\"{}\",", session.live().as_uuid()),
                "",
                1,
            );
        StorageGameDocument::recover_record(&legacy)?;
        Ok(legacy)
    }

    fn owner(fixture: &DesktopGameRegistryFixtureRuntime) -> DesktopNativeLibraryStartup {
        DesktopNativeLibraryStartup::new(
            fixture.store.client(),
            Arc::clone(&fixture.executor),
            Instant::now() + Duration::from_secs(60),
        )
    }

    async fn pending(owner: &mut DesktopNativeLibraryStartup) -> bool {
        let mut wait = std::pin::pin!(owner.advance());
        poll_fn(|context| Poll::Ready(wait.as_mut().poll(context).is_pending())).await
    }
}

#[tokio::test]
async fn adoption_classifies_more_than_one_page_and_retains_corrupt_and_future_issues()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let client = fixture.store.client();
    let mut originals = Vec::new();
    for value in 1..=105 {
        let body = DesktopNativeLibraryStartupFixture::played(value)?;
        let id = DesktopNativeLibraryStartupFixture::id(value)?;
        let receipt = client.save(id.as_uuid(), 0, body.clone()).await?;
        originals.push((id, receipt.revision(), body));
    }
    for (value, body) in [
        (
            106,
            DesktopNativeGameServiceFixtureEnumDocument::Notation
                .body(DesktopNativeLibraryStartupFixture::id(106)?)?,
        ),
        (
            107,
            r#"{"workspace":{"selected_panel":"library"}}"#.to_owned(),
        ),
        (
            108,
            r#"{"schema_version":3,"kind":"played","game":null}"#.to_owned(),
        ),
        (
            109,
            r#"{"schema_version":99,"kind":"played","game":null}"#.to_owned(),
        ),
    ] {
        let id = DesktopNativeLibraryStartupFixture::id(value)?;
        let receipt = client.save(id.as_uuid(), 0, body.clone()).await?;
        originals.push((id, receipt.revision(), body));
    }
    let mut owner = DesktopNativeLibraryStartupFixture::owner(&fixture);
    owner.run().await?;
    let DesktopNativeLibraryStartupEnumReadiness::RecoveryRequired(summary) = owner.readiness()
    else {
        return Err("recognized invalid played records were certified ready".into());
    };
    assert_eq!(summary.issue_count(), 2);
    assert_eq!(summary.issues().len(), 2);
    for (issue, value) in summary.issues().iter().zip([108, 109]) {
        assert_eq!(
            issue.id(),
            DesktopNativeLibraryStartupFixture::id(value)?.as_uuid()
        );
        assert_eq!(issue.content_revision(), GameStorageRevision::new(1));
        assert!(matches!(
            issue.cause(),
            DesktopNativeLibraryStartupEnumIssueCause::Adoption(_)
        ));
    }
    let inventory = StorageLibraryInventoryRepository::new(client.clone());
    let first = inventory
        .page(StorageLibraryInventoryCursor::start())
        .await?;
    assert_eq!(first.entries().len(), 100);
    let second = inventory
        .page(
            first
                .next_cursor()
                .ok_or("missing inventory continuation")?,
        )
        .await?;
    assert_eq!(second.entries().len(), 5);
    assert!(second.next_cursor().is_none());
    let repository = StorageLibraryRepository::new(client.clone(), &fixture.executor);
    let page = repository.query(StorageLibraryQuery::default()).await?;
    assert!(page.unindexed_document_count() > 0);
    drop(page);
    for (id, revision, body) in originals {
        let document = client
            .read(id.as_uuid())
            .await?
            .ok_or("missing preserved document")?;
        assert_eq!(document.revision(), revision);
        assert_eq!(document.body(), body);
        if id.as_uuid().as_u128() <= 106 {
            let recovered = repository
                .load(id)
                .await?
                .ok_or("valid record was not adopted")?;
            assert_eq!(recovered.modified(), None);
            assert_eq!(recovered.content_revision().value(), revision);
        }
    }
    drop(first);
    drop(second);
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn changed_candidate_is_reported_without_false_ready() -> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let id = DesktopNativeLibraryStartupFixture::id(200)?;
    let client = fixture.store.client();
    client
        .save(
            id.as_uuid(),
            0,
            DesktopNativeLibraryStartupFixture::played(200)?,
        )
        .await?;
    let mut owner = DesktopNativeLibraryStartupFixture::owner(&fixture);
    owner.advance().await?;
    client
        .save(
            id.as_uuid(),
            1,
            DesktopNativeLibraryStartupFixture::played(200)?,
        )
        .await?;
    owner.run().await?;
    let DesktopNativeLibraryStartupEnumReadiness::RecoveryRequired(summary) = owner.readiness()
    else {
        return Err("a changed candidate was certified ready".into());
    };
    assert!(summary.issues().iter().any(|issue| matches!(
        issue.cause(),
        DesktopNativeLibraryStartupEnumIssueCause::Changed(_)
    )));
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn new_candidate_behind_cursor_requires_a_reconciled_sweep() -> DesktopGameRegistryTestResult
{
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let client = fixture.store.client();
    let high = DesktopNativeLibraryStartupFixture::id(300)?;
    client
        .save(
            high.as_uuid(),
            0,
            DesktopNativeLibraryStartupFixture::played(300)?,
        )
        .await?;
    let mut owner = DesktopNativeLibraryStartupFixture::owner(&fixture);
    owner.advance().await?;
    owner.advance().await?;
    owner.advance().await?;
    let low = DesktopNativeLibraryStartupFixture::id(250)?;
    client
        .save(
            low.as_uuid(),
            0,
            DesktopNativeLibraryStartupFixture::played(250)?,
        )
        .await?;
    owner.run().await?;
    assert!(matches!(
        owner.readiness(),
        DesktopNativeLibraryStartupEnumReadiness::Ready(_)
    ));
    let loaded = StorageLibraryRepository::new(client, &fixture.executor)
        .load(low)
        .await?;
    assert!(loaded.is_some());
    drop(loaded);
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn cancelled_preparation_and_adoption_waits_retain_owner_cursor_and_deadline()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let client = fixture.store.client();
    let id = DesktopNativeLibraryStartupFixture::id(400)?;
    client
        .save(
            id.as_uuid(),
            0,
            DesktopNativeLibraryStartupFixture::played(400)?,
        )
        .await?;
    let mut owner = DesktopNativeLibraryStartupFixture::owner(&fixture);
    let deadline = owner.deadline();
    owner.advance().await?;
    let capacity = fixture.budget.lease(fixture.budget.workers()).await?;
    assert!(DesktopNativeLibraryStartupFixture::pending(&mut owner).await);
    assert_eq!(owner.deadline(), deadline);
    assert_eq!(owner.progress().last_observed(), None);
    assert_eq!(
        owner.progress().stage(),
        DesktopNativeLibraryStartupEnumStage::Preparing
    );
    drop(capacity);
    owner.advance().await?;
    let capacity = fixture.budget.lease(fixture.budget.workers()).await?;
    assert!(DesktopNativeLibraryStartupFixture::pending(&mut owner).await);
    assert_eq!(owner.progress().last_observed(), None);
    assert_eq!(
        owner.progress().stage(),
        DesktopNativeLibraryStartupEnumStage::Adopting
    );
    assert_eq!(owner.deadline(), deadline);
    drop(capacity);
    owner.run().await?;
    assert_eq!(owner.progress().last_observed(), Some(id.as_uuid()));
    assert!(matches!(
        owner.readiness(),
        DesktopNativeLibraryStartupEnumReadiness::Ready(_)
    ));
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn changed_after_preparation_retains_the_rejected_adoption_receipt()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let client = fixture.store.client();
    let id = DesktopNativeLibraryStartupFixture::id(500)?;
    let body = DesktopNativeLibraryStartupFixture::played(500)?;
    client.save(id.as_uuid(), 0, body.clone()).await?;
    let mut owner = DesktopNativeLibraryStartupFixture::owner(&fixture);
    owner.advance().await?;
    owner.advance().await?;
    client.save(id.as_uuid(), 1, body.clone()).await?;
    owner.advance().await?;
    assert_eq!(owner.progress().last_observed(), Some(id.as_uuid()));
    owner.run().await?;
    let DesktopNativeLibraryStartupEnumReadiness::RecoveryRequired(summary) = owner.readiness()
    else {
        return Err("Changed adoption receipt was replaced by Ready".into());
    };
    assert!(summary.issues().iter().any(|issue| matches!(issue.cause(), DesktopNativeLibraryStartupEnumIssueCause::Changed(Some(revision)) if *revision == GameStorageRevision::new(2))));
    let preserved = client
        .read(id.as_uuid())
        .await?
        .ok_or("missing changed record")?;
    assert_eq!(preserved.body(), body);
    assert_eq!(preserved.revision(), 2);
    drop(preserved);
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn unrelated_documents_do_not_prevent_a_settled_classification_receipt()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let client = fixture.store.client();
    let id = DesktopNativeLibraryStartupFixture::id(600)?;
    client
        .save(
            id.as_uuid(),
            0,
            r#"{"workspace":{"selected_panel":"library"}}"#.to_owned(),
        )
        .await?;
    let mut owner = DesktopNativeLibraryStartupFixture::owner(&fixture);
    owner.run().await?;
    assert!(matches!(
        owner.readiness(),
        DesktopNativeLibraryStartupEnumReadiness::Ready(_)
    ));
    assert_eq!(owner.progress().unrelated(), 1);
    let page = StorageLibraryRepository::new(client, &fixture.executor)
        .query(StorageLibraryQuery::default())
        .await?;
    assert_eq!(page.unindexed_document_count(), 1);
    drop(page);
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn issue_reports_are_bounded_without_certifying_unreported_failures_ready()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let client = fixture.store.client();
    for value in 1..=107 {
        client
            .save(
                DesktopNativeLibraryStartupFixture::id(value)?.as_uuid(),
                0,
                r#"{"schema_version":3,"kind":"played","game":null}"#.to_owned(),
            )
            .await?;
    }
    let mut owner = DesktopNativeLibraryStartupFixture::owner(&fixture);
    owner.run().await?;
    let DesktopNativeLibraryStartupEnumReadiness::RecoveryRequired(summary) = owner.readiness()
    else {
        return Err("bounded issue capacity hid unresolved failures".into());
    };
    assert_eq!(summary.issue_count(), 107);
    assert_eq!(summary.issues().len(), 100);
    assert_eq!(owner.progress().observed(), 107);
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn expired_cancelled_adoption_retains_its_receipt_until_cleanup_observes_commit()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let client = fixture.store.client();
    let id = DesktopNativeLibraryStartupFixture::id(700)?;
    client
        .save(
            id.as_uuid(),
            0,
            DesktopNativeLibraryStartupFixture::played(700)?,
        )
        .await?;
    let mut owner = DesktopNativeLibraryStartupFixture::owner(&fixture);
    owner.advance().await?;
    owner.advance().await?;
    tokio::time::pause();
    let deadline = owner.deadline();
    let capacity = fixture.budget.lease(fixture.budget.workers()).await?;
    assert!(DesktopNativeLibraryStartupFixture::pending(&mut owner).await);
    tokio::time::advance(Duration::from_secs(61)).await;
    let expired = owner.advance().await;
    assert!(matches!(
        expired,
        Err(chess::desktop::native::library_startup::DesktopNativeLibraryStartupError::Deadline(_))
    ));
    assert!(owner.has_pending());
    assert_eq!(owner.deadline(), deadline);
    assert_eq!(owner.progress().last_observed(), None);
    tokio::time::resume();
    drop(capacity);
    owner
        .drain_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(!owner.has_pending());
    assert_eq!(owner.progress().last_observed(), Some(id.as_uuid()));
    let recovered = StorageLibraryRepository::new(client, &fixture.executor)
        .load(id)
        .await?
        .ok_or("cleanup lost the accepted adoption")?;
    assert_eq!(recovered.content_revision(), GameStorageRevision::new(1));
    assert_eq!(recovered.modified(), None);
    drop(recovered);
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn repeated_catalog_changes_exhaust_bounded_sweeps_without_false_ready()
-> DesktopGameRegistryTestResult {
    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let client = fixture.store.client();
    let high = DesktopNativeLibraryStartupFixture::id(1_000)?;
    client
        .save(high.as_uuid(), 0, r#"{"workspace":{}}"#.to_owned())
        .await?;
    let mut owner = DesktopNativeLibraryStartupFixture::owner(&fixture);
    for sweep in 1..=DesktopNativeLibraryStartup::MAX_SWEEPS {
        owner.advance().await?;
        while owner.progress().last_observed() != Some(high.as_uuid()) {
            owner.advance().await?;
        }
        let low = DesktopNativeLibraryStartupFixture::id(u128::from(sweep))?;
        client
            .save(low.as_uuid(), 0, r#"{"workspace":{}}"#.to_owned())
            .await?;
        owner.advance().await?;
    }
    owner.run().await?;
    let DesktopNativeLibraryStartupEnumReadiness::RecoveryRequired(summary) = owner.readiness()
    else {
        return Err("unstable generations were certified ready".into());
    };
    assert!(summary.is_catalog_unsettled());
    assert_eq!(summary.issue_count(), 0);
    assert_eq!(
        owner.progress().sweep(),
        DesktopNativeLibraryStartup::MAX_SWEEPS
    );
    drop(owner);
    fixture.close().await
}

struct DesktopNativeLibraryStartupCertificationWall;

impl chess::game::storage_repository::GameStorageWallTimeProvider
    for DesktopNativeLibraryStartupCertificationWall
{
    fn now(
        &self,
    ) -> Result<
        chess::game::library_organization::GameLibraryOrganizationTimestamp,
        chess::game::storage_repository::GameStorageWallTimeError,
    > {
        Ok(chess::game::library_organization::GameLibraryOrganizationTimestamp::new(100)?)
    }
}

#[tokio::test]
async fn settled_startup_certifies_actor_with_legitimate_unrelated_documents()
-> DesktopGameRegistryTestResult {
    use chess::storage::import_session::{
        StorageImportSessionDestination, StorageImportSessionRepository,
    };
    use chess::storage::library_import::StorageLibraryImportRepository;

    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    let client = fixture.store.client();
    client
        .save(
            uuid::Uuid::from_u128(90_000),
            0,
            "{\"preferences\":true}".to_owned(),
        )
        .await?;
    let repository = StorageLibraryImportRepository::new(
        client.clone(),
        StorageImportSessionDestination::unfiled(),
        Arc::new(DesktopNativeLibraryStartupCertificationWall),
    );
    assert!(!repository.snapshot().await?.is_adoption_complete());
    let mut owner = DesktopNativeLibraryStartupFixture::owner(&fixture);
    owner.run().await?;
    assert!(matches!(
        owner.readiness(),
        DesktopNativeLibraryStartupEnumReadiness::Ready(_)
    ));
    assert!(repository.snapshot().await?.is_adoption_complete());
    assert!(client.read(uuid::Uuid::from_u128(90_000)).await?.is_some());
    drop(owner);
    fixture.close().await
}

#[tokio::test]
async fn recovery_required_startup_never_certifies_a_recognized_broken_game()
-> DesktopGameRegistryTestResult {
    use chess::storage::import_session::{
        StorageImportSessionDestination, StorageImportSessionRepository,
    };
    use chess::storage::library_import::StorageLibraryImportRepository;

    let fixture =
        DesktopGameRegistryFixtureRuntime::new(DesktopGameRegistryFixtureProof::default()).await?;
    fixture
        .store
        .client()
        .save(
            uuid::Uuid::from_u128(90_001),
            0,
            "{\"schema_version\":3,\"kind\":\"played\",\"game\":null}".to_owned(),
        )
        .await?;
    let mut owner = DesktopNativeLibraryStartupFixture::owner(&fixture);
    owner.run().await?;
    assert!(matches!(
        owner.readiness(),
        DesktopNativeLibraryStartupEnumReadiness::RecoveryRequired(_)
    ));
    let repository = StorageLibraryImportRepository::new(
        fixture.store.client(),
        StorageImportSessionDestination::unfiled(),
        Arc::new(DesktopNativeLibraryStartupCertificationWall),
    );
    assert!(!repository.snapshot().await?.is_adoption_complete());
    drop(owner);
    fixture.close().await
}
