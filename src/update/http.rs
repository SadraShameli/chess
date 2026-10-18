use std::future::Future;

use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum UpdateHttpError {
    #[error("The update connection failed: {0}")]
    Transport(String),
    #[error("The update response exceeds its {0} byte bound")]
    TooLarge(u64),
    #[error("The update redirect is not allowed")]
    Redirect,
    #[error("The update request timed out")]
    Timeout,
}

#[derive(Clone)]
pub struct UpdateHttpRequest {
    pub url: Url,
    pub bearer: Option<String>,
    pub accept: &'static str,
}

impl std::fmt::Debug for UpdateHttpRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UpdateHttpRequest")
            .field("url", &self.url)
            .field("bearer", &self.bearer.as_ref().map(|_token| "redacted"))
            .field("accept", &self.accept)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateHttpHead {
    pub status: u16,
    pub retry_after: Option<u64>,
    pub content_length: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateHttpRedirect(pub Option<String>);

pub trait UpdateHttpStream: Send {
    fn next_chunk(
        &mut self,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, UpdateHttpError>> + Send;
}

pub trait UpdateHttp: Send + Sync {
    type Stream: UpdateHttpStream;

    fn open(
        &self,
        request: UpdateHttpRequest,
    ) -> impl Future<
        Output = Result<(UpdateHttpHead, UpdateHttpRedirect, Self::Stream), UpdateHttpError>,
    > + Send;
}

#[derive(Clone)]
pub struct UpdateReqwest {
    client: reqwest::Client,
}

impl UpdateReqwest {
    pub fn new() -> Result<Self, UpdateHttpError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .https_only(true)
            .user_agent("chess-desktop-updater")
            .build()
            .map_err(|error| UpdateHttpError::Transport(error.to_string()))?;
        Ok(Self { client })
    }
}

pub struct UpdateReqwestStream(reqwest::Response);

impl UpdateHttpStream for UpdateReqwestStream {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, UpdateHttpError> {
        self.0
            .chunk()
            .await
            .map(|chunk| chunk.map(|bytes| bytes.to_vec()))
            .map_err(|error| UpdateHttpError::Transport(error.to_string()))
    }
}

impl UpdateHttp for UpdateReqwest {
    type Stream = UpdateReqwestStream;

    async fn open(
        &self,
        request: UpdateHttpRequest,
    ) -> Result<(UpdateHttpHead, UpdateHttpRedirect, Self::Stream), UpdateHttpError> {
        let mut builder = self
            .client
            .get(request.url.as_str())
            .header(reqwest::header::ACCEPT, request.accept);
        if let Some(token) = &request.bearer {
            builder = builder.bearer_auth(token);
        }
        let response = builder
            .send()
            .await
            .map_err(|error| UpdateHttpError::Transport(error.to_string()))?;
        let headers = response.headers();
        let text = |name: reqwest::header::HeaderName| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        let head = UpdateHttpHead {
            status: response.status().as_u16(),
            retry_after: text(reqwest::header::RETRY_AFTER).and_then(|value| value.parse().ok()),
            content_length: response.content_length(),
        };
        let redirect = UpdateHttpRedirect(text(reqwest::header::LOCATION));
        Ok((head, redirect, UpdateReqwestStream(response)))
    }
}
