use std::io::Cursor;
use std::sync::Arc;

use crate::desktop::application::{
    DesktopApplication, DesktopApplicationReceipt, DesktopApplicationWakeError,
    DesktopApplicationWakeProvider, DesktopApplicationWork,
};
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestClock, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceError, DesktopLibraryServiceRequest,
};
use crate::desktop::messages::{DesktopCommand, DesktopRequest};
use crate::game::enums::GameId;
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::storage_repository::GameStorageRevision;
use crate::notation::pgn_import::NotationPgnImport;
use crate::presentation::library::controller::PresentationLibraryController;
use crate::presentation::library::intent::PresentationLibraryIntent;
use crate::storage::game_document::StorageGameDocument;
use crate::storage::library_prepared::StorageLibraryEnumContentChange;
use crate::storage::library_repository::StorageLibraryRepository;

pub(super) struct PresentationLibraryTestWake;

impl DesktopApplicationWakeProvider for PresentationLibraryTestWake {
    fn wake(&self) -> Result<(), DesktopApplicationWakeError> {
        Ok(())
    }
}

pub(super) struct PresentationLibraryTestFixture {
    pub(super) service: DesktopLibraryServiceFixture,
    pub(super) application:
        DesktopApplication<DesktopLibraryServiceTestClock, PresentationLibraryTestWake>,
}

impl PresentationLibraryTestFixture {
    pub(super) async fn new() -> DesktopLibraryServiceTestResult<Self> {
        Ok(Self {
            service: DesktopLibraryServiceFixture::new().await?,
            application: DesktopApplication::new(
                Arc::new(DesktopLibraryServiceTestClock),
                Arc::new(PresentationLibraryTestWake),
            ),
        })
    }

    pub(super) fn request(
        &self,
        intent: &PresentationLibraryIntent,
    ) -> DesktopLibraryServiceTestResult<DesktopRequest> {
        Ok(self
            .application
            .port()
            .request(DesktopCommand::library(intent.operation().clone()))?)
    }

    pub(super) async fn seed_notation(
        &self,
        id: GameId,
        pgn: &str,
    ) -> DesktopLibraryServiceTestResult {
        let mut node = 0_u128;
        let mut parser = NotationPgnImport::new(Cursor::new(pgn), move || {
            node += 1;
            uuid::Uuid::from_u128(node).to_string().parse()
        });
        let game = parser
            .next_game(&|| false)?
            .ok_or("missing notation game")?;
        let body = StorageGameDocument::capture_notation(id, &game)?.json()?;
        let repository =
            StorageLibraryRepository::new(self.service.store.client(), &self.service.executor);
        let prepared = repository
            .prepare_write(
                id,
                GameStorageRevision::INITIAL,
                body,
                StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(
                    20,
                )?),
            )
            .await?;
        repository.begin_save(prepared)?.wait().await?;
        Ok(())
    }

    pub(super) fn admit(
        &self,
        controller: &mut PresentationLibraryController,
        intent: &PresentationLibraryIntent,
    ) -> DesktopLibraryServiceTestResult<(DesktopApplicationReceipt, DesktopApplicationWork)> {
        let request = self.request(intent)?;
        controller.attach_request(intent, request.clone())?;
        let receipt = self.application.port().try_submit(&request)?;
        controller.attach_receipt(intent, receipt.clone())?;
        let work = self
            .application
            .try_next()?
            .ok_or("missing admitted work")?;
        if intent.operation().mutation_entity().is_some() {
            self.application.begin_commit(&work)?;
        }
        Ok((receipt, work))
    }

    pub(super) async fn execute(
        &mut self,
        controller: &mut PresentationLibraryController,
        intent: &PresentationLibraryIntent,
    ) -> DesktopLibraryServiceTestResult<
        Result<DesktopLibraryServiceEnumResult, Arc<DesktopLibraryServiceError>>,
    > {
        let (receipt, work) = self.admit(controller, intent)?;
        let mut service_receipt =
            self.service
                .port
                .try_submit(DesktopLibraryServiceRequest::new(
                    receipt.clone(),
                    intent.operation().clone(),
                ))?;
        let response = tokio::select! {
            response = service_receipt.wait() => response,
            response = Box::pin(self.service.service.run()) => {
                return Err(format!("service exited: {response:?}").into());
            }
            response = Box::pin(self.service.registry.run()) => {
                return Err(format!("registry exited: {response:?}").into());
            }
        };
        let mut completion = Some(response.clone());
        self.application.complete_library(&work, &mut completion)?;
        match &response {
            Ok(result) => controller.observe_result(&receipt, intent.operation(), result)?,
            Err(error) => {
                controller.observe_error(&receipt, intent.operation(), Arc::clone(error))?
            }
        }
        Ok(response)
    }

    pub(super) async fn close(self) -> DesktopLibraryServiceTestResult {
        drop(self.application);
        self.service.close().await
    }
}
