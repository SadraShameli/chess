use std::future::{Future, poll_fn};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use tokio::time::Instant;

use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumResult,
    DesktopLibraryServiceOperation, DesktopLibraryServiceOptions, DesktopLibraryServiceRequest,
};
use crate::desktop::native::library_service::DesktopNativeLibraryService;
use crate::storage::library_query::StorageLibraryQueryOptions;

async fn poll_once<F: Future>(future: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    let mut future = future;
    poll_fn(|context| Poll::Ready(future.as_mut().poll(context))).await
}

fn service(fixture: &DesktopLibraryServiceFixture) -> DesktopNativeLibraryService {
    DesktopNativeLibraryService::new(
        DesktopLibraryServiceOptions {
            workspace: fixture.workspace,
            storage: fixture.store.client(),
            executor: Arc::clone(&fixture.executor),
            games: fixture.games.clone(),
            wall_time: Arc::clone(&fixture.wall) as Arc<_>,
            imports: None,
        },
        tokio::runtime::Handle::current(),
    )
}

#[tokio::test]
async fn native_library_query_retains_the_actual_application_receipt()
-> DesktopLibraryServiceTestResult {
    let fixture = DesktopLibraryServiceFixture::new().await?;
    fixture.seed(6500, true).await?;
    let mut native = service(&fixture);
    let port = native.start()?;
    let application = fixture.receipt()?;
    let mut receipt = port.try_submit(DesktopLibraryServiceRequest::new(
        application.clone(),
        DesktopLibraryServiceOperation::new(
            fixture.workspace,
            DesktopLibraryServiceEnumAction::Query {
                options: StorageLibraryQueryOptions::default(),
                cursor: None,
            },
        ),
    ))?;
    let result = receipt.wait().await;
    let correlated = receipt.application_receipt().shares_outcome(&application);
    native
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await?;
    let complete = native.is_shutdown_complete();
    drop((receipt, port, native));
    fixture.close().await?;
    let DesktopLibraryServiceEnumResult::Query(page) = result? else {
        return Err("native library returned an incompatible query result".into());
    };
    assert!(correlated);
    assert!(complete);
    assert_eq!(page.entries().len(), 1);
    Ok(())
}

#[tokio::test]
async fn cancelled_native_library_shutdown_wait_retains_its_actor_and_request()
-> DesktopLibraryServiceTestResult {
    let fixture = DesktopLibraryServiceFixture::new().await?;
    let mut native = service(&fixture);
    let port = native.start()?;
    let blocker = fixture.budget.lease(1).await?;
    let mut receipt = port.try_submit(DesktopLibraryServiceRequest::new(
        fixture.receipt()?,
        DesktopLibraryServiceOperation::new(
            fixture.workspace,
            DesktopLibraryServiceEnumAction::Query {
                options: StorageLibraryQueryOptions::default(),
                cursor: None,
            },
        ),
    ))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut cleanup = Box::pin(native.shutdown_retained(deadline));
    let pending = poll_once(cleanup.as_mut()).await.is_pending();
    drop(cleanup);
    let retained = !native.is_shutdown_complete();
    drop(blocker);
    native.shutdown_retained(deadline).await?;
    let result = receipt.wait().await;
    let complete = native.is_shutdown_complete();
    drop((receipt, port, native));
    fixture.close().await?;
    assert!(pending);
    assert!(retained);
    assert!(complete);
    assert!(matches!(result?, DesktopLibraryServiceEnumResult::Query(_)));
    Ok(())
}

#[tokio::test]
async fn expired_native_library_cleanup_keeps_ownership_until_observed_retry()
-> DesktopLibraryServiceTestResult {
    let fixture = DesktopLibraryServiceFixture::new().await?;
    let mut native = service(&fixture);
    let port = native.start()?;
    let blocker = fixture.budget.lease(1).await?;
    let mut receipt = port.try_submit(DesktopLibraryServiceRequest::new(
        fixture.receipt()?,
        DesktopLibraryServiceOperation::new(
            fixture.workspace,
            DesktopLibraryServiceEnumAction::Query {
                options: StorageLibraryQueryOptions::default(),
                cursor: None,
            },
        ),
    ))?;
    let expired = native.shutdown_retained(Instant::now()).await.is_err();
    let retained = !native.is_shutdown_complete();
    drop(blocker);
    let resumed = native
        .shutdown_retained(Instant::now() + Duration::from_secs(3))
        .await;
    let result = receipt.wait().await;
    let complete = native.is_shutdown_complete();
    let finalized = native.shutdown_retained(Instant::now()).await;
    drop((receipt, port, native));
    fixture.close().await?;
    assert!(expired);
    assert!(retained);
    assert!(complete);
    assert!(resumed.is_err());
    assert!(finalized.is_ok());
    assert!(matches!(result?, DesktopLibraryServiceEnumResult::Query(_)));
    Ok(())
}
