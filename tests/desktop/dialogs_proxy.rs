use std::future::Future;
use std::task::Poll;

use chess::desktop::coordinator::DesktopCoordinatorDialogProvider;
use chess::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileEnumPurpose, DesktopFileHandleError,
};
use chess::desktop::native::dialogs_proxy::DesktopNativeDialogsProxy;
use tokio::sync::watch;

#[tokio::test]
async fn a_selection_waiting_for_its_real_native_parent_cancels_without_opening_a_dialog()
-> Result<(), Box<dyn std::error::Error>> {
    let (_sender, receiver) = watch::channel(None);
    let proxy = DesktopNativeDialogsProxy::new(receiver);
    let waiting = proxy.select(DesktopFileEnumPurpose::ReadFile);
    tokio::pin!(waiting);
    assert!(
        std::future::poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context)))
            .await
            .is_pending()
    );
    proxy.request_cancel()?;
    assert!(waiting.await?.is_none());
    Ok(())
}

#[tokio::test]
async fn a_closed_native_host_is_an_error_instead_of_a_dismissed_file_selection() {
    let (sender, receiver) = watch::channel(None);
    let proxy = DesktopNativeDialogsProxy::new(receiver);
    drop(sender);
    assert!(matches!(
        proxy.select(DesktopFileEnumPurpose::ReadFile).await,
        Err(DesktopFileHandleError::NativeDialog { .. })
    ));
}
