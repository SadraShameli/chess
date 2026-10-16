use std::fs;
use std::future::Future;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Poll;
use std::time::Duration;

use chess::desktop::identity::DesktopRequestId;
use chess::desktop::native::activation::unix::{
    DesktopNativeActivationUnixConnectionTarget, DesktopNativeActivationUnixEnumPermissions,
    DesktopNativeActivationUnixListener, DesktopNativeActivationUnixStream,
};
use chess::desktop::profile::activation::server::DesktopProfileActivationAcceptor;
use chess::desktop::profile::activation::{
    DesktopProfileActivationBinding, DesktopProfileActivationCodec,
    DesktopProfileActivationEnumResult, DesktopProfileActivationError,
    DesktopProfileActivationExchange, DesktopProfileActivationPeer,
    DesktopProfileActivationRequest, DesktopProfileActivationWindow,
};
use chess::desktop::profile::owner_record::{
    DesktopProfileOwnerRecordBoundEndpoint, DesktopProfileOwnerRecordEndpoint,
    DesktopProfileOwnerRecordError,
};
use chess::desktop::profile::{
    DesktopProfileAdmission, DesktopProfileEnumAdmission, DesktopProfileError, DesktopProfileId,
    DesktopProfileLease, DesktopProfileOsLock, DesktopProfileSecurity,
};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct DesktopNativeActivationUnixFixture {
    directory: TempDir,
    lease: DesktopProfileLease,
    security: DesktopProfileSecurity,
    binding: DesktopProfileActivationBinding,
}

impl DesktopNativeActivationUnixFixture {
    const TIMER_RESOLUTION: Duration = Duration::from_millis(1);

    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let directory = TempDir::new()?;
        let security = DesktopProfileSecurity::current()?;
        let admission = DesktopProfileAdmission::new(security.clone(), DesktopProfileOsLock);
        let profile = DesktopProfileId::from_str("51606ef1-e259-42ce-8510-aa380a8f739e")?;
        let lease = match admission.acquire(&directory.path().join("profile"), profile)? {
            DesktopProfileEnumAdmission::Owned(lease) => *lease,
            DesktopProfileEnumAdmission::Busy => {
                return Err(std::io::Error::other("The test profile is unexpectedly busy").into());
            }
        };
        let binding = DesktopProfileActivationBinding::new(
            lease.identity().profile_id(),
            DesktopRequestId::from_str("f135d9c6-f0a4-4c69-bc18-a042a86b105d")?,
        );
        Ok(Self {
            directory,
            lease,
            security,
            binding,
        })
    }

    fn listener(
        &self,
    ) -> Result<DesktopNativeActivationUnixListener, DesktopProfileActivationError> {
        DesktopNativeActivationUnixListener::bind(&self.lease, self.binding, &self.security)
    }

    fn request(&self) -> Result<DesktopProfileActivationRequest, Box<dyn std::error::Error>> {
        Ok(DesktopProfileActivationRequest::new(
            self.binding,
            DesktopRequestId::from_str("d129d71c-ed98-41aa-a71c-9840659bd468")?,
        ))
    }

    fn target(
        &self,
        listener: &DesktopNativeActivationUnixListener,
    ) -> Result<DesktopNativeActivationUnixConnectionTarget, DesktopProfileActivationError> {
        DesktopNativeActivationUnixConnectionTarget::prepare(listener.endpoint(), &self.security)
    }

    fn socket_path(
        listener: &DesktopNativeActivationUnixListener,
    ) -> Result<&Path, Box<dyn std::error::Error>> {
        listener.endpoint().unix_path().ok_or_else(|| {
            std::io::Error::other("The Unix listener did not publish a socket endpoint").into()
        })
    }

    fn parent(path: &Path) -> Result<&Path, Box<dyn std::error::Error>> {
        path.parent()
            .ok_or_else(|| std::io::Error::other("The socket path has no directory").into())
    }

    fn deadline() -> tokio::time::Instant {
        tokio::time::Instant::now() + DesktopProfileActivationExchange::CLIENT_DEADLINE
    }

    async fn pair(
        &self,
        listener: &mut DesktopNativeActivationUnixListener,
    ) -> Result<
        (
            DesktopNativeActivationUnixStream,
            DesktopNativeActivationUnixStream,
        ),
        DesktopProfileActivationError,
    > {
        let target = self.target(listener)?;
        let (owner, secondary) = tokio::join!(
            listener.accept(),
            DesktopNativeActivationUnixStream::connect(&target, Self::deadline()),
        );
        Ok((owner?, secondary?))
    }
}

