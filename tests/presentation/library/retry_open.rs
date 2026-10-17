use std::sync::Arc;

use crate::clock::duration::ClockInstant;
use crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumAction;
use crate::desktop::game_registry::DesktopGameRegistryRestored;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumDisposition,
    DesktopLibraryServiceEnumResult,
};
use crate::desktop::messages::DesktopMessagesEnumResult;
use crate::presentation::library::controller::PresentationLibraryController;
use crate::presentation::library::intent::PresentationLibraryIntent;
use crate::presentation::library::tests::fixture::PresentationLibraryTestFixture;

impl PresentationLibraryTestFixture {
    async fn prepare_open_retry(
        &mut self,
    ) -> DesktopLibraryServiceTestResult<(PresentationLibraryController, PresentationLibraryIntent)>
    {
        let mut session = DesktopLibraryServiceFixture::played(100)?;
        session.begin_analysis(session.revision(), ClockInstant::from_nanos(50))?;
        let revision = self.service.seed_played(&session).await?;
        let game = session.id();
        self.service
            .registry
            .restore(DesktopGameRegistryRestored {
                session,
                storage_revision: revision,
            })
            .await?;
        let mut controller = PresentationLibraryController::new(self.service.workspace);
        let query = controller
            .refresh(DesktopRequestId::new())?
            .ok_or("missing query")?;
        self.execute(&mut controller, &query).await??;
        let open = controller
            .open_saved(DesktopRequestId::new(), game)?
            .ok_or("missing Open")?;
        Ok((controller, open))
    }
}

#[tokio::test]
async fn certified_open_retry_preserves_original_binding_across_fresh_application_failures()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let (mut controller, open) = fixture.prepare_open_retry().await?;
    let game = DesktopLibraryServiceFixture::id(100)?;
    let pressure = fixture.service.fill_memory()?;
    let original_error = fixture
        .execute(&mut controller, &open)
        .await?
        .err()
        .ok_or("Open did not hit real publication pressure")?;
    let original_request = controller
        .retained_request(&open)
        .ok_or("missing original request")?
        .clone();
    let original_receipt = controller
        .retained_receipt(&open)
        .ok_or("missing original receipt")?
        .clone();
    let DesktopLibraryServiceEnumDisposition::Retained {
        request,
        expected: Some(certified),
    } = original_error.disposition()
    else {
        return Err("missing service-certified lower Open context".into());
    };
    let first = controller
        .retry_open(DesktopRequestId::new())?
        .ok_or("missing first retry")?;
    let exact = matches!(first.operation().action(), DesktopLibraryServiceEnumAction::Editor { expected, action: DesktopAnnotationCommandsEnumAction::RetrySave } if *expected == certified);
    let repeated = controller.retry_open(first.input_id())?.is_none();
    let retried_error = fixture
        .execute(&mut controller, &first)
        .await?
        .err()
        .ok_or("retry lost actual pressure")?;
    let retry_receipt = controller
        .retained_receipt(&first)
        .ok_or("missing retry receipt")?
        .clone();
    let frozen = controller
        .retained_request(&open)
        .is_some_and(|held| held.request_id() == original_request.request_id())
        && controller
            .retained_receipt(&open)
            .is_some_and(|held| held.shares_outcome(&original_receipt));
    let older_origin = matches!(retried_error.disposition(), DesktopLibraryServiceEnumDisposition::Retained { request: origin, expected: Some(context) } if origin == request && origin == original_receipt.request_id() && origin != retry_receipt.request_id() && context == certified);
    drop(pressure);
    let second = controller
        .retry_open(DesktopRequestId::new())?
        .ok_or("missing fresh recovery retry")?;
    let result = fixture.execute(&mut controller, &second).await??;
    let recovered = matches!(&result, DesktopLibraryServiceEnumResult::Opened(publication) if publication.context().game() == game && publication.context().epoch() == certified.epoch());
    let old_failure = matches!(original_receipt.response(), Some(DesktopMessagesEnumResult::LibraryFailure(error)) if Arc::ptr_eq(error, &original_error));
    let finished = !controller.is_open_pending() && !controller.can_retry_open();
    drop((
        controller,
        original_request,
        original_receipt,
        original_error,
        retry_receipt,
        retried_error,
        result,
    ));
    fixture.close().await?;
    assert!(exact && repeated && frozen && older_origin && recovered && old_failure && finished);
    Ok(())
}

#[tokio::test]
async fn old_actual_publication_and_original_app_receipt_cannot_complete_fresh_open_retry()
-> DesktopLibraryServiceTestResult {
    let mut old = DesktopLibraryServiceFixture::new().await?;
    let mut session = DesktopLibraryServiceFixture::played(100)?;
    session.begin_analysis(session.revision(), ClockInstant::from_nanos(50))?;
    let revision = old.seed_played(&session).await?;
    let old_publication = old.open(100, revision).await?;
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let (mut controller, open) = fixture.prepare_open_retry().await?;
    let pressure = fixture.service.fill_memory()?;
    let error = fixture
        .execute(&mut controller, &open)
        .await?
        .err()
        .ok_or("missing real Open failure")?;
    let original = controller
        .retained_receipt(&open)
        .ok_or("missing original")?
        .clone();
    let retry = controller
        .retry_open(DesktopRequestId::new())?
        .ok_or("missing retry")?;
    let (current, work) = fixture.admit(&mut controller, &retry)?;
    let wrong_owner = controller
        .observe_result(
            &current,
            retry.operation(),
            &DesktopLibraryServiceEnumResult::Opened(old_publication.clone()),
        )
        .is_err();
    let old_receipt = controller
        .observe_error(&original, retry.operation(), Arc::clone(&error))
        .is_err();
    drop(pressure);
    let mut lower = fixture.service.port.try_submit(
        crate::desktop::library_service::DesktopLibraryServiceRequest::new(
            current.clone(),
            retry.operation().clone(),
        ),
    )?;
    let result = fixture.service.drive(&mut lower).await?;
    fixture
        .application
        .complete_library(&work, &mut Some(Ok(result.clone())))?;
    controller.observe_result(&current, retry.operation(), &result)?;
    let final_open = !controller.is_open_pending();
    drop((
        lower,
        result,
        error,
        current,
        original,
        controller,
        old_publication,
    ));
    fixture.close().await?;
    old.close().await?;
    assert!(wrong_owner && old_receipt && final_open);
    Ok(())
}
