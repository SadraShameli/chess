use std::fs::{self, Metadata, Permissions};
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use interprocess::local_socket::traits::StreamCommon;
use interprocess::local_socket::traits::tokio::{Listener as _, Stream as _};
use interprocess::local_socket::{GenericFilePath, ListenerOptions, ToFsName};
use interprocess::os::unix::local_socket::ListenerOptionsExt;
use interprocess::os::unix::uds_local_socket::tokio::{Listener, Stream};
use same_file::Handle;
use tempfile::TempDir;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::desktop::profile::activation::server::DesktopProfileActivationAcceptor;
use crate::desktop::profile::activation::{
    DesktopProfileActivationBinding, DesktopProfileActivationError, DesktopProfileActivationPeer,
};
use crate::desktop::profile::owner_record::{
    DesktopProfileOwnerRecordBoundEndpoint, DesktopProfileOwnerRecordEndpoint,
    DesktopProfileOwnerRecordError,
};
use crate::desktop::profile::{DesktopProfileError, DesktopProfileLease, DesktopProfileSecurity};

#[derive(Debug, Error)]
#[error("Activation binding failed ({operation}); endpoint cleanup also failed ({cleanup})")]
struct DesktopNativeActivationUnixCleanupError {
    #[source]
    operation: DesktopProfileActivationError,
    cleanup: DesktopProfileActivationError,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DesktopNativeActivationUnixEnumPermissions {
    SocketAndDirectory,
    PrivateDirectory,
}

struct DesktopNativeActivationUnixDirectoryIdentity {
    path: PathBuf,
    handle: Handle,
}

impl DesktopNativeActivationUnixDirectoryIdentity {
    const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
    const SOCKET_NAME: &str = "activation.sock";
    const TEMPORARY_ROOT: &str = "/tmp";
    const TEMPORARY_PREFIX: &str = "chess-";

    fn open(path: &Path, security: &DesktopProfileSecurity) -> Result<Self, DesktopProfileError> {
        let file = security.open_private_directory(path)?;
        let handle = Handle::from_file(file).map_err(|source| DesktopProfileError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            handle,
        })
    }

    fn verify(&self, security: &DesktopProfileSecurity) -> Result<(), DesktopProfileError> {
        let named = Self::open(&self.path, security)?;
        if self.handle != named.handle {
            return Err(DesktopProfileError::IdentityChanged {
                path: self.path.clone(),
            });
        }
        Ok(())
    }

    fn socket_path(&self) -> PathBuf {
        self.path.join(Self::SOCKET_NAME)
    }

    fn remove_empty(&self, security: &DesktopProfileSecurity) -> Result<(), DesktopProfileError> {
        self.verify(security)?;
        fs::remove_dir(&self.path).map_err(|source| DesktopProfileError::Io {
            path: self.path.clone(),
            source,
        })
    }

    fn remove_socket(&self) -> Result<(), DesktopProfileError> {
        rustix::fs::unlinkat(
            self.handle.as_file(),
            Self::SOCKET_NAME,
            rustix::fs::AtFlags::empty(),
        )
        .map_err(|source| DesktopProfileError::Io {
            path: self.socket_path(),
            source: source.into(),
        })
    }

    fn temporary_root() -> Result<PathBuf, DesktopProfileActivationError> {
        Ok(Path::new(Self::TEMPORARY_ROOT).canonicalize()?)
    }
}

struct DesktopNativeActivationUnixDirectory {
    temporary: TempDir,
    identity: DesktopNativeActivationUnixDirectoryIdentity,
}

impl DesktopNativeActivationUnixDirectory {
    fn create(security: &DesktopProfileSecurity) -> Result<Self, DesktopProfileActivationError> {
        let mut temporary = tempfile::Builder::new()
            .prefix(DesktopNativeActivationUnixDirectoryIdentity::TEMPORARY_PREFIX)
            .permissions(Permissions::from_mode(
                DesktopNativeActivationUnixDirectoryIdentity::PRIVATE_DIRECTORY_MODE,
            ))
            .tempdir_in(DesktopNativeActivationUnixDirectoryIdentity::temporary_root()?)?;
        temporary.disable_cleanup(true);
        let identity =
            DesktopNativeActivationUnixDirectoryIdentity::open(temporary.path(), security)?;
        Ok(Self {
            temporary,
            identity,
        })
    }

