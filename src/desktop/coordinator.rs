use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::time::Instant;

use crate::capacity::executor::CapacityExecutor;
use crate::clock::duration::ClockDurationError;
use crate::clock::time_provider::ClockTimeProvider;
use crate::desktop::application::{
    DesktopApplication, DesktopApplicationError, DesktopApplicationWakeProvider,
    DesktopApplicationWork,
};
use crate::desktop::dto::{DesktopBootstrapSnapshot, DesktopEnumEvent, DesktopEnumValue};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopError, DesktopFailure};
use crate::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileHandleError, DesktopFileHandleId,
    DesktopFileHandleRegistry, DesktopFileHandleRequest,
};
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};
use crate::desktop::messages::DesktopEnumCommand;
use crate::desktop::stream::{DesktopEnumStreamClass, DesktopStream};

#[derive(Debug, Error)]
pub enum DesktopCoordinatorError {
    #[error(transparent)]
    Application(#[from] DesktopApplicationError),
    #[error(transparent)]
    Time(#[from] ClockDurationError),
    #[error(transparent)]
    Stream(#[from] DesktopError),
    #[error(transparent)]
    Dialog(#[from] DesktopFileHandleError),
    #[error("Application shutdown is incomplete: {source}")]
    Shutdown {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

pub trait DesktopCoordinatorDialogProvider: DesktopFileDialogProvider {
    fn request_cancel(&self) -> Result<(), DesktopFileHandleError>;
}

pub trait DesktopCoordinatorLifecycleProvider: Send {
    fn shutdown(
        &mut self,
        deadline: Instant,
    ) -> impl Future<Output = Result<(), DesktopCoordinatorError>> + Send;
}

pub struct DesktopCoordinatorServices<P, L> {
    provider: Arc<P>,
    executor: Arc<CapacityExecutor>,
    lifecycle: L,
}

impl<P, L> DesktopCoordinatorServices<P, L> {
    pub fn new(provider: Arc<P>, executor: Arc<CapacityExecutor>, lifecycle: L) -> Self {
        Self {
            provider,
            executor,
            lifecycle,
        }
    }
}

struct DesktopCoordinatorFiles<P> {
    provider: Arc<P>,
    registry: DesktopFileHandleRegistry,
    executor: Arc<CapacityExecutor>,
}

type DesktopCoordinatorSelectionFuture<P> = Pin<
    Box<
        dyn Future<
                Output = (
                    DesktopCoordinatorFiles<P>,
                    Result<Option<DesktopFileHandleId>, DesktopFileHandleError>,
                ),
            > + Send,
    >,
>;

struct DesktopCoordinatorSelection<P> {
    work: DesktopApplicationWork,
    selection: DesktopCoordinatorSelectionFuture<P>,
}

pub struct DesktopCoordinator<C, W, P, L>
where
    C: ClockTimeProvider,
    W: DesktopApplicationWakeProvider,
{
    application: DesktopApplication<C, W>,
    clock: Arc<C>,
    snapshot: DesktopBootstrapSnapshot,
    files: Option<DesktopCoordinatorFiles<P>>,
    pending_selection: Option<DesktopCoordinatorSelection<P>>,
    provider: Arc<P>,
    controls: Vec<DesktopApplicationWork>,
    stream: Option<DesktopStream<DesktopEnumEvent>>,
    subscription: Option<DesktopRequestId>,
    lifecycle: L,
    cancellation_failure: Option<DesktopFileHandleError>,
    shutdown_failure: Option<DesktopCoordinatorError>,
    reported_deadlines: HashSet<DesktopRequestId>,
}

impl<C, W, P, L> DesktopCoordinator<C, W, P, L>
where
    C: ClockTimeProvider + 'static,
    W: DesktopApplicationWakeProvider + 'static,
    P: DesktopCoordinatorDialogProvider + 'static,
    L: DesktopCoordinatorLifecycleProvider,
{
    pub const SHUTDOWN: Duration = Duration::from_secs(3);

    pub fn new(
        application: DesktopApplication<C, W>,
        clock: Arc<C>,
        snapshot: DesktopBootstrapSnapshot,
        services: DesktopCoordinatorServices<P, L>,
    ) -> Self {
        let DesktopCoordinatorServices {
            provider,
            executor,
            lifecycle,
        } = services;
        Self {
            application,
            clock,
            snapshot,
            files: Some(DesktopCoordinatorFiles {
                provider: Arc::clone(&provider),
                registry: DesktopFileHandleRegistry::new(),
                executor,
            }),
            pending_selection: None,
            provider,
            controls: Vec::new(),
            stream: None,
            subscription: None,
            lifecycle,
            cancellation_failure: None,
            shutdown_failure: None,
            reported_deadlines: HashSet::new(),
        }
    }

    pub fn snapshot(&self) -> &DesktopBootstrapSnapshot {
        &self.snapshot
    }

    pub async fn run(&mut self) -> Result<(), DesktopCoordinatorError> {
        loop {
            if self.complete_controls().await? {
                return Ok(());
            }
            let mut processed = 0;
            while processed < Self::COMMAND_BATCH {
                let Some(work) = Self::retry(|| self.application.try_next()).await? else {
                    break;
                };
                self.dispatch(work).await?;
                processed += 1;
            }
            if self.complete_controls().await? {
                return Ok(());
            }
            self.report_pending_controls().await?;
            let deadline = self.next_deadline()?;
            tokio::select! {
                biased;
                selected = Self::selection_ready(&mut self.pending_selection), if self.pending_selection.is_some() => {
                    self.finish_selection(selected).await?;
                }
                () = Self::wait_deadline(deadline) => self.check_stream()?,
                () = self.application.ready() => {}
                () = tokio::task::yield_now(), if processed == Self::COMMAND_BATCH => {}
            }
        }
    }

    const COMMAND_BATCH: usize = 32;

    pub fn cancellation_failure(&self) -> Option<&DesktopFileHandleError> {
        self.cancellation_failure.as_ref()
    }

    pub fn shutdown_failure(&self) -> Option<&DesktopCoordinatorError> {
        self.shutdown_failure.as_ref()
    }

    async fn report_pending_controls(&mut self) -> Result<(), DesktopCoordinatorError> {
        let now = self.clock.now()?;
        for control in &self.controls {
            if !self
                .reported_deadlines
                .contains(&control.receipt().request_id())
                && control
                    .receipt()
                    .deadline()
                    .is_some_and(|deadline| now >= deadline)
            {
                Self::retry(|| self.application.report_pending_control(control, DesktopFailure::new(
                    DesktopEnumFailureCode::ResourceRefusal,
                    "Close the native file dialog to finish this action. Its operation remains owned until the dialog closes.".to_owned(),
                    true,
                ))).await?;
                self.reported_deadlines
                    .insert(control.receipt().request_id());
            }
        }
        Ok(())
    }

    async fn retry<T>(
        mut operation: impl FnMut() -> Result<T, DesktopApplicationError>,
    ) -> Result<T, DesktopApplicationError> {
        loop {
            match operation() {
                Err(DesktopApplicationError::Busy) => tokio::task::yield_now().await,
                result => return result,
            }
        }
    }

    async fn dispatch(
        &mut self,
        work: DesktopApplicationWork,
    ) -> Result<(), DesktopCoordinatorError> {
        match work.request().command().operation().clone() {
            DesktopEnumCommand::Bootstrap => {
                self.complete(&work, DesktopEnumValue::Bootstrap(self.snapshot.clone()))
                    .await?;
            }
            DesktopEnumCommand::FileSelect { owner, purpose } => {
                if work.request().command().expected_revision() != Some(DesktopRevision::ZERO) {
                    self.fail(
                        &work,
                        DesktopFailure::new(
                            DesktopEnumFailureCode::StaleRevision,
                            "The file-selection revision is stale".to_owned(),
                            true,
                        ),
                    )
                    .await?;
                } else if let Some(mut files) = self.files.take() {
                    let receipt = work.receipt().clone();
                    let clock = Arc::clone(&self.clock);
                    let selection = Box::pin(async move {
                        let outcome = async {
                            if receipt.is_cancellation_requested() {
                                return Ok(None);
                            }
                            let Some(path) = files.provider.select(purpose).await? else {
                                return Ok(None);
                            };
                            if receipt.is_cancellation_requested() {
                                return Ok(None);
                            }
                            let request = DesktopFileHandleRequest::new(
                                owner.file_handle_owner(),
                                purpose,
                                clock.now()?,
                            );
                            files
                                .registry
                                .register_selection(path, request, &files.executor)
                                .await
                                .map(Some)
                        }
                        .await;
                        (files, outcome)
                    });
                    self.pending_selection = Some(DesktopCoordinatorSelection { work, selection });
                } else {
                    self.fail(
                        &work,
                        DesktopFailure::new(
                            DesktopEnumFailureCode::ResourceRefusal,
                            "Close the existing native file dialog before opening another"
                                .to_owned(),
                            true,
                        ),
                    )
                    .await?;
                }
            }
            DesktopEnumCommand::StreamSubscribe => {
                Self::retry(|| self.application.clear_events()).await?;
                let subscription = DesktopRequestId::new();
                self.stream = Some(DesktopStream::new(subscription, self.clock.now()?));
                self.subscription = Some(subscription);
                self.complete(
                    &work,
                    DesktopEnumValue::StreamSubscribe {
                        subscription_id: subscription,
                        sequence: DesktopRevision::ZERO,
                        snapshot: self.snapshot.clone(),
                    },
                )
                .await?;
            }
            DesktopEnumCommand::StreamAcknowledge {
                subscription_id,
                sequence,
            } => {
                let result = self.acknowledge(subscription_id, sequence);
                self.complete_stream(&work, result).await?;
            }
            DesktopEnumCommand::StreamResync { subscription_id } => {
                let result = self.resynchronize(subscription_id).await;
                self.complete_stream(&work, result).await?;
            }
            DesktopEnumCommand::Stop { .. }
            | DesktopEnumCommand::Pause { .. }
            | DesktopEnumCommand::Quit => {
                if self
                    .pending_selection
                    .as_ref()
                    .is_some_and(|selection| selection.work.is_cancellation_requested())
                    && let Err(error) = self.provider.request_cancel()
                {
                    self.cancellation_failure = Some(error);
                }
                self.controls.push(work);
            }
        }
        Ok(())
    }

    async fn complete_controls(&mut self) -> Result<bool, DesktopCoordinatorError> {
        if self.application.take_quit_retry() {
            self.shutdown_failure = None;
        }
        self.controls.sort_by_key(|work| {
            matches!(
                work.request().command().operation(),
                DesktopEnumCommand::Quit
            )
        });
        let mut index = 0;
        while index < self.controls.len() {
            let is_quit = matches!(
                self.controls[index].request().command().operation(),
                DesktopEnumCommand::Quit
            );
            if is_quit {
                if self.pending_selection.is_some() || self.shutdown_failure.is_some() {
                    index += 1;
                    continue;
                }
                if let Err(error) = self
                    .lifecycle
                    .shutdown(Instant::now() + Self::SHUTDOWN)
                    .await
                {
                    let notice = DesktopFailure::new(
                        DesktopEnumFailureCode::ResourceRefusal,
                        format!(
                            "{error}. Services remain owned. Choose Retry quit to try cleanup again."
                        ),
                        true,
                    );
                    self.shutdown_failure = Some(error);
                    Self::retry(|| {
                        self.application
                            .report_pending_control(&self.controls[index], notice.clone())
                    })
                    .await?;
                    self.reported_deadlines
                        .insert(self.controls[index].receipt().request_id());
                    index += 1;
                    continue;
                }
            }
            match Self::retry(|| self.application.complete_control(&self.controls[index])).await {
                Ok(()) => {
                    self.reported_deadlines
                        .remove(&self.controls[index].receipt().request_id());
                    self.controls.remove(index);
                    if is_quit {
                        return Ok(true);
                    }
                }
                Err(DesktopApplicationError::CancellationUnacknowledged) => index += 1,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(false)
    }

    async fn selection_ready(
        pending: &mut Option<DesktopCoordinatorSelection<P>>,
    ) -> (
        DesktopCoordinatorFiles<P>,
        Result<Option<DesktopFileHandleId>, DesktopFileHandleError>,
    ) {
        match pending {
            Some(selection) => selection.selection.as_mut().await,
            None => std::future::pending().await,
        }
    }

    async fn finish_selection(
        &mut self,
        (mut files, outcome): (
            DesktopCoordinatorFiles<P>,
            Result<Option<DesktopFileHandleId>, DesktopFileHandleError>,
        ),
    ) -> Result<(), DesktopCoordinatorError> {
        let Some(selection) = self.pending_selection.take() else {
            return Err(DesktopApplicationError::UnknownWork.into());
        };
        let work = selection.work;
        if work.is_cancellation_requested() {
            match outcome {
                Ok(Some(handle)) => {
                    files.registry.revoke(handle);
                }
                Ok(None) => {}
                Err(error) => self.cancellation_failure = Some(error),
            }
            self.files = Some(files);
            Self::retry(|| self.application.acknowledge_stopped(&work)).await?;
            return Ok(());
        }
        self.files = Some(files);
        match outcome {
            Ok(handle) => match Self::retry(|| self.application.begin_commit(&work)).await {
                Ok(()) => {
                    self.complete(&work, DesktopEnumValue::FileSelect(handle))
                        .await?
                }
                Err(DesktopApplicationError::CancellationRequested) => {
                    if let Some(handle) = handle
                        && let Some(files) = &mut self.files
                    {
                        files.registry.revoke(handle);
                    }
                    Self::retry(|| self.application.acknowledge_stopped(&work)).await?;
                }
                Err(error) => return Err(error.into()),
            },
            Err(error) => {
                self.fail(
                    &work,
                    DesktopFailure::new(
                        DesktopEnumFailureCode::FileUnavailable,
                        error.to_string(),
                        true,
                    ),
                )
                .await?
            }
        }
        Ok(())
    }

    async fn complete(
        &mut self,
        work: &DesktopApplicationWork,
        value: DesktopEnumValue,
    ) -> Result<(), DesktopCoordinatorError> {
        match Self::retry(|| self.application.complete_value(work, value.clone(), None)).await {
            Ok(()) => Ok(()),
            Err(DesktopApplicationError::CancellationRequested) => {
                Self::retry(|| self.application.acknowledge_stopped(work)).await?;
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn fail(
        &mut self,
        work: &DesktopApplicationWork,
        failure: DesktopFailure,
    ) -> Result<(), DesktopCoordinatorError> {
        match Self::retry(|| self.application.complete_failure(work, failure.clone())).await {
            Ok(()) => Ok(()),
            Err(DesktopApplicationError::CancellationRequested) => {
                Self::retry(|| self.application.acknowledge_stopped(work)).await?;
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    fn acknowledge(
        &mut self,
        subscription: DesktopRequestId,
        sequence: DesktopRevision,
    ) -> Result<DesktopEnumValue, DesktopCoordinatorError> {
        let now = self.clock.now()?;
        self.stream_for(subscription)?.acknowledge(sequence, now)?;
        Ok(DesktopEnumValue::StreamAcknowledge)
    }

    async fn resynchronize(
        &mut self,
        subscription: DesktopRequestId,
    ) -> Result<DesktopEnumValue, DesktopCoordinatorError> {
        let now = self.clock.now()?;
        let snapshot = self.snapshot.clone();
        let event = self
            .stream_for(subscription)?
            .resynchronize(DesktopEnumEvent::Snapshot(snapshot.clone()), now)?;
        let sequence = event.sequence();
        Self::retry(|| self.application.clear_events()).await?;
        Self::retry(|| self.application.publish_event(event.clone())).await?;
        Ok(DesktopEnumValue::StreamResync {
            subscription_id: subscription,
            sequence,
            snapshot,
        })
    }

    async fn complete_stream(
        &mut self,
        work: &DesktopApplicationWork,
        result: Result<DesktopEnumValue, DesktopCoordinatorError>,
    ) -> Result<(), DesktopCoordinatorError> {
        match result {
            Ok(value) => self.complete(work, value).await,
            Err(DesktopCoordinatorError::Stream(error)) => self.fail(work, error.into()).await,
            Err(error) => Err(error),
        }
    }

    fn stream_for(
        &mut self,
        subscription: DesktopRequestId,
    ) -> Result<&mut DesktopStream<DesktopEnumEvent>, DesktopCoordinatorError> {
        if self.subscription != Some(subscription) {
            return Err(DesktopError::ResyncRequired.into());
        }
        self.stream
            .as_mut()
            .ok_or(DesktopError::ResyncRequired.into())
    }

    fn next_deadline(&self) -> Result<Option<Instant>, DesktopCoordinatorError> {
        let stream_deadline = self
            .stream
            .as_ref()
            .map(DesktopStream::acknowledgement_deadline)
            .transpose()?
            .flatten();
        let control_deadline = self
            .controls
            .iter()
            .filter(|control| {
                !self
                    .reported_deadlines
                    .contains(&control.receipt().request_id())
            })
            .filter_map(|control| control.receipt().deadline())
            .min();
        let Some(deadline) = stream_deadline.into_iter().chain(control_deadline).min() else {
            return Ok(None);
        };
        let now = self.clock.now()?;
        Ok(Some(
            Instant::now()
                + if now >= deadline {
                    Duration::ZERO
                } else {
                    Duration::from_nanos(deadline.elapsed_since(now)?.as_nanos())
                },
        ))
    }

    async fn wait_deadline(deadline: Option<Instant>) {
        match deadline {
            Some(deadline) => tokio::time::sleep_until(deadline).await,
            None => std::future::pending().await,
        }
    }

    fn check_stream(&mut self) -> Result<(), DesktopCoordinatorError> {
        let now = self.clock.now()?;
        if let Some(stream) = &mut self.stream {
            match stream.check_health(now) {
                Ok(()) | Err(DesktopError::ResyncRequired) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    pub fn publish_snapshot(
        &mut self,
        snapshot: DesktopBootstrapSnapshot,
    ) -> Result<(), DesktopCoordinatorError> {
        self.snapshot = snapshot;
        let now = self.clock.now()?;
        if let Some(stream) = &mut self.stream
            && let Some(event) = stream.publish(
                DesktopEnumEvent::Snapshot(self.snapshot.clone()),
                DesktopEnumStreamClass::Snapshot,
                now,
            )?
        {
            self.application.publish_event(event)?;
        }
        Ok(())
    }
}
