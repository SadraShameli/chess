use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use chess::desktop::application::DesktopApplicationError;
use chess::desktop::dto::DesktopEnumValue;
use chess::desktop::error::DesktopEnumFailureCode;
use chess::desktop::game_editor::DesktopGameEditorError;
use chess::desktop::identity::DesktopRevision;
use chess::desktop::library_service::{
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceError,
};
use chess::desktop::messages::{
    DesktopCommand, DesktopEnumCommand, DesktopMessagesEntityId, DesktopMessagesEnumResult,
};

use crate::desktop_library_routing::fixture::{
    DesktopLibraryRoutingFixture, DesktopLibraryRoutingTestResult,
};

#[tokio::test]
async fn query_reaches_provider_once_with_the_actual_application_receipt()
-> DesktopLibraryRoutingTestResult {
    let mut fixture = DesktopLibraryRoutingFixture::new().await?;
    let request = fixture.port.request(DesktopCommand::library(
        DesktopLibraryRoutingFixture::query(fixture.workspace),
    ))?;
    let receipt = fixture.port.try_submit(&request)?;
    assert!(!fixture.poll().await?);
    let actual = fixture.requests.try_recv()?;
    assert!(actual.request.receipt().shares_outcome(&receipt));
    assert_eq!(actual.request.receipt().request_id(), request.request_id());
    assert_eq!(
        actual.request.receipt().received_at(),
        receipt.received_at()
    );
    assert_eq!(actual.request.operation().workspace(), fixture.workspace);
    assert!(receipt.shares_outcome(&fixture.port.try_submit(&request)?));
    assert!(!fixture.poll().await?);
    assert!(fixture.requests.try_recv().is_err());
    let response = fixture.query_result().await?;
    let revision = response.revision();
    actual.finish(Ok(response))?;
    assert!(!fixture.poll().await?);
    let observed = matches!(receipt.response(), Some(DesktopMessagesEnumResult::Value { value: DesktopEnumValue::Library(value), revision: actual }) if *actual == revision && matches!(value.as_ref(), DesktopLibraryServiceEnumResult::Query(_)));
    fixture.quit()?;
    assert!(fixture.poll().await?);
    fixture.close().await?;
    assert!(observed);
    Ok(())
}

#[tokio::test]
async fn open_completion_uses_result_revision_instead_of_requested_storage_revision()
-> DesktopLibraryRoutingTestResult {
    let mut fixture = DesktopLibraryRoutingFixture::new().await?;
    let operation = DesktopLibraryRoutingFixture::open(
        fixture.workspace,
        DesktopLibraryRoutingFixture::game(1)?,
    );
    let expected = operation.expected_revision();
    let receipt = fixture.submit(operation)?;
    assert!(!fixture.poll().await?);
    let actual = fixture.requests.try_recv()?;
    let response = DesktopLibraryServiceEnumResult::Opened(fixture.publication(1)?);
    let revision = response.revision();
    assert_ne!(revision, expected);
    actual.finish(Ok(response))?;
    assert!(!fixture.poll().await?);
    let observed = matches!(receipt.response(), Some(DesktopMessagesEnumResult::Value { value: DesktopEnumValue::Library(value), revision: actual }) if *actual == revision && matches!(value.as_ref(), DesktopLibraryServiceEnumResult::Opened(_)));
    fixture.quit()?;
    assert!(fixture.poll().await?);
    fixture.close().await?;
    assert!(observed);
    Ok(())
}

#[tokio::test]
async fn provider_error_keeps_canonical_typed_identity_for_ui_disposition()
-> DesktopLibraryRoutingTestResult {
    let mut fixture = DesktopLibraryRoutingFixture::new().await?;
    let receipt = fixture.submit(DesktopLibraryRoutingFixture::query(fixture.workspace))?;
    assert!(!fixture.poll().await?);
    let error = Arc::new(DesktopLibraryServiceError::Context(
        DesktopGameEditorError::StaleContext,
    ));
    fixture
        .requests
        .try_recv()?
        .finish(Err(Arc::clone(&error)))?;
    assert!(!fixture.poll().await?);
    let retained = matches!(receipt.response(), Some(DesktopMessagesEnumResult::LibraryFailure(actual)) if Arc::ptr_eq(actual, &error));
    fixture.quit()?;
    assert!(fixture.poll().await?);
    fixture.close().await?;
    assert!(retained);
    Ok(())
}

