use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::capacity::executor::CapacityExecutor;
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::file_handle::DesktopFileEnumSelection;
use crate::engine::tablebase::location::{
    EngineTablebaseLocation, EngineTablebaseLocationPolicy, EngineTablebaseLocationSelection,
};
use crate::engine::tablebase::status::EngineTablebaseStatus;

pub trait DesktopEngineTablebasesPolicyProvider: Send + Sync {
    fn policy(&self) -> Result<EngineTablebaseLocationPolicy, DesktopFailure>;
}

pub struct DesktopEngineTablebasesPolicy {
    parent: PathBuf,
}

impl DesktopEngineTablebasesPolicy {
    pub fn new(parent: PathBuf) -> Self {
        Self { parent }
    }
}

impl DesktopEngineTablebasesPolicyProvider for DesktopEngineTablebasesPolicy {
    fn policy(&self) -> Result<EngineTablebaseLocationPolicy, DesktopFailure> {
        if cfg!(unix) {
            Ok(EngineTablebaseLocationPolicy::unix_temporary(
                self.parent.clone(),
            ))
        } else {
            Err(DesktopFailure::new(
                DesktopEnumFailureCode::ResourceRefusal,
                "Tablebase folder setup is unavailable on Windows in this build".to_owned(),
                false,
            ))
        }
    }
}

type DesktopEngineTablebasesPreparation =
    Pin<Box<dyn Future<Output = Result<EngineTablebaseLocation, DesktopFailure>> + Send>>;
type DesktopEngineTablebasesCleanup =
    Pin<Box<dyn Future<Output = Result<(), DesktopFailure>> + Send>>;

pub(crate) struct DesktopEngineTablebases {
    retained: Vec<Arc<Mutex<EngineTablebaseLocation>>>,
    status: Option<Arc<EngineTablebaseStatus>>,
    preparing: Option<DesktopEngineTablebasesPreparation>,
    preparation_receipt: Option<DesktopApplicationReceipt>,
    cleaning: Option<(usize, DesktopEngineTablebasesCleanup)>,
}

