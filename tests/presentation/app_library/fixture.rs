use std::sync::Arc;

use bevy_egui::egui::{self, Context, RawInput};

use crate::board::enums::BoardEnumColor;
use crate::desktop::application::DesktopApplication;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestClock, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceRequest,
};
use crate::desktop::messages::DesktopEnumCommand;
use crate::desktop::native::event_loop::DesktopNativeEventLoopWake;
use crate::presentation::analysis::annotations::PresentationAnalysisAnnotationsEnumGuardChoice;
use crate::presentation::app::library::{
    PresentationAppLibrary, PresentationAppLibraryEnumNavigation,
};
use crate::presentation::app::{PresentationApp, PresentationAppEnumWorkspace};
use crate::presentation::board::scene::PresentationBoardScene;
use crate::scene::camera::SceneCamera;
use crate::scene::enums::SceneEnumLifecycle;
use crate::scene::renderer::SceneRendererGeneration;
use crate::storage::library_query::StorageLibraryQueryOptions;

pub(super) struct PresentationAppLibraryFixture {
    pub(super) source: DesktopLibraryServiceFixture,
    pub(super) application:
        DesktopApplication<DesktopLibraryServiceTestClock, DesktopNativeEventLoopWake>,
    pub(super) app: PresentationApp,
    context: Context,
}

impl PresentationAppLibraryFixture {
    pub(super) async fn new() -> DesktopLibraryServiceTestResult<Self> {
        let source = DesktopLibraryServiceFixture::new().await?;
        let clock = Arc::new(DesktopLibraryServiceTestClock);
        let application = DesktopApplication::new(
            Arc::clone(&clock),
            Arc::new(DesktopNativeEventLoopWake::new()),
        );
        let mut app = PresentationApp::new(clock);
        app.mount_library(source.workspace);
        app.select_workspace(PresentationAppEnumWorkspace::Library, &application.port())?;
        Ok(Self {
            source,
            application,
            app,
            context: Context::default(),
        })
    }

    pub(super) fn library(
        &mut self,
    ) -> DesktopLibraryServiceTestResult<&mut PresentationAppLibrary> {
        self.app
            .library
            .as_mut()
            .ok_or_else(|| "missing library workspace".into())
    }

    pub(super) fn frame(&mut self) -> DesktopLibraryServiceTestResult<egui::FullOutput> {
        let mut result = None;
        let mut output = self.context.run_ui(
            RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| {
                let mut picker = |_point| Ok(None);
                result = Some(
                    self.app
                        .show(ui, &self.application.port(), None, &mut picker),
                );
            },
        );
        output.textures_delta.clear();
        result.ok_or("missing application frame")??;
        Ok(output)
    }

