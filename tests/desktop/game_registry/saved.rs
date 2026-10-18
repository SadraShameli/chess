use std::future::{Future, poll_fn};
use std::sync::atomic::Ordering;
use std::task::Poll;
use std::time::Duration;
use tokio::time::Instant;

use crate::board::enums::BoardEnumColor;
use crate::clock::duration::ClockInstant;
use crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumResult;
use crate::desktop::game_editor::DesktopGameEditorSnapshotEnumState;
use crate::desktop::game_registry::{
    DesktopGameRegistryError, DesktopGameRegistryPort, DesktopGameRegistryRestored,
};
use crate::desktop::library_service::tests::fixture::DesktopLibraryServiceTestProof;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::game::enums::{GameEnumEngineReadiness, GameEnumPhase};
use crate::game::pause::GamePauseEnumEngineBarrier;
use crate::game::result::GameResultResignRequest;
use crate::game::storage_repository::GameStorageRevision;

#[tokio::test]
async fn completed_and_recovered_active_sessions_open_without_start_or_resume()
-> DesktopLibraryServiceTestResult {
    for completed in [false, true] {
        let mut fixture = DesktopLibraryServiceFixture::new().await?;
        let mut session = DesktopLibraryServiceFixture::played(100)?;
        session.start(
            session.revision(),
            ClockInstant::from_nanos(50),
            GameEnumEngineReadiness::NotRequired,
            &DesktopLibraryServiceTestProof,
        )?;
        if completed {
            session.resign(
                GameResultResignRequest {
                    expected_revision: session.revision(),
                    resigning: BoardEnumColor::White,
                    received_at: ClockInstant::from_nanos(50),
                    engine: GamePauseEnumEngineBarrier::NoEngine,
                },
                &DesktopLibraryServiceTestProof,
            )?;
        }
        let revision = fixture.seed_played(&session).await?;
        let opened = fixture.open(100, revision).await?;
        let DesktopGameEditorSnapshotEnumState::Played { phase, outcome, .. } = opened.state()
        else {
            return Err("missing played state".into());
        };
        assert_eq!(
            *phase,
            if completed {
                GameEnumPhase::Completed
            } else {
                GameEnumPhase::Interrupted
            }
        );
        assert_eq!(*outcome, session.outcome());
        assert_eq!(opened.context().storage_revision(), revision);
        drop(opened);
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn existing_live_owner_keeps_its_epoch_and_source_without_any_library_record()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let mut session = DesktopLibraryServiceFixture::played(100)?;
    session.start(
        session.revision(),
        ClockInstant::from_nanos(50),
        GameEnumEngineReadiness::NotRequired,
        &DesktopLibraryServiceTestProof,
    )?;
    let live_revision = session.revision();
    fixture
        .registry
        .restore(DesktopGameRegistryRestored {
            session,
            storage_revision: GameStorageRevision::INITIAL,
        })
        .await?;
    let first = fixture.open(100, GameStorageRevision::new(999)).await?;
    let second = fixture.open(100, GameStorageRevision::INITIAL).await?;
    assert_eq!(first.context(), second.context());
    assert_eq!(
        first.context().source(),
        crate::desktop::game_editor::DesktopGameEditorEnumSource::Played {
            revision: live_revision
        }
    );
    assert!(matches!(
        first.state(),
        DesktopGameEditorSnapshotEnumState::Played {
            phase: GameEnumPhase::Active,
            ..
        }
    ));
    assert_eq!(fixture.factories.load(Ordering::SeqCst), 1);
    drop((first, second));
    fixture.close().await
}

#[tokio::test]
async fn saved_open_admission_is_bounded_and_queued_responses_are_observed_on_close()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let mut replies = Vec::new();
    for _ in 0..DesktopGameRegistryPort::CREATE_CAPACITY {
        replies.push(fixture.games.try_open_saved_editor(
            DesktopLibraryServiceFixture::id(100)?,
            GameStorageRevision::INITIAL,
            fixture.receipt()?,
        )?);
    }
    assert!(matches!(
        fixture.games.try_open_saved_editor(
            DesktopLibraryServiceFixture::id(100)?,
            GameStorageRevision::INITIAL,
            fixture.receipt()?
        ),
        Err(DesktopGameRegistryError::Busy)
    ));
    fixture
        .registry
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    for mut reply in replies {
        assert!(
            matches!(reply.wait().await, Err(error) if matches!(error.as_ref(), DesktopGameRegistryError::Closed))
        );
    }
    fixture.close().await
}