    fn close(self, security: &DesktopProfileSecurity) -> Result<(), DesktopProfileActivationError> {
        self.identity.remove_empty(security)?;
        drop(self.temporary);
        Ok(())
    }

    fn cleanup_error(
        operation: DesktopProfileActivationError,
        cleanup: Result<(), DesktopProfileActivationError>,
    ) -> DesktopProfileActivationError {
        match cleanup {
            Ok(()) => operation,
            Err(cleanup) => {
                io::Error::other(DesktopNativeActivationUnixCleanupError { operation, cleanup })
                    .into()
            }
        }
    }
}

struct DesktopNativeActivationUnixSocketIdentity {
    device: u64,
    inode: u64,
    mode: u32,
}

impl DesktopNativeActivationUnixSocketIdentity {
    fn read(path: &Path, security: &DesktopProfileSecurity) -> Result<Self, DesktopProfileError> {
        let metadata = fs::symlink_metadata(path).map_err(|source| DesktopProfileError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::validate(&metadata, path, security)?;
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
        })
    }

    fn validate(
        metadata: &Metadata,
        path: &Path,
        security: &DesktopProfileSecurity,
    ) -> Result<(), DesktopProfileError> {
        if !metadata.file_type().is_socket() || metadata.nlink() != 1 {
            return Err(DesktopProfileError::UnsafeIdentity {
                path: path.to_path_buf(),
            });
        }
        if metadata.uid() != security.effective_user_id() {
            return Err(DesktopProfileError::UnsafePermissions {
                path: path.to_path_buf(),
            });
        }
        Ok(())
    }

    fn verify(
        &self,
        path: &Path,
        security: &DesktopProfileSecurity,
    ) -> Result<(), DesktopProfileError> {
        let named = Self::read(path, security)?;
        if self.device != named.device || self.inode != named.inode {
            return Err(DesktopProfileError::IdentityChanged {
                path: path.to_path_buf(),
            });
        }
        if self.mode != named.mode {
            return Err(DesktopProfileError::UnsafePermissions {
                path: path.to_path_buf(),
            });
        }
        Ok(())
    }
}

pub struct DesktopNativeActivationUnixListener {
    listener: Listener,
    binding: DesktopProfileActivationBinding,
    endpoint: DesktopProfileOwnerRecordEndpoint,
    security: DesktopProfileSecurity,
    directory: DesktopNativeActivationUnixDirectory,
    socket: DesktopNativeActivationUnixSocketIdentity,
    permissions: DesktopNativeActivationUnixEnumPermissions,
}

impl DesktopNativeActivationUnixListener {
    const PRIVATE_SOCKET_MODE: rustix::fs::RawMode = 0o600;

    pub fn bind(
        lease: &DesktopProfileLease,
        binding: DesktopProfileActivationBinding,
        security: &DesktopProfileSecurity,
    ) -> Result<Self, DesktopProfileActivationError> {
        if lease.identity().profile_id() != binding.profile() {
            return Err(DesktopProfileActivationError::BindingMismatch);
        }
        lease.verify_identity()?;
        let directory = DesktopNativeActivationUnixDirectory::create(security)?;
        let path = directory.identity.socket_path();
        let created = Self::create_listener(&path);
        let (listener, permissions) = match created {
            Ok(created) => created,
            Err(operation) => {
                return Err(DesktopNativeActivationUnixDirectory::cleanup_error(
                    operation,
                    directory.close(security),
                ));
            }
        };
        let socket = match DesktopNativeActivationUnixSocketIdentity::read(&path, security) {
            Ok(socket) => socket,
            Err(operation) => {
                drop(listener);
                return Err(DesktopNativeActivationUnixDirectory::cleanup_error(
                    operation.into(),
                    directory.close(security),
                ));
            }
        };
        let endpoint = DesktopProfileOwnerRecordEndpoint::unix_socket(&path)?;
        let bound = Self {
            listener,
            binding,
            endpoint,
            security: security.clone(),
            directory,
            socket,
            permissions,
        };
        if let Err(operation) = bound.verify_bound().and_then(|()| lease.verify_identity()) {
            return Err(DesktopNativeActivationUnixDirectory::cleanup_error(
                operation.into(),
                bound.shutdown(),
            ));
        }
        Ok(bound)
    }

