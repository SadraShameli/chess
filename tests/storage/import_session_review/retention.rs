use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::storage::import_session::StorageImportSessionError;
use crate::storage::import_session::stage::tests::review::support::{
    StorageImportSessionReviewFixture, StorageImportSessionReviewPollBudget,
};

#[derive(Clone, Copy)]
enum StorageImportSessionReviewRetentionEnumPrimary {
    Parser,
    Io,
}

struct StorageImportSessionReviewRetentionObservation {
    has_expected_primary: bool,
    first_kept_primary: bool,
    second_kept_primary: Option<bool>,
}

impl StorageImportSessionReviewRetentionEnumPrimary {
    fn source(self) -> &'static str {
        match self {
            Self::Parser => "1. e4 *\n1. d4 {unterminated",
            Self::Io => "1. e4 *",
        }
    }

    fn matches(self, error: Option<&StorageImportSessionError>) -> bool {
        match self {
            Self::Parser => matches!(error, Some(StorageImportSessionError::Parser(_error))),
            Self::Io => {
                if let Some(StorageImportSessionError::Io(error)) = error {
                    error.kind() == std::io::ErrorKind::StorageFull
                } else {
                    false
                }
            }
        }
    }
}

impl StorageImportSessionReviewRetentionObservation {
    fn retains(
        error: &Arc<StorageImportSessionError>,
        primary: &Arc<StorageImportSessionError>,
    ) -> bool {
        if Arc::ptr_eq(error, primary) {
            return true;
        }
        if let StorageImportSessionError::Cleanup {
            primary: nested, ..
        } = error.as_ref()
        {
            return Self::retains(nested, primary);
        }
        if let StorageImportSessionError::Retained(nested) = error.as_ref() {
            return Self::retains(nested, primary);
        }
        false
    }

    fn contains(
        error: Option<&Arc<StorageImportSessionError>>,
        primary: Option<&Arc<StorageImportSessionError>>,
    ) -> bool {
        match (error, primary) {
            (Some(error), Some(primary)) => Self::retains(error, primary),
            (None, _) | (_, None) => false,
        }
    }

    async fn repeated(
        kind: StorageImportSessionReviewRetentionEnumPrimary,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let fixture = StorageImportSessionReviewFixture::new(false).await?;
        fixture.filesystem.source_io_failure.store(
            matches!(kind, StorageImportSessionReviewRetentionEnumPrimary::Io),
            Ordering::SeqCst,
        );
        fixture
            .filesystem
            .cleanup_failures
            .store(2, Ordering::SeqCst);
        let mut session = fixture.session(kind.source()).await?;
        let primary = session.prepare().await.err();
        let first = session.cancel_retained().await.err();
        let second = session.cancel_retained().await.err();
        let observation = Self {
            has_expected_primary: kind.matches(primary.as_deref()),
            first_kept_primary: Self::contains(first.as_ref(), primary.as_ref()),
            second_kept_primary: Some(Self::contains(second.as_ref(), primary.as_ref())),
        };
        fixture
            .filesystem
            .cleanup_failures
            .store(0, Ordering::SeqCst);
        let drained = session.cancel_retained().await;
        drop(session);
        fixture.finish().await?;
        drained?;
        Ok(observation)
    }

    async fn dropped(
        kind: StorageImportSessionReviewRetentionEnumPrimary,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let fixture = StorageImportSessionReviewFixture::new(false).await?;
        fixture.filesystem.source_io_failure.store(
            matches!(kind, StorageImportSessionReviewRetentionEnumPrimary::Io),
            Ordering::SeqCst,
        );
        fixture
            .filesystem
            .cleanup_failures
            .store(2, Ordering::SeqCst);
        let mut session = fixture.session(kind.source()).await?;
        let primary = session.prepare().await.err();
        fixture.filesystem.cleanup_gate.arm();
        let reached = StorageImportSessionReviewPollBudget::reach(
            session.cancel_retained(),
            fixture.filesystem.cleanup_gate.entered(),
        )
        .await;
        fixture.filesystem.cleanup_gate.release().await?;
        let resumed = session.cancel_retained().await.err();
        let observation = Self {
            has_expected_primary: kind.matches(primary.as_deref()),
            first_kept_primary: Self::contains(resumed.as_ref(), primary.as_ref()),
            second_kept_primary: None,
        };
        fixture
            .filesystem
            .cleanup_failures
            .store(0, Ordering::SeqCst);
        let drained = session.cancel_retained().await;
        drop(session);
        fixture.finish().await?;
        drained?;
        reached?;
        Ok(observation)
    }
}

#[tokio::test]
async fn review_parser_primary_survives_two_cleanup_failures()
-> Result<(), Box<dyn std::error::Error>> {
    let observation = StorageImportSessionReviewRetentionObservation::repeated(
        StorageImportSessionReviewRetentionEnumPrimary::Parser,
    )
    .await?;
    assert!(observation.has_expected_primary);
    assert!(observation.first_kept_primary);
    assert_eq!(observation.second_kept_primary, Some(true));
    Ok(())
}

#[tokio::test]
async fn review_io_primary_survives_two_cleanup_failures() -> Result<(), Box<dyn std::error::Error>>
{
    let observation = StorageImportSessionReviewRetentionObservation::repeated(
        StorageImportSessionReviewRetentionEnumPrimary::Io,
    )
    .await?;
    assert!(observation.has_expected_primary);
    assert!(observation.first_kept_primary);
    assert_eq!(observation.second_kept_primary, Some(true));
    Ok(())
}

#[tokio::test]
async fn review_parser_primary_survives_dropped_cancel_wait()
-> Result<(), Box<dyn std::error::Error>> {
    let observation = StorageImportSessionReviewRetentionObservation::dropped(
        StorageImportSessionReviewRetentionEnumPrimary::Parser,
    )
    .await?;
    assert!(observation.has_expected_primary);
    assert!(observation.first_kept_primary);
    Ok(())
}

#[tokio::test]
async fn review_io_primary_survives_dropped_cancel_wait() -> Result<(), Box<dyn std::error::Error>>
{
    let observation = StorageImportSessionReviewRetentionObservation::dropped(
        StorageImportSessionReviewRetentionEnumPrimary::Io,
    )
    .await?;
    assert!(observation.has_expected_primary);
    assert!(observation.first_kept_primary);
    Ok(())
}
