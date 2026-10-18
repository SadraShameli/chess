use std::sync::Arc;

use tokio::sync::watch;

use crate::capacity::lease::CapacityLease;
use crate::desktop::engine_commands::{
    DesktopEngineCommandsAdmissionProvider, DesktopEngineCommandsAllocation,
    DesktopEngineCommandsEnumWorkload, DesktopEngineCommandsResourceRequest,
};
use crate::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
use crate::desktop::messages::DesktopMessagesEntityId;
use crate::desktop::native::game_resources::{
    DesktopNativeGameResources, DesktopNativeGameResourcesAnalysisReservation,
    DesktopNativeGameResourcesMemoryProvider,
};
use crate::engine::resource_config::EngineResourceConfig;
use crate::game::engine_provider::GameEngineContext;

pub struct DesktopNativeAnalysisResources<M> {
    owner: DesktopMessagesEntityId,
    resources: Arc<DesktopNativeGameResources<M>>,
}

impl<M: DesktopNativeGameResourcesMemoryProvider> DesktopNativeAnalysisResources<M> {
    pub const fn new(
        owner: DesktopMessagesEntityId,
        resources: Arc<DesktopNativeGameResources<M>>,
    ) -> Self {
        Self { owner, resources }
    }

    pub fn live_demand(&self) -> watch::Receiver<Option<GameEngineContext>> {
        self.resources.live_demand()
    }

    pub async fn initial(
        &self,
        resources: EngineResourceConfig,
    ) -> Result<(CapacityLease, DesktopNativeGameResourcesAnalysisReservation), DesktopFailure>
    {
        self.reserve(DesktopEngineCommandsResourceRequest::new(
            DesktopEngineCommandsEnumWorkload::Retry,
            resources,
            Some(self.resources.policy().startup_hash_mib()),
        ))
        .await
    }

    pub async fn reserve(
        &self,
        request: DesktopEngineCommandsResourceRequest,
    ) -> Result<(CapacityLease, DesktopNativeGameResourcesAnalysisReservation), DesktopFailure>
    {
        self.resources
            .reserve_analysis(self.owner, request)
            .await
            .map_err(|error| {
                DesktopFailure::new(
                    DesktopEnumFailureCode::ResourceRefusal,
                    error.to_string(),
                    true,
                )
            })
    }
}

impl<M: DesktopNativeGameResourcesMemoryProvider> DesktopEngineCommandsAdmissionProvider
    for DesktopNativeAnalysisResources<M>
{
    type Reservation = DesktopNativeGameResourcesAnalysisReservation;

    async fn admit(
        &mut self,
        request: DesktopEngineCommandsResourceRequest,
    ) -> Result<DesktopEngineCommandsAllocation<Self::Reservation>, DesktopFailure> {
        let (lease, reservation) = self.reserve(request).await?;
        Ok(DesktopEngineCommandsAllocation::new(lease, reservation))
    }

    async fn acknowledge(&mut self, reservation: &Self::Reservation) -> Result<(), DesktopFailure> {
        self.resources
            .acknowledge_analysis(self.owner, reservation)
            .await
            .map_err(|error| {
                DesktopFailure::new(
                    DesktopEnumFailureCode::ResourceRefusal,
                    error.to_string(),
                    true,
                )
            })
    }
}
