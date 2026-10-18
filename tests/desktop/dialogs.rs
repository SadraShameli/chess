use std::future::Future;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll, Waker};
use std::thread::ThreadId;

use chess::capacity::budget::{CapacityBudget, CapacityBudgetError};
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::ClockInstant;
use chess::desktop::application::{DesktopApplicationWakeError, DesktopApplicationWakeProvider};
use chess::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileEnumPurpose, DesktopFileHandleError,
    DesktopFileHandleOwner, DesktopFileHandleRegistry, DesktopFileHandleRequest,
};
use chess::desktop::native::dialogs::{
    DesktopNativeDialogs, DesktopNativeDialogsBackend, DesktopNativeDialogsEnumCancellation,
    DesktopNativeDialogsEnumCompletion, DesktopNativeDialogsEnumOutcome,
    DesktopNativeDialogsEnumState, DesktopNativeDialogsError, DesktopNativeDialogsOperation,
    DesktopNativeDialogsPort,
};
use thiserror::Error;
use tokio::sync::oneshot;

#[derive(Debug, Error)]
enum DesktopNativeDialogsFixtureError {
    #[error("The controlled native dialog has no remaining completion sender")]
    MissingSender,
    #[error("The controlled native dialog lost its result owner")]
    ResultOwnerClosed,
    #[error("The controlled native dialog observations are unavailable")]
    Observations,
}

#[derive(Default)]
struct DesktopNativeDialogsFixtureObservations {
    purposes: Mutex<Vec<DesktopFileEnumPurpose>>,
    thread: OnceLock<ThreadId>,
    parents: AtomicUsize,
    futures: AtomicUsize,
    completions: AtomicUsize,
}

impl DesktopNativeDialogsFixtureObservations {
    fn purposes(&self) -> Result<Vec<DesktopFileEnumPurpose>, DesktopNativeDialogsFixtureError> {
        self.purposes
            .try_lock()
            .map(|purposes| purposes.clone())
            .map_err(|_error| DesktopNativeDialogsFixtureError::Observations)
    }

    fn assert_owned(&self, count: usize) {
        assert_eq!(self.parents.load(Ordering::SeqCst), count);
        assert_eq!(self.futures.load(Ordering::SeqCst), count);
    }
}

struct DesktopNativeDialogsFixtureParent(Arc<DesktopNativeDialogsFixtureObservations>);

impl DesktopNativeDialogsFixtureParent {
    fn new(observations: Arc<DesktopNativeDialogsFixtureObservations>) -> Self {
        observations.parents.fetch_add(1, Ordering::SeqCst);
        Self(observations)
    }
}

impl Drop for DesktopNativeDialogsFixtureParent {
    fn drop(&mut self) {
        self.0.parents.fetch_sub(1, Ordering::SeqCst);
    }
}

struct DesktopNativeDialogsFixtureFuture {
    result: oneshot::Receiver<Option<PathBuf>>,
    observations: Arc<DesktopNativeDialogsFixtureObservations>,
}

impl DesktopNativeDialogsFixtureFuture {
    fn new(
        result: oneshot::Receiver<Option<PathBuf>>,
        observations: Arc<DesktopNativeDialogsFixtureObservations>,
    ) -> Self {
        observations.futures.fetch_add(1, Ordering::SeqCst);
        Self {
            result,
            observations,
        }
    }
}

impl Future for DesktopNativeDialogsFixtureFuture {
    type Output = Result<Option<PathBuf>, DesktopNativeDialogsError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let current = self.get_mut();
        match Pin::new(&mut current.result).poll(context) {
            Poll::Ready(Ok(selection)) => {
                current
                    .observations
                    .completions
                    .fetch_add(1, Ordering::SeqCst);
                Poll::Ready(Ok(selection))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error.into())),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for DesktopNativeDialogsFixtureFuture {
    fn drop(&mut self) {
        self.observations.futures.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy)]
enum DesktopNativeDialogsFixtureEnumStart {
    Open,
    Reject,
}

struct DesktopNativeDialogsFixtureBackend {
    observations: Arc<DesktopNativeDialogsFixtureObservations>,
    result: Option<oneshot::Receiver<Option<PathBuf>>>,
    start: DesktopNativeDialogsFixtureEnumStart,
    cancel_on_open: Arc<OnceLock<DesktopNativeDialogsPort<DesktopNativeDialogsFixtureWake>>>,
}

impl DesktopNativeDialogsBackend for DesktopNativeDialogsFixtureBackend {
    type Parent = DesktopNativeDialogsFixtureParent;