#[tokio::test]
async fn saved_open_deadline_retains_load_install_and_original_editor_receipt()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, false).await?;
    let lease = fixture.budget.lease(1).await?;
    let application = fixture.receipt()?;
    let mut reply = fixture.games.try_open_saved_editor(
        DesktopLibraryServiceFixture::id(100)?,
        revision,
        application.clone(),
    )?;
    let mut running = Box::pin(fixture.registry.run());
    poll_fn(|context| {
        assert!(running.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(running);
    assert!(
        fixture
            .registry
            .shutdown_retained(Instant::now())
            .await
            .is_err()
    );
    assert!(!fixture.registry.is_shutdown_complete());
    drop(lease);
    fixture
        .registry
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(matches!(
        reply.wait().await?,
        DesktopAnnotationCommandsEnumResult::Snapshot(_)
    ));
    assert!(
        reply
            .editor_receipt()
            .ok_or("missing actual forwarded editor receipt")?
            .application_receipt()
            .shares_outcome(&application)
    );
    assert!(fixture.registry.is_shutdown_complete());
    assert_eq!(fixture.factories.load(Ordering::SeqCst), 1);
    drop(reply);
    fixture.close().await
}

#[tokio::test]
async fn running_registry_saved_open_installs_once_without_start() -> DesktopLibraryServiceTestResult
{
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, false).await?;
    let application = fixture.receipt()?;
    let mut reply = fixture.games.try_open_saved_editor(
        DesktopLibraryServiceFixture::id(100)?,
        revision,
        application.clone(),
    )?;
    let result = tokio::select! { result = reply.wait() => result?, result = fixture.registry.run() => return Err(format!("registry ended: {result:?}").into()), };
    let DesktopAnnotationCommandsEnumResult::Snapshot(first) = result else {
        return Err("missing saved editor".into());
    };
    assert!(matches!(
        first.state(),
        DesktopGameEditorSnapshotEnumState::Played {
            phase: GameEnumPhase::Setup | GameEnumPhase::Interrupted,
            ..
        }
    ));
    assert_eq!(
        reply.application_receipt().request_id(),
        application.request_id()
    );
    let mut again = fixture.games.try_open_saved_editor(
        first.context().game(),
        GameStorageRevision::INITIAL,
        fixture.receipt()?,
    )?;
    let result = tokio::select! { result = again.wait() => result?, result = fixture.registry.run() => return Err(format!("registry ended: {result:?}").into()), };
    let DesktopAnnotationCommandsEnumResult::Snapshot(second) = result else {
        return Err("missing reused editor".into());
    };
    assert_eq!(first.context(), second.context());
    assert_eq!(fixture.factories.load(Ordering::SeqCst), 1);
    drop((first, second, reply, again));
    fixture.close().await
}

#[tokio::test]
async fn saved_open_rejects_stale_absent_record_without_constructing_factory()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    fixture.seed(100, false).await?;
    let mut reply = fixture.games.try_open_saved_editor(
        DesktopLibraryServiceFixture::id(100)?,
        GameStorageRevision::INITIAL,
        fixture.receipt()?,
    )?;
    let result = tokio::select! { result = reply.wait() => result, result = fixture.registry.run() => return Err(format!("registry ended: {result:?}").into()), };
    assert!(
        matches!(result, Err(error) if matches!(error.as_ref(), DesktopGameRegistryError::Library(_)))
    );
    assert_eq!(fixture.factories.load(Ordering::SeqCst), 0);
    drop(reply);
    fixture.close().await
}

#[tokio::test]
async fn dropping_saved_open_wait_does_not_cancel_load_or_install()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let revision = fixture.seed(100, false).await?;
    let lease = fixture.budget.try_lease(1)?;
    let mut reply = fixture.games.try_open_saved_editor(
        DesktopLibraryServiceFixture::id(100)?,
        revision,
        fixture.receipt()?,
    )?;
    let mut running = Box::pin(fixture.registry.run());
    poll_fn(|context| {
        assert!(running.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(running);
    let mut waiting = Box::pin(reply.wait());
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    drop(lease);
    let result = tokio::select! { result = reply.wait() => result?, result = fixture.registry.run() => return Err(format!("registry ended: {result:?}").into()), };
    assert!(matches!(
        result,
        DesktopAnnotationCommandsEnumResult::Snapshot(_)
    ));
    assert_eq!(fixture.factories.load(Ordering::SeqCst), 1);
    drop(reply);
    fixture.close().await
}
#[path = "saved_open_review.rs"]
mod saved_open_review;