#[derive(Default)]
struct DesktopNativeActivationUnixWindowFixture {
    activations: AtomicUsize,
}

impl DesktopProfileActivationWindow for DesktopNativeActivationUnixWindowFixture {
    fn request_activation(
        &self,
    ) -> Result<DesktopProfileActivationEnumResult, DesktopProfileActivationError> {
        self.activations.fetch_add(1, Ordering::Relaxed);
        Ok(DesktopProfileActivationEnumResult::OwnerStarting)
    }
}

#[tokio::test]
async fn actual_unix_peers_complete_one_authenticated_activation_and_release_the_endpoint()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let mut listener = fixture.listener()?;
    let endpoint = listener.confirm_bound(fixture.binding)?;
    let path = DesktopNativeActivationUnixFixture::socket_path(&listener)?.to_path_buf();
    let parent = DesktopNativeActivationUnixFixture::parent(&path)?.to_path_buf();
    let target = fixture.target(&listener)?;
    target.verify_identity()?;
    let window = DesktopNativeActivationUnixWindowFixture::default();
    let request = fixture.request()?;
    let accepted_at = tokio::time::Instant::now();
    let server = async {
        let stream = listener.accept().await?;
        stream.authenticate().await?;
        DesktopProfileActivationExchange::serve(stream, fixture.binding, &window, accepted_at).await
    };
    let client = async {
        let stream = DesktopNativeActivationUnixStream::connect(
            &target,
            DesktopNativeActivationUnixFixture::deadline(),
        )
        .await?;
        stream.authenticate().await?;
        DesktopProfileActivationExchange::activate(
            stream,
            request,
            DesktopNativeActivationUnixFixture::deadline(),
        )
        .await
    };
    let (server, client) = tokio::join!(server, client);
    assert_eq!(server?, DesktopProfileActivationEnumResult::OwnerStarting);
    assert_eq!(client?, DesktopProfileActivationEnumResult::OwnerStarting);
    assert_eq!(window.activations.load(Ordering::Relaxed), 1);
    assert_eq!(listener.confirm_bound(fixture.binding)?, endpoint);
    listener.shutdown()?;
    assert!(!path.exists());
    assert!(!parent.exists());
    fixture.lease.verify_identity()?;
    assert!(fixture.directory.path().exists());
    Ok(())
}

