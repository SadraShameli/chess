use crate::desktop::annotation_owner::DesktopAnnotationOwnerError;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceError, DesktopLibraryServiceOperation,
    DesktopLibraryServiceRequest,
};
use crate::game::move_node::GameMoveNodeAnnotationError;
use crate::game::tree::GameTreeError;
use crate::notation::game::NotationGameError;
use crate::presentation::analysis::annotations::{
    PresentationAnalysisAnnotations, PresentationAnalysisAnnotationsEnumDestination,
    PresentationAnalysisAnnotationsEnumGuardChoice,
};

#[tokio::test]
async fn invalid_notes_refused_by_the_real_service_allow_correction_and_discard()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let revision = fixture.seed(4300, true).await?;
        let publication = fixture.open(4300, revision).await?;
        let expected = publication.context();
        let mut observations = Vec::new();
        for correct in [true, false] {
        let mut panel = PresentationAnalysisAnnotations::new();
        panel.synchronize(&publication, None);
        panel.set_notes(expected, "invalid\0notes".to_owned())?;
        let switch = fixture.receipt()?;
        panel.request_switch(
            switch.request_id(),
            PresentationAnalysisAnnotationsEnumDestination::Navigate(publication.root()),
        )?;
        let application = fixture.receipt()?;
        let intent = panel
            .save_notes(application.request_id())?
            .ok_or("missing invalid-note intent")?;
        panel.attach_receipt(&intent, application.clone())?;
        let mut reply = fixture.port.try_submit(DesktopLibraryServiceRequest::new(
            application.clone(),
            DesktopLibraryServiceOperation::new(
                fixture.workspace,
                DesktopLibraryServiceEnumAction::Editor {
                    expected,
                    action: intent.into_action(),
                },
            ),
        ))?;
        let error = tokio::select! {
            result = reply.wait() => result.err().ok_or("invalid notes were accepted")?,
            result = Box::pin(fixture.service.run()) => return Err(format!("service ended: {result:?}").into()),
            result = Box::pin(fixture.registry.run()) => return Err(format!("registry ended: {result:?}").into()),
        };
        let validation_refused = matches!(error.as_ref(),
            DesktopLibraryServiceError::Annotation(source)
                if matches!(source.as_ref(), DesktopAnnotationOwnerError::Retained(error)
                    if matches!(error.as_ref(), DesktopAnnotationOwnerError::Notation(
                    NotationGameError::Tree(GameTreeError::Annotation(GameMoveNodeAnnotationError::InvalidNotes))
                )))
        );
        panel.observe_service_error(&application, expected, &error)?;
        let retained_notes = panel.notes().map(str::to_owned);
        let retained_guard = panel.has_guard();
        let (allowed, error) = if correct {
            panel.set_notes(expected, "corrected notes".to_owned())?;
            let corrected = fixture.receipt()?;
            let correction = panel.save_notes(corrected.request_id());
            (matches!(&correction, Ok(Some(_))), correction.err())
        } else {
            let discard = panel.resolve_guard(
            PresentationAnalysisAnnotationsEnumGuardChoice::Discard,
            fixture.receipt()?.request_id(),
            );
            (discard.is_ok() && !panel.has_guard() && !panel.has_notes_changes(), discard.err())
        };
        observations.push((validation_refused, retained_notes, retained_guard, allowed, error));
        }
        Ok(observations)
    }.await;
    fixture.close().await?;
    for (validation_refused, retained_notes, retained_guard, allowed, error) in observation? {
        assert!(validation_refused);
        assert_eq!(retained_notes.as_deref(), Some("invalid\0notes"));
        assert!(retained_guard);
        assert!(
            allowed,
            "service refused the invalid note without retaining work, but correction or discard remained blocked: {error:?}"
        );
    }
    Ok(())
}
