use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};

use muda::accelerator::{Key, KeyAccelerator, Modifiers};
use muda::{Menu, MenuEvent, MenuItem, Submenu};
use thiserror::Error;

use crate::desktop::application::{DesktopApplicationWakeError, DesktopApplicationWakeProvider};
use crate::desktop::native::event_loop::DesktopNativeEventLoopWake;

static DESKTOP_NATIVE_MENU_HANDLER_INSTALLED: OnceLock<()> = OnceLock::new();

#[derive(Debug, Error)]
pub enum DesktopNativeMenuError {
    #[error("The native application menu already has an owner")]
    AlreadyInstalled,
    #[error("The native application menu could not be configured: {0}")]
    Configure(#[from] muda::Error),
    #[error("The native application menu could not wake its event loop: {0}")]
    Wake(#[source] Arc<DesktopApplicationWakeError>),
}

#[derive(Clone, Copy)]
pub(super) enum DesktopNativeMenuEnumAction {
    Quit,
    Recovery,
}

impl DesktopNativeMenuEnumAction {
    const fn mask(self) -> u8 {
        match self {
            Self::Quit => 1,
            Self::Recovery => 2,
        }
    }

    fn item(self) -> Result<MenuItem, muda::Error> {
        match self {
            Self::Quit => {
                let item = MenuItem::new("Quit Chess", true, None);
                item.set_key_accelerator(Some(KeyAccelerator::new(
                    Modifiers::META,
                    Key::Character("q".to_owned()),
                )))?;
                Ok(item)
            }
            Self::Recovery => Ok(MenuItem::new("Graphics recovery options", false, None)),
        }
    }
}

#[derive(Default)]
pub(super) struct DesktopNativeMenuRequests {
    actions: u8,
}

impl DesktopNativeMenuRequests {
    pub(super) const fn is_requested(&self, action: DesktopNativeMenuEnumAction) -> bool {
        self.actions & action.mask() != 0
    }
}

#[derive(Default)]
struct DesktopNativeMenuPending {
    actions: AtomicU8,
    failure: OnceLock<Arc<DesktopApplicationWakeError>>,
}

impl DesktopNativeMenuPending {
    fn request(
        &self,
        action: DesktopNativeMenuEnumAction,
        wake: &impl DesktopApplicationWakeProvider,
    ) {
        if self.actions.fetch_or(action.mask(), Ordering::AcqRel) & action.mask() == 0
            && let Err(error) = wake.wake()
        {
            self.failure.get_or_init(|| Arc::new(error));
        }
    }

    fn take(&self) -> DesktopNativeMenuRequests {
        DesktopNativeMenuRequests {
            actions: self.actions.swap(0, Ordering::AcqRel),
        }
    }
}

pub(super) struct DesktopNativeMenu {
    menu: Menu,
    recovery: MenuItem,
    pending: Arc<DesktopNativeMenuPending>,
    recovery_enabled: bool,
    wake_failure_reported: bool,
}

impl DesktopNativeMenu {
    pub(super) fn install(
        wake: Arc<DesktopNativeEventLoopWake>,
    ) -> Result<Self, DesktopNativeMenuError> {
        let menu = Menu::new();
        let application = Submenu::new("Chess", true);
        let quit = DesktopNativeMenuEnumAction::Quit.item()?;
        let recovery = DesktopNativeMenuEnumAction::Recovery.item()?;
        application.append_items(&[&recovery, &quit])?;
        menu.append(&application)?;
        let pending = Arc::new(DesktopNativeMenuPending::default());
        let handler_pending = Arc::clone(&pending);
        let quit_id = quit.id().clone();
        let recovery_id = recovery.id().clone();
        DESKTOP_NATIVE_MENU_HANDLER_INSTALLED
            .set(())
            .map_err(|()| DesktopNativeMenuError::AlreadyInstalled)?;
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let action = if event.id() == &quit_id {
                Some(DesktopNativeMenuEnumAction::Quit)
            } else if event.id() == &recovery_id {
                Some(DesktopNativeMenuEnumAction::Recovery)
            } else {
                None
            };
            if let Some(action) = action {
                handler_pending.request(action, wake.as_ref());
            }
        }));
        let installed = Self {
            menu,
            recovery,
            pending,
            recovery_enabled: false,
            wake_failure_reported: false,
        };
        installed.menu.init_for_nsapp();
        Ok(installed)
    }

    pub(super) fn set_recovery_enabled(&mut self, enabled: bool) {
        if self.recovery_enabled != enabled {
            self.recovery.set_enabled(enabled);
            self.recovery_enabled = enabled;
        }
    }

    pub(super) fn poll(&mut self) -> Result<DesktopNativeMenuRequests, DesktopNativeMenuError> {
        if !self.wake_failure_reported
            && let Some(error) = self.pending.failure.get()
        {
            self.wake_failure_reported = true;
            return Err(DesktopNativeMenuError::Wake(Arc::clone(error)));
        }
        Ok(self.pending.take())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::desktop::application::{
        DesktopApplicationWakeError, DesktopApplicationWakeProvider,
    };
    use crate::desktop::native::menu::{DesktopNativeMenuEnumAction, DesktopNativeMenuPending};

    #[derive(Default)]
    struct DesktopNativeMenuWake {
        calls: AtomicUsize,
        rejected: bool,
    }

    impl DesktopApplicationWakeProvider for DesktopNativeMenuWake {
        fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.rejected {
                Err(DesktopApplicationWakeError::Unavailable)
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn native_menu_requests_coalesce_and_transfer_without_losing_later_actions() {
        let pending = DesktopNativeMenuPending::default();
        let wake = DesktopNativeMenuWake::default();
        for _request in 0..1024 {
            pending.request(DesktopNativeMenuEnumAction::Quit, &wake);
            pending.request(DesktopNativeMenuEnumAction::Recovery, &wake);
        }
        assert_eq!(wake.calls.load(Ordering::Relaxed), 2);
        let requests = pending.take();
        assert!(requests.is_requested(DesktopNativeMenuEnumAction::Quit));
        assert!(requests.is_requested(DesktopNativeMenuEnumAction::Recovery));
        let drained = pending.take();
        assert!(!drained.is_requested(DesktopNativeMenuEnumAction::Quit));
        assert!(!drained.is_requested(DesktopNativeMenuEnumAction::Recovery));
        pending.request(DesktopNativeMenuEnumAction::Recovery, &wake);
        let later = pending.take();
        assert!(!later.is_requested(DesktopNativeMenuEnumAction::Quit));
        assert!(later.is_requested(DesktopNativeMenuEnumAction::Recovery));
        assert_eq!(wake.calls.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn native_menu_wake_failure_retains_its_typed_cause_and_requested_action() {
        let pending = DesktopNativeMenuPending::default();
        let wake = DesktopNativeMenuWake {
            rejected: true,
            ..DesktopNativeMenuWake::default()
        };
        pending.request(DesktopNativeMenuEnumAction::Quit, &wake);
        assert!(matches!(
            pending.failure.get().map(AsRef::as_ref),
            Some(DesktopApplicationWakeError::Unavailable)
        ));
        assert!(
            pending
                .take()
                .is_requested(DesktopNativeMenuEnumAction::Quit)
        );
    }
}
