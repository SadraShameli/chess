use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use chess::update::http::{
    UpdateHttp, UpdateHttpError, UpdateHttpHead, UpdateHttpRedirect, UpdateHttpRequest,
    UpdateHttpStream,
};

pub const MANIFEST: &[u8] = include_bytes!("fixtures/manifest.json");
pub const SIGNATURE: &[u8] = include_bytes!("fixtures/manifest.json.minisig");
pub const PUBLIC_KEY: &str = include_str!("fixtures/public.key");
pub const MACOS_PACKAGE: &[u8] = b"hello world";

#[derive(Clone)]
pub struct UpdateFixtureResponse {
    pub status: u16,
    pub location: Option<String>,
    pub retry_after: Option<u64>,
    pub body: Vec<u8>,
    pub chunk: usize,
    pub stall_open: bool,
    pub stall_body: bool,
}

impl UpdateFixtureResponse {
    pub fn ok(body: &[u8]) -> Self {
        Self {
            status: 200,
            location: None,
            retry_after: None,
            body: body.to_vec(),
            chunk: 4,
            stall_open: false,
            stall_body: false,
        }
    }

    pub fn redirect(location: &str) -> Self {
        Self {
            status: 302,
            location: Some(location.to_owned()),
            ..Self::ok(b"")
        }
    }

    pub fn status(status: u16) -> Self {
        Self {
            status,
            ..Self::ok(b"")
        }
    }
}

#[derive(Clone, Default)]
pub struct UpdateFixtureHttp {
    routes: Arc<Mutex<HashMap<String, UpdateFixtureResponse>>>,
    log: Arc<Mutex<Vec<(String, Option<String>)>>>,
}

impl UpdateFixtureHttp {
    pub fn route(&self, url: &str, response: UpdateFixtureResponse) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.insert(url.to_owned(), response);
        }
    }

    pub fn log(&self) -> Vec<(String, Option<String>)> {
        self.log.lock().map(|log| log.clone()).unwrap_or_default()
    }
}

pub struct UpdateFixtureStream {
    chunks: VecDeque<Vec<u8>>,
    stall: bool,
}

impl UpdateHttpStream for UpdateFixtureStream {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, UpdateHttpError> {
        if let Some(chunk) = self.chunks.pop_front() {
            return Ok(Some(chunk));
        }
        if self.stall {
            std::future::pending::<()>().await;
        }
        Ok(None)
    }
}

impl UpdateHttp for UpdateFixtureHttp {
    type Stream = UpdateFixtureStream;

    async fn open(
        &self,
        request: UpdateHttpRequest,
    ) -> Result<(UpdateHttpHead, UpdateHttpRedirect, Self::Stream), UpdateHttpError> {
        if let Ok(mut log) = self.log.lock() {
            log.push((request.url.to_string(), request.bearer.clone()));
        }
        let response = self
            .routes
            .lock()
            .ok()
            .and_then(|routes| routes.get(request.url.as_str()).cloned())
            .unwrap_or_else(|| UpdateFixtureResponse::status(404));
        if response.stall_open {
            std::future::pending::<()>().await;
        }
        let chunks = response
            .body
            .chunks(response.chunk.max(1))
            .map(<[u8]>::to_vec)
            .collect();
        Ok((
            UpdateHttpHead {
                status: response.status,
                retry_after: response.retry_after,
                content_length: u64::try_from(response.body.len()).ok(),
            },
            UpdateHttpRedirect(response.location),
            UpdateFixtureStream {
                chunks,
                stall: response.stall_body,
            },
        ))
    }
}