impl DesktopEngineTablebases {
    pub(crate) fn new() -> Self {
        Self {
            retained: Vec::new(),
            status: None,
            preparing: None,
            preparation_receipt: None,
            cleaning: None,
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.retained.is_empty() && self.preparing.is_none() && self.cleaning.is_none()
    }
    pub(crate) fn status(&self) -> Option<Arc<EngineTablebaseStatus>> {
        self.status.as_ref().map(Arc::clone)
    }
    pub(crate) fn record_configuration(
        &mut self,
        result: Result<
            crate::engine::options::EngineOptionsEnumOutcome,
            crate::engine::session::EngineSessionError,
        >,
    ) -> Result<(), DesktopFailure> {
        let selected = self
            .status
            .as_ref()
            .ok_or_else(|| Self::failure("No prepared tablebase selection remains owned"))?;
        let owner = self
            .retained
            .iter()
            .find_map(|owner| match owner.try_lock() {
                Ok(location) if location.engine_path() == selected.engine_path() => {
                    Some(Ok(Arc::clone(owner)))
                }
                Ok(_) => None,
                Err(error) => Some(Err(Self::effect(error))),
            })
            .ok_or_else(|| Self::failure("The selected tablebase aliases are unavailable"))??;
        let mut status = {
            let location = owner.try_lock().map_err(Self::effect)?;
            EngineTablebaseStatus::configured(&location)
        };
        status.record_configuration(result);
        self.status = Some(Arc::new(status));
        Ok(())
    }

    pub(crate) async fn prepare(
        &mut self,
        selection: DesktopFileEnumSelection,
        policy: EngineTablebaseLocationPolicy,
        receipt: DesktopApplicationReceipt,
        executor: &CapacityExecutor,
    ) -> Result<(), DesktopFailure> {
        const MAX_RETAINED: usize = 2;
        if self.preparing.is_some()
            || self.cleaning.is_some()
            || self.retained.len() >= MAX_RETAINED
        {
            return Err(Self::failure(
                "Complete or clean up the previous tablebase selection before preparing another",
            ));
        }
        let cancellation = receipt.cancellation();
        let job = executor
            .submit(move |worker| {
                let directory = selection.into_directory().map_err(Self::effect)?;
                let selection =
                    EngineTablebaseLocationSelection::new(vec![directory]).map_err(Self::effect)?;
                EngineTablebaseLocation::prepare_blocking(selection, policy, || {
                    worker.is_cancelled() || cancellation.is_requested()
                })
                .map_err(Self::effect)
            })
            .map_err(Self::effect)?;
        self.preparation_receipt = Some(receipt);
        self.preparing = Some(Box::pin(
            async move { job.join().await.map_err(Self::effect)? },
        ));
        let cancelled = self.observe_preparation().await?;
        if cancelled {
            let index = self.retained.len().checked_sub(1).ok_or_else(|| {
                Self::failure("Cancelled tablebase preparation lost its cleanup owner")
            })?;
            self.cleanup_index(index, executor).await?;
            return Err(DesktopFailure::new(
                DesktopEnumFailureCode::Cancelled,
                "Tablebase preparation was cancelled and cleaned up".to_owned(),
                true,
            ));
        }
        Ok(())
    }

    async fn observe_preparation(&mut self) -> Result<bool, DesktopFailure> {
        let Some(preparing) = &mut self.preparing else {
            return Ok(false);
        };
        let outcome = preparing.await;
        self.preparing = None;
        let cancelled = self
            .preparation_receipt
            .take()
            .is_some_and(|receipt| receipt.is_cancellation_requested());
        let location = outcome?;
        if !cancelled {
            self.status = Some(Arc::new(EngineTablebaseStatus::configured(&location)));
        }
        self.retained.push(Arc::new(Mutex::new(location)));
        Ok(cancelled)
    }

    async fn cleanup_index(
        &mut self,
        index: usize,
        executor: &CapacityExecutor,
    ) -> Result<(), DesktopFailure> {
        if self.cleaning.is_none() {
            let owner = Arc::clone(
                self.retained
                    .get(index)
                    .ok_or_else(|| Self::failure("Missing tablebase cleanup owner"))?,
            );
            let job = executor
                .submit(move |_cancellation| {
                    let mut location = owner.try_lock().map_err(Self::effect)?;
                    location.cleanup_blocking().map_err(Self::effect)
                })
                .map_err(Self::effect)?;
            self.cleaning = Some((
                index,
                Box::pin(async move { job.join().await.map_err(Self::effect)? }),
            ));
        }
        let (cleaning_index, future) = self
            .cleaning
            .as_mut()
            .ok_or_else(|| Self::failure("Missing tablebase cleanup operation"))?;
        let index = *cleaning_index;
        let result = future.await;
        self.cleaning = None;
        result?;
        self.retained.remove(index);
        Ok(())
    }

    pub(crate) async fn cleanup_retired(
        &mut self,
        executor: &CapacityExecutor,
    ) -> Result<(), DesktopFailure> {
        if let Some((index, _)) = &self.cleaning {
            self.cleanup_index(*index, executor).await?;
        }
        let selected = self
            .status
            .as_ref()
            .ok_or_else(|| Self::failure("No selected tablebase status remains"))?
            .engine_path()
            .clone();
        let mut index = 0;
        while index < self.retained.len() {
            let is_selected = self.retained[index]
                .try_lock()
                .map_err(Self::effect)?
                .engine_path()
                == &selected;
            if is_selected {
                index += 1;
            } else {
                self.cleanup_index(index, executor).await?;
            }
        }
        Ok(())
    }

    pub(crate) async fn cleanup(
        &mut self,
        executor: &CapacityExecutor,
        deadline: tokio::time::Instant,
    ) -> Result<(), DesktopFailure> {
        if let Some(receipt) = &self.preparation_receipt {
            receipt.cancellation().request();
        }
        tokio::time::timeout_at(deadline, self.finish_cleanup(executor))
            .await
            .map_err(Self::effect)?
    }

    async fn finish_cleanup(&mut self, executor: &CapacityExecutor) -> Result<(), DesktopFailure> {
        self.observe_preparation().await?;
        if let Some((index, _)) = &self.cleaning {
            self.cleanup_index(*index, executor).await?;
        }
        while let Some(index) = self.retained.len().checked_sub(1) {
            self.cleanup_index(index, executor).await?;
        }
        self.status = None;
        Ok(())
    }

    fn failure(message: &str) -> DesktopFailure {
        DesktopFailure::new(
            DesktopEnumFailureCode::FileUnavailable,
            message.to_owned(),
            true,
        )
    }
    fn effect(error: impl std::fmt::Display) -> DesktopFailure {
        Self::failure(&error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::time::Duration;

    use tokio::sync::oneshot;
    use tokio::time::{Instant, timeout_at};

    use crate::capacity::budget::CapacityBudget;
    use crate::capacity::executor::CapacityExecutor;
    use crate::desktop::engine_tablebases::DesktopEngineTablebases;

    #[tokio::test(start_paused = true)]
    async fn shutdown_deadline_retains_pending_preparation_until_observed()
    -> Result<(), Box<dyn std::error::Error>> {
        let executor = CapacityExecutor::uninitialized(CapacityBudget::new(NonZeroUsize::MIN)?);
        let mut tablebases = DesktopEngineTablebases::new();
        let (completion, waiting) = oneshot::channel();
        tablebases.preparing = Some(Box::pin(async move {
            waiting.await.map_err(DesktopEngineTablebases::effect)?
        }));
        let deadline = Instant::now() + Duration::from_millis(10);
        let observed = timeout_at(
            deadline + Duration::from_millis(10),
            tablebases.cleanup(&executor, deadline),
        )
        .await;
        assert!(
            observed.is_ok(),
            "tablebase cleanup ignored its shutdown deadline"
        );
        assert!(observed?.is_err());
        assert_eq!(Instant::now(), deadline);
        assert!(!tablebases.is_empty());
        assert!(tablebases.preparing.is_some());
        completion
            .send(Err(DesktopEngineTablebases::failure("preparation failed")))
            .map_err(|_result| "pending preparation was discarded")?;
        let error = tablebases
            .cleanup(&executor, Instant::now() + Duration::from_millis(10))
            .await;
        assert!(matches!(error, Err(ref failure) if failure.message() == "preparation failed"));
        assert!(tablebases.is_empty());
        executor.shutdown().await?;
        Ok(())
    }
}
