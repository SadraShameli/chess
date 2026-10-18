use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;
use url::Url;

use crate::update::credentials::UpdateCredential;
use crate::update::discovery::{UpdateOrigins, UpdateRedirects};
use crate::update::http::{UpdateHttp, UpdateHttpError, UpdateHttpRequest, UpdateHttpStream};
use crate::update::manifest::{UpdateManifest, UpdateManifestAsset};

#[derive(Debug, Error)]
pub enum UpdateDownloadError {
    #[error(transparent)]
    Http(#[from] UpdateHttpError),
    #[error("The download I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("The installer is larger than the {0} byte cap")]
    TooLarge(u64),
    #[error("The destination volume has {available} bytes free; {required} are required")]
    Space { available: u64, required: u64 },
    #[error("The server answered with status {0}")]
    Status(u16),
    #[error("No bytes arrived within {0:?}")]
    Stalled(Duration),
    #[error("The download exceeded {0:?}")]
    Deadline(Duration),
    #[error("The download was cancelled")]
    Cancelled,
    #[error("The downloaded bytes do not match the signed manifest")]
    Mismatch,
}

pub trait UpdateDiskSpace {
    fn available(&self, path: &Path) -> Result<u64, std::io::Error>;
}

pub struct UpdateDownloadRequest<'a> {
    pub asset: &'a UpdateManifestAsset,
    pub url: Url,
    pub partial: &'a Path,
    pub credential: &'a UpdateCredential,
}

pub struct UpdateDownloader<H: UpdateHttp> {
    http: H,
    origins: UpdateOrigins,
}

impl<H: UpdateHttp> UpdateDownloader<H> {
    pub const NO_BYTE_TIMEOUT: Duration = Duration::from_secs(60);
    pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
    pub const SPACE_MARGIN_BYTES: u64 = 64 * 1024 * 1024;

    pub fn new(http: H, origins: UpdateOrigins) -> Self {
        Self { http, origins }
    }

    pub async fn download(
        &self,
        request: UpdateDownloadRequest<'_>,
        space: &impl UpdateDiskSpace,
        cancel: watch::Receiver<bool>,
        mut progress: impl FnMut(u64) + Send,
    ) -> Result<(), UpdateDownloadError> {
        let length = request.asset.length();
        if length > UpdateManifest::MAX_PACKAGE_BYTES {
            return Err(UpdateDownloadError::TooLarge(
                UpdateManifest::MAX_PACKAGE_BYTES,
            ));
        }
        let parent = request.partial.parent().unwrap_or_else(|| Path::new("."));
        let required = length.saturating_add(Self::SPACE_MARGIN_BYTES);
        let available = space.available(parent)?;
        if available < required {
            return Err(UpdateDownloadError::Space {
                available,
                required,
            });
        }
        let outcome = tokio::time::timeout(
            Self::TOTAL_TIMEOUT,
            self.stream_to_file(&request, cancel, &mut progress),
        )
        .await
        .unwrap_or(Err(UpdateDownloadError::Deadline(Self::TOTAL_TIMEOUT)));
        if outcome.is_err()
            && let Err(error) = tokio::fs::remove_file(request.partial).await
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(error.into());
        }
        outcome
    }

    async fn stream_to_file(
        &self,
        request: &UpdateDownloadRequest<'_>,
        mut cancel: watch::Receiver<bool>,
        progress: &mut (impl FnMut(u64) + Send),
    ) -> Result<(), UpdateDownloadError> {
        let length = request.asset.length();
        let opened = tokio::time::timeout(
            Self::NO_BYTE_TIMEOUT,
            UpdateRedirects::open(
                &self.http,
                &self.origins,
                UpdateHttpRequest {
                    url: request.url.clone(),
                    bearer: Some(request.credential.expose().to_owned()),
                    accept: "application/octet-stream",
                },
            ),
        )
        .await
        .map_err(|_elapsed| UpdateDownloadError::Stalled(Self::NO_BYTE_TIMEOUT))??;
        let (head, mut stream) = opened;
        if head.status != 200 {
            return Err(UpdateDownloadError::Status(head.status));
        }
        if head
            .content_length
            .is_some_and(|declared| declared != length)
        {
            return Err(UpdateDownloadError::Mismatch);
        }
        let mut file = tokio::fs::File::create(request.partial).await?;
        let mut hasher = Sha256::new();
        let mut received = 0_u64;
        loop {
            if *cancel.borrow() {
                return Err(UpdateDownloadError::Cancelled);
            }
            let chunk = tokio::select! {
                _changed = cancel.changed() => {
                    return Err(UpdateDownloadError::Cancelled);
                }
                next = tokio::time::timeout(Self::NO_BYTE_TIMEOUT, stream.next_chunk()) => {
                    next.map_err(|_elapsed| UpdateDownloadError::Stalled(Self::NO_BYTE_TIMEOUT))??
                }
            };
            let Some(chunk) = chunk else {
                break;
            };
            received = received
                .checked_add(
                    u64::try_from(chunk.len()).map_err(|_error| UpdateDownloadError::Mismatch)?,
                )
                .ok_or(UpdateDownloadError::Mismatch)?;
            if received > length {
                return Err(UpdateDownloadError::Mismatch);
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
            progress(received);
        }
        file.flush().await?;
        file.sync_all().await?;
        let digest: [u8; 32] = hasher.finalize().into();
        if received != length || digest != request.asset.sha256() {
            return Err(UpdateDownloadError::Mismatch);
        }
        Ok(())
    }
}