    fn open(
        &mut self,
        purpose: DesktopFileEnumPurpose,
    ) -> Result<DesktopNativeDialogsOperation<Self::Parent>, DesktopNativeDialogsError> {
        self.observations
            .purposes
            .try_lock()
            .map_err(|_error| DesktopNativeDialogsError::Mailbox)?
            .push(purpose);
        self.observations
            .thread
            .get_or_init(|| std::thread::current().id());
        match self.start {
            DesktopNativeDialogsFixtureEnumStart::Open => {}
            DesktopNativeDialogsFixtureEnumStart::Reject => {
                return Err(DesktopNativeDialogsError::WrongThread);
            }
        }
        let result = self
            .result
            .take()
            .ok_or(DesktopNativeDialogsError::Lifecycle)?;
        let operation = DesktopNativeDialogsOperation::new(
            DesktopNativeDialogsFixtureParent::new(Arc::clone(&self.observations)),
            DesktopNativeDialogsFixtureFuture::new(result, Arc::clone(&self.observations)),
        );
        if let Some(port) = self.cancel_on_open.get() {
            assert_eq!(
                port.try_cancel()?,
                DesktopNativeDialogsEnumCancellation::DeferredUntilNativeClose
            );
        }
        Ok(operation)
    }
}

#[derive(Default)]
struct DesktopNativeDialogsFixtureWakeState {
    calls: AtomicUsize,
    unavailable: AtomicBool,
}

#[derive(Clone, Default)]
struct DesktopNativeDialogsFixtureWake(Arc<DesktopNativeDialogsFixtureWakeState>);

impl DesktopApplicationWakeProvider for DesktopNativeDialogsFixtureWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        if self.0.unavailable.load(Ordering::SeqCst) {
            return Err(DesktopApplicationWakeError::Unavailable);
        }
        Ok(())
    }
}

struct DesktopNativeDialogsFixture {
    host: DesktopNativeDialogs<DesktopNativeDialogsFixtureBackend, DesktopNativeDialogsFixtureWake>,
    port: DesktopNativeDialogsPort<DesktopNativeDialogsFixtureWake>,
    budget: CapacityBudget,
    observations: Arc<DesktopNativeDialogsFixtureObservations>,
    result: Option<oneshot::Sender<Option<PathBuf>>>,
    wake: DesktopNativeDialogsFixtureWake,
    cancel_on_open: Arc<OnceLock<DesktopNativeDialogsPort<DesktopNativeDialogsFixtureWake>>>,
}

impl DesktopNativeDialogsFixture {
    fn new() -> Result<Self, CapacityBudgetError> {
        Self::with_start(DesktopNativeDialogsFixtureEnumStart::Open)
    }

    fn with_start(
        start: DesktopNativeDialogsFixtureEnumStart,
    ) -> Result<Self, CapacityBudgetError> {
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let observations = Arc::new(DesktopNativeDialogsFixtureObservations::default());
        let (result, receiver) = oneshot::channel();
        let cancel_on_open = Arc::new(OnceLock::new());
        let backend = DesktopNativeDialogsFixtureBackend {
            observations: Arc::clone(&observations),
            result: Some(receiver),
            start,
            cancel_on_open: Arc::clone(&cancel_on_open),
        };
        let wake = DesktopNativeDialogsFixtureWake::default();
        let (host, port) = DesktopNativeDialogs::new(backend, budget.clone(), wake.clone());
        Ok(Self {
            host,
            port,
            budget,
            observations,
            result: Some(result),
            wake,
            cancel_on_open,
        })
    }

