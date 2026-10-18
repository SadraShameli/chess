use std::sync::Arc;

use thiserror::Error;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};

use crate::desktop::library_service::{
    DesktopLibraryService, DesktopLibraryServiceError, DesktopLibraryServiceOptions,
    DesktopLibraryServicePort, DesktopLibraryServiceShutdownReceipt,
};

#[derive(Debug, Error)]
pub enum DesktopNativeLibraryServiceError {
    #[error("The native Library service has already started")]
    Started,
    #[error("The native Library service is closing")]
    Closing,
    #[error("The native Library service did not prove shutdown")]
    Incomplete,
    #[error("The native Library cleanup exceeded its deadline: {0}")]
    Deadline(#[source] tokio::time::error::Elapsed),
    #[error("The native Library actor failed: {0}")]
    Service(#[source] Arc<DesktopLibraryServiceError>),
    #[error("The native Library worker failed: {0}")]
    Worker(#[source] Arc<tokio::task::JoinError>),
}

type DesktopNativeLibraryServiceOutcome = (
    DesktopLibraryService,
    Result<(), DesktopLibraryServiceError>,
);

pub struct DesktopNativeLibraryService {
    startup: Option<DesktopLibraryService>,
    retained: Option<DesktopLibraryService>,
    worker: Option<JoinHandle<DesktopNativeLibraryServiceOutcome>>,
    worker_failure: Option<Arc<tokio::task::JoinError>>,
    shutdown: Option<DesktopLibraryServiceShutdownReceipt>,
    port: DesktopLibraryServicePort,
    runtime: Handle,
    closing: bool,
}

impl DesktopNativeLibraryService {
    pub fn new(options: DesktopLibraryServiceOptions, runtime: Handle) -> Self {
        let (service, port) = DesktopLibraryService::new(options);
        Self {
            startup: Some(service),
            retained: None,
            worker: None,
            worker_failure: None,
            shutdown: None,
            port,
            runtime,
            closing: false,
        }
    }

    pub fn start(&mut self) -> Result<DesktopLibraryServicePort, DesktopNativeLibraryServiceError> {
        if self.closing {
            return Err(DesktopNativeLibraryServiceError::Closing);
        }
        let mut service = self
            .startup
            .take()
            .ok_or(DesktopNativeLibraryServiceError::Started)?;
        self.worker = Some(self.runtime.spawn(async move {
            let result = service.run().await;
            (service, result)
        }));
        Ok(self.port.clone())
    }

    pub fn is_shutdown_complete(&self) -> bool {
        self.closing
            && self.startup.is_none()
            && self.retained.is_none()
            && self.worker.is_none()
            && self.worker_failure.is_none()
            && self.shutdown.is_none()
    }

    pub async fn shutdown_retained(
        &mut self,
        deadline: Instant,
    ) -> Result<(), DesktopNativeLibraryServiceError> {
        self.closing = true;
        self.port.begin_closing();
        if let Some(service) = &mut self.startup {
            let result = service.shutdown_retained(deadline).await;
            if service.is_shutdown_complete() {
                self.startup = None;
            }
            result.map_err(|error| DesktopNativeLibraryServiceError::Service(Arc::new(error)))?;
        }
        let mut failure = None;
        if self.worker.is_some() {
            if self
                .worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
                && self.shutdown.is_none()
            {
                self.shutdown =
                    Some(self.port.try_shutdown(deadline).map_err(|error| {
                        DesktopNativeLibraryServiceError::Service(Arc::new(error))
                    })?);
            }
            if let Some(shutdown) = &mut self.shutdown {
                let result = timeout_at(deadline, shutdown.wait())
                    .await
                    .map_err(DesktopNativeLibraryServiceError::Deadline)?;
                self.shutdown = None;
                if let Err(error) = result {
                    failure = Some(DesktopNativeLibraryServiceError::Service(error));
                }
            }
            if let Some(worker) = &mut self.worker {
                let observed = timeout_at(deadline, worker)
                    .await
                    .map_err(DesktopNativeLibraryServiceError::Deadline)?;
                self.worker = None;
                match observed {
                    Ok((service, result)) => {
                        if !service.is_shutdown_complete() {
                            self.retained = Some(service);
                        }
                        if let Err(error) = result {
                            failure =
                                Some(DesktopNativeLibraryServiceError::Service(Arc::new(error)));
                        }
                    }
                    Err(error) => self.worker_failure = Some(Arc::new(error)),
                }
            }
        }
        if let Some(service) = &mut self.retained {
            let result = service.shutdown_retained(deadline).await;
            if service.is_shutdown_complete() {
                self.retained = None;
            }
            if let Err(error) = result {
                failure = Some(DesktopNativeLibraryServiceError::Service(Arc::new(error)));
            }
        }
        if let Some(error) = &self.worker_failure {
            return Err(DesktopNativeLibraryServiceError::Worker(Arc::clone(error)));
        }
        if let Some(error) = failure {
            return Err(error);
        }
        if !self.is_shutdown_complete() {
            return Err(DesktopNativeLibraryServiceError::Incomplete);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../../tests/desktop/native/library_service.rs"]
mod tests;