    pub(super) fn has_text(output: &egui::FullOutput, label: &str) -> bool {
        output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(text) if text.galley.job.text == label))
    }

    pub(super) async fn query(&mut self) -> DesktopLibraryServiceTestResult {
        let port = self.application.port();
        let intent = self
            .library()?
            .panel
            .controller_mut()
            .query(
                DesktopRequestId::new(),
                StorageLibraryQueryOptions::default(),
            )?
            .ok_or("missing query intent")?;
        self.library()?.dispatch_panel(intent, &port)?;
        self.finish().await?;
        Ok(())
    }

    pub(super) fn request_open(&mut self, game: u128) -> DesktopLibraryServiceTestResult {
        let port = self.application.port();
        let intent = self
            .library()?
            .panel
            .controller_mut()
            .open_saved(
                DesktopRequestId::new(),
                DesktopLibraryServiceFixture::id(game)?,
            )?
            .ok_or("missing open intent")?;
        if let Some(PresentationAppLibraryEnumNavigation::OpenSaved(intent)) = self
            .library()?
            .request_navigation(PresentationAppLibraryEnumNavigation::OpenSaved(Box::new(
                intent,
            )))?
        {
            self.library()?.dispatch_panel(*intent, &port)?;
        }
        self.app.synchronize_library(&port)?;
        Ok(())
    }

    pub(super) async fn open(&mut self, game: u128) -> DesktopLibraryServiceTestResult {
        self.request_open(game)?;
        self.finish().await?;
        Ok(())
    }

    pub(super) fn resolve(
        &mut self,
        choice: PresentationAnalysisAnnotationsEnumGuardChoice,
    ) -> DesktopLibraryServiceTestResult {
        let port = self.application.port();
        if choice == PresentationAnalysisAnnotationsEnumGuardChoice::Save {
            self.library()?.save_notes(true, &port)?;
        } else if let Some(effect) = self
            .library()?
            .annotations
            .resolve_guard(choice, DesktopRequestId::new())?
        {
            self.library()?.annotation_effect(effect, &port)?;
        }
        self.app.synchronize_library(&port)?;
        Ok(())
    }

    pub(super) fn notes(&mut self, text: &str) -> DesktopLibraryServiceTestResult {
        let library = self.library()?;
        let expected = library
            .annotations
            .context()
            .ok_or("missing notes context")?;
        library.annotations.set_notes(expected, text.to_owned())?;
        Ok(())
    }

    pub(super) async fn finish(
        &mut self,
    ) -> DesktopLibraryServiceTestResult<DesktopLibraryServiceEnumResult> {
        let work = self
            .application
            .try_next()?
            .ok_or("missing application work")?;
        let DesktopEnumCommand::Library(operation) = work.request().command().operation() else {
            return Err("unexpected application work".into());
        };
        self.application.begin_commit(&work)?;
        let mut receipt = self
            .source
            .port
            .try_submit(DesktopLibraryServiceRequest::new(
                work.receipt().clone(),
                operation.clone(),
            ))?;
        let result = self.source.drive(&mut receipt).await?;
        self.application
            .complete_library(&work, &mut Some(Ok(result.clone())))?;
        self.app.synchronize_library(&self.application.port())?;
        Ok(result)
    }

    pub(super) async fn finish_failure(&mut self) -> DesktopLibraryServiceTestResult {
        let work = self
            .application
            .try_next()?
            .ok_or("missing application work")?;
        let DesktopEnumCommand::Library(operation) = work.request().command().operation() else {
            return Err("unexpected application work".into());
        };
        self.application.begin_commit(&work)?;
        let mut receipt = self
            .source
            .port
            .try_submit(DesktopLibraryServiceRequest::new(
                work.receipt().clone(),
                operation.clone(),
            ))?;
        let error = tokio::select! {
            result = receipt.wait() => result.err().ok_or("expected save failure")?,
            result = Box::pin(self.source.service.run()) => return Err(format!("service ended: {result:?}").into()),
            result = Box::pin(self.source.registry.run()) => return Err(format!("registry ended: {result:?}").into()),
        };
        self.application
            .complete_library(&work, &mut Some(Err(error)))?;
        self.app.synchronize_library(&self.application.port())?;
        Ok(())
    }

    pub(super) async fn drain(&mut self) -> DesktopLibraryServiceTestResult {
        for _ in 0..8 {
            if !self.app.has_pending_work() {
                return Ok(());
            }
            self.finish().await?;
        }
        Err("application requests did not drain".into())
    }

    pub(super) fn assert_scene(&mut self) -> DesktopLibraryServiceTestResult {
        let mut viewport = None;
        let mut result = None;
        for _ in 0..2 {
            let mut output = self.context.run_ui(
                RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1440.0, 900.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let mut picker = |_point| Ok(None);
                    result = Some(
                        self.app
                            .show(ui, &self.application.port(), None, &mut picker),
                    );
                },
            );
            output.textures_delta.clear();
            viewport = result
                .take()
                .ok_or("missing application frame")??
                .viewport();
            drop(output);
        }
        let viewport = viewport.ok_or("missing native 3D viewport")?;
        let scene = PresentationBoardScene::new(
            egui::TextureId::User(37),
            SceneRendererGeneration::initial(),
            SceneEnumLifecycle::Ready,
            viewport,
            SceneCamera::initial(BoardEnumColor::White).fit(viewport.physical_size())?,
        );
        let mut output = self.context.run_ui(
            RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| {
                let mut picker = |_point| Ok(None);
                result =
                    Some(
                        self.app
                            .show(ui, &self.application.port(), Some(&scene), &mut picker),
                    );
            },
        );
        output.textures_delta.clear();
        assert!(result.ok_or("missing scene frame")??.viewport().is_some());
        assert!(Self::has_text(&output, "Notes and Markings"));
        Ok(())
    }

    pub(super) fn close(
        self,
    ) -> std::pin::Pin<Box<impl std::future::Future<Output = DesktopLibraryServiceTestResult>>>
    {
        drop((self.app, self.application, self.context));
        Box::pin(self.source.close())
    }
}