    fn complete(
        &mut self,
        selection: Option<PathBuf>,
    ) -> Result<(), DesktopNativeDialogsFixtureError> {
        self.result
            .take()
            .ok_or(DesktopNativeDialogsFixtureError::MissingSender)?
            .send(selection)
            .map_err(|_selection| DesktopNativeDialogsFixtureError::ResultOwnerClosed)
    }
}

#[tokio::test]
async fn cancellation_during_native_creation_keeps_the_started_operation_owned()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopNativeDialogsFixture::new()?;
    fixture.cancel_on_open.get_or_init(|| fixture.port.clone());
    let request = fixture.port.try_request(DesktopFileEnumPurpose::ReadFile)?;
    assert_eq!(
        fixture.host.poll()?,
        DesktopNativeDialogsEnumState::CancellationPending
    );
    fixture.observations.assert_owned(1);
    assert_eq!(fixture.budget.available(), 0);
    fixture.host.close_admission()?;
    assert!(!fixture.host.can_shutdown()?);
    fixture.complete(Some(PathBuf::from("cancelled-during-open.pgn")))?;
    fixture.host.poll()?;
    assert_eq!(
        request.await?,
        DesktopNativeDialogsEnumOutcome::CancelledAfterNativeClose
    );
    fixture.observations.assert_owned(0);
    assert!(fixture.host.can_shutdown()?);
    Ok(())
}

#[tokio::test]
async fn native_future_failure_releases_ownership_without_faking_no_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopNativeDialogsFixture::new()?;
    let request = fixture.port.try_request(DesktopFileEnumPurpose::ReadFile)?;
    fixture.host.poll()?;
    drop(fixture.result.take());
    fixture.host.poll()?;
    assert!(matches!(
        request.await,
        Err(DesktopNativeDialogsError::Result(_))
    ));
    fixture.observations.assert_owned(0);
    assert_eq!(fixture.budget.available(), 1);
    assert_eq!(
        fixture.port.try_completion()?,
        Some(DesktopNativeDialogsEnumCompletion::Failed)
    );
    Ok(())
}

#[tokio::test]
async fn one_dialog_bounds_queued_and_active_admission() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopNativeDialogsFixture::new()?;
    let request = fixture.port.try_request(DesktopFileEnumPurpose::ReadFile)?;
    assert_eq!(fixture.budget.available(), 0);
    assert!(matches!(
        fixture.port.try_request(DesktopFileEnumPurpose::WriteFile),
        Err(DesktopNativeDialogsError::Busy)
    ));
    assert_eq!(fixture.host.poll()?, DesktopNativeDialogsEnumState::Active);
    fixture.observations.assert_owned(1);
    assert!(matches!(
        fixture.port.try_request(DesktopFileEnumPurpose::Directory),
        Err(DesktopNativeDialogsError::Busy)
    ));
    assert_eq!(fixture.observations.purposes()?.len(), 1);
    fixture.complete(Some(PathBuf::from("selected.pgn")))?;
    assert_eq!(
        fixture.host.poll()?,
        DesktopNativeDialogsEnumState::Completed
    );
    assert_eq!(
        request.await?,
        DesktopNativeDialogsEnumOutcome::Selected(PathBuf::from("selected.pgn"))
    );
    fixture.observations.assert_owned(0);
    assert_eq!(fixture.budget.available(), 1);
    Ok(())
}