#[tokio::test]
async fn foreign_workspace_and_mismatched_revision_are_refused_before_provider_dispatch()
-> DesktopLibraryRoutingTestResult {
    let mut fixture = DesktopLibraryRoutingFixture::new().await?;
    let foreign = fixture.submit(DesktopLibraryRoutingFixture::query(
        DesktopMessagesEntityId::new(),
    ))?;
    let operation = DesktopLibraryRoutingFixture::open(
        fixture.workspace,
        DesktopLibraryRoutingFixture::game(1)?,
    );
    let stale = fixture
        .port
        .try_submit(&fixture.port.request(DesktopCommand::new(
            DesktopEnumCommand::Library(operation),
            Some(DesktopRevision::ZERO),
        ))?)?;
    assert!(!fixture.poll().await?);
    let refused = matches!(foreign.response(), Some(DesktopMessagesEnumResult::Failure(failure)) if matches!(failure.code(), DesktopEnumFailureCode::InvalidInput));
    let conflicted = matches!(stale.response(), Some(DesktopMessagesEnumResult::Failure(failure)) if matches!(failure.code(), DesktopEnumFailureCode::StaleRevision));
    assert!(fixture.requests.try_recv().is_err());
    fixture.quit()?;
    assert!(fixture.poll().await?);
    fixture.close().await?;
    assert!(refused && conflicted);
    Ok(())
}

#[tokio::test]
async fn ordinary_bootstrap_remains_reachable_while_library_provider_is_pending()
-> DesktopLibraryRoutingTestResult {
    let mut fixture = DesktopLibraryRoutingFixture::new().await?;
    let pending = fixture.submit(DesktopLibraryRoutingFixture::query(fixture.workspace))?;
    assert!(!fixture.poll().await?);
    let actual = fixture.requests.try_recv()?;
    for _ in 0..3 {
        assert!(!fixture.poll().await?);
    }
    let bootstrap = fixture.port.try_submit(
        &fixture
            .port
            .request(DesktopCommand::new(DesktopEnumCommand::Bootstrap, None))?,
    )?;
    assert!(!fixture.poll().await?);
    let responsive = matches!(
        bootstrap.response(),
        Some(DesktopMessagesEnumResult::Value {
            value: DesktopEnumValue::Bootstrap(_),
            ..
        })
    );
    assert!(pending.response().is_none());
    assert!(fixture.requests.try_recv().is_err());
    actual.finish(Err(Arc::new(DesktopLibraryServiceError::Cancelled)))?;
    assert!(!fixture.poll().await?);
    fixture.quit()?;
    assert!(fixture.poll().await?);
    fixture.close().await?;
    assert!(responsive);
    Ok(())
}

#[tokio::test]
async fn quit_retains_dispatched_work_and_delivers_accepted_success_before_cleanup()
-> DesktopLibraryRoutingTestResult {
    let mut fixture = DesktopLibraryRoutingFixture::new().await?;
    let receipt = fixture.submit(DesktopLibraryRoutingFixture::open(
        fixture.workspace,
        DesktopLibraryRoutingFixture::game(1)?,
    ))?;
    assert!(!fixture.poll().await?);
    let actual = fixture.requests.try_recv()?;
    let quit = fixture.quit()?;
    assert!(!fixture.poll().await?);
    assert!(actual.request.receipt().is_cancellation_requested());
    assert!(quit.response().is_none());
    assert_eq!(fixture.shutdowns.load(Ordering::Acquire), 0);
    actual.finish(Ok(DesktopLibraryServiceEnumResult::Opened(
        fixture.publication(1)?,
    )))?;
    assert!(fixture.poll().await?);
    let delivered = matches!(
        receipt.response(),
        Some(DesktopMessagesEnumResult::Value {
            value: DesktopEnumValue::Library(_),
            ..
        })
    );
    let closed = matches!(quit.response(), Some(DesktopMessagesEnumResult::QuitReady));
    let shutdowns = fixture.shutdowns.load(Ordering::Acquire);
    fixture.close().await?;
    assert!(delivered && closed);
    assert_eq!(shutdowns, 1);
    Ok(())
}

