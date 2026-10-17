use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::capacity::executor::CapacityExecutor;
use crate::game::enums::GameId;
use crate::game::library_organization::{
    GameLibraryOrganizationState, GameLibraryOrganizationTimestamp,
};
use crate::storage::document_store::StorageDocumentStore;
use crate::storage::import_session::{
    StorageImportSessionEnumCommitOutcome, StorageImportSessionError,
    StorageImportSessionPageRequest, StorageImportSessionRepository,
};
use crate::storage::library_import::tests::StorageLibraryImportTestResult;
use crate::storage::library_import::tests::space_review::{
    StorageLibraryImportSpaceReview, StorageLibraryImportSpaceReviewFiles,
    StorageLibraryImportSpaceReviewFilesystem, StorageLibraryImportSpaceReviewSession,
};
use crate::storage::library_repository::StorageLibraryRepository;
use crate::storage::trash::StorageTrashEnumLocation;

#[derive(Clone, Copy)]
enum StorageLibraryImportPopulatedReviewEnumBatch {
    Existing,
    Incoming,
}

impl StorageLibraryImportPopulatedReviewEnumBatch {
    const fn name(self) -> &'static str {
        match self {
            Self::Existing => "existing",
            Self::Incoming => "incoming",
        }
    }

    const fn seed(self) -> u64 {
        match self {
            Self::Existing => 1_000_100,
            Self::Incoming => 2_000_100,
        }
    }

    const fn movement(self) -> &'static str {
        match self {
            Self::Existing => "d4",
            Self::Incoming => "e4",
        }
    }

    fn white(self, index: u64) -> String {
        format!("Populated {} {index:04}", self.name())
    }

    fn annotation(self, index: u64) -> String {
        format!("populated {} λ {index:04}", self.name())
    }

    fn pgn(self, count: u64) -> String {
        (0..count)
            .map(|index| {
                format!(
                    "[White \"{}\"]\n[Black \"Fixed opponent\"]\n[Date \"2026.01.01\"]\n[Result \"*\"]\n\n1. {} {{{}}} *\n\n",
                    self.white(index),
                    self.movement(),
                    self.annotation(index),
                )
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StorageLibraryImportPopulatedReviewBinding {
    game: GameId,
    revision: u64,
    body: String,
    organization: GameLibraryOrganizationState,
    location: StorageTrashEnumLocation,
    modified: Option<GameLibraryOrganizationTimestamp>,
}

impl StorageLibraryImportPopulatedReviewBinding {
    fn assert_same(actual: &[Self], expected: &[Self]) {
        assert_eq!(actual.len(), expected.len(), "populated binding count");
        for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
            assert!(
                actual == expected,
                "populated binding changed at index={index}, actual_game={}, expected_game={}",
                actual.game.as_uuid(),
                expected.game.as_uuid(),
            );
        }
    }

    async fn read(
        store: &StorageDocumentStore,
        executor: &CapacityExecutor,
        games: &[GameId],
    ) -> StorageLibraryImportTestResult<Vec<Self>> {
        let library = StorageLibraryRepository::new(store.client(), executor);
        let mut bindings = Vec::with_capacity(games.len());
        for &game in games {
            let document = store
                .client()
                .read(game.as_uuid())
                .await?
                .ok_or("missing populated review document")?;
            let loaded = library
                .load(game)
                .await?
                .ok_or("missing populated review library binding")?;
            assert_eq!(loaded.record().id(), game);
            assert_eq!(loaded.content_revision().value(), document.revision());
            assert_eq!(document.revision(), 1);
            bindings.push(Self {
                game,
                revision: document.revision(),
                body: document.body().to_owned(),
                organization: loaded.organization().clone(),
                location: loaded.location(),
                modified: loaded.modified(),
            });
        }
        Ok(bindings)
    }
}

struct StorageLibraryImportPopulatedReviewStage {
    session: StorageLibraryImportSpaceReviewSession,
    filesystem: Arc<StorageLibraryImportSpaceReviewFilesystem>,
    games: Vec<GameId>,
    source: u64,
}

impl StorageLibraryImportPopulatedReviewStage {
    async fn drain(self, executor: &CapacityExecutor) -> StorageLibraryImportTestResult {
        let Self {
            mut session,
            filesystem,
            ..
        } = self;
        session.cancel_retained().await?;
        drop(session);
        executor
            .submit_waiting(move |_| {
                assert!(
                    filesystem
                        .files
                        .lock()
                        .map_err(|_poisoned| std::io::Error::other(
                            "poisoned populated review spools"
                        ))?
                        .is_empty()
                );
                Ok::<(), std::io::Error>(())
            })
            .await?
            .join()
            .await??;
        Ok(())
    }
}

#[derive(Debug)]
struct StorageLibraryImportPopulatedReviewAttempt {
    admission: Result<Arc<StorageImportSessionEnumCommitOutcome>, Arc<StorageImportSessionError>>,
    sqlite: Option<u64>,
    before: StorageLibraryImportSpaceReviewFiles,
    committed: StorageLibraryImportSpaceReviewFiles,
    checkpointed: StorageLibraryImportSpaceReviewFiles,
    stage: u64,
    source: u64,
    database: u64,
    additional: u64,
    writer_calls: usize,
}

impl StorageLibraryImportPopulatedReviewAttempt {
    fn is_committed(&self, count: u64) -> bool {
        matches!(&self.admission, Ok(outcome) if matches!(outcome.as_ref(), StorageImportSessionEnumCommitOutcome::Committed(receipt) if receipt.count() == count))
    }

    fn assert_sufficient_control(&self, count: u64, allowance: u64) {
        assert!(self.is_committed(count), "control failed: {self:?}");
        assert!(self.committed.wal > 0);
        assert_eq!(self.before.wal, 0);
        assert_eq!(self.checkpointed.wal, 0);
        assert!(self.stage > 0);
        assert!(self.source > 0);
        assert!(self.database > 0);
        assert!(self.sqlite.is_some());
        assert_eq!(self.writer_calls, 1);
        assert!(
            self.additional < allowance / 4,
            "control demand is not well below allowance={allowance}: {self:?}"
        );
        eprintln!("populated admission control: allowance={allowance}, {self:?}");
    }

    fn assert_admitted(&self, count: u64, allowance: u64) {
        if let Err(error) = &self.admission {
            assert!(
                matches!(error.as_ref(), StorageImportSessionError::Space { required, available } if *required > *available && *available == allowance),
                "unexpected populated admission failure: {self:?}"
            );
            assert!(
                self.sqlite.is_none(),
                "refusal happened after freeze: {self:?}"
            );
            assert_eq!(self.writer_calls, 0, "writer ran before refusal: {self:?}");
        }
        assert!(
            self.is_committed(count),
            "populated import refused measured-sufficient allowance={allowance}: {self:?}"
        );
        assert!(self.additional <= allowance, "admitted growth: {self:?}");
    }
}

struct StorageLibraryImportPopulatedReviewTrial {
    attempt: StorageLibraryImportPopulatedReviewAttempt,
    preview: Option<Result<u64, Arc<StorageImportSessionError>>>,
    existing: Vec<StorageLibraryImportPopulatedReviewBinding>,
    final_bindings: Vec<StorageLibraryImportPopulatedReviewBinding>,
}

struct StorageLibraryImportPopulatedReview {
    review: StorageLibraryImportSpaceReview,
    existing: Vec<StorageLibraryImportPopulatedReviewBinding>,
    has_ballast: bool,
}

impl StorageLibraryImportPopulatedReview {
    const BULK_COUNT: u64 = 1000;
    const BULK_ALLOWANCE: u64 = 256 * 1024 * 1024;
    const BOUNDARY_COUNT: u64 = 64;
    const BOUNDARY_ALLOWANCE: u64 = 64 * 1024 * 1024;
    const BALLAST_ID: uuid::Uuid = uuid::Uuid::from_u128(92_002);
    const BALLAST_BODY: &str = "{\"populated_review_unrelated\":true}";

    async fn new(count: u64) -> StorageLibraryImportTestResult<Self> {
        let review = StorageLibraryImportSpaceReview::new().await?;
        review.fixture.capture_page_limit().await?;
        let mut populated = Self {
            review,
            existing: Vec::new(),
            has_ballast: false,
        };
        let mut stage = populated
            .prepare(
                StorageLibraryImportPopulatedReviewEnumBatch::Existing,
                count - 1,
            )
            .await?;
        let confirmation = stage.session.freeze().await?;
        assert!(
            matches!(stage.session.commit(confirmation).await?.as_ref(), StorageImportSessionEnumCommitOutcome::Committed(receipt) if receipt.count() == count - 1)
        );
        let mut games = vec![populated.review.retained];
        games.extend_from_slice(&stage.games);
        stage.drain(&populated.review.fixture.executor).await?;
        populated.review.checkpoint().await?;
        populated.existing = StorageLibraryImportPopulatedReviewBinding::read(
            &populated.review.fixture.store,
            &populated.review.fixture.executor,
            &games,
        )
        .await?;
        populated
            .assert_batch(
                StorageLibraryImportPopulatedReviewEnumBatch::Existing,
                &populated.existing[1..],
            )
            .await?;
        populated.assert_counts(count).await?;
        Ok(populated)
    }

    async fn prepare(
        &self,
        batch: StorageLibraryImportPopulatedReviewEnumBatch,
        count: u64,
    ) -> StorageLibraryImportTestResult<StorageLibraryImportPopulatedReviewStage> {
        let pgn = self
            .review
            .fixture
            .executor
            .submit_waiting(move |_| batch.pgn(count))
            .await?
            .join()
            .await?;
        let source = u64::try_from(pgn.len())?;
        self.review
            .fixture
            .next_ids
            .store(batch.seed(), Ordering::SeqCst);
        let (mut session, filesystem) =
            StorageLibraryImportSpaceReview::session(&self.review.fixture, &pgn).await?;
        let summary = session.prepare().await?;
        assert_eq!(summary.valid(), count);
        assert_eq!(summary.selected(), count);
        assert_eq!(summary.excluded(), 0);
        assert_eq!(summary.duplicates(), 0);
        let mut games = Vec::with_capacity(usize::try_from(count)?);
        let mut offset = 0;
        loop {
            let page = session
                .page(StorageImportSessionPageRequest::new(
                    offset,
                    StorageImportSessionPageRequest::MAX_ITEMS,
                    StorageImportSessionPageRequest::MAX_BYTES,
                )?)
                .await?;
            for row in page.rows() {
                let index = u64::try_from(games.len())?;
                assert_eq!(row.white(), batch.white(index));
                games.push(row.game().ok_or("missing populated review staged game")?);
            }
            let Some(next) = page.next_cursor() else {
                break;
            };
            offset = next;
        }
        assert_eq!(u64::try_from(games.len())?, count);
        Ok(StorageLibraryImportPopulatedReviewStage {
            session,
            filesystem,
            games,
            source,
        })
    }

    async fn assert_batch(
        &self,
        batch: StorageLibraryImportPopulatedReviewEnumBatch,
        bindings: &[StorageLibraryImportPopulatedReviewBinding],
    ) -> StorageLibraryImportTestResult {
        let library = StorageLibraryRepository::new(
            self.review.fixture.store.client(),
            &self.review.fixture.executor,
        );
        for (index, binding) in bindings.iter().enumerate() {
            let index = u64::try_from(index)?;
            assert!(binding.body.contains(&batch.annotation(index)));
            let loaded = library
                .load(binding.game)
                .await?
                .ok_or("missing populated review metadata")?;
            assert_eq!(
                loaded
                    .record()
                    .metadata()
                    .known()
                    .white
                    .as_ref()
                    .ok_or("missing populated review player")?
                    .as_str(),
                batch.white(index),
            );
        }
        Ok(())
    }

    async fn assert_counts(&self, entries: u64) -> StorageLibraryImportTestResult {
        let documents = entries
            .checked_add(u64::from(self.has_ballast))
            .ok_or("populated count overflow")?;
        self.review.fixture.sql(&format!("INSERT INTO import_space_review_checks SELECT (SELECT count(*) FROM library_entries) = {entries} AND (SELECT count(*) FROM documents) = {documents}")).await
    }

    async fn add_checkpointed_ballast(&mut self) -> StorageLibraryImportTestResult {
        let snapshot = self.review.fixture.repository().snapshot().await?;
        self.review
            .fixture
            .store
            .client()
            .save(Self::BALLAST_ID, 0, Self::BALLAST_BODY.to_owned())
            .await?;
        let revoked = self.review.fixture.repository().snapshot().await?;
        assert_eq!(
            revoked.catalog_stamp().value(),
            snapshot
                .catalog_stamp()
                .value()
                .checked_add(1)
                .ok_or("populated catalog overflow")?
        );
        assert_eq!(revoked.destination(), snapshot.destination());
        self.has_ballast = true;
        self.assert_counts(u64::try_from(self.existing.len())?)
            .await?;
        self.review.checkpoint().await?;
        Ok(())
    }

    async fn finish(
        self,
        expected: &[StorageLibraryImportPopulatedReviewBinding],
        absent: &[GameId],
    ) -> StorageLibraryImportTestResult {
        self.review.fixture.assert_page_limit_restored().await?;
        self.review.assert_retained(false).await?;
        self.assert_counts(u64::try_from(expected.len())?).await?;
        let games = expected
            .iter()
            .map(|binding| binding.game)
            .collect::<Vec<_>>();
        StorageLibraryImportPopulatedReviewBinding::assert_same(
            &StorageLibraryImportPopulatedReviewBinding::read(
                &self.review.fixture.store,
                &self.review.fixture.executor,
                &games,
            )
            .await?,
            expected,
        );
        self.review.fixture.assert_missing(absent).await?;
        let fixture = self.review.fixture;
        fixture.store.close().await?;
        let reopened = StorageDocumentStore::open(
            fixture.directory.path().join("import.sqlite"),
            fixture.budget.clone(),
        )
        .await?;
        let durable = async {
            StorageLibraryImportPopulatedReviewBinding::assert_same(
                &StorageLibraryImportPopulatedReviewBinding::read(
                    &reopened,
                    &fixture.executor,
                    &games,
                )
                .await?,
                expected,
            );
            for game in absent {
                assert!(reopened.client().read(game.as_uuid()).await?.is_none());
            }
            if self.has_ballast {
                let ballast = reopened
                    .client()
                    .read(Self::BALLAST_ID)
                    .await?
                    .ok_or("lost populated ballast after reopen")?;
                assert_eq!(ballast.revision(), 1);
                assert_eq!(ballast.body(), Self::BALLAST_BODY);
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        }
        .await;
        let storage_closed = reopened.close().await;
        let executor_closed = Arc::try_unwrap(fixture.executor)
            .map_err(|_retained_executor| "populated import retained executor")?
            .shutdown()
            .await;
        drop((fixture.directory, fixture.budget));
        durable?;
        storage_closed?;
        executor_closed?;
        Ok(())
    }

    async fn trial(
        existing_count: u64,
        incoming_count: u64,
        allowance: u64,
        has_ballast: bool,
    ) -> StorageLibraryImportTestResult<StorageLibraryImportPopulatedReviewTrial> {
        let mut populated = Self::new(existing_count).await?;
        populated
            .review
            .fixture
            .filesystem
            .free
            .store(allowance, Ordering::SeqCst);
        let preview = if has_ballast {
            let mut stage = populated
                .prepare(
                    StorageLibraryImportPopulatedReviewEnumBatch::Incoming,
                    incoming_count,
                )
                .await?;
            let preview = stage
                .session
                .freeze()
                .await
                .map(|confirmation| confirmation.sqlite_bytes());
            stage.drain(&populated.review.fixture.executor).await?;
            populated.add_checkpointed_ballast().await?;
            Some(preview)
        } else {
            None
        };
        let before = populated.review.checkpoint().await?;
        let mut staged = populated
            .prepare(
                StorageLibraryImportPopulatedReviewEnumBatch::Incoming,
                incoming_count,
            )
            .await?;
        let writer_before = populated.review.fixture.wall.reads.load(Ordering::SeqCst);
        let frozen = staged.session.freeze().await;
        let sqlite = frozen
            .as_ref()
            .ok()
            .map(|confirmation| confirmation.sqlite_bytes());
        let admission = match frozen {
            Ok(confirmation) => {
                assert_eq!(confirmation.source_bytes(), staged.source);
                staged.session.commit(confirmation).await
            }
            Err(error) => Err(error),
        };
        let writer_calls = populated
            .review
            .fixture
            .wall
            .reads
            .load(Ordering::SeqCst)
            .checked_sub(writer_before)
            .ok_or("populated writer reads regressed")?;
        let committed = populated.review.files().await?;
        let checkpointed = populated.review.checkpoint().await?;
        let stage = staged.filesystem.high_water.load(Ordering::SeqCst);
        let source = staged.source;
        let database = before.additional_high_water(committed, checkpointed)?;
        let additional = database
            .checked_add(staged.filesystem.additional_high_water(source)?)
            .ok_or("populated additional demand overflow")?;
        let games = staged.games.clone();
        staged.drain(&populated.review.fixture.executor).await?;
        let attempt = StorageLibraryImportPopulatedReviewAttempt {
            admission,
            sqlite,
            before,
            committed,
            checkpointed,
            stage,
            source,
            database,
            additional,
            writer_calls,
        };
        let existing = populated.existing.clone();
        let mut final_bindings = existing.clone();
        let absent = if attempt.is_committed(incoming_count) {
            let incoming = StorageLibraryImportPopulatedReviewBinding::read(
                &populated.review.fixture.store,
                &populated.review.fixture.executor,
                &games,
            )
            .await?;
            populated
                .assert_batch(
                    StorageLibraryImportPopulatedReviewEnumBatch::Incoming,
                    &incoming,
                )
                .await?;
            final_bindings.extend(incoming);
            Vec::new()
        } else {
            games
        };
        populated.finish(&final_bindings, &absent).await?;
        Ok(StorageLibraryImportPopulatedReviewTrial {
            attempt,
            preview,
            existing,
            final_bindings,
        })
    }
}

#[tokio::test]
async fn a_thousand_existing_games_admit_a_thousand_distinct_tiny_games_with_measured_headroom()
-> StorageLibraryImportTestResult {
    let count = StorageLibraryImportPopulatedReview::BULK_COUNT;
    let allowance = StorageLibraryImportPopulatedReview::BULK_ALLOWANCE;
    let control = StorageLibraryImportPopulatedReview::trial(count, count, u64::MAX, false).await?;
    control.attempt.assert_sufficient_control(count, allowance);
    assert_eq!(
        u64::try_from(control.final_bindings.len())?,
        count
            .checked_mul(2)
            .ok_or("populated total count overflow")?
    );
    let limited =
        StorageLibraryImportPopulatedReview::trial(count, count, allowance, false).await?;
    StorageLibraryImportPopulatedReviewBinding::assert_same(&limited.existing, &control.existing);
    assert_eq!(limited.attempt.before, control.attempt.before);
    limited.attempt.assert_admitted(count, allowance);
    StorageLibraryImportPopulatedReviewBinding::assert_same(
        &limited.final_bindings,
        &control.final_bindings,
    );
    Ok(())
}

#[tokio::test]
async fn one_checkpointed_unrelated_document_at_the_import_cardinality_boundary_keeps_useful_admission()
-> StorageLibraryImportTestResult {
    let count = StorageLibraryImportPopulatedReview::BOUNDARY_COUNT;
    let allowance = StorageLibraryImportPopulatedReview::BOUNDARY_ALLOWANCE;
    let without_ballast =
        StorageLibraryImportPopulatedReview::trial(count - 1, count, u64::MAX, false).await?;
    without_ballast
        .attempt
        .assert_sufficient_control(count, allowance);
    let with_ballast =
        StorageLibraryImportPopulatedReview::trial(count - 1, count, u64::MAX, true).await?;
    with_ballast
        .attempt
        .assert_sufficient_control(count, allowance);
    StorageLibraryImportPopulatedReviewBinding::assert_same(
        &without_ballast.existing,
        &with_ballast.existing,
    );
    StorageLibraryImportPopulatedReviewBinding::assert_same(
        &without_ballast.final_bindings,
        &with_ballast.final_bindings,
    );
    let limited =
        StorageLibraryImportPopulatedReview::trial(count - 1, count, allowance, true).await?;
    StorageLibraryImportPopulatedReviewBinding::assert_same(
        &limited.existing,
        &with_ballast.existing,
    );
    assert_eq!(limited.attempt.before, with_ballast.attempt.before);
    assert!(
        matches!(&limited.preview, Some(Ok(sqlite)) if *sqlite <= allowance),
        "pre-ballast import did not freeze with useful allowance={allowance}: {:?}",
        limited.preview
    );
    limited.attempt.assert_admitted(count, allowance);
    StorageLibraryImportPopulatedReviewBinding::assert_same(
        &limited.final_bindings,
        &with_ballast.final_bindings,
    );
    Ok(())
}