#[tokio::test]
async fn cancellation_before_open_never_creates_a_native_operation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopNativeDialogsFixture::new()?;
    let request = fixture.port.try_request(DesktopFileEnumPurpose::ReadFile)?;
    assert_eq!(
        request.cancel()?,
        DesktopNativeDialogsEnumCancellation::CancelledBeforeOpen
    );
    assert_eq!(
        fixture.port.try_state()?,
        DesktopNativeDialogsEnumState::CancelledBeforeOpen
    );
    assert_eq!(fixture.budget.available(), 0);
    fixture.host.poll()?;
    assert_eq!(
        request.await?,
        DesktopNativeDialogsEnumOutcome::CancelledBeforeOpen
    );
    assert!(fixture.observations.purposes()?.is_empty());
    fixture.observations.assert_owned(0);
    assert_eq!(fixture.budget.available(), 1);
    assert_eq!(
        fixture.port.try_completion()?,
        Some(DesktopNativeDialogsEnumCompletion::CancelledBeforeOpen)
    );
    Ok(())
}

#[tokio::test]
async fn active_cancellation_retains_native_ownership_until_actual_close()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopNativeDialogsFixture::new()?;
    let mut request = fixture.port.try_request(DesktopFileEnumPurpose::ReadFile)?;
    fixture.host.poll()?;
    assert_eq!(
        request.cancel()?,
        DesktopNativeDialogsEnumCancellation::DeferredUntilNativeClose
    );
    assert_eq!(
        fixture.host.close_admission()?,
        DesktopNativeDialogsEnumCancellation::DeferredUntilNativeClose
    );
    assert_eq!(
        fixture.host.poll()?,
        DesktopNativeDialogsEnumState::CancellationPending
    );
    assert!(!fixture.host.can_shutdown()?);
    fixture.observations.assert_owned(1);
    assert_eq!(fixture.budget.available(), 0);
    assert_eq!(fixture.port.try_completion()?, None);
    assert!(
        Pin::new(&mut request)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    fixture.complete(Some(PathBuf::from("must-not-publish.pgn")))?;
    fixture.host.poll()?;
    assert_eq!(
        request.await?,
        DesktopNativeDialogsEnumOutcome::CancelledAfterNativeClose
    );
    assert!(fixture.host.can_shutdown()?);
    fixture.observations.assert_owned(0);
    assert_eq!(fixture.budget.available(), 1);
    assert_eq!(fixture.observations.completions.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture.port.try_completion()?,
        Some(DesktopNativeDialogsEnumCompletion::CancelledAfterNativeClose)
    );
    Ok(())
}

#[test]
fn dropping_an_active_request_keeps_the_owned_native_future_alive()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopNativeDialogsFixture::new()?;
    let request = fixture.port.try_request(DesktopFileEnumPurpose::ReadFile)?;
    fixture.host.poll()?;
    drop(request);
    assert_eq!(
        fixture.port.try_state()?,
        DesktopNativeDialogsEnumState::CancellationPending
    );
    fixture.host.close_admission()?;
    assert!(!fixture.host.can_shutdown()?);
    fixture.observations.assert_owned(1);
    assert_eq!(fixture.budget.available(), 0);
    fixture.complete(Some(PathBuf::from("abandoned-selection.pgn")))?;
    fixture.host.poll()?;
    assert!(fixture.host.can_shutdown()?);
    fixture.observations.assert_owned(0);
    assert_eq!(fixture.budget.available(), 1);
    assert_eq!(
        fixture.port.try_completion()?,
        Some(DesktopNativeDialogsEnumCompletion::CancelledAfterNativeClose)
    );
    assert!(fixture.port.background_failure().is_none());
    Ok(())
}

#[tokio::test]
async fn native_none_remains_no_selection_without_inferred_user_cancellation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopNativeDialogsFixture::new()?;
    let request = fixture.port.try_request(DesktopFileEnumPurpose::ReadFile)?;
    fixture.host.poll()?;
    fixture.complete(None)?;
    fixture.host.poll()?;
    assert_eq!(request.await?, DesktopNativeDialogsEnumOutcome::NoSelection);
    assert_eq!(
        fixture.port.try_completion()?,
        Some(DesktopNativeDialogsEnumCompletion::NoSelection)
    );
    assert_eq!(
        fixture.host.close_admission()?,
        DesktopNativeDialogsEnumCancellation::AlreadyCompleted
    );
    assert!(fixture.host.can_shutdown()?);
    Ok(())
}