#[tokio::test]
async fn shutdown_sends_real_eof_while_the_sender_remains_alive()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let mut listener = fixture.listener()?;
    let (mut owner, mut secondary) = fixture.pair(&mut listener).await?;
    secondary.write_all(b"receipt").await?;
    secondary.shutdown().await?;
    let mut bytes = Vec::new();
    tokio::time::timeout(
        DesktopProfileActivationExchange::CONNECTION_DEADLINE,
        owner.read_to_end(&mut bytes),
    )
    .await??;
    assert_eq!(bytes, b"receipt");
    secondary.authenticate().await?;
    drop((owner, secondary));
    listener.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn socket_and_private_parent_are_owned_bounded_and_canonical()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let listener = fixture.listener()?;
    let path = DesktopNativeActivationUnixFixture::socket_path(&listener)?;
    let parent = DesktopNativeActivationUnixFixture::parent(path)?;
    let directory = fs::symlink_metadata(parent)?;
    let socket = fs::symlink_metadata(path)?;
    assert!(directory.is_dir());
    assert_eq!(directory.permissions().mode() & 0o7777, 0o700);
    assert_eq!(
        directory.uid(),
        fs::metadata(fixture.directory.path())?.uid()
    );
    assert!(socket.file_type().is_socket());
    assert_eq!(socket.uid(), directory.uid());
    assert!(path.as_os_str().len() <= 103);
    assert_eq!(path.canonicalize()?, path);
    assert_eq!(
        parent.parent(),
        Some(Path::new("/tmp").canonicalize()?.as_path())
    );
    if listener.permissions() == DesktopNativeActivationUnixEnumPermissions::SocketAndDirectory {
        assert_eq!(socket.permissions().mode() & 0o7777, 0o600);
    }
    listener.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn incorrect_profile_or_owner_generation_cannot_publish_a_bound_endpoint()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let listener = fixture.listener()?;
    let wrong_profile = DesktopProfileActivationBinding::new(
        DesktopProfileId::from_str("b92f840f-e575-4654-92bd-79b3ba8b8e55")?,
        fixture.binding.generation(),
    );
    assert!(matches!(
        DesktopNativeActivationUnixListener::bind(&fixture.lease, wrong_profile, &fixture.security),
        Err(DesktopProfileActivationError::BindingMismatch)
    ));
    let wrong_generation = DesktopProfileActivationBinding::new(
        fixture.binding.profile(),
        DesktopRequestId::from_str("a129d71c-ed98-41aa-a71c-9840659bd468")?,
    );
    for binding in [wrong_profile, wrong_generation] {
        assert!(matches!(
            listener.confirm_bound(binding),
            Err(DesktopProfileOwnerRecordError::BindingMismatch)
        ));
    }
    listener.confirm_bound(fixture.binding)?;
    listener.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn malformed_local_peer_cannot_request_window_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let mut listener = fixture.listener()?;
    let (owner, mut secondary) = fixture.pair(&mut listener).await?;
    secondary
        .write_u32(DesktopProfileActivationCodec::MAX_FRAME_BYTES + 1)
        .await?;
    secondary.shutdown().await?;
    let window = DesktopNativeActivationUnixWindowFixture::default();
    assert!(matches!(
        DesktopProfileActivationExchange::serve(
            owner,
            fixture.binding,
            &window,
            tokio::time::Instant::now(),
        )
        .await,
        Err(DesktopProfileActivationError::FrameLength { .. })
    ));
    assert_eq!(window.activations.load(Ordering::Relaxed), 0);
    listener.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn stale_generation_is_rejected_before_window_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let mut listener = fixture.listener()?;
    let (owner, mut secondary) = fixture.pair(&mut listener).await?;
    let request = DesktopProfileActivationRequest::new(
        DesktopProfileActivationBinding::new(
            fixture.binding.profile(),
            DesktopRequestId::from_str("a129d71c-ed98-41aa-a71c-9840659bd468")?,
        ),
        fixture.request()?.id(),
    );
    DesktopProfileActivationCodec::write_request(&mut secondary, &request).await?;
    secondary.shutdown().await?;
    let window = DesktopNativeActivationUnixWindowFixture::default();
    assert!(matches!(
        DesktopProfileActivationExchange::serve(
            owner,
            fixture.binding,
            &window,
            tokio::time::Instant::now(),
        )
        .await,
        Err(DesktopProfileActivationError::BindingMismatch)
    ));
    assert_eq!(window.activations.load(Ordering::Relaxed), 0);
    listener.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn an_incomplete_local_frame_expires_without_resetting_its_deadline()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let mut listener = fixture.listener()?;
    let (owner, mut secondary) = fixture.pair(&mut listener).await?;
    secondary.write_all(&[0, 0, 0]).await?;
    tokio::time::pause();
    let accepted_at = tokio::time::Instant::now();
    let window = DesktopNativeActivationUnixWindowFixture::default();
    let server =
        DesktopProfileActivationExchange::serve(owner, fixture.binding, &window, accepted_at);
    tokio::pin!(server);
    assert!(
        std::future::poll_fn(|context| Poll::Ready(server.as_mut().poll(context)))
            .await
            .is_pending()
    );
    let advanced = DesktopProfileActivationExchange::CONNECTION_DEADLINE
        + DesktopNativeActivationUnixFixture::TIMER_RESOLUTION;
    tokio::time::advance(advanced).await;
    assert!(matches!(
        server.await,
        Err(DesktopProfileActivationError::Deadline)
    ));
    assert_eq!(tokio::time::Instant::now() - accepted_at, advanced);
    assert_eq!(window.activations.load(Ordering::Relaxed), 0);
    listener.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn an_expired_connect_deadline_wins_over_a_ready_local_listener()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let listener = fixture.listener()?;
    let target = fixture.target(&listener)?;
    tokio::time::pause();
    let deadline = tokio::time::Instant::now();
    assert!(matches!(
        DesktopNativeActivationUnixStream::connect(&target, deadline).await,
        Err(DesktopProfileActivationError::Deadline)
    ));
    assert_eq!(tokio::time::Instant::now(), deadline);
    listener.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn public_parent_permissions_are_rejected_without_automatic_repair()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let listener = fixture.listener()?;
    let path = DesktopNativeActivationUnixFixture::socket_path(&listener)?;
    let parent = DesktopNativeActivationUnixFixture::parent(path)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o755))?;
    let result = fixture.target(&listener);
    let publication = listener.confirm_bound(fixture.binding);
    let actual = fs::metadata(parent)?.permissions().mode() & 0o7777;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    assert!(matches!(
        result,
        Err(DesktopProfileActivationError::Profile(
            DesktopProfileError::UnsafePermissions { .. }
        ))
    ));
    assert!(matches!(
        publication,
        Err(DesktopProfileOwnerRecordError::Profile(
            DesktopProfileError::UnsafePermissions { .. }
        ))
    ));
    assert_eq!(actual, 0o755);
    listener.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn a_symlink_to_the_retained_private_directory_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let listener = fixture.listener()?;
    let path = DesktopNativeActivationUnixFixture::socket_path(&listener)?.to_path_buf();
    let parent = DesktopNativeActivationUnixFixture::parent(&path)?.to_path_buf();
    let relocated = parent.with_extension("relocated");
    fs::rename(&parent, &relocated)?;
    std::os::unix::fs::symlink(&relocated, &parent)?;
    let result = fixture.target(&listener);
    let publication = listener.confirm_bound(fixture.binding);
    fs::remove_file(&parent)?;
    fs::rename(&relocated, &parent)?;
    assert!(matches!(
        result,
        Err(DesktopProfileActivationError::Profile(
            DesktopProfileError::UnsafeIdentity { .. }
        ))
    ));
    assert!(matches!(
        publication,
        Err(DesktopProfileOwnerRecordError::Profile(
            DesktopProfileError::UnsafeIdentity { .. }
        ))
    ));
    listener.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn endpoint_replacement_is_preserved_and_refuses_owner_publication_and_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let listener = fixture.listener()?;
    let path = DesktopNativeActivationUnixFixture::socket_path(&listener)?.to_path_buf();
    let parent = DesktopNativeActivationUnixFixture::parent(&path)?.to_path_buf();
    let retained = parent.join("retained.sock");
    fs::rename(&path, &retained)?;
    fs::write(&path, b"replacement data")?;
    assert!(matches!(
        listener.confirm_bound(fixture.binding),
        Err(DesktopProfileOwnerRecordError::Profile(
            DesktopProfileError::UnsafeIdentity { .. }
        ))
    ));
    assert!(matches!(
        listener.shutdown(),
        Err(DesktopProfileActivationError::Profile(
            DesktopProfileError::UnsafeIdentity { .. }
        ))
    ));
    assert_eq!(fs::read(&path)?, b"replacement data");
    fs::remove_file(&path)?;
    fs::remove_file(&retained)?;
    fs::remove_dir(parent)?;
    Ok(())
}

