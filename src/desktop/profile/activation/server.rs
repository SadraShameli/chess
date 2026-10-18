use std::future::Future;
use std::sync::Arc;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{Semaphore, watch};
use tokio::task::{JoinError, JoinSet};

use crate::desktop::profile::activation::{
    DesktopProfileActivationBinding, DesktopProfileActivationEnumResult,
    DesktopProfileActivationError, DesktopProfileActivationExchange, DesktopProfileActivationPeer,
    DesktopProfileActivationWindow,
};

pub trait DesktopProfileActivationAcceptor {
    type Stream: AsyncRead + AsyncWrite + Unpin + DesktopProfileActivationPeer + Send + 'static;

    fn accept(
        &mut self,
    ) -> impl Future<Output = Result<Self::Stream, DesktopProfileActivationError>> + Send;
}

#[derive(Debug, Error)]
pub enum DesktopProfileActivationServerError {
    #[error("The activation listener failed: {0}")]
    Listener(#[from] DesktopProfileActivationError),
    #[error("An activation handler failed: {0}")]
    Handler(#[from] JoinError),
    #[error("The activation admission semaphore was closed")]
    AdmissionClosed,
    #[error("The activation report counter is exhausted")]
    CounterOverflow,
    #[error("Activation service failed ({operation}); cleanup also failed ({cleanup})")]
    Cleanup {
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
}

#[derive(Debug, Default)]
pub struct DesktopProfileActivationServerReport {
    accepted: u64,
    completed: u64,
    refused: u64,
    last_refusal: Option<DesktopProfileActivationError>,
}

impl DesktopProfileActivationServerReport {
    pub fn accepted(&self) -> u64 {
        self.accepted
    }

    pub fn completed(&self) -> u64 {
        self.completed
    }

    pub fn refused(&self) -> u64 {
        self.refused
    }

    pub fn last_refusal(&self) -> Option<&DesktopProfileActivationError> {
        self.last_refusal.as_ref()
    }

    fn accepted_connection(&mut self) -> Result<(), DesktopProfileActivationServerError> {
        self.accepted = self
            .accepted
            .checked_add(1)
            .ok_or(DesktopProfileActivationServerError::CounterOverflow)?;
        Ok(())
    }

    fn observe(
        &mut self,
        result: Result<
            Result<DesktopProfileActivationEnumResult, DesktopProfileActivationError>,
            JoinError,
        >,
    ) -> Result<(), DesktopProfileActivationServerError> {
        let counter = match result? {
            Ok(_) => &mut self.completed,
            Err(error) => {
                self.last_refusal = Some(error);
                &mut self.refused
            }
        };
        *counter = counter
            .checked_add(1)
            .ok_or(DesktopProfileActivationServerError::CounterOverflow)?;
        Ok(())
    }
}

pub struct DesktopProfileActivationServer;

impl DesktopProfileActivationServer {
    pub async fn run<L, W>(
        listener: &mut L,
        binding: DesktopProfileActivationBinding,
        window: Arc<W>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<DesktopProfileActivationServerReport, DesktopProfileActivationServerError>
    where
        L: DesktopProfileActivationAcceptor,
        W: DesktopProfileActivationWindow + 'static,
    {
        let capacity = Arc::new(Semaphore::new(
            DesktopProfileActivationExchange::CLIENT_CAPACITY,
        ));
        let mut handlers = JoinSet::new();
        let mut pending = None;
        let mut report = DesktopProfileActivationServerReport::default();
        let mut outcome = loop {
            if *shutdown.borrow() {
                break Ok(());
            }
            if pending.is_none() {
                match Arc::clone(&capacity).try_acquire_owned() {
                    Ok(permit) => pending = Some(permit),
                    Err(tokio::sync::TryAcquireError::NoPermits) => {}
                    Err(tokio::sync::TryAcquireError::Closed) => {
                        break Err(DesktopProfileActivationServerError::AdmissionClosed);
                    }
                }
            }
            tokio::select! {
                biased;
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow_and_update() {
                        break Ok(());
                    }
                }
                result = handlers.join_next(), if !handlers.is_empty() => {
                    if let Some(result) = result
                        && let Err(error) = report.observe(result) {
                        break Err(error);
                    }
                }
                result = listener.accept(), if pending.is_some() => {
                    match result {
                        Ok(stream) => {
                            let accepted_at = tokio::time::Instant::now();
                            let Some(permit) = pending.take() else {
                                break Err(DesktopProfileActivationServerError::AdmissionClosed);
                            };
                            if let Err(error) = report.accepted_connection() {
                                break Err(error);
                            }
                            let window = Arc::clone(&window);
                            handlers.spawn(async move {
                                let _permit = permit;
                                DesktopProfileActivationExchange::serve(
                                    stream,
                                    binding,
                                    window.as_ref(),
                                    accepted_at,
                                )
                                .await
                            });
                        }
                        Err(error) => break Err(error.into()),
                    }
                }
            }
        };
        drop(pending);
        while let Some(result) = handlers.join_next().await {
            if let Err(cleanup) = report.observe(result) {
                outcome = Err(match outcome {
                    Ok(()) => cleanup,
                    Err(operation) => DesktopProfileActivationServerError::Cleanup {
                        operation: Box::new(operation),
                        cleanup: Box::new(cleanup),
                    },
                });
            }
        }
        outcome?;
        Ok(report)
    }
}
