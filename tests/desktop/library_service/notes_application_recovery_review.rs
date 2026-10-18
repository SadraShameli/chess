use crate::desktop::library_service::tests::application_recovery_fixture as application_fixture;
#[path = "notes_application_recovery_review_fixture.rs"]
mod fixture;

use crate::desktop::library_service::tests::fixture::DesktopLibraryServiceTestResult;
use crate::desktop::library_service::tests::notes_application_recovery_review::fixture::{
    DesktopLibraryServiceNotesApplicationRecoveryReviewEnumCase,
    DesktopLibraryServiceNotesApplicationRecoveryReviewObservation,
};

#[tokio::test]
async fn retained_notes_failure_emits_retry_save_bound_to_a_fresh_actual_application_request()
-> DesktopLibraryServiceTestResult {
    let observation = Box::pin(
        DesktopLibraryServiceNotesApplicationRecoveryReviewObservation::capture(
            DesktopLibraryServiceNotesApplicationRecoveryReviewEnumCase::PresentationNotationSql,
        ),
    )
    .await?;
    observation.assert_fresh_panel_intent()?;
    observation.assert_recovered()
}

#[tokio::test]
async fn fresh_notation_application_retry_accepts_the_original_certified_binding_after_sqlite_failure()
-> DesktopLibraryServiceTestResult {
    Box::pin(
        DesktopLibraryServiceNotesApplicationRecoveryReviewObservation::capture(
            DesktopLibraryServiceNotesApplicationRecoveryReviewEnumCase::NotationSql,
        ),
    )
    .await?
    .assert_recovered()
}

#[tokio::test]
async fn fresh_played_application_retry_recovers_receiving_publication_pressure_without_replaying_notes()
-> DesktopLibraryServiceTestResult {
    Box::pin(
        DesktopLibraryServiceNotesApplicationRecoveryReviewObservation::capture(
            DesktopLibraryServiceNotesApplicationRecoveryReviewEnumCase::PlayedPublication,
        ),
    )
    .await?
    .assert_recovered()
}