#[tokio::test]
async fn stop_for_actual_game_waits_for_library_response_and_preserves_typed_error()
-> DesktopLibraryRoutingTestResult {
    let mut fixture = DesktopLibraryRoutingFixture::new().await?;
    let game = DesktopLibraryRoutingFixture::game(1)?;
    let receipt = fixture.submit(DesktopLibraryRoutingFixture::open(fixture.workspace, game))?;
    assert!(!fixture.poll().await?);
    let actual = fixture.requests.try_recv()?;
    let stop = fixture
        .port
        .try_submit(&fixture.port.request(DesktopCommand::new(
            DesktopEnumCommand::Stop {
                owner: DesktopMessagesEntityId::for_game(game),
            },
            None,
        ))?)?;
    assert!(!fixture.poll().await?);
    assert!(actual.request.receipt().is_cancellation_requested());
    assert!(stop.response().is_none());
    let error = Arc::new(DesktopLibraryServiceError::Retained(Arc::new(
        DesktopLibraryServiceError::Closed,
    )));
    actual.finish(Err(Arc::clone(&error)))?;
    assert!(!fixture.poll().await?);
    let delivered = matches!(receipt.response(), Some(DesktopMessagesEnumResult::LibraryFailure(actual)) if Arc::ptr_eq(actual, &error));
    let reconciles = matches!(
        stop.response(),
        Some(DesktopMessagesEnumResult::ReconciliationRequired)
    );
    fixture.quit()?;
    assert!(fixture.poll().await?);
    fixture.close().await?;
    assert!(delivered && reconciles);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn quit_deadline_keeps_library_future_until_explicit_retry_observes_result()
-> DesktopLibraryRoutingTestResult {
    let mut fixture = DesktopLibraryRoutingFixture::new().await?;
    let receipt = fixture.submit(DesktopLibraryRoutingFixture::query(fixture.workspace))?;
    assert!(!fixture.poll().await?);
    let actual = fixture.requests.try_recv()?;
    let quit = fixture.quit()?;
    assert!(!fixture.poll().await?);
    tokio::time::advance(Duration::from_secs(3)).await;
    assert!(!fixture.poll().await?);
    assert!(quit.pending_notice()?.is_some());
    actual.finish(Err(Arc::new(DesktopLibraryServiceError::Cancelled)))?;
    assert!(!fixture.poll().await?);
    assert!(receipt.response().is_some());
    assert!(quit.response().is_none());
    assert_eq!(fixture.shutdowns.load(Ordering::Acquire), 0);
    fixture.port.retry_quit()?;
    assert!(fixture.poll().await?);
    fixture.close().await
}

#[tokio::test]
async fn wake_failure_after_completion_keeps_result_and_does_not_repeat_provider()
-> DesktopLibraryRoutingTestResult {
    let mut fixture = DesktopLibraryRoutingFixture::new().await?;
    let receipt = fixture.submit(DesktopLibraryRoutingFixture::query(fixture.workspace))?;
    assert!(!fixture.poll().await?);
    let actual = fixture.requests.try_recv()?;
    fixture.port.acknowledge_wake();
    fixture.wake.0.store(true, Ordering::Release);
    let error = Arc::new(DesktopLibraryServiceError::Closed);
    actual.finish(Err(Arc::clone(&error)))?;
    assert!(fixture.poll().await.is_err());
    let retained = matches!(receipt.response(), Some(DesktopMessagesEnumResult::LibraryFailure(actual)) if Arc::ptr_eq(actual, &error));
    fixture.wake.0.store(false, Ordering::Release);
    assert!(!fixture.poll().await?);
    assert!(fixture.requests.try_recv().is_err());
    fixture.quit()?;
    assert!(fixture.poll().await?);
    fixture.close().await?;
    assert!(retained);
    Ok(())
}

#[tokio::test]
async fn dispatched_library_futures_keep_ordinary_admission_bounded()
-> DesktopLibraryRoutingTestResult {
    let mut fixture = DesktopLibraryRoutingFixture::new().await?;
    let capacity = chess::desktop::application::DesktopApplication::<
        crate::desktop_library_routing::fixture::DesktopLibraryRoutingClock,
        crate::desktop_library_routing::fixture::DesktopLibraryRoutingWake,
    >::ORDINARY_CAPACITY;
    let mut receipts = Vec::new();
    for _ in 0..capacity {
        receipts.push(fixture.submit(DesktopLibraryRoutingFixture::query(fixture.workspace))?);
    }
    for _ in 0..capacity {
        if fixture.requests.len() == capacity {
            break;
        }
        assert!(!fixture.poll().await?);
    }
    let additional = fixture.port.request(DesktopCommand::library(
        DesktopLibraryRoutingFixture::query(fixture.workspace),
    ))?;
    let bounded = matches!(
        fixture.port.try_submit(&additional),
        Err(DesktopApplicationError::Busy)
    );
    assert_eq!(fixture.requests.len(), capacity);
    while let Ok(actual) = fixture.requests.try_recv() {
        actual.finish(Err(Arc::new(DesktopLibraryServiceError::Cancelled)))?;
    }
    for _ in 0..capacity {
        if receipts.iter().all(|receipt| receipt.response().is_some()) {
            break;
        }
        assert!(!fixture.poll().await?);
    }
    fixture.quit()?;
    tokio::time::timeout(Duration::from_secs(3), fixture.coordinator.run()).await??;
    let observed = receipts.iter().all(|receipt| receipt.response().is_some());
    fixture.close().await?;
    assert!(bounded && observed);
    Ok(())
}
