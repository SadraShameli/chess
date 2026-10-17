use crate::storage::import_session::stage::tests::review::support::{
    StorageImportSessionReviewEnumRead, StorageImportSessionReviewFixture,
    StorageImportSessionReviewPollBudget,
};

#[derive(Debug)]
struct StorageImportSessionReviewCancellationObservation {
    completed_before_release: bool,
    read_dropped_before_release: bool,
    read_active_before_release: bool,
    estimate_token_cancelled: Option<bool>,
}

impl StorageImportSessionReviewCancellationObservation {
    async fn observe(
        operation: StorageImportSessionReviewEnumRead,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let fixture = StorageImportSessionReviewFixture::new(true).await?;
        let mut session = fixture.session("1. d4 *").await?;
        if operation == StorageImportSessionReviewEnumRead::Estimate {
            session.prepare().await?;
        }
        fixture.repository.gate.arm(operation)?;
        let reached = if operation == StorageImportSessionReviewEnumRead::Estimate {
            StorageImportSessionReviewPollBudget::reach(
                session.freeze(),
                fixture.repository.gate.entered(),
            )
            .await
        } else {
            StorageImportSessionReviewPollBudget::reach(
                session.prepare(),
                fixture.repository.gate.entered(),
            )
            .await
        };
        let (observation, settlement) = {
            let mut cancelling = Box::pin(session.cancel_retained());
            let result = StorageImportSessionReviewPollBudget::observe(cancelling.as_mut()).await;
            let observation = Self {
                completed_before_release: result.as_ref().is_some_and(Result::is_ok),
                read_dropped_before_release: fixture.repository.gate.was_cancelled(),
                read_active_before_release: fixture.repository.gate.is_active(),
                estimate_token_cancelled: fixture
                    .repository
                    .estimate_token
                    .lock()
                    .map_err(|_poisoned| "poisoned estimate token")?
                    .as_ref()
                    .map(|token| token.is_cancelled()),
            };
            fixture.repository.gate.release();
            let settlement = match result {
                Some(result) => result,
                None => cancelling.await,
            };
            (observation, settlement)
        };
        let drained = session.cancel_retained().await;
        drop(session);
        fixture.finish().await?;
        drained?;
        settlement?;
        reached?;
        Ok(observation)
    }

    fn was_cancelled(&self) -> bool {
        !self.read_active_before_release
            && (self.completed_before_release || self.read_dropped_before_release)
    }
}

#[tokio::test]
async fn review_snapshot_wait_cancels_before_repository_release()
-> Result<(), Box<dyn std::error::Error>> {
    let observation = StorageImportSessionReviewCancellationObservation::observe(
        StorageImportSessionReviewEnumRead::Snapshot,
    )
    .await?;
    assert!(observation.was_cancelled(), "{observation:?}");
    Ok(())
}

#[tokio::test]
async fn review_candidates_wait_cancels_before_repository_release()
-> Result<(), Box<dyn std::error::Error>> {
    let observation = StorageImportSessionReviewCancellationObservation::observe(
        StorageImportSessionReviewEnumRead::Candidates,
    )
    .await?;
    assert!(observation.was_cancelled(), "{observation:?}");
    Ok(())
}

#[tokio::test]
async fn review_candidate_body_wait_cancels_before_repository_release()
-> Result<(), Box<dyn std::error::Error>> {
    let observation = StorageImportSessionReviewCancellationObservation::observe(
        StorageImportSessionReviewEnumRead::CandidateBody,
    )
    .await?;
    assert!(observation.was_cancelled(), "{observation:?}");
    Ok(())
}

#[tokio::test]
async fn review_estimate_wait_cancels_before_repository_release()
-> Result<(), Box<dyn std::error::Error>> {
    let observation = StorageImportSessionReviewCancellationObservation::observe(
        StorageImportSessionReviewEnumRead::Estimate,
    )
    .await?;
    assert!(observation.was_cancelled(), "{observation:?}");
    assert_eq!(
        observation.estimate_token_cancelled,
        Some(true),
        "{observation:?}"
    );
    Ok(())
}
