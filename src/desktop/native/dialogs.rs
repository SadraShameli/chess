use std::future::Future;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::ThreadId;

use bevy::window::{RawHandleWrapper, WindowWrapper};
use thiserror::Error;
use tokio::sync::oneshot;

use crate::capacity::budget::{CapacityBudget, CapacityBudgetError};
use crate::capacity::lease::CapacityLease;
use crate::desktop::application::{DesktopApplicationWakeError, DesktopApplicationWakeProvider};
use crate::desktop::coordinator::DesktopCoordinatorDialogProvider;
use crate::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileEnumPurpose, DesktopFileHandleError,
};

#[derive(Debug, Error)]
pub enum DesktopNativeDialogsError {
    #[error("A native file dialog is already queued or open")]
    Busy,
    #[error("The native file-dialog host has stopped accepting requests")]
    Closed,
    #[error("The native file-dialog mailbox is unavailable")]
    Mailbox,
    #[error("Native file dialogs must be created and polled on their owning window thread")]
    WrongThread,
    #[error("The native file-dialog lifecycle contains an invalid state")]
    Lifecycle,
    #[error("Cannot reserve native file-dialog capacity: {0}")]
    Capacity(#[from] CapacityBudgetError),
    #[error("Cannot retain the native dialog's owning window: {0}")]
    Window(#[from] winit::raw_window_handle::HandleError),
    #[error("The native file-dialog result owner closed unexpectedly: {0}")]
    Result(#[from] oneshot::error::RecvError),
    #[error("Cannot wake the native file-dialog host: {0}")]
    Wake(#[source] Arc<DesktopApplicationWakeError>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeDialogsEnumCancellation {
    Idle,
    CancelledBeforeOpen,
    DeferredUntilNativeClose,
    AlreadyCompleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeDialogsEnumState {
    Idle,
    Queued,
    Opening,
    Active,
    CancellationPending,
    CancelledBeforeOpen,
    Completing,
    Completed,
}

impl DesktopNativeDialogsEnumState {
    fn code(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Queued => 1,
            Self::Opening => 2,
            Self::Active => 3,
            Self::CancellationPending => 4,
            Self::CancelledBeforeOpen => 5,
            Self::Completing => 6,
            Self::Completed => 7,
        }
    }

    fn from_code(code: u8) -> Result<Self, DesktopNativeDialogsError> {
        match code {
            0 => Ok(Self::Idle),
            1 => Ok(Self::Queued),
            2 => Ok(Self::Opening),
            3 => Ok(Self::Active),
            4 => Ok(Self::CancellationPending),
            5 => Ok(Self::CancelledBeforeOpen),
            6 => Ok(Self::Completing),
            7 => Ok(Self::Completed),
            _ => Err(DesktopNativeDialogsError::Lifecycle),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNativeDialogsEnumCompletion {
    Selected,
    NoSelection,
    CancelledBeforeOpen,
    CancelledAfterNativeClose,
    Failed,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DesktopNativeDialogsEnumOutcome {
    Selected(PathBuf),
    NoSelection,
    CancelledBeforeOpen,
    CancelledAfterNativeClose,
}

impl DesktopNativeDialogsEnumOutcome {
    fn completion(&self) -> DesktopNativeDialogsEnumCompletion {
        match self {
            Self::Selected(_) => DesktopNativeDialogsEnumCompletion::Selected,
            Self::NoSelection => DesktopNativeDialogsEnumCompletion::NoSelection,
            Self::CancelledBeforeOpen => DesktopNativeDialogsEnumCompletion::CancelledBeforeOpen,
            Self::CancelledAfterNativeClose => {
                DesktopNativeDialogsEnumCompletion::CancelledAfterNativeClose
            }
        }
    }
}

pub struct DesktopNativeDialogsOperation<P> {
    parent: P,
    result:
        Pin<Box<dyn Future<Output = Result<Option<PathBuf>, DesktopNativeDialogsError>> + Send>>,
}

impl<P> DesktopNativeDialogsOperation<P> {
    pub fn new(
        parent: P,
        result: impl Future<Output = Result<Option<PathBuf>, DesktopNativeDialogsError>>
        + Send
        + 'static,
    ) -> Self {
        Self {
            parent,
            result: Box::pin(result),
        }
    }
}

pub trait DesktopNativeDialogsBackend {
    type Parent;

    fn open(
        &mut self,
        purpose: DesktopFileEnumPurpose,
    ) -> Result<DesktopNativeDialogsOperation<Self::Parent>, DesktopNativeDialogsError>;
}

pub struct DesktopNativeDialogsRfd {
    parent: RawHandleWrapper,
    builder: rfd::AsyncFileDialog,
    thread: ThreadId,
}

impl DesktopNativeDialogsRfd {
    pub fn new(
        window: &WindowWrapper<winit::window::Window>,
    ) -> Result<Self, DesktopNativeDialogsError> {
        Ok(Self {
            parent: RawHandleWrapper::new(window)?,
            builder: rfd::AsyncFileDialog::new().set_parent(&**window),
            thread: std::thread::current().id(),
        })
    }

    fn operation(
        parent: RawHandleWrapper,
        result: impl Future<Output = Option<rfd::FileHandle>> + Send + 'static,
    ) -> DesktopNativeDialogsOperation<RawHandleWrapper> {
        DesktopNativeDialogsOperation::new(parent, async move {
            Ok(result.await.map(|handle| handle.path().to_owned()))
        })
    }
}

impl DesktopNativeDialogsBackend for DesktopNativeDialogsRfd {
    type Parent = RawHandleWrapper;

    fn open(
        &mut self,
        purpose: DesktopFileEnumPurpose,
    ) -> Result<DesktopNativeDialogsOperation<Self::Parent>, DesktopNativeDialogsError> {
        if self.thread != std::thread::current().id() {
            return Err(DesktopNativeDialogsError::WrongThread);
        }
        let builder = self.builder.clone();
        let parent = self.parent.clone();
        let operation = match purpose {
            DesktopFileEnumPurpose::ReadFile => Self::operation(parent, builder.pick_file()),
            DesktopFileEnumPurpose::WriteFile => Self::operation(parent, builder.save_file()),
            DesktopFileEnumPurpose::Directory => Self::operation(parent, builder.pick_folder()),
        };
        Ok(operation)
    }
}

struct DesktopNativeDialogsControl {
    state: AtomicU8,
    completion: OnceLock<DesktopNativeDialogsEnumCompletion>,
}

impl DesktopNativeDialogsControl {
    fn new() -> Self {
        Self {
            state: AtomicU8::new(DesktopNativeDialogsEnumState::Queued.code()),
            completion: OnceLock::new(),
        }
    }

    fn state(&self) -> Result<DesktopNativeDialogsEnumState, DesktopNativeDialogsError> {
        DesktopNativeDialogsEnumState::from_code(self.state.load(Ordering::SeqCst))
    }

    fn transition(
        &self,
        expected: DesktopNativeDialogsEnumState,
        next: DesktopNativeDialogsEnumState,
    ) -> bool {
        self.state
            .compare_exchange(
                expected.code(),
                next.code(),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }

    fn cancel(&self) -> Result<DesktopNativeDialogsEnumCancellation, DesktopNativeDialogsError> {
        loop {
            let state = self.state()?;
            let (next, cancellation) = match state {
                DesktopNativeDialogsEnumState::Queued => (
                    DesktopNativeDialogsEnumState::CancelledBeforeOpen,
                    DesktopNativeDialogsEnumCancellation::CancelledBeforeOpen,
                ),
                DesktopNativeDialogsEnumState::Opening | DesktopNativeDialogsEnumState::Active => (
                    DesktopNativeDialogsEnumState::CancellationPending,
                    DesktopNativeDialogsEnumCancellation::DeferredUntilNativeClose,
                ),
                DesktopNativeDialogsEnumState::CancellationPending => {
                    return Ok(DesktopNativeDialogsEnumCancellation::DeferredUntilNativeClose);
                }
                DesktopNativeDialogsEnumState::CancelledBeforeOpen => {
                    return Ok(DesktopNativeDialogsEnumCancellation::CancelledBeforeOpen);
                }
                DesktopNativeDialogsEnumState::Completing
                | DesktopNativeDialogsEnumState::Completed => {
                    return Ok(DesktopNativeDialogsEnumCancellation::AlreadyCompleted);
                }
                DesktopNativeDialogsEnumState::Idle => {
                    return Err(DesktopNativeDialogsError::Lifecycle);
                }
            };
            if self.transition(state, next) {
                return Ok(cancellation);
            }
        }
    }

    fn finish(
        &self,
        selection: Option<PathBuf>,
    ) -> Result<DesktopNativeDialogsEnumOutcome, DesktopNativeDialogsError> {
        loop {
            let state = self.state()?;
            let cancelled = match state {
                DesktopNativeDialogsEnumState::Opening | DesktopNativeDialogsEnumState::Active => {
                    None
                }
                DesktopNativeDialogsEnumState::CancellationPending => {
                    Some(DesktopNativeDialogsEnumOutcome::CancelledAfterNativeClose)
                }
                DesktopNativeDialogsEnumState::CancelledBeforeOpen => {
                    Some(DesktopNativeDialogsEnumOutcome::CancelledBeforeOpen)
                }
                DesktopNativeDialogsEnumState::Idle
                | DesktopNativeDialogsEnumState::Queued
                | DesktopNativeDialogsEnumState::Completing
                | DesktopNativeDialogsEnumState::Completed => {
                    return Err(DesktopNativeDialogsError::Lifecycle);
                }
            };
            if self.transition(state, DesktopNativeDialogsEnumState::Completing) {
                let outcome = match cancelled {
                    Some(outcome) => outcome,
                    None => match selection {
                        Some(path) => DesktopNativeDialogsEnumOutcome::Selected(path),
                        None => DesktopNativeDialogsEnumOutcome::NoSelection,
                    },
                };
                self.completion
                    .set(outcome.completion())
                    .map_err(|_completion| DesktopNativeDialogsError::Lifecycle)?;
                self.state.store(
                    DesktopNativeDialogsEnumState::Completed.code(),
                    Ordering::SeqCst,
                );
                return Ok(outcome);
            }
        }
    }

    fn fail(&self) -> Result<(), DesktopNativeDialogsError> {
        self.completion
            .set(DesktopNativeDialogsEnumCompletion::Failed)
            .map_err(|_completion| DesktopNativeDialogsError::Lifecycle)?;
        self.state.store(
            DesktopNativeDialogsEnumState::Completed.code(),
            Ordering::SeqCst,
        );
        Ok(())
    }
}

struct DesktopNativeDialogsPending {
    purpose: DesktopFileEnumPurpose,
    lease: CapacityLease,
    control: Arc<DesktopNativeDialogsControl>,
    reply: oneshot::Sender<Result<DesktopNativeDialogsEnumOutcome, DesktopNativeDialogsError>>,
}

#[derive(Default)]
struct DesktopNativeDialogsMailbox {
    pending: Option<DesktopNativeDialogsPending>,
    current: Option<Arc<DesktopNativeDialogsControl>>,
}

struct DesktopNativeDialogsShared<W: DesktopApplicationWakeProvider> {
    mailbox: Mutex<DesktopNativeDialogsMailbox>,
    accepting: AtomicBool,
    budget: CapacityBudget,
    wake: W,
    wake_failure: OnceLock<Arc<DesktopApplicationWakeError>>,
    background_failure: OnceLock<Arc<DesktopNativeDialogsError>>,
}

impl<W: DesktopApplicationWakeProvider> DesktopNativeDialogsShared<W> {
    fn signal(&self) -> Result<(), DesktopNativeDialogsError> {
        self.wake.wake().map_err(|source| {
            let source = Arc::new(source);
            self.wake_failure.get_or_init(|| Arc::clone(&source));
            DesktopNativeDialogsError::Wake(source)
        })
    }

    fn mailbox(
        &self,
    ) -> Result<MutexGuard<'_, DesktopNativeDialogsMailbox>, DesktopNativeDialogsError> {
        self.mailbox.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => DesktopNativeDialogsError::Busy,
            TryLockError::Poisoned(_) => DesktopNativeDialogsError::Mailbox,
        })
    }

    fn control(
        &self,
    ) -> Result<Option<Arc<DesktopNativeDialogsControl>>, DesktopNativeDialogsError> {
        Ok(self.mailbox()?.current.as_ref().map(Arc::clone))
    }

    fn cancel(&self) -> Result<DesktopNativeDialogsEnumCancellation, DesktopNativeDialogsError> {
        match self.control()? {
            Some(control) => {
                let cancellation = control.cancel()?;
                self.signal()?;
                Ok(cancellation)
            }
            None => Ok(DesktopNativeDialogsEnumCancellation::Idle),
        }
    }
}

impl<W: DesktopApplicationWakeProvider + 'static> Wake for DesktopNativeDialogsShared<W> {
    fn wake(self: Arc<Self>) {
        if let Err(error) = self.signal() {
            drop(error);
        }
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if let Err(error) = self.signal() {
            drop(error);
        }
    }
}

pub struct DesktopNativeDialogsRequest<W: DesktopApplicationWakeProvider> {
    control: Arc<DesktopNativeDialogsControl>,
    shared: Arc<DesktopNativeDialogsShared<W>>,
    result: oneshot::Receiver<Result<DesktopNativeDialogsEnumOutcome, DesktopNativeDialogsError>>,
}

impl<W: DesktopApplicationWakeProvider> DesktopNativeDialogsRequest<W> {
    pub fn cancel(
        &self,
    ) -> Result<DesktopNativeDialogsEnumCancellation, DesktopNativeDialogsError> {
        let cancellation = self.control.cancel()?;
        match cancellation {
            DesktopNativeDialogsEnumCancellation::CancelledBeforeOpen
            | DesktopNativeDialogsEnumCancellation::DeferredUntilNativeClose => {
                self.shared.signal()?;
            }
            DesktopNativeDialogsEnumCancellation::Idle
            | DesktopNativeDialogsEnumCancellation::AlreadyCompleted => {}
        }
        Ok(cancellation)
    }
}

impl<W: DesktopApplicationWakeProvider> Future for DesktopNativeDialogsRequest<W> {
    type Output = Result<DesktopNativeDialogsEnumOutcome, DesktopNativeDialogsError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.get_mut().result).poll(context) {
            Poll::Ready(Ok(result)) => Poll::Ready(result),
            Poll::Ready(Err(error)) => Poll::Ready(Err(error.into())),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<W: DesktopApplicationWakeProvider> Drop for DesktopNativeDialogsRequest<W> {
    fn drop(&mut self) {
        if let Err(error) = self.cancel() {
            self.shared
                .background_failure
                .get_or_init(|| Arc::new(error));
        }
    }
}

pub struct DesktopNativeDialogsPort<W: DesktopApplicationWakeProvider> {
    shared: Arc<DesktopNativeDialogsShared<W>>,
}

impl<W: DesktopApplicationWakeProvider> Clone for DesktopNativeDialogsPort<W> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<W: DesktopApplicationWakeProvider> DesktopNativeDialogsPort<W> {
    pub fn try_request(
        &self,
        purpose: DesktopFileEnumPurpose,
    ) -> Result<DesktopNativeDialogsRequest<W>, DesktopNativeDialogsError> {
        if !self.shared.accepting.load(Ordering::SeqCst) {
            return Err(DesktopNativeDialogsError::Closed);
        }
        let mut mailbox = self.shared.mailbox()?;
        if !self.shared.accepting.load(Ordering::SeqCst) {
            return Err(DesktopNativeDialogsError::Closed);
        }
        if mailbox.pending.is_some()
            || mailbox.current.as_ref().is_some_and(|control| {
                control.state.load(Ordering::SeqCst)
                    != DesktopNativeDialogsEnumState::Completed.code()
            })
        {
            return Err(DesktopNativeDialogsError::Busy);
        }
        let lease = self.shared.budget.try_lease(1)?;
        let control = Arc::new(DesktopNativeDialogsControl::new());
        let (reply, result) = oneshot::channel();
        mailbox.current = Some(Arc::clone(&control));
        mailbox.pending = Some(DesktopNativeDialogsPending {
            purpose,
            lease,
            control: Arc::clone(&control),
            reply,
        });
        drop(mailbox);
        let request = DesktopNativeDialogsRequest {
            control,
            shared: Arc::clone(&self.shared),
            result,
        };
        self.shared.signal()?;
        Ok(request)
    }

    pub fn try_cancel(
        &self,
    ) -> Result<DesktopNativeDialogsEnumCancellation, DesktopNativeDialogsError> {
        self.shared.cancel()
    }

    pub fn try_state(&self) -> Result<DesktopNativeDialogsEnumState, DesktopNativeDialogsError> {
        self.shared
            .control()?
            .map_or(Ok(DesktopNativeDialogsEnumState::Idle), |control| {
                control.state()
            })
    }

    pub fn try_completion(
        &self,
    ) -> Result<Option<DesktopNativeDialogsEnumCompletion>, DesktopNativeDialogsError> {
        Ok(self
            .shared
            .control()?
            .and_then(|control| control.completion.get().copied()))
    }

    pub fn wake_failure(&self) -> Option<&DesktopApplicationWakeError> {
        self.shared.wake_failure.get().map(Arc::as_ref)
    }

    pub fn background_failure(&self) -> Option<&DesktopNativeDialogsError> {
        self.shared.background_failure.get().map(Arc::as_ref)
    }
}

impl<W: DesktopApplicationWakeProvider> DesktopCoordinatorDialogProvider
    for DesktopNativeDialogsPort<W>
{
    fn request_cancel(&self) -> Result<(), DesktopFileHandleError> {
        self.try_cancel().map(|_outcome| ()).map_err(|source| {
            DesktopFileHandleError::NativeDialog {
                source: Box::new(source),
            }
        })
    }
}

impl<W: DesktopApplicationWakeProvider> DesktopFileDialogProvider for DesktopNativeDialogsPort<W> {
    async fn select(
        &self,
        purpose: DesktopFileEnumPurpose,
    ) -> Result<Option<PathBuf>, DesktopFileHandleError> {
        let outcome = async { self.try_request(purpose)?.await }
            .await
            .map_err(|source| DesktopFileHandleError::NativeDialog {
                source: Box::new(source),
            })?;
        match outcome {
            DesktopNativeDialogsEnumOutcome::Selected(path) => Ok(Some(path)),
            DesktopNativeDialogsEnumOutcome::NoSelection
            | DesktopNativeDialogsEnumOutcome::CancelledBeforeOpen
            | DesktopNativeDialogsEnumOutcome::CancelledAfterNativeClose => Ok(None),
        }
    }
}

struct DesktopNativeDialogsActive<P> {
    pending: DesktopNativeDialogsPending,
    operation: DesktopNativeDialogsOperation<P>,
}

pub struct DesktopNativeDialogs<B: DesktopNativeDialogsBackend, W: DesktopApplicationWakeProvider> {
    backend: B,
    shared: Arc<DesktopNativeDialogsShared<W>>,
    active: Option<DesktopNativeDialogsActive<B::Parent>>,
    thread: ThreadId,
    owning_thread: PhantomData<Rc<()>>,
}

impl<B: DesktopNativeDialogsBackend, W: DesktopApplicationWakeProvider + 'static>
    DesktopNativeDialogs<B, W>
{
    pub fn new(backend: B, budget: CapacityBudget, wake: W) -> (Self, DesktopNativeDialogsPort<W>) {
        let shared = Arc::new(DesktopNativeDialogsShared {
            mailbox: Mutex::new(DesktopNativeDialogsMailbox::default()),
            accepting: AtomicBool::new(true),
            budget,
            wake,
            wake_failure: OnceLock::new(),
            background_failure: OnceLock::new(),
        });
        let port = DesktopNativeDialogsPort {
            shared: Arc::clone(&shared),
        };
        let host = Self {
            backend,
            shared,
            active: None,
            thread: std::thread::current().id(),
            owning_thread: PhantomData,
        };
        (host, port)
    }

    pub fn poll(&mut self) -> Result<DesktopNativeDialogsEnumState, DesktopNativeDialogsError> {
        if self.thread != std::thread::current().id() {
            return Err(DesktopNativeDialogsError::WrongThread);
        }
        if self.active.is_none() {
            let pending = self.shared.mailbox()?.pending.take();
            if let Some(pending) = pending {
                self.open(pending)?;
            }
        }
        let Some(active) = &mut self.active else {
            return Ok(DesktopNativeDialogsEnumState::Idle);
        };
        let waker = Waker::from(Arc::clone(&self.shared));
        let mut context = Context::from_waker(&waker);
        match active.operation.result.as_mut().poll(&mut context) {
            Poll::Pending => active.pending.control.state(),
            Poll::Ready(selection) => {
                let active = self
                    .active
                    .take()
                    .ok_or(DesktopNativeDialogsError::Lifecycle)?;
                let DesktopNativeDialogsActive { pending, operation } = active;
                let DesktopNativeDialogsOperation { parent, result } = operation;
                drop(result);
                drop(parent);
                match selection {
                    Ok(selection) => self.complete(pending, selection)?,
                    Err(error) => Self::fail(pending, error)?,
                }
                Ok(DesktopNativeDialogsEnumState::Completed)
            }
        }
    }

    pub fn close_admission(
        &self,
    ) -> Result<DesktopNativeDialogsEnumCancellation, DesktopNativeDialogsError> {
        self.shared.accepting.store(false, Ordering::SeqCst);
        self.shared.cancel()
    }

    pub fn can_shutdown(&self) -> Result<bool, DesktopNativeDialogsError> {
        let mailbox = self.shared.mailbox()?;
        Ok(self.active.is_none() && mailbox.pending.is_none())
    }

    fn open(
        &mut self,
        pending: DesktopNativeDialogsPending,
    ) -> Result<(), DesktopNativeDialogsError> {
        if !pending.control.transition(
            DesktopNativeDialogsEnumState::Queued,
            DesktopNativeDialogsEnumState::Opening,
        ) {
            if pending.control.state()? == DesktopNativeDialogsEnumState::CancelledBeforeOpen {
                self.complete(pending, None)?;
                return Ok(());
            }
            return Err(DesktopNativeDialogsError::Lifecycle);
        }
        match self.backend.open(pending.purpose) {
            Ok(operation) => {
                let control = Arc::clone(&pending.control);
                self.active = Some(DesktopNativeDialogsActive { pending, operation });
                if !control.transition(
                    DesktopNativeDialogsEnumState::Opening,
                    DesktopNativeDialogsEnumState::Active,
                ) && control.state()? != DesktopNativeDialogsEnumState::CancellationPending
                {
                    return Err(DesktopNativeDialogsError::Lifecycle);
                }
            }
            Err(error) => Self::fail(pending, error)?,
        }
        Ok(())
    }

    fn fail(
        pending: DesktopNativeDialogsPending,
        error: DesktopNativeDialogsError,
    ) -> Result<(), DesktopNativeDialogsError> {
        let DesktopNativeDialogsPending {
            lease,
            control,
            reply,
            purpose: _,
        } = pending;
        drop(lease);
        control.fail()?;
        if let Err(abandoned) = reply.send(Err(error)) {
            drop(abandoned);
        }
        Ok(())
    }

    fn complete(
        &self,
        pending: DesktopNativeDialogsPending,
        selection: Option<PathBuf>,
    ) -> Result<(), DesktopNativeDialogsError> {
        let DesktopNativeDialogsPending {
            lease,
            control,
            reply,
            purpose: _,
        } = pending;
        drop(lease);
        let outcome = control.finish(selection)?;
        if let Err(abandoned) = reply.send(Ok(outcome)) {
            drop(abandoned);
        }
        Ok(())
    }
}
