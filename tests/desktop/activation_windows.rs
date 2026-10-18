use std::future::Future;
use std::pin::pin;
use std::str::FromStr;
use std::task::{Context, Poll, Waker};

use chess::desktop::identity::DesktopRequestId;
use chess::desktop::native::activation::windows::{
    DesktopNativeActivationWindowsListener, DesktopNativeActivationWindowsStream,
};
use chess::desktop::profile::activation::server::DesktopProfileActivationAcceptor;
use chess::desktop::profile::activation::{
    DesktopProfileActivationBinding, DesktopProfileActivationEnumResult,
    DesktopProfileActivationError, DesktopProfileActivationExchange, DesktopProfileActivationPeer,
    DesktopProfileActivationRequest, DesktopProfileActivationWindow,
};
use chess::desktop::profile::owner_record::DesktopProfileOwnerRecordEndpoint;
use chess::desktop::profile::{
    DesktopProfileAdmission, DesktopProfileEnumAdmission, DesktopProfileId, DesktopProfileLease,
    DesktopProfileOsLock, DesktopProfileSecurity,
};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};

struct DesktopNativeActivationWindowsFixture {
    directory: TempDir,
    lease: Box<DesktopProfileLease>,
    binding: DesktopProfileActivationBinding,
    security: DesktopProfileSecurity,
}

impl DesktopNativeActivationWindowsFixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let directory = TempDir::new()?;
        let security = DesktopProfileSecurity::current()?;
        let profile = DesktopProfileId::from_str("d92f840f-e575-4654-92bd-79b3ba8b8e55")?;
        let admission = DesktopProfileAdmission::new(security.clone(), DesktopProfileOsLock);
        let DesktopProfileEnumAdmission::Owned(lease) =
            admission.acquire(&directory.path().join("profile"), profile)?
        else {
            return Err(std::io::Error::other("Expected profile ownership").into());
        };
        Ok(Self {
            directory,
            lease,
            binding: DesktopProfileActivationBinding::new(profile, DesktopRequestId::new()),
            security,
        })
    }

    fn listener(
        &self,
    ) -> Result<DesktopNativeActivationWindowsListener, DesktopProfileActivationError> {
        DesktopNativeActivationWindowsListener::bind(&self.lease, self.binding, &self.security)
    }

    fn verify_unchanged_profile(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.lease.verify_identity()?;
        assert!(self.directory.path().join("profile").exists());
        Ok(())
    }
}

struct DesktopNativeActivationWindowsWindow;

impl DesktopProfileActivationWindow for DesktopNativeActivationWindowsWindow {
    fn request_activation(
        &self,
    ) -> Result<DesktopProfileActivationEnumResult, DesktopProfileActivationError> {
        Ok(DesktopProfileActivationEnumResult::ActivationRequested)
    }
}

#[tokio::test]
async fn real_named_pipe_authenticates_both_process_tokens_and_completes_one_exchange()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationWindowsFixture::new()?;
    let mut listener = fixture.listener()?;
    let endpoint = listener.endpoint().clone();
    let request = DesktopProfileActivationRequest::new(fixture.binding, DesktopRequestId::new());
    let server = async {
        let stream = listener.accept().await?;
        DesktopProfileActivationExchange::serve(
            stream,
            fixture.binding,
            &DesktopNativeActivationWindowsWindow,
            tokio::time::Instant::now(),
        )
        .await
    };
    let client = async {
        let stream =
            DesktopNativeActivationWindowsStream::connect(&endpoint, &fixture.security).await?;
        DesktopProfileActivationExchange::activate(
            stream,
            request,
            tokio::time::Instant::now() + DesktopProfileActivationExchange::CLIENT_DEADLINE,
        )
        .await
    };
    let (server_result, client_result) = tokio::join!(server, client);
    assert_eq!(
        server_result?,
        DesktopProfileActivationEnumResult::ActivationRequested
    );
    assert_eq!(
        client_result?,
        DesktopProfileActivationEnumResult::ActivationRequested
    );
    fixture.verify_unchanged_profile()?;
    Ok(())
}

#[tokio::test]
async fn a_busy_pipe_connect_returns_on_its_first_poll_without_a_wait_worker()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationWindowsFixture::new()?;
    let listener = fixture.listener()?;
    let first =
        DesktopNativeActivationWindowsStream::connect(listener.endpoint(), &fixture.security)
            .await?;
    let mut second = pin!(DesktopNativeActivationWindowsStream::connect(
        listener.endpoint(),
        &fixture.security,
    ));
    let mut context = Context::from_waker(Waker::noop());
    assert!(matches!(
        second.as_mut().poll(&mut context),
        Poll::Ready(Err(DesktopProfileActivationError::Io(source)))
            if source.kind() == std::io::ErrorKind::TimedOut
    ));
    drop(first);
    fixture.verify_unchanged_profile()?;
    Ok(())
}

#[tokio::test]
async fn flushing_unread_bytes_never_starts_a_pipe_consumption_wait()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationWindowsFixture::new()?;
    let mut listener = fixture.listener()?;
    let client =
        DesktopNativeActivationWindowsStream::connect(listener.endpoint(), &fixture.security)
            .await?;
    let mut server = listener.accept().await?;
    server.authenticate().await?;
    server.write_all(b"unread response").await?;
    let mut context = Context::from_waker(Waker::noop());
    assert!(matches!(
        std::pin::Pin::new(&mut server).poll_flush(&mut context),
        Poll::Ready(Ok(()))
    ));
    drop(server);
    drop(client);
    fixture.verify_unchanged_profile()?;
    Ok(())
}

#[tokio::test]
async fn closing_either_pipe_peer_is_observed_as_eof() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationWindowsFixture::new()?;
    let mut listener = fixture.listener()?;
    let client =
        DesktopNativeActivationWindowsStream::connect(listener.endpoint(), &fixture.security)
            .await?;
    let mut server = listener.accept().await?;
    drop(client);
    let mut byte = [0_u8; 1];
    assert_eq!(
        tokio::time::timeout(
            DesktopProfileActivationExchange::CONNECTION_DEADLINE,
            server.read(&mut byte),
        )
        .await??,
        0,
    );
    drop(server);
    let mut client =
        DesktopNativeActivationWindowsStream::connect(listener.endpoint(), &fixture.security)
            .await?;
    let server = listener.accept().await?;
    drop(server);
    assert_eq!(
        tokio::time::timeout(
            DesktopProfileActivationExchange::CONNECTION_DEADLINE,
            client.read(&mut byte),
        )
        .await??,
        0,
    );
    fixture.verify_unchanged_profile()?;
    Ok(())
}

#[tokio::test]
async fn binding_another_profile_and_connecting_a_unix_endpoint_fail_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationWindowsFixture::new()?;
    let wrong_binding = DesktopProfileActivationBinding::new(
        DesktopProfileId::from_str("b726b8e1-813c-4190-8be1-b17c07b0aa16")?,
        fixture.binding.generation(),
    );
    assert!(matches!(
        DesktopNativeActivationWindowsListener::bind(
            &fixture.lease,
            wrong_binding,
            &fixture.security
        ),
        Err(DesktopProfileActivationError::BindingMismatch)
    ));
    let unix =
        DesktopProfileOwnerRecordEndpoint::unix_socket(std::path::Path::new("/tmp/chess.sock"))?;
    assert!(
        DesktopNativeActivationWindowsStream::connect(&unix, &fixture.security)
            .await
            .is_err()
    );
    assert!(
        !fixture
            .lease
            .identity()
            .control_directory()
            .join("owner.json")
            .exists()
    );
    fixture.verify_unchanged_profile()?;
    Ok(())
}