    pub fn endpoint(&self) -> &DesktopProfileOwnerRecordEndpoint {
        &self.endpoint
    }

    pub fn permissions(&self) -> DesktopNativeActivationUnixEnumPermissions {
        self.permissions
    }

    pub fn shutdown(self) -> Result<(), DesktopProfileActivationError> {
        self.verify_bound()?;
        drop(self.listener);
        self.directory.identity.remove_socket()?;
        self.directory.close(&self.security)
    }

    fn create_listener(
        path: &Path,
    ) -> Result<(Listener, DesktopNativeActivationUnixEnumPermissions), DesktopProfileActivationError>
    {
        let create = || {
            Ok::<_, io::Error>(
                ListenerOptions::new()
                    .name(path.to_fs_name::<GenericFilePath>()?)
                    .reclaim_name(false),
            )
        };
        match create()?
            .mode(Self::PRIVATE_SOCKET_MODE)
            .create_tokio_as::<Listener>()
        {
            Ok(listener) => Ok((
                listener,
                DesktopNativeActivationUnixEnumPermissions::SocketAndDirectory,
            )),
            #[cfg(target_os = "macos")]
            Err(source) if source.kind() == io::ErrorKind::Unsupported => Ok((
                create()?.create_tokio_as::<Listener>()?,
                DesktopNativeActivationUnixEnumPermissions::PrivateDirectory,
            )),
            Err(source) => Err(source.into()),
        }
    }

    fn verify_bound(&self) -> Result<(), DesktopProfileError> {
        self.directory.identity.verify(&self.security)?;
        let path = self.directory.identity.socket_path();
        self.socket.verify(&path, &self.security)
    }
}

impl DesktopProfileActivationAcceptor for DesktopNativeActivationUnixListener {
    type Stream = DesktopNativeActivationUnixStream;

    async fn accept(&mut self) -> Result<Self::Stream, DesktopProfileActivationError> {
        Ok(DesktopNativeActivationUnixStream {
            stream: self.listener.accept().await?,
            effective_user_id: self.security.effective_user_id(),
        })
    }
}

impl DesktopProfileOwnerRecordBoundEndpoint for DesktopNativeActivationUnixListener {
    fn confirm_bound(
        &self,
        binding: DesktopProfileActivationBinding,
    ) -> Result<DesktopProfileOwnerRecordEndpoint, DesktopProfileOwnerRecordError> {
        if self.binding != binding {
            return Err(DesktopProfileOwnerRecordError::BindingMismatch);
        }
        self.verify_bound()?;
        Ok(self.endpoint.clone())
    }
}

pub struct DesktopNativeActivationUnixConnectionTarget {
    path: PathBuf,
    directory: DesktopNativeActivationUnixDirectoryIdentity,
    socket: DesktopNativeActivationUnixSocketIdentity,
    security: DesktopProfileSecurity,
}

impl DesktopNativeActivationUnixConnectionTarget {
    pub fn prepare(
        endpoint: &DesktopProfileOwnerRecordEndpoint,
        security: &DesktopProfileSecurity,
    ) -> Result<Self, DesktopProfileActivationError> {
        let path = endpoint
            .unix_path()
            .ok_or(DesktopProfileOwnerRecordError::InvalidEndpoint)?;
        let directory = path
            .parent()
            .ok_or(DesktopProfileOwnerRecordError::InvalidEndpoint)?;
        if directory.parent()
            != Some(DesktopNativeActivationUnixDirectoryIdentity::temporary_root()?.as_path())
            || path.file_name()
                != Some(std::ffi::OsStr::new(
                    DesktopNativeActivationUnixDirectoryIdentity::SOCKET_NAME,
                ))
        {
            return Err(DesktopProfileOwnerRecordError::InvalidEndpoint.into());
        }
        let target = Self {
            path: path.to_path_buf(),
            directory: DesktopNativeActivationUnixDirectoryIdentity::open(directory, security)?,
            socket: DesktopNativeActivationUnixSocketIdentity::read(path, security)?,
            security: security.clone(),
        };
        target.verify_identity()?;
        Ok(target)
    }