#[tokio::test]
async fn closing_admission_cancels_queued_work_and_rejects_new_work()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopNativeDialogsFixture::new()?;
    assert_eq!(
        fixture.port.try_state()?,
        DesktopNativeDialogsEnumState::Idle
    );
    assert_eq!(
        fixture.port.try_cancel()?,
        DesktopNativeDialogsEnumCancellation::Idle
    );
    let request = fixture.port.try_request(DesktopFileEnumPurpose::ReadFile)?;
    assert_eq!(
        fixture.host.close_admission()?,
        DesktopNativeDialogsEnumCancellation::CancelledBeforeOpen
    );
    assert!(!fixture.host.can_shutdown()?);
    assert!(matches!(
        fixture.port.try_request(DesktopFileEnumPurpose::ReadFile),
        Err(DesktopNativeDialogsError::Closed)
    ));
    fixture.host.poll()?;
    assert_eq!(
        request.await?,
        DesktopNativeDialogsEnumOutcome::CancelledBeforeOpen
    );
    assert!(fixture.host.can_shutdown()?);
    assert!(fixture.observations.purposes()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn every_registered_purpose_starts_on_the_owning_thread()
-> Result<(), Box<dyn std::error::Error>> {
    {
        use strum::IntoEnumIterator;
        assert!(DesktopFileEnumPurpose::iter().next().is_some());
    }
    for purpose in <DesktopFileEnumPurpose as strum::IntoEnumIterator>::iter() {
        let mut fixture = DesktopNativeDialogsFixture::new()?;
        let request = fixture.port.try_request(purpose)?;
        fixture.host.poll()?;
        assert_eq!(fixture.observations.purposes()?, vec![purpose]);
        assert_eq!(
            fixture.observations.thread.get(),
            Some(&std::thread::current().id())
        );
        fixture.complete(Some(PathBuf::from("native-selection")))?;
        fixture.host.poll()?;
        assert_eq!(
            request.await?,
            DesktopNativeDialogsEnumOutcome::Selected(PathBuf::from("native-selection"))
        );
        fixture.observations.assert_owned(0);
    }
    Ok(())
}

#[tokio::test]
async fn native_future_completion_wakes_the_injected_event_loop()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopNativeDialogsFixture::new()?;
    let request = fixture.port.try_request(DesktopFileEnumPurpose::ReadFile)?;
    assert_eq!(fixture.wake.0.calls.load(Ordering::SeqCst), 1);
    fixture.host.poll()?;
    fixture.complete(None)?;
    assert_eq!(fixture.wake.0.calls.load(Ordering::SeqCst), 2);
    fixture.host.poll()?;
    request.await?;
    assert_eq!(fixture.wake.0.calls.load(Ordering::SeqCst), 2);
    assert!(fixture.port.wake_failure().is_none());
    Ok(())
}

#[test]
fn wake_failure_preserves_queued_ownership_and_exposes_the_cause()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopNativeDialogsFixture::new()?;
    fixture.wake.0.unavailable.store(true, Ordering::SeqCst);
    assert!(matches!(
        fixture.port.try_request(DesktopFileEnumPurpose::ReadFile),
        Err(DesktopNativeDialogsError::Wake(_))
    ));
    assert_eq!(fixture.budget.available(), 0);
    assert_eq!(
        fixture.port.try_state()?,
        DesktopNativeDialogsEnumState::CancelledBeforeOpen
    );
    assert!(matches!(
        fixture.port.wake_failure(),
        Some(DesktopApplicationWakeError::Unavailable)
    ));
    assert!(matches!(
        fixture.port.background_failure(),
        Some(DesktopNativeDialogsError::Wake(_))
    ));
    fixture.host.poll()?;
    assert_eq!(fixture.budget.available(), 1);
    assert!(fixture.observations.purposes()?.is_empty());
    assert_eq!(
        fixture.port.try_completion()?,
        Some(DesktopNativeDialogsEnumCompletion::CancelledBeforeOpen)
    );
    Ok(())
}

