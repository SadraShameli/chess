use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::time::Instant;

use crate::desktop::native::bootstrap::{
    DesktopNativeBootstrapError, DesktopNativeBootstrapPuzzleService,
};
use crate::desktop::native::library_service::DesktopNativeLibraryService;

pub(super) trait DesktopNativeBootstrapCleanupOwner: Send + Sized + 'static {
    fn is_shutdown_complete(&self) -> bool;
    fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> impl Future<Output = Result<(), DesktopNativeBootstrapError>> + Send;
}

impl DesktopNativeBootstrapCleanupOwner for DesktopNativeBootstrapPuzzleService {
    fn is_shutdown_complete(&self) -> bool {
        Self::is_shutdown_complete(self)
    }

    async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopNativeBootstrapError> {
        Self::shutdown_retained(self, deadline)
            .await
            .map_err(Into::into)
    }
}

impl DesktopNativeBootstrapCleanupOwner for DesktopNativeLibraryService {
    fn is_shutdown_complete(&self) -> bool {
        Self::is_shutdown_complete(self)
    }

    async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopNativeBootstrapError> {
        Self::shutdown_retained(self, deadline)
            .await
            .map_err(Into::into)
    }
}

type DesktopNativeBootstrapCleanupFuture<S> =
    Pin<Box<dyn Future<Output = (S, Result<(), DesktopNativeBootstrapError>)> + Send>>;

pub(super) enum DesktopNativeBootstrapCleanupEnumOutcome {
    Complete,
    Terminal(DesktopNativeBootstrapError),
    Retained(DesktopNativeBootstrapError),
}

pub(super) struct DesktopNativeBootstrapCleanup<S> {
    pub(super) deadline: Instant,
    future: DesktopNativeBootstrapCleanupFuture<S>,
}

impl<S: DesktopNativeBootstrapCleanupOwner> DesktopNativeBootstrapCleanup<S> {
    pub(super) fn poll(
        owner: &mut Option<S>,
        cleanup: &mut Option<Self>,
        deadline: Instant,
        context: &mut Context<'_>,
    ) -> Poll<DesktopNativeBootstrapCleanupEnumOutcome> {
        if cleanup.is_none() {
            let Some(mut service) = owner.take() else {
                return Poll::Ready(DesktopNativeBootstrapCleanupEnumOutcome::Complete);
            };
            *cleanup = Some(Self {
                deadline,
                future: Box::pin(async move {
                    let result = service.shutdown_retained(deadline).await;
                    (service, result)
                }),
            });
        }
        let Some(pending) = cleanup.as_mut() else {
            return Poll::Ready(DesktopNativeBootstrapCleanupEnumOutcome::Retained(
                DesktopNativeBootstrapError::HostServices,
            ));
        };
        let Poll::Ready((service, result)) = pending.future.as_mut().poll(context) else {
            return Poll::Pending;
        };
        *cleanup = None;
        if service.is_shutdown_complete() {
            return Poll::Ready(match result {
                Ok(()) => DesktopNativeBootstrapCleanupEnumOutcome::Complete,
                Err(error) => DesktopNativeBootstrapCleanupEnumOutcome::Terminal(error),
            });
        }
        *owner = Some(service);
        Poll::Ready(DesktopNativeBootstrapCleanupEnumOutcome::Retained(
            result
                .err()
                .unwrap_or(DesktopNativeBootstrapError::CleanupDeadline),
        ))
    }
}
