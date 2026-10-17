use std::future::{Future, poll_fn};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use tokio::time::Instant;

use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceOperation, DesktopLibraryServiceOptions,
    DesktopLibraryServiceRequest,
};
use crate::desktop::native::bootstrap::DesktopNativeBootstrapLifecycle;
use crate::desktop::native::library_service::DesktopNativeLibraryService;
use crate::storage::library_query::StorageLibraryQueryOptions;

#[tokio::test]
async fn dropped_bootstrap_library_cleanup_retains_the_actor_until_its_query_finishes()
-> DesktopLibraryServiceTestResult {
    let fixture = DesktopLibraryServiceFixture::new().await?;
    let mut native = DesktopNativeLibraryService::new(
        DesktopLibraryServiceOptions {
            workspace: fixture.workspace,
            storage: fixture.store.client(),
            executor: Arc::clone(&fixture.executor),
            games: fixture.games.clone(),
            wall_time: Arc::clone(&fixture.wall) as Arc<_>,
        },
        tokio::runtime::Handle::current(),
    );
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
    let lifecycle = DesktopNativeBootstrapLifecycle::new();
    lifecycle.services.lock().await.library = Some(native);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut cleanup = Box::pin(lifecycle.cleanup_feature_services(deadline));
    let pending = poll_fn(|context| Poll::Ready(cleanup.as_mut().poll(context).is_pending())).await;
    drop(cleanup);
    let retained = lifecycle
        .services
        .try_lock()?
        .library_cleanup
        .as_ref()
        .is_some_and(|cleanup| cleanup.deadline == deadline);
    drop(blocker);
    let outcome = lifecycle.cleanup(deadline).await;
    let query = receipt.wait().await;
    let complete = lifecycle.services.try_lock()?.is_shutdown_complete();
    drop((lifecycle, port, receipt));
    fixture.close().await?;
    outcome?;
    query?;
    assert!(pending);
    assert!(retained);
    assert!(complete);
    Ok(())
}