    pub fn verify_identity(&self) -> Result<(), DesktopProfileError> {
        self.directory.verify(&self.security)?;
        self.socket.verify(&self.path, &self.security)
    }
}

pub struct DesktopNativeActivationUnixStream {
    stream: Stream,
    effective_user_id: u32,
}

impl DesktopNativeActivationUnixStream {
    pub async fn connect(
        target: &DesktopNativeActivationUnixConnectionTarget,
        absolute_deadline: tokio::time::Instant,
    ) -> Result<Self, DesktopProfileActivationError> {
        Self::check_deadline(absolute_deadline)?;
        let stream = tokio::select! {
            biased;
            _ = tokio::time::sleep_until(absolute_deadline) => {
                return Err(DesktopProfileActivationError::Deadline);
            }
            result = Stream::connect(target.path.as_path().to_fs_name::<GenericFilePath>()?) => result?,
        };
        Self::check_deadline(absolute_deadline)?;
        if let Some(source) = stream.take_error()? {
            return Err(source.into());
        }
        let connected = Self {
            stream,
            effective_user_id: target.security.effective_user_id(),
        };
        connected.authenticate().await?;
        Self::check_deadline(absolute_deadline)?;
        Ok(connected)
    }

    fn check_deadline(deadline: tokio::time::Instant) -> Result<(), DesktopProfileActivationError> {
        if tokio::time::Instant::now() >= deadline {
            return Err(DesktopProfileActivationError::Deadline);
        }
        Ok(())
    }
}

impl DesktopProfileActivationPeer for DesktopNativeActivationUnixStream {
    async fn authenticate(&self) -> Result<(), DesktopProfileActivationError> {
        if self.stream.peer_creds()?.euid() != Some(self.effective_user_id) {
            return Err(DesktopProfileActivationError::Unauthenticated);
        }
        Ok(())
    }
}

impl AsyncRead for DesktopNativeActivationUnixStream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().stream.inner_mut()).poll_read(context, buffer)
    }
}

impl AsyncWrite for DesktopNativeActivationUnixStream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(self.get_mut().stream.inner_mut()).poll_write(context, buffer)
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().stream.inner_mut()).poll_flush(context)
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().stream.inner_mut()).poll_shutdown(context)
    }
}

#[cfg(test)]
mod tests {
    use interprocess::os::unix::uds_local_socket::tokio::Stream;

    use crate::desktop::native::activation::unix::DesktopNativeActivationUnixStream;
    use crate::desktop::profile::DesktopProfileSecurity;
    use crate::desktop::profile::activation::{
        DesktopProfileActivationError, DesktopProfileActivationPeer,
    };

    #[tokio::test]
    async fn actual_peer_credentials_must_match_the_expected_effective_user_on_both_ends()
    -> Result<(), Box<dyn std::error::Error>> {
        let expected_user = DesktopProfileSecurity::current()?.effective_user_id();
        let different_user = expected_user.checked_add(1).ok_or_else(|| {
            std::io::Error::other("The effective user has no greater test identity")
        })?;
        let (owner, secondary) = tokio::net::UnixStream::pair()?;
        for stream in [owner, secondary] {
            let mut peer = DesktopNativeActivationUnixStream {
                stream: Stream::from(stream),
                effective_user_id: different_user,
            };
            assert!(matches!(
                peer.authenticate().await,
                Err(DesktopProfileActivationError::Unauthenticated)
            ));
            peer.effective_user_id = expected_user;
            peer.authenticate().await?;
        }
        Ok(())
    }
}