#[tokio::test]
async fn a_different_socket_identity_cannot_replace_the_bound_endpoint()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let listener = fixture.listener()?;
    let target = fixture.target(&listener)?;
    let path = DesktopNativeActivationUnixFixture::socket_path(&listener)?.to_path_buf();
    let parent = DesktopNativeActivationUnixFixture::parent(&path)?.to_path_buf();
    let retained = parent.join("retained.sock");
    let replacement_path = parent.join("replacement.sock");
    let replacement = tokio::net::UnixListener::bind(&replacement_path)?;
    fs::rename(&path, &retained)?;
    fs::rename(&replacement_path, &path)?;
    assert!(matches!(
        target.verify_identity(),
        Err(DesktopProfileError::IdentityChanged { .. })
    ));
    assert!(matches!(
        listener.confirm_bound(fixture.binding),
        Err(DesktopProfileOwnerRecordError::Profile(
            DesktopProfileError::IdentityChanged { .. }
        ))
    ));
    assert!(matches!(
        listener.shutdown(),
        Err(DesktopProfileActivationError::Profile(
            DesktopProfileError::IdentityChanged { .. }
        ))
    ));
    assert!(fs::symlink_metadata(&path)?.file_type().is_socket());
    drop(replacement);
    fs::remove_file(&path)?;
    fs::remove_file(&retained)?;
    fs::remove_dir(parent)?;
    Ok(())
}

