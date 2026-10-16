use chess::desktop::native::bootstrap::{DesktopNativeBootstrap, DesktopNativeBootstrapError};

fn main() -> Result<(), DesktopNativeBootstrapError> {
    DesktopNativeBootstrap::run()
}
