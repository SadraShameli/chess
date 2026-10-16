use std::io;
use std::num::NonZeroU8;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use interprocess::ConnectWaitMode;
use interprocess::os::windows::named_pipe::PipeListenerOptions;
use interprocess::os::windows::named_pipe::pipe_mode::Bytes;
use interprocess::os::windows::named_pipe::tokio::{DuplexPipeStream, PipeListener};
use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use widestring::U16CString;
use winsafe::{HPROCESS, TokenInfo, co};

use crate::desktop::profile::activation::server::DesktopProfileActivationAcceptor;
use crate::desktop::profile::activation::{
    DesktopProfileActivationBinding, DesktopProfileActivationCodec, DesktopProfileActivationError,
    DesktopProfileActivationExchange, DesktopProfileActivationPeer,
};
use crate::desktop::profile::owner_record::{
    DesktopProfileOwnerRecordBoundEndpoint, DesktopProfileOwnerRecordEndpoint,
    DesktopProfileOwnerRecordError,
};
use crate::desktop::profile::{DesktopProfileLease, DesktopProfileSecurity};

pub struct DesktopNativeActivationWindowsListener {
    listener: PipeListener<Bytes, Bytes>,
    binding: DesktopProfileActivationBinding,
    endpoint: DesktopProfileOwnerRecordEndpoint,
    security: DesktopProfileSecurity,
}

impl DesktopNativeActivationWindowsListener {
    pub fn bind(
        lease: &DesktopProfileLease,
        binding: DesktopProfileActivationBinding,
        security: &DesktopProfileSecurity,
    ) -> Result<Self, DesktopProfileActivationError> {
        if binding.profile() != lease.identity().profile_id() {
            return Err(DesktopProfileActivationError::BindingMismatch);
        }
        lease.verify_identity()?;
        let name = format!(
            r"\\.\pipe\{}.{}",
            binding.profile(),
            String::from(binding.generation()),
        );
        let endpoint = DesktopProfileOwnerRecordEndpoint::windows_pipe(&name)?;
        let path = Self::wide_text(&name)?;
        let descriptor =
            Self::wide_text(&format!("D:P(A;;GA;;;SY)(A;;GA;;;{})", security.user_sid(),))?;
        let listener = PipeListenerOptions::new()
            .path(path)
            .accept_remote(false)
            .inheritable(false)
            .instance_limit(Some(Self::instance_limit()?))
            .input_buffer_size_hint(DesktopProfileActivationCodec::MAX_FRAME_BYTES)
            .output_buffer_size_hint(DesktopProfileActivationCodec::MAX_FRAME_BYTES)
            .security_descriptor(Some(SecurityDescriptor::deserialize(&descriptor)?))
            .create_tokio_duplex::<Bytes>()?;
        lease.verify_identity()?;
        Ok(Self {
            listener,
            binding,
            endpoint,
            security: security.clone(),
        })
    }

    pub fn endpoint(&self) -> &DesktopProfileOwnerRecordEndpoint {
        &self.endpoint
    }

    fn wide_text(text: &str) -> Result<U16CString, DesktopProfileActivationError> {
        U16CString::from_str(text)
            .map_err(|source| io::Error::new(io::ErrorKind::InvalidInput, source).into())
    }

    fn instance_limit() -> Result<NonZeroU8, DesktopProfileActivationError> {
        let instances = DesktopProfileActivationExchange::CLIENT_CAPACITY
            .checked_add(1)
            .ok_or_else(|| io::Error::other("The activation pipe capacity exceeds its range"))?;
        NonZeroU8::new(u8::try_from(instances)?).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "The activation pipe capacity is zero",
            )
            .into()
        })
    }
}

impl DesktopProfileActivationAcceptor for DesktopNativeActivationWindowsListener {
    type Stream = DesktopNativeActivationWindowsStream;

    async fn accept(&mut self) -> Result<Self::Stream, DesktopProfileActivationError> {
        let stream = self.listener.accept().await?;
        Ok(DesktopNativeActivationWindowsStream {
            stream,
            security: self.security.clone(),
        })
    }
}

impl DesktopProfileOwnerRecordBoundEndpoint for DesktopNativeActivationWindowsListener {
    fn confirm_bound(
        &self,
        binding: DesktopProfileActivationBinding,
    ) -> Result<DesktopProfileOwnerRecordEndpoint, DesktopProfileOwnerRecordError> {
        if self.binding != binding {
            return Err(DesktopProfileOwnerRecordError::BindingMismatch);
        }
        Ok(self.endpoint.clone())
    }
}

pub struct DesktopNativeActivationWindowsStream {
    stream: DuplexPipeStream<Bytes>,
    security: DesktopProfileSecurity,
}

impl DesktopNativeActivationWindowsStream {
    pub async fn connect(
        endpoint: &DesktopProfileOwnerRecordEndpoint,
        security: &DesktopProfileSecurity,
    ) -> Result<Self, DesktopProfileActivationError> {
        let name = endpoint
            .windows_name()
            .ok_or(DesktopProfileOwnerRecordError::InvalidEndpoint)?;
        let path = DesktopNativeActivationWindowsListener::wide_text(name)?;
        let stream = DuplexPipeStream::<Bytes>::connect_by_path_with_wait_mode(
            path,
            ConnectWaitMode::Timeout(Duration::ZERO),
        )
        .await?;
        Ok(Self {
            stream,
            security: security.clone(),
        })
    }

    fn authenticate_process_token(&self) -> Result<(), DesktopProfileActivationError> {
        let peer_pid = self.stream.peer_process_id()?;
        let process =
            HPROCESS::OpenProcess(co::PROCESS::QUERY_LIMITED_INFORMATION, false, peer_pid)
                .map_err(io::Error::other)?;
        let token = process
            .OpenProcessToken(co::TOKEN::QUERY)
            .map_err(io::Error::other)?;
        let TokenInfo::User(user) = token
            .GetTokenInformation(co::TOKEN_INFORMATION_CLASS::User)
            .map_err(io::Error::other)?
        else {
            return Err(DesktopProfileActivationError::Unauthenticated);
        };
        let sid = user
            .User
            .Sid()
            .ok_or(DesktopProfileActivationError::Unauthenticated)?;
        let peer_sid = winsafe::ConvertSidToStringSid(sid).map_err(io::Error::other)?;
        if peer_sid != self.security.user_sid() {
            return Err(DesktopProfileActivationError::Unauthenticated);
        }
        Ok(())
    }

    fn is_peer_closed(error: &io::Error) -> bool {
        error.kind() == io::ErrorKind::BrokenPipe
            || error
                .raw_os_error()
                .is_some_and(|code| u32::try_from(code) == Ok(co::ERROR::PIPE_NOT_CONNECTED.raw()))
    }
}

impl DesktopProfileActivationPeer for DesktopNativeActivationWindowsStream {
    async fn authenticate(&self) -> Result<(), DesktopProfileActivationError> {
        self.authenticate_process_token()
    }
}

impl AsyncRead for DesktopNativeActivationWindowsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match Pin::new(&mut self.get_mut().stream).poll_read(context, buffer) {
            Poll::Ready(Err(source)) if Self::is_peer_closed(&source) => Poll::Ready(Ok(())),
            result => result,
        }
    }
}

impl AsyncWrite for DesktopNativeActivationWindowsStream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().stream).poll_write(context, buffer)
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "The local activation named pipe does not support a write half-close",
        )))
    }
}

impl Drop for DesktopNativeActivationWindowsStream {
    fn drop(&mut self) {
        self.stream.assume_flushed();
    }
}