#[tokio::test]
async fn a_replaced_parent_is_rejected_without_deleting_either_directory()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::DirBuilderExt;

    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let listener = fixture.listener()?;
    let target = fixture.target(&listener)?;
    let path = DesktopNativeActivationUnixFixture::socket_path(&listener)?.to_path_buf();
    let parent = DesktopNativeActivationUnixFixture::parent(&path)?.to_path_buf();
    let relocated = parent.with_extension("relocated");
    fs::rename(&parent, &relocated)?;
    fs::DirBuilder::new().mode(0o700).create(&parent)?;
    let sentinel = parent.join("keep.txt");
    fs::write(&sentinel, b"replacement directory")?;
    assert!(matches!(
        target.verify_identity(),
        Err(DesktopProfileError::IdentityChanged { .. })
    ));
    assert!(matches!(
        listener.confirm_bound(fixture.binding),
        Err(DesktopProfileOwnerRecordError::Profile(
            DesktopProfileError::IdentityChanged { .. }
        ))
    ));
    assert!(matches!(
        listener.shutdown(),
        Err(DesktopProfileActivationError::Profile(
            DesktopProfileError::IdentityChanged { .. }
        ))
    ));
    assert_eq!(fs::read(&sentinel)?, b"replacement directory");
    let socket_name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("The retained socket has no filename"))?;
    let retained_socket = relocated.join(socket_name);
    assert!(
        fs::symlink_metadata(&retained_socket)?
            .file_type()
            .is_socket()
    );
    fs::remove_file(retained_socket)?;
    fs::remove_dir(relocated)?;
    fs::remove_file(sentinel)?;
    fs::remove_dir(parent)?;
    Ok(())
}

#[tokio::test]
async fn cleanup_never_recursively_deletes_unrecognized_directory_contents()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let listener = fixture.listener()?;
    let path = DesktopNativeActivationUnixFixture::socket_path(&listener)?.to_path_buf();
    let parent = DesktopNativeActivationUnixFixture::parent(&path)?.to_path_buf();
    let unexpected = parent.join("keep.txt");
    fs::write(&unexpected, b"retain this data")?;
    assert!(matches!(
        listener.shutdown(),
        Err(DesktopProfileActivationError::Profile(
            DesktopProfileError::Io { .. }
        ))
    ));
    assert!(!path.exists());
    assert_eq!(fs::read(&unexpected)?, b"retain this data");
    fs::remove_file(unexpected)?;
    fs::remove_dir(parent)?;
    Ok(())
}

#[tokio::test]
async fn missing_and_foreign_endpoints_do_not_create_or_replace_sockets()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = DesktopNativeActivationUnixFixture::new()?;
    let listener = fixture.listener()?;
    let endpoint = listener.endpoint().clone();
    let target = fixture.target(&listener)?;
    listener.shutdown()?;
    assert!(matches!(
        DesktopNativeActivationUnixConnectionTarget::prepare(&endpoint, &fixture.security),
        Err(DesktopProfileActivationError::Profile(
            DesktopProfileError::Io { .. }
        ))
    ));
    assert!(matches!(
        DesktopNativeActivationUnixStream::connect(
            &target,
            DesktopNativeActivationUnixFixture::deadline(),
        )
        .await,
        Err(DesktopProfileActivationError::Io(_))
    ));
    let foreign = DesktopProfileOwnerRecordEndpoint::windows_pipe(r"\\.\pipe\chess-test")?;
    assert!(matches!(
        DesktopNativeActivationUnixConnectionTarget::prepare(&foreign, &fixture.security),
        Err(DesktopProfileActivationError::Endpoint(
            DesktopProfileOwnerRecordError::InvalidEndpoint
        ))
    ));
    let data_socket = DesktopProfileOwnerRecordEndpoint::unix_socket(
        &fixture.directory.path().join("activation.sock"),
    )?;
    assert!(matches!(
        DesktopNativeActivationUnixConnectionTarget::prepare(&data_socket, &fixture.security),
        Err(DesktopProfileActivationError::Endpoint(
            DesktopProfileOwnerRecordError::InvalidEndpoint
        ))
    ));
    assert!(!fixture.directory.path().join("activation.sock").exists());
    Ok(())
}