#[tokio::test]
async fn backend_start_failure_preserves_typed_error_and_releases_capacity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture =
        DesktopNativeDialogsFixture::with_start(DesktopNativeDialogsFixtureEnumStart::Reject)?;
    let request = fixture.port.try_request(DesktopFileEnumPurpose::ReadFile)?;
    fixture.host.poll()?;
    assert!(matches!(
        request.await,
        Err(DesktopNativeDialogsError::WrongThread)
    ));
    assert_eq!(fixture.budget.available(), 1);
    fixture.observations.assert_owned(0);
    assert_eq!(
        fixture.port.try_completion()?,
        Some(DesktopNativeDialogsEnumCompletion::Failed)
    );
    Ok(())
}

#[test]
fn unavailable_capacity_refuses_work_before_native_creation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = DesktopNativeDialogsFixture::new()?;
    let reserved = fixture.budget.try_lease(1)?;
    assert!(matches!(
        fixture.port.try_request(DesktopFileEnumPurpose::ReadFile),
        Err(DesktopNativeDialogsError::Capacity(
            CapacityBudgetError::Unavailable
        ))
    ));
    assert_eq!(
        fixture.port.try_state()?,
        DesktopNativeDialogsEnumState::Idle
    );
    fixture.host.poll()?;
    assert!(fixture.observations.purposes()?.is_empty());
    drop(reserved);
    assert_eq!(fixture.budget.available(), 1);
    Ok(())
}

#[tokio::test]
async fn selected_path_enters_the_existing_opaque_handle_registry()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("selected.pgn");
    tokio::fs::write(&path, "selected game").await?;
    let mut fixture = DesktopNativeDialogsFixture::new()?;
    let executor = CapacityExecutor::new(fixture.budget.clone())?;
    let port = fixture.port.clone();
    let owner = DesktopFileHandleOwner::new();
    let stranger = DesktopFileHandleOwner::new();
    let now = ClockInstant::from_nanos(0);
    let mut registry = DesktopFileHandleRegistry::new();
    let handle = {
        let select = registry.select(
            &port,
            DesktopFileHandleRequest::new(owner, DesktopFileEnumPurpose::ReadFile, now),
            &executor,
        );
        let mut select = std::pin::pin!(select);
        assert!(
            select
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        fixture.host.poll()?;
        fixture.complete(Some(path))?;
        fixture.host.poll()?;
        select.await?
    }
    .ok_or("No opaque file handle was published")?;
    assert!(matches!(
        registry.consume(handle, stranger, DesktopFileEnumPurpose::ReadFile, now),
        Err(DesktopFileHandleError::WrongScope)
    ));
    assert!(matches!(
        registry.consume(handle, owner, DesktopFileEnumPurpose::WriteFile, now),
        Err(DesktopFileHandleError::WrongScope)
    ));
    let selected = registry.consume(handle, owner, DesktopFileEnumPurpose::ReadFile, now)?;
    assert_eq!(selected.into_source()?.metadata()?.len(), 13);
    assert!(matches!(
        registry.consume(handle, owner, DesktopFileEnumPurpose::ReadFile, now),
        Err(DesktopFileHandleError::UnknownHandle)
    ));
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn provider_boundary_preserves_native_failure_source()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture =
        DesktopNativeDialogsFixture::with_start(DesktopNativeDialogsFixtureEnumStart::Reject)?;
    let port = fixture.port.clone();
    let selection = port.select(DesktopFileEnumPurpose::ReadFile);
    let mut selection = std::pin::pin!(selection);
    assert!(
        selection
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    fixture.host.poll()?;
    let error = selection
        .await
        .err()
        .ok_or("A native backend failure was hidden")?;
    let DesktopFileHandleError::NativeDialog { source } = error else {
        return Err(error.into());
    };
    assert!(source.is::<DesktopNativeDialogsError>());
    assert!(source.to_string().contains("owning window thread"));
    Ok(())
}
