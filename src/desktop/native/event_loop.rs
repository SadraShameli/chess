use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::winit::WinitUserEvent;
use thiserror::Error;
use tokio::sync::Notify;
use winit::event_loop::EventLoopProxy;

use crate::desktop::application::{DesktopApplicationWakeError, DesktopApplicationWakeProvider};
use crate::desktop::profile::activation::{
    DesktopProfileActivationEnumResult, DesktopProfileActivationError,
    DesktopProfileActivationWindow,
};

#[derive(Debug, Error)]
pub enum DesktopNativeEventLoopError {
    #[error("The native event-loop proxy has already been attached")]
    AlreadyAttached,
    #[error("The native event loop is closed")]
    Closed(#[source] winit::event_loop::EventLoopClosed<WinitUserEvent>),
}

#[derive(Default)]
pub struct DesktopNativeEventLoopWake {
    proxy: OnceLock<EventLoopProxy<WinitUserEvent>>,
    pending: AtomicBool,
    ready: Notify,
    activation_pending: AtomicBool,
}

impl DesktopNativeEventLoopWake {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attach(
        &self,
        proxy: EventLoopProxy<WinitUserEvent>,
    ) -> Result<(), DesktopNativeEventLoopError> {
        self.proxy
            .set(proxy)
            .map_err(|_proxy| DesktopNativeEventLoopError::AlreadyAttached)?;
        if self.pending.load(Ordering::Acquire) {
            self.send()?;
        }
        Ok(())
    }

    pub fn acknowledge(&self) {
        self.pending.store(false, Ordering::Release);
    }

    pub fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }

    pub async fn notified(&self) {
        self.ready.notified().await;
    }

    pub fn take_activation_request(&self) -> bool {
        self.activation_pending.swap(false, Ordering::AcqRel)
    }

    fn send(&self) -> Result<(), DesktopNativeEventLoopError> {
        if let Some(proxy) = self.proxy.get()
            && let Err(source) = proxy.send_event(WinitUserEvent::WakeUp)
        {
            self.pending.store(false, Ordering::Release);
            return Err(DesktopNativeEventLoopError::Closed(source));
        }
        Ok(())
    }
}

impl DesktopProfileActivationWindow for DesktopNativeEventLoopWake {
    fn request_activation(
        &self,
    ) -> Result<DesktopProfileActivationEnumResult, DesktopProfileActivationError> {
        self.activation_pending.store(true, Ordering::Release);
        self.wake()
            .map_err(|source| DesktopProfileActivationError::Io(std::io::Error::other(source)))?;
        Ok(if self.proxy.get().is_some() {
            DesktopProfileActivationEnumResult::ActivationRequested
        } else {
            DesktopProfileActivationEnumResult::OwnerStarting
        })
    }
}

impl DesktopApplicationWakeProvider for DesktopNativeEventLoopWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        self.ready.notify_one();
        if self
            .pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.send()
                .map_err(|source| DesktopApplicationWakeError::Native {
                    source: Box::new(source),
                })?;
        }
        Ok(())
    }
}
