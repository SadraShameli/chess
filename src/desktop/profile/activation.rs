use std::future::Future;
use std::num::TryFromIntError;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::desktop::error::DesktopError;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::profile::{DesktopProfileError, DesktopProfileId};

#[derive(Debug, Error)]
pub enum DesktopProfileActivationError {
    #[error("The activation endpoint is invalid: {0}")]
    Endpoint(#[from] crate::desktop::profile::owner_record::DesktopProfileOwnerRecordError),
    #[error("The local activation transport failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("The activation frame length {received} is outside 1 through 4096 bytes")]
    FrameLength { received: u32 },
    #[error("The activation frame is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("The activation profile identity is invalid: {0}")]
    Profile(#[from] DesktopProfileError),
    #[error("The activation correlation identity is invalid: {0}")]
    Identity(#[from] DesktopError),
    #[error("The activation frame length exceeds the supported range: {0}")]
    Range(#[from] TryFromIntError),
    #[error("Activation protocol version {0} is unsupported")]
    Version(u32),
    #[error("The activation message is not the expected exchange step")]
    UnexpectedMessage,
    #[error("The activation response does not match the requested profile, owner or action")]
    BindingMismatch,
    #[error("The activation exchange contains an additional frame")]
    ExtraFrame,
    #[error("The activation exchange exceeded its absolute deadline")]
    Deadline,
    #[error("The local activation peer could not be authenticated")]
    Unauthenticated,
    #[error("The activation deadline exceeds the monotonic clock range")]
    DeadlineOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopProfileActivationBinding {
    profile: DesktopProfileId,
    generation: DesktopRequestId,
}

impl DesktopProfileActivationBinding {
    pub fn new(profile: DesktopProfileId, generation: DesktopRequestId) -> Self {
        Self {
            profile,
            generation,
        }
    }

    pub fn profile(self) -> DesktopProfileId {
        self.profile
    }

    pub fn generation(self) -> DesktopRequestId {
        self.generation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopProfileActivationRequest {
    binding: DesktopProfileActivationBinding,
    id: DesktopRequestId,
}

impl DesktopProfileActivationRequest {
    pub fn new(binding: DesktopProfileActivationBinding, id: DesktopRequestId) -> Self {
        Self { binding, id }
    }

    pub fn binding(self) -> DesktopProfileActivationBinding {
        self.binding
    }

    pub fn id(self) -> DesktopRequestId {
        self.id
    }

    pub fn check_binding(
        self,
        expected: DesktopProfileActivationBinding,
    ) -> Result<(), DesktopProfileActivationError> {
        if self.binding == expected {
            Ok(())
        } else {
            Err(DesktopProfileActivationError::BindingMismatch)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopProfileActivationEnumResult {
    OwnerStarting,
    Focused,
    ActivationRequested,
    Busy,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopProfileActivationResponse {
    request: DesktopProfileActivationRequest,
    result: DesktopProfileActivationEnumResult,
}

impl DesktopProfileActivationResponse {
    pub fn new(
        request: DesktopProfileActivationRequest,
        result: DesktopProfileActivationEnumResult,
    ) -> Self {
        Self { request, result }
    }

    pub fn request(self) -> DesktopProfileActivationRequest {
        self.request
    }

    pub fn result(self) -> DesktopProfileActivationEnumResult {
        self.result
    }

    pub fn check_request(
        self,
        expected: DesktopProfileActivationRequest,
    ) -> Result<(), DesktopProfileActivationError> {
        if self.request == expected {
            Ok(())
        } else {
            Err(DesktopProfileActivationError::BindingMismatch)
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DesktopProfileActivationWireIdentity {
    profile_id: String,
    generation: String,
    request_id: String,
}

impl From<DesktopProfileActivationRequest> for DesktopProfileActivationWireIdentity {
    fn from(request: DesktopProfileActivationRequest) -> Self {
        Self {
            profile_id: request.binding.profile.to_string(),
            generation: String::from(request.binding.generation),
            request_id: String::from(request.id),
        }
    }
}

impl TryFrom<DesktopProfileActivationWireIdentity> for DesktopProfileActivationRequest {
    type Error = DesktopProfileActivationError;

    fn try_from(identity: DesktopProfileActivationWireIdentity) -> Result<Self, Self::Error> {
        Ok(Self::new(
            DesktopProfileActivationBinding::new(
                DesktopProfileId::from_str(&identity.profile_id)?,
                DesktopRequestId::from_str(&identity.generation)?,
            ),
            DesktopRequestId::from_str(&identity.request_id)?,
        ))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(
    tag = "operation",
    content = "result",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum DesktopProfileActivationEnumWireMessage {
    Activate,
    Response(DesktopProfileActivationEnumResult),
    Receipt,
    ReceiptAcknowledged,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DesktopProfileActivationWireMessage {
    version: u32,
    identity: DesktopProfileActivationWireIdentity,
    message: DesktopProfileActivationEnumWireMessage,
}

pub struct DesktopProfileActivationCodec;

impl DesktopProfileActivationCodec {
    pub const VERSION: u32 = 2;
    pub const MAX_FRAME_BYTES: u32 = 4096;

    pub async fn read_request<R: AsyncRead + Unpin>(
        reader: &mut R,
    ) -> Result<DesktopProfileActivationRequest, DesktopProfileActivationError> {
        let (request, message) = Self::read(reader).await?;
        match message {
            DesktopProfileActivationEnumWireMessage::Activate => Ok(request),
            DesktopProfileActivationEnumWireMessage::Response(_)
            | DesktopProfileActivationEnumWireMessage::Receipt
            | DesktopProfileActivationEnumWireMessage::ReceiptAcknowledged => {
                Err(DesktopProfileActivationError::UnexpectedMessage)
            }
        }
    }

    pub async fn write_request<W: AsyncWrite + Unpin>(
        writer: &mut W,
        request: &DesktopProfileActivationRequest,
    ) -> Result<(), DesktopProfileActivationError> {
        Self::write(
            writer,
            *request,
            DesktopProfileActivationEnumWireMessage::Activate,
        )
        .await
    }

    pub async fn read_response<R: AsyncRead + Unpin>(
        reader: &mut R,
    ) -> Result<DesktopProfileActivationResponse, DesktopProfileActivationError> {
        let (request, message) = Self::read(reader).await?;
        match message {
            DesktopProfileActivationEnumWireMessage::Response(result) => {
                Ok(DesktopProfileActivationResponse::new(request, result))
            }
            DesktopProfileActivationEnumWireMessage::Activate
            | DesktopProfileActivationEnumWireMessage::Receipt
            | DesktopProfileActivationEnumWireMessage::ReceiptAcknowledged => {
                Err(DesktopProfileActivationError::UnexpectedMessage)
            }
        }
    }

    pub async fn write_response<W: AsyncWrite + Unpin>(
        writer: &mut W,
        response: DesktopProfileActivationResponse,
    ) -> Result<(), DesktopProfileActivationError> {
        Self::write(
            writer,
            response.request,
            DesktopProfileActivationEnumWireMessage::Response(response.result),
        )
        .await
    }

    pub async fn read_receipt<R: AsyncRead + Unpin>(
        reader: &mut R,
    ) -> Result<DesktopProfileActivationRequest, DesktopProfileActivationError> {
        let (request, message) = Self::read(reader).await?;
        match message {
            DesktopProfileActivationEnumWireMessage::Receipt => Ok(request),
            DesktopProfileActivationEnumWireMessage::Activate
            | DesktopProfileActivationEnumWireMessage::Response(_)
            | DesktopProfileActivationEnumWireMessage::ReceiptAcknowledged => {
                Err(DesktopProfileActivationError::UnexpectedMessage)
            }
        }
    }

    pub async fn write_receipt<W: AsyncWrite + Unpin>(
        writer: &mut W,
        response: DesktopProfileActivationResponse,
    ) -> Result<(), DesktopProfileActivationError> {
        Self::write(
            writer,
            response.request,
            DesktopProfileActivationEnumWireMessage::Receipt,
        )
        .await
    }

    pub async fn read_receipt_acknowledged<R: AsyncRead + Unpin>(
        reader: &mut R,
    ) -> Result<DesktopProfileActivationRequest, DesktopProfileActivationError> {
        let (request, message) = Self::read(reader).await?;
        match message {
            DesktopProfileActivationEnumWireMessage::ReceiptAcknowledged => Ok(request),
            DesktopProfileActivationEnumWireMessage::Activate
            | DesktopProfileActivationEnumWireMessage::Response(_)
            | DesktopProfileActivationEnumWireMessage::Receipt => {
                Err(DesktopProfileActivationError::UnexpectedMessage)
            }
        }
    }

    pub async fn write_receipt_acknowledged<W: AsyncWrite + Unpin>(
        writer: &mut W,
        request: DesktopProfileActivationRequest,
    ) -> Result<(), DesktopProfileActivationError> {
        Self::write(
            writer,
            request,
            DesktopProfileActivationEnumWireMessage::ReceiptAcknowledged,
        )
        .await
    }

    pub async fn require_end<R: AsyncRead + Unpin>(
        reader: &mut R,
    ) -> Result<(), DesktopProfileActivationError> {
        let mut byte = [0_u8; 1];
        match reader.read(&mut byte).await? {
            0 => Ok(()),
            _received => Err(DesktopProfileActivationError::ExtraFrame),
        }
    }

    async fn read<R: AsyncRead + Unpin>(
        reader: &mut R,
    ) -> Result<
        (
            DesktopProfileActivationRequest,
            DesktopProfileActivationEnumWireMessage,
        ),
        DesktopProfileActivationError,
    > {
        let length = reader.read_u32().await?;
        Self::validate_length(length)?;
        let mut bytes = vec![0; usize::try_from(length)?];
        reader.read_exact(&mut bytes).await?;
        let wire: DesktopProfileActivationWireMessage = serde_json::from_slice(&bytes)?;
        if wire.version != Self::VERSION {
            return Err(DesktopProfileActivationError::Version(wire.version));
        }
        Ok((wire.identity.try_into()?, wire.message))
    }

    async fn write<W: AsyncWrite + Unpin>(
        writer: &mut W,
        request: DesktopProfileActivationRequest,
        message: DesktopProfileActivationEnumWireMessage,
    ) -> Result<(), DesktopProfileActivationError> {
        let bytes = serde_json::to_vec(&DesktopProfileActivationWireMessage {
            version: Self::VERSION,
            identity: request.into(),
            message,
        })?;
        let length = u32::try_from(bytes.len())?;
        Self::validate_length(length)?;
        writer.write_u32(length).await?;
        writer.write_all(&bytes).await?;
        Ok(())
    }

    fn validate_length(length: u32) -> Result<(), DesktopProfileActivationError> {
        if (1..=Self::MAX_FRAME_BYTES).contains(&length) {
            Ok(())
        } else {
            Err(DesktopProfileActivationError::FrameLength { received: length })
        }
    }
}

pub trait DesktopProfileActivationPeer {
    fn authenticate(
        &self,
    ) -> impl Future<Output = Result<(), DesktopProfileActivationError>> + Send;
}

pub trait DesktopProfileActivationWindow: Send + Sync {
    fn request_activation(
        &self,
    ) -> Result<DesktopProfileActivationEnumResult, DesktopProfileActivationError>;
}

pub struct DesktopProfileActivationExchange;

impl DesktopProfileActivationExchange {
    pub const CONNECTION_DEADLINE: Duration = Duration::from_secs(1);
    pub const CLIENT_DEADLINE: Duration = Duration::from_secs(2);
    pub const CLIENT_TRIALS: usize = 4;
    pub const CLIENT_CAPACITY: usize = 4;

    pub async fn serve<S, W>(
        mut stream: S,
        binding: DesktopProfileActivationBinding,
        window: &W,
        accepted_at: tokio::time::Instant,
    ) -> Result<DesktopProfileActivationEnumResult, DesktopProfileActivationError>
    where
        S: AsyncRead + AsyncWrite + Unpin + DesktopProfileActivationPeer,
        W: DesktopProfileActivationWindow,
    {
        let deadline = accepted_at
            .checked_add(Self::CONNECTION_DEADLINE)
            .ok_or(DesktopProfileActivationError::DeadlineOverflow)?;
        Self::check_deadline(deadline)?;
        Self::before_deadline(deadline, async {
            stream.authenticate().await?;
            Self::check_deadline(deadline)?;
            let request = DesktopProfileActivationCodec::read_request(&mut stream).await?;
            request.check_binding(binding)?;
            Self::check_deadline(deadline)?;
            let response =
                DesktopProfileActivationResponse::new(request, window.request_activation()?);
            Self::check_deadline(deadline)?;
            DesktopProfileActivationCodec::write_response(&mut stream, response).await?;
            Self::check_deadline(deadline)?;
            let receipt = DesktopProfileActivationCodec::read_receipt(&mut stream).await?;
            Self::check_deadline(deadline)?;
            if receipt != request {
                return Err(DesktopProfileActivationError::BindingMismatch);
            }
            DesktopProfileActivationCodec::write_receipt_acknowledged(&mut stream, request).await?;
            Self::check_deadline(deadline)?;
            DesktopProfileActivationCodec::require_end(&mut stream).await?;
            Self::check_deadline(deadline)?;
            Ok(response.result())
        })
        .await
    }

    pub async fn activate<S>(
        mut stream: S,
        request: DesktopProfileActivationRequest,
        attempt_deadline: tokio::time::Instant,
    ) -> Result<DesktopProfileActivationEnumResult, DesktopProfileActivationError>
    where
        S: AsyncRead + AsyncWrite + Unpin + DesktopProfileActivationPeer,
    {
        Self::check_deadline(attempt_deadline)?;
        Self::before_deadline(attempt_deadline, async {
            stream.authenticate().await?;
            Self::check_deadline(attempt_deadline)?;
            DesktopProfileActivationCodec::write_request(&mut stream, &request).await?;
            Self::check_deadline(attempt_deadline)?;
            let response = DesktopProfileActivationCodec::read_response(&mut stream).await?;
            Self::check_deadline(attempt_deadline)?;
            response.check_request(request)?;
            DesktopProfileActivationCodec::write_receipt(&mut stream, response).await?;
            Self::check_deadline(attempt_deadline)?;
            let acknowledged =
                DesktopProfileActivationCodec::read_receipt_acknowledged(&mut stream).await?;
            Self::check_deadline(attempt_deadline)?;
            if acknowledged != request {
                return Err(DesktopProfileActivationError::BindingMismatch);
            }
            Ok(response.result())
        })
        .await
    }

    async fn before_deadline<T>(
        deadline: tokio::time::Instant,
        operation: impl Future<Output = Result<T, DesktopProfileActivationError>>,
    ) -> Result<T, DesktopProfileActivationError> {
        Self::check_deadline(deadline)?;
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => {
                Err(DesktopProfileActivationError::Deadline)
            }
            result = operation => {
                Self::check_deadline(deadline)?;
                result
            }
        }
    }

    fn check_deadline(deadline: tokio::time::Instant) -> Result<(), DesktopProfileActivationError> {
        if tokio::time::Instant::now() < deadline {
            Ok(())
        } else {
            Err(DesktopProfileActivationError::Deadline)
        }
    }
}
pub mod server;
