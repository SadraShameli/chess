use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::capacity::budget::CapacityBudget;
use crate::capacity::executor::CapacityExecutor;
use crate::game::enums::{GameId, GameIdentityError, GameMoveId};
use crate::notation::game_identity::NotationGameIdentityEnumMatch;
use crate::storage::game_document::StorageGameDocument;
use crate::storage::import_session::{
    StorageImportSession, StorageImportSessionCancellation, StorageImportSessionCandidate,
    StorageImportSessionCandidateBody, StorageImportSessionCandidatePage,
    StorageImportSessionCandidateQuery, StorageImportSessionCatalogStamp,
    StorageImportSessionCommitFootprint, StorageImportSessionCommitReceipt,
    StorageImportSessionDestination, StorageImportSessionEnumChoice,
    StorageImportSessionEnumCommitOutcome, StorageImportSessionEnumVolume,
    StorageImportSessionError, StorageImportSessionFilesystemProvider,
    StorageImportSessionIdentityProvider, StorageImportSessionPageRequest,
    StorageImportSessionPreparedCommit, StorageImportSessionRepository,
    StorageImportSessionRepositoryError, StorageImportSessionRepositoryFuture,
    StorageImportSessionResources, StorageImportSessionSnapshot, StorageImportSessionSpace,
};
use crate::storage::library_record::StorageLibraryRecord;

struct StorageImportSessionTestIds(AtomicU64);

impl StorageImportSessionIdentityProvider for StorageImportSessionTestIds {
    fn next_game(&self) -> Result<GameId, GameIdentityError> {
        let value = self.0.fetch_add(1, Ordering::SeqCst);
        format!("00000000-0000-4000-8000-{value:012x}").parse()
    }

    fn next_move(&self) -> Result<GameMoveId, GameIdentityError> {
        let value = self.0.fetch_add(1, Ordering::SeqCst);
        format!("00000000-0000-4000-8000-{value:012x}").parse()
    }
}

struct StorageImportSessionTestFilesystem {
    free: AtomicU64,
    writes: AtomicUsize,
    fail_write: AtomicUsize,
    fail_cleanup: AtomicBool,
    created: AtomicUsize,
    commit_file: Mutex<Option<File>>,
    pause_write: AtomicBool,
    write_entered: tokio::sync::Notify,
    write_gate: (Mutex<bool>, std::sync::Condvar),
}

impl StorageImportSessionTestFilesystem {
    fn new() -> Self {
        Self {
            free: AtomicU64::new(u64::MAX),
            writes: AtomicUsize::new(0),
            fail_write: AtomicUsize::new(usize::MAX),
            fail_cleanup: AtomicBool::new(false),
            created: AtomicUsize::new(0),
            commit_file: Mutex::new(None),
            pause_write: AtomicBool::new(false),
            write_entered: tokio::sync::Notify::new(),
            write_gate: (Mutex::new(false), std::sync::Condvar::new()),
        }
    }
}

impl StorageImportSessionFilesystemProvider for StorageImportSessionTestFilesystem {
    fn duplicate(&self, file: &File) -> std::io::Result<File> {
        file.try_clone()
    }
    fn create_spool(&self) -> Result<File, StorageImportSessionError> {
        let file = tempfile::tempfile()?;
        if self.created.fetch_add(1, Ordering::SeqCst) == 8 {
            let duplicate = file.try_clone()?;
            *self
                .commit_file
                .lock()
                .map_err(|_error| StorageImportSessionError::Poisoned)? = Some(duplicate);
        }
        Ok(file)
    }

    fn observe_space(
        &self,
        _volume: StorageImportSessionEnumVolume,
    ) -> Result<StorageImportSessionSpace, StorageImportSessionError> {
        Ok(StorageImportSessionSpace::new(
            1,
            self.free.load(Ordering::SeqCst),
        ))
    }

    fn read_at(&self, file: &mut File, offset: u64, bytes: &mut [u8]) -> std::io::Result<usize> {
        file.seek(SeekFrom::Start(offset))?;
        file.read(bytes)
    }

    fn write_at(&self, file: &mut File, offset: u64, bytes: &[u8]) -> std::io::Result<()> {
        if self.pause_write.swap(false, Ordering::SeqCst) {
            self.write_entered.notify_one();
            let mut gate = self
                .write_gate
                .0
                .lock()
                .map_err(|_error| std::io::Error::from(std::io::ErrorKind::Other))?;
            while !*gate {
                gate = self
                    .write_gate
                    .1
                    .wait(gate)
                    .map_err(|_error| std::io::Error::from(std::io::ErrorKind::Other))?;
            }
        }
        if self.writes.fetch_add(1, Ordering::SeqCst) == self.fail_write.load(Ordering::SeqCst) {
            return Err(std::io::Error::from(std::io::ErrorKind::StorageFull));
        }
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(bytes)
    }

    fn length(&self, file: &File) -> std::io::Result<u64> {
        Ok(file.metadata()?.len())
    }

    fn flush(&self, file: &mut File) -> std::io::Result<()> {
        file.sync_all()
    }

    fn release(&self, file: &mut File) -> std::io::Result<()> {
        if self.fail_cleanup.load(Ordering::SeqCst) {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        file.set_len(0)
    }

    fn cleanup_retained(&self) -> Result<(), StorageImportSessionError> {
        Ok(())
    }
}

struct StorageImportSessionTestRepository {
    executor: Mutex<Arc<CapacityExecutor>>,
    stamp: AtomicU64,
    complete: AtomicBool,
    commits: AtomicUsize,
    bodies: Vec<StorageImportSessionCandidateBody>,
    pause: AtomicBool,
    pause_commit: AtomicBool,
    stale_page: AtomicBool,
    stale_candidate: AtomicBool,
    entered: tokio::sync::Notify,
    resume: tokio::sync::Notify,
}

impl StorageImportSessionTestRepository {
    fn new(executor: Arc<CapacityExecutor>) -> Self {
        Self {
            executor: Mutex::new(executor),
            stamp: AtomicU64::new(1),
            complete: AtomicBool::new(true),
            commits: AtomicUsize::new(0),
            bodies: Vec::new(),
            pause: AtomicBool::new(false),
            pause_commit: AtomicBool::new(false),
            stale_page: AtomicBool::new(false),
            stale_candidate: AtomicBool::new(false),
            entered: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
        }
    }
}

impl StorageImportSessionRepository for StorageImportSessionTestRepository {
    type PreparedProjection = Vec<u8>;
    fn snapshot(&self) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionSnapshot> {
        Box::pin(async move {
            if self.pause.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                self.resume.notified().await;
            }
            Ok(StorageImportSessionSnapshot::new(
                StorageImportSessionCatalogStamp::new(self.stamp.load(Ordering::SeqCst)),
                self.complete.load(Ordering::SeqCst),
                StorageImportSessionDestination::unfiled(),
            ))
        })
    }

    fn candidates(
        &self,
        query: StorageImportSessionCandidateQuery,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCandidatePage> {
        Box::pin(async move {
            let start = usize::try_from(query.cursor())
                .map_err(|error| StorageImportSessionRepositoryError::Effect(Box::new(error)))?;
            let candidates = self
                .bodies
                .iter()
                .skip(start)
                .take(100)
                .map(StorageImportSessionCandidateBody::candidate)
                .collect::<Vec<_>>();
            let end = start
                .checked_add(candidates.len())
                .ok_or(StorageImportSessionRepositoryError::InvalidResponse)?;
            StorageImportSessionCandidatePage::new(
                StorageImportSessionCatalogStamp::new(
                    self.stamp.load(Ordering::SeqCst)
                        + u64::from(self.stale_page.load(Ordering::SeqCst)),
                ),
                candidates,
                (end < self.bodies.len()).then_some(query.cursor() + 100),
            )
        })
    }

    fn read_candidate(
        &self,
        candidate: StorageImportSessionCandidate,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCandidateBody> {
        Box::pin(async move {
            let body = self
                .bodies
                .iter()
                .find(|body| body.candidate() == candidate)
                .cloned()
                .ok_or(StorageImportSessionRepositoryError::StaleCatalog)?;
            if self.stale_candidate.load(Ordering::SeqCst) {
                return StorageImportSessionCandidateBody::new(
                    StorageImportSessionCandidate::new(
                        candidate.game(),
                        candidate
                            .revision()
                            .checked_add(1)
                            .ok_or(StorageImportSessionRepositoryError::InvalidResponse)?,
                    ),
                    Arc::from(body.body()),
                );
            }
            Ok(body)
        })
    }

    fn prepare_projection(
        &self,
        record: &StorageLibraryRecord,
        _body: crate::storage::import_session::StorageImportSessionBodyProof<'_>,
        _cancellation: &crate::capacity::executor::CapacityExecutorCancellation,
    ) -> Result<Vec<u8>, StorageImportSessionRepositoryError> {
        Ok(record.id().as_uuid().as_bytes().to_vec())
    }

    fn estimate_commit_space(
        &self,
        _snapshot: StorageImportSessionSnapshot,
        footprint: StorageImportSessionCommitFootprint,
    ) -> StorageImportSessionRepositoryFuture<'_, u64> {
        Box::pin(async move {
            footprint
                .body_bytes()
                .checked_add(footprint.projection_bytes())
                .ok_or(StorageImportSessionRepositoryError::InvalidResponse)
        })
    }

    fn commit_import(
        &self,
        mut prepared: StorageImportSessionPreparedCommit,
        cancellation: StorageImportSessionCancellation,
    ) -> StorageImportSessionRepositoryFuture<'_, StorageImportSessionCommitReceipt> {
        Box::pin(async move {
            if self.pause_commit.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                self.resume.notified().await;
            }
            if prepared.catalog_stamp().value() != self.stamp.load(Ordering::SeqCst) {
                return Err(StorageImportSessionRepositoryError::StaleCatalog);
            }
            let executor = Arc::clone(
                &*self
                    .executor
                    .try_lock()
                    .map_err(|_error| StorageImportSessionRepositoryError::InvalidResponse)?,
            );
            let receipt = executor
                .submit_waiting(
                    move |_token| -> Result<_, StorageImportSessionRepositoryError> {
                        let mut count = 0_u64;
                        while let Some(frame) =
                            prepared.next_frame(&cancellation).map_err(|error| {
                                StorageImportSessionRepositoryError::Effect(Box::new(error))
                            })?
                        {
                            if frame.projection() != frame.game().as_uuid().as_bytes() {
                                return Err(StorageImportSessionRepositoryError::InvalidResponse);
                            }
                            let record = StorageGameDocument::recover_record(
                                std::str::from_utf8(frame.body()).map_err(|error| {
                                    StorageImportSessionRepositoryError::Effect(Box::new(error))
                                })?,
                            )
                            .map_err(|error| {
                                StorageImportSessionRepositoryError::Effect(Box::new(error))
                            })?;
                            if record.id() != frame.game() {
                                return Err(StorageImportSessionRepositoryError::InvalidResponse);
                            }
                            count = count
                                .checked_add(1)
                                .ok_or(StorageImportSessionRepositoryError::InvalidResponse)?;
                        }
                        prepared.finish(&cancellation).map_err(|error| {
                            StorageImportSessionRepositoryError::Effect(Box::new(error))
                        })?;
                        Ok(StorageImportSessionCommitReceipt::new(
                            prepared.commit_id(),
                            count,
                        ))
                    },
                )
                .await
                .map_err(|error| StorageImportSessionRepositoryError::Effect(Box::new(error)))?
                .join()
                .await
                .map_err(|error| StorageImportSessionRepositoryError::Effect(Box::new(error)))??;
            self.commits.fetch_add(1, Ordering::SeqCst);
            Ok(receipt)
        })
    }
}

struct StorageImportSessionTestFixture {
    executor: Arc<CapacityExecutor>,
    repository: Arc<StorageImportSessionTestRepository>,
    filesystem: Arc<StorageImportSessionTestFilesystem>,
}

impl StorageImportSessionTestFixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let executor = Arc::new(CapacityExecutor::new(CapacityBudget::new(
            std::num::NonZeroUsize::new(1).ok_or("zero workers")?,
        )?)?);
        Ok(Self {
            repository: Arc::new(StorageImportSessionTestRepository::new(Arc::clone(
                &executor,
            ))),
            executor,
            filesystem: Arc::new(StorageImportSessionTestFilesystem::new()),
        })
    }

    async fn session(
        &self,
        text: &str,
    ) -> Result<
        StorageImportSession<
            StorageImportSessionTestRepository,
            StorageImportSessionTestFilesystem,
            StorageImportSessionTestIds,
        >,
        Box<dyn std::error::Error>,
    > {
        let text = text.to_owned();
        let source = self
            .executor
            .submit_waiting(move |_token| -> std::io::Result<File> {
                let mut source = tempfile::tempfile()?;
                source.write_all(text.as_bytes())?;
                Ok(source)
            })
            .await?
            .join()
            .await??;
        Ok(StorageImportSession::new(
            source,
            StorageImportSessionResources {
                executor: Arc::clone(&self.executor),
                repository: Arc::clone(&self.repository),
                filesystem: Arc::clone(&self.filesystem),
                identities: Arc::new(StorageImportSessionTestIds(AtomicU64::new(1))),
            },
        )?)
    }
}

#[tokio::test]
async fn valid_games_and_invalid_branch_are_counted_without_partial_import()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *\n1. d4 (1. e5) *\n1. c4 *").await?;
    let summary = session.prepare().await?;
    assert_eq!(summary.valid(), 2);
    assert_eq!(summary.excluded(), 1);
    assert_eq!(summary.selected(), 2);
    let page = session
        .page(StorageImportSessionPageRequest::new(0, 100, 64 * 1024)?)
        .await?;
    assert_eq!(page.rows().len(), 3);
    assert!(page.rows()[1].exclusion().is_some());
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn fatal_termination_refuses_a_preview_after_previously_valid_games()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *\n1. d4 {unterminated").await?;
    assert!(session.prepare().await.is_err());
    assert!(session.freeze().await.is_err());
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn zero_valid_games_cannot_be_confirmed() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e5 *").await?;
    assert_eq!(session.prepare().await?.valid(), 0);
    assert!(session.freeze().await.is_err());
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn missing_player_bridge_does_not_merge_conflicting_known_players()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture
        .session("[White \"Alice\"]\n1. e4 *\n[White \"?\"]\n1. e4 *\n[White \"Bob\"]\n1. e4 *")
        .await?;
    let summary = session.prepare().await?;
    assert_eq!(summary.duplicates(), 2);
    let page = session
        .page(StorageImportSessionPageRequest::new(0, 100, 64 * 1024)?)
        .await?;
    assert_eq!(
        page.rows()[0].choice(),
        Some(StorageImportSessionEnumChoice::Import)
    );
    assert_eq!(
        page.rows()[1].choice(),
        Some(StorageImportSessionEnumChoice::Skip)
    );
    assert_eq!(page.rows()[2].potential_matches(), 1);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn exact_duplicates_require_explicit_keep_copy() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let game = "[White \"Alice\"]\n[Black \"Bob\"]\n[Date \"2026.10.17\"]\n1. e4 *\n";
    let mut session = fixture.session(&game.repeat(2)).await?;
    assert_eq!(session.prepare().await?.duplicates(), 1);
    assert!(
        session
            .choose(2, StorageImportSessionEnumChoice::Import)
            .await
            .is_err()
    );
    assert_eq!(
        session
            .choose(2, StorageImportSessionEnumChoice::KeepCopy)
            .await?
            .selected(),
        2
    );
    let confirmation = session.freeze().await?;
    let outcome = session.commit(confirmation.clone()).await?;
    assert!(matches!(
        &*outcome,
        StorageImportSessionEnumCommitOutcome::Committed(_)
    ));
    session.commit(confirmation).await?;
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn selected_commit_frames_preserve_record_identity_choices_and_annotations_after_refreeze()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut source = String::new();
    for (movement, note) in [("e4", "first"), ("e4", "second"), ("d4", "third")] {
        source.push_str(&format!(
            "[White \"Alice\"]\n[Black \"Bob\"]\n[Date \"2026.10.17\"]\n1. {movement} {{{note}}} *\n"
        ));
    }
    let mut session = fixture.session(&source).await?;
    let summary = session.prepare().await?;
    assert_eq!((summary.duplicates(), summary.selected()), (1, 2));
    let page = session
        .page(StorageImportSessionPageRequest::new(0, 100, 64 * 1024)?)
        .await?;
    let copied = page.rows()[1].game().ok_or("missing copied game")?;
    let imported = page.rows()[2].game().ok_or("missing imported game")?;
    let original = session.freeze().await?;
    session
        .choose(1, StorageImportSessionEnumChoice::Skip)
        .await?;
    assert_eq!(
        session
            .choose(2, StorageImportSessionEnumChoice::KeepCopy)
            .await?
            .selected(),
        2
    );
    let confirmation = session.freeze().await?;
    assert_ne!(original.digest(), confirmation.digest());
    let streamed = session
        .runtime
        .worker(move |work, token, cancellation| {
            assert!(
                work.stage
                    .take_prepared(&original, cancellation.clone())
                    .is_err()
            );
            let mut prepared = work
                .stage
                .take_prepared(&confirmation, cancellation.clone())?;
            assert_eq!(prepared.footprint().count(), 2);
            let mut observed = Vec::new();
            while let Some(frame) = prepared.next_frame(cancellation)? {
                let record = StorageGameDocument::recover_record_cancellable(
                    std::str::from_utf8(frame.body())?,
                    token,
                )?;
                assert_eq!(record.id(), frame.game());
                assert_eq!(frame.projection(), frame.game().as_uuid().as_bytes());
                let crate::storage::library_record::StorageLibraryEnumContent::Notation(game) =
                    record.content()
                else {
                    return Err(StorageImportSessionError::State);
                };
                observed.push((
                    frame.ordinal(),
                    frame.game(),
                    frame.choice(),
                    game.tree()
                        .node(game.mainline_tip())?
                        .annotations()
                        .notes()
                        .to_owned(),
                ));
            }
            prepared.finish(cancellation)?;
            Ok(observed)
        })
        .await;
    session.cancel_retained().await?;
    assert_eq!(
        streamed?,
        [
            (
                2,
                copied,
                StorageImportSessionEnumChoice::KeepCopy,
                "second".to_owned()
            ),
            (
                3,
                imported,
                StorageImportSessionEnumChoice::Import,
                "third".to_owned()
            ),
        ]
    );
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn choice_change_invalidates_frozen_confirmation() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *\n1. d4 *").await?;
    session.prepare().await?;
    let confirmation = session.freeze().await?;
    session
        .choose(2, StorageImportSessionEnumChoice::Skip)
        .await?;
    assert!(session.commit(confirmation).await.is_err());
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn a_new_catalog_insert_refuses_the_old_confirmation()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *").await?;
    session.prepare().await?;
    let confirmation = session.freeze().await?;
    fixture.repository.stamp.fetch_add(1, Ordering::SeqCst);
    assert!(session.commit(confirmation).await.is_err());
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn incomplete_catalog_adoption_refuses_preparation() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = StorageImportSessionTestFixture::new()?;
    fixture.repository.complete.store(false, Ordering::SeqCst);
    let mut session = fixture.session("1. e4 *").await?;
    assert!(session.prepare().await.is_err());
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn stage_space_refusal_happens_before_copy_writes() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = StorageImportSessionTestFixture::new()?;
    fixture.filesystem.free.store(0, Ordering::SeqCst);
    let mut session = fixture.session("1. e4 *").await?;
    assert!(session.prepare().await.is_err());
    assert_eq!(fixture.filesystem.writes.load(Ordering::SeqCst), 0);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn disk_full_during_copy_keeps_the_library_unmodified()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    fixture.filesystem.fail_write.store(0, Ordering::SeqCst);
    let mut session = fixture.session("1. e4 *").await?;
    assert!(session.prepare().await.is_err());
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn cancelled_prepare_wait_resumes_the_same_operation()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    fixture.repository.pause.store(true, Ordering::SeqCst);
    let mut session = fixture.session("1. e4 *").await?;
    {
        let waiting = session.prepare();
        tokio::pin!(waiting);
        tokio::select! {
            result = &mut waiting => { result?; return Err("prepare did not reach its barrier".into()); }
            () = fixture.repository.entered.notified() => {}
        }
    }
    fixture.repository.resume.notify_one();
    assert_eq!(session.prepare().await?.valid(), 1);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn committed_cleanup_failure_is_retained_and_cannot_insert_again()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *").await?;
    session.prepare().await?;
    let confirmation = session.freeze().await?;
    fixture
        .filesystem
        .fail_cleanup
        .store(true, Ordering::SeqCst);
    let outcome = session.commit(confirmation.clone()).await?;
    assert!(matches!(
        &*outcome,
        StorageImportSessionEnumCommitOutcome::CommittedCleanupFailed { .. }
    ));
    fixture
        .filesystem
        .fail_cleanup
        .store(false, Ordering::SeqCst);
    session.commit(confirmation).await?;
    session.cancel_retained().await?;
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn page_bounds_reject_count_and_byte_overflows() {
    assert!(StorageImportSessionPageRequest::new(0, 101, 64 * 1024).is_err());
    assert!(StorageImportSessionPageRequest::new(0, 100, 0).is_err());
    assert!(StorageImportSessionPageRequest::new(u64::MAX, 100, 64 * 1024).is_err());
}

#[test]
fn identity_matching_remains_nontransitive() -> Result<(), Box<dyn std::error::Error>> {
    let parse = |player: &str| -> Result<_, Box<dyn std::error::Error>> {
        let ids = StorageImportSessionTestIds(AtomicU64::new(1));
        let text = format!("[White \"{player}\"]\n1. e4 *");
        let mut parser = crate::notation::pgn_import::NotationPgnImport::new(
            std::io::Cursor::new(text.as_bytes()),
            || ids.next_move(),
        );
        let game = parser.next_game(&|| false)?.ok_or("missing game")?;
        let record = StorageLibraryRecord::new(
            ids.next_game()?,
            crate::storage::library_record::StorageLibraryEnumContent::Notation(Box::new(game)),
        )?;
        Ok(record.identity()?)
    };
    let alice = parse("Alice")?;
    let unknown = parse("?")?;
    let bob = parse("Bob")?;
    assert_eq!(
        alice.compare(&unknown),
        NotationGameIdentityEnumMatch::Potential
    );
    assert_eq!(
        unknown.compare(&bob),
        NotationGameIdentityEnumMatch::Potential
    );
    assert_eq!(
        alice.compare(&bob),
        NotationGameIdentityEnumMatch::Different
    );
    Ok(())
}

impl StorageImportSessionTestFixture {
    fn notation(
        text: &str,
        first_id: u64,
    ) -> Result<StorageLibraryRecord, StorageImportSessionError> {
        let ids = StorageImportSessionTestIds(AtomicU64::new(first_id));
        let mut parser = crate::notation::pgn_import::NotationPgnImport::new(
            std::io::Cursor::new(text.as_bytes()),
            || ids.next_move(),
        );
        let game = parser
            .next_game(&|| false)?
            .ok_or(StorageImportSessionError::State)?;
        Ok(StorageLibraryRecord::new(
            ids.next_game()?,
            crate::storage::library_record::StorageLibraryEnumContent::Notation(Box::new(game)),
        )?)
    }
}

#[test]
fn annotation_differences_ignore_all_game_and_move_uuids() -> Result<(), Box<dyn std::error::Error>>
{
    let pgn = "{root} 1. e4 $1 {note [%clk 0:00:10] [%csl Ge4]} (1. d4) *";
    let left = StorageImportSessionTestFixture::notation(pgn, 1)?;
    let right = StorageImportSessionTestFixture::notation(pgn, 100)?;
    let differences =
        crate::storage::import_session::difference::StorageImportSessionDifferences::between(
            &left, &right,
        )?;
    assert!(!differences.has_changes());
    Ok(())
}

#[test]
fn selected_mainline_and_variation_order_are_compared_without_uuid_noise()
-> Result<(), Box<dyn std::error::Error>> {
    let left = StorageImportSessionTestFixture::notation("1. e4 (1. d4) *", 1)?;
    let right = StorageImportSessionTestFixture::notation("1. d4 (1. e4) *", 100)?;
    let differences =
        crate::storage::import_session::difference::StorageImportSessionDifferences::between(
            &left, &right,
        )?;
    assert!(differences.tree());
    assert!(differences.selected_mainline());
    assert!(!differences.notes());
    Ok(())
}

#[test]
fn annotation_categories_preserve_notes_warnings_and_source_clocks()
-> Result<(), Box<dyn std::error::Error>> {
    let left =
        StorageImportSessionTestFixture::notation("1. e4 {a [%clk 0:00:10] [%unknown one]} *", 1)?;
    let right = StorageImportSessionTestFixture::notation(
        "1. e4 {b [%clk 0:00:11] [%unknown two]} *",
        100,
    )?;
    let differences =
        crate::storage::import_session::difference::StorageImportSessionDifferences::between(
            &left, &right,
        )?;
    assert!(differences.notes());
    assert!(differences.warnings());
    assert!(differences.source_clocks());
    assert!(!differences.tree());
    Ok(())
}

#[test]
fn external_index_batches_obey_the_typed_128_mib_ceiling() {
    use crate::storage::import_session::index::StorageImportSessionIndexBudget;
    assert!(StorageImportSessionIndexBudget::new(128 * 1024 * 1024).is_ok());
    assert!(StorageImportSessionIndexBudget::new(128 * 1024 * 1024 + 1).is_err());
    assert!(StorageImportSessionIndexBudget::new(1).is_err());
}

impl StorageImportSessionTestFixture {
    async fn mutate_commit(
        &self,
        operation: impl FnOnce(&mut File) -> Result<(), StorageImportSessionError>
        + Send
        + std::panic::UnwindSafe
        + 'static,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let fs = Arc::clone(&self.filesystem);
        self.executor
            .submit_waiting(move |_token| {
                let mut file = fs
                    .commit_file
                    .lock()
                    .map_err(|_error| StorageImportSessionError::Poisoned)?
                    .take()
                    .ok_or(StorageImportSessionError::State)?;
                let result = operation(&mut file);
                *fs.commit_file
                    .lock()
                    .map_err(|_error| StorageImportSessionError::Poisoned)? = Some(file);
                result
            })
            .await?
            .join()
            .await??;
        Ok(())
    }
}

#[tokio::test]
async fn altered_body_and_local_checksum_still_fail_the_frozen_aggregate()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("[White \"Alice\"]\n1. e4 *").await?;
    session.prepare().await?;
    let confirmation = session.freeze().await?;
    fixture
        .mutate_commit(|file| {
            use sha2::{Digest, Sha256};
            file.seek(SeekFrom::Start(0))?;
            let mut header = [0; 112];
            file.read_exact(&mut header)?;
            let length = usize::try_from(u64::from_le_bytes(
                header[32..40]
                    .try_into()
                    .map_err(|_error| StorageImportSessionError::Proof)?,
            ))
            .map_err(|_error| StorageImportSessionError::Arithmetic)?;
            let mut body = vec![0; length];
            file.read_exact(&mut body)?;
            let offset = body
                .windows(5)
                .position(|bytes| bytes == b"Alice")
                .ok_or(StorageImportSessionError::Proof)?;
            body[offset] = b'M';
            header[48..80].copy_from_slice(&Sha256::digest(&body));
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&header)?;
            file.write_all(&body)?;
            Ok(())
        })
        .await?;
    assert!(session.commit(confirmation).await.is_err());
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn trailing_bytes_cannot_be_ignored_by_the_commit_stream()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *").await?;
    session.prepare().await?;
    let confirmation = session.freeze().await?;
    fixture
        .mutate_commit(|file| {
            file.seek(SeekFrom::End(0))?;
            file.write_all(b"extra")?;
            Ok(())
        })
        .await?;
    assert!(session.commit(confirmation).await.is_err());
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn truncated_later_frame_cannot_acknowledge_an_earlier_insert()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *\n1. d4 *").await?;
    session.prepare().await?;
    let confirmation = session.freeze().await?;
    fixture
        .mutate_commit(|file| {
            let length = file.metadata()?.len();
            file.set_len(
                length
                    .checked_sub(1)
                    .ok_or(StorageImportSessionError::Arithmetic)?,
            )?;
            Ok(())
        })
        .await?;
    assert!(session.commit(confirmation).await.is_err());
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn cancelled_commit_wait_resumes_once_without_a_second_insert()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *").await?;
    session.prepare().await?;
    let confirmation = session.freeze().await?;
    fixture
        .repository
        .pause_commit
        .store(true, Ordering::SeqCst);
    {
        let waiting = session.commit(confirmation.clone());
        tokio::pin!(waiting);
        tokio::select! {
            result = &mut waiting => { result?; return Err("commit did not reach its barrier".into()); }
            () = fixture.repository.entered.notified() => {}
        }
    }
    assert!(
        session
            .choose(1, StorageImportSessionEnumChoice::Skip)
            .await
            .is_err()
    );
    fixture.repository.resume.notify_one();
    session.commit(confirmation).await?;
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn explicit_cancel_during_retained_commit_cannot_acknowledge_a_write()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *").await?;
    session.prepare().await?;
    let confirmation = session.freeze().await?;
    fixture
        .repository
        .pause_commit
        .store(true, Ordering::SeqCst);
    {
        let waiting = session.commit(confirmation);
        tokio::pin!(waiting);
        tokio::select! {
            result = &mut waiting => { result?; return Err("commit did not reach its barrier".into()); }
            () = fixture.repository.entered.notified() => {}
        }
    }
    fixture.repository.resume.notify_one();
    session.cancel_retained().await?;
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn cleanup_failure_can_be_retried_without_repeating_prepare_or_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *").await?;
    session.prepare().await?;
    fixture
        .filesystem
        .fail_cleanup
        .store(true, Ordering::SeqCst);
    assert!(session.cancel_retained().await.is_err());
    fixture
        .filesystem
        .fail_cleanup
        .store(false, Ordering::SeqCst);
    session.cancel_retained().await?;
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn metadata_page_obeys_its_byte_cap_and_own_cursor() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *\n1. d4 *\n1. c4 *").await?;
    session.prepare().await?;
    let page = session
        .page(StorageImportSessionPageRequest::new(0, 1, 4096)?)
        .await?;
    assert_eq!(page.rows()[0].ordinal(), 1);
    assert_eq!(page.next_cursor(), Some(1));
    let page = session
        .page(StorageImportSessionPageRequest::new(1, 1, 4096)?)
        .await?;
    assert_eq!(page.rows()[0].ordinal(), 2);
    assert!(
        session
            .page(StorageImportSessionPageRequest::new(0, 100, 1)?)
            .await
            .is_err()
    );
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn staged_duplicate_relationships_have_a_separate_bounded_cursor()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *\n1. e4 *\n1. e4 *").await?;
    session.prepare().await?;
    let request =
        crate::storage::import_session::StorageImportSessionDetailRequest::new(3, 1, 4096)?;
    let page = session.detail(request).await?;
    assert_eq!(page.relationships().len(), 1);
    let request = page.next_request().ok_or("missing detail cursor")?;
    let page = session.detail(request).await?;
    assert_eq!(page.relationships().len(), 1);
    session.cancel_retained().await?;
    Ok(())
}

#[test]
fn external_sort_merges_multiple_disk_runs_and_orders_collisions_by_ordinal()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::storage::import_session::disk::{
        StorageImportSessionDisk, StorageImportSessionDiskEnumFile,
    };
    use crate::storage::import_session::index::{
        StorageImportSessionIndex, StorageImportSessionIndexBudget, StorageImportSessionIndexEntry,
    };
    let fs = Arc::new(StorageImportSessionTestFilesystem::new());
    let mut disk = StorageImportSessionDisk::new(fs);
    disk.initialize()?;
    let mut expected = Vec::new();
    for ordinal in (1..=19).rev() {
        let entry = StorageImportSessionIndexEntry {
            key: [u8::try_from(ordinal % 3)?; 32],
            ordinal,
        };
        disk.append(
            StorageImportSessionDiskEnumFile::StructuralIndex,
            &entry.encoded(),
        )?;
        expected.push(entry);
    }
    expected.sort_unstable();
    StorageImportSessionIndex::sort(
        &mut disk,
        StorageImportSessionDiskEnumFile::StructuralIndex,
        StorageImportSessionIndexBudget::new(160)?,
        || Ok(()),
    )?;
    for (index, expected) in expected.into_iter().enumerate() {
        let actual = StorageImportSessionIndex::entry(
            &mut disk,
            StorageImportSessionDiskEnumFile::StructuralIndex,
            u64::try_from(index)?,
        )?;
        assert!(actual == expected);
    }
    disk.cleanup()?;
    Ok(())
}

#[test]
fn directory_and_span_arithmetic_refuse_overflow() -> Result<(), Box<dyn std::error::Error>> {
    use crate::storage::import_session::disk::{
        StorageImportSessionDisk, StorageImportSessionDiskEnumFile, StorageImportSessionDiskSpan,
    };
    assert!(
        StorageImportSessionDiskSpan {
            offset: u64::MAX,
            length: 1
        }
        .end()
        .is_err()
    );
    let fs = Arc::new(StorageImportSessionTestFilesystem::new());
    let mut disk = StorageImportSessionDisk::new(fs);
    disk.initialize()?;
    assert!(disk.record(0).is_err());
    assert!(disk.record(u64::MAX).is_err());
    assert_eq!(disk.len(StorageImportSessionDiskEnumFile::Directory)?, 0);
    disk.cleanup()?;
    Ok(())
}

impl StorageImportSessionTestFixture {
    async fn catalog_game(
        &mut self,
        text: &str,
        first_id: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let text = text.to_owned();
        let body = self
            .executor
            .submit_waiting(move |token| -> Result<_, StorageImportSessionError> {
                let record = Self::notation(&text, first_id)?;
                let crate::storage::library_record::StorageLibraryEnumContent::Notation(game) =
                    record.content()
                else {
                    return Err(StorageImportSessionError::State);
                };
                let body =
                    StorageGameDocument::capture_notation_cancellable(record.id(), game, token)?
                        .json_cancellable(token)?;
                Ok(StorageImportSessionCandidateBody::new(
                    StorageImportSessionCandidate::new(record.id(), 1),
                    Arc::from(body),
                )?)
            })
            .await?
            .join()
            .await??;
        Arc::get_mut(&mut self.repository)
            .ok_or("repository is already shared")?
            .bodies
            .push(body);
        Ok(())
    }
}

#[tokio::test]
async fn library_candidate_revision_and_catalog_stamp_are_both_required()
-> Result<(), Box<dyn std::error::Error>> {
    for stale_page in [false, true] {
        let mut fixture = StorageImportSessionTestFixture::new()?;
        fixture.catalog_game("1. e4 *", 100).await?;
        fixture
            .repository
            .stale_page
            .store(stale_page, Ordering::SeqCst);
        fixture
            .repository
            .stale_candidate
            .store(!stale_page, Ordering::SeqCst);
        let mut session = fixture.session("1. e4 *").await?;
        assert!(session.prepare().await.is_err());
        assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
        session.cancel_retained().await?;
    }
    Ok(())
}

#[tokio::test]
async fn existing_annotations_are_presented_as_differences_and_never_overwritten()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = StorageImportSessionTestFixture::new()?;
    fixture.catalog_game("1. e4 {existing note} *", 100).await?;
    let original = Arc::<str>::from(fixture.repository.bodies[0].body());
    let mut session = fixture.session("1. e4 {incoming note} *").await?;
    assert_eq!(session.prepare().await?.duplicates(), 1);
    let page = session
        .detail(
            crate::storage::import_session::StorageImportSessionDetailRequest::new(1, 100, 4096)?,
        )
        .await?;
    assert_eq!(page.relationships().len(), 1);
    assert!(page.relationships()[0].differences().notes());
    session
        .choose(1, StorageImportSessionEnumChoice::KeepCopy)
        .await?;
    let confirmation = session.freeze().await?;
    session.commit(confirmation).await?;
    assert_eq!(fixture.repository.bodies[0].body(), original.as_ref());
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn staged_matching_uses_complete_partial_and_conflicting_identity_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    for (left, right, duplicates) in [
        (
            "[White \"Alice\"]\n[Date \"2026.10.17\"]",
            "[White \" ALICE \"]\n[Date \"2026.10.17\"]",
            1,
        ),
        ("[White \"Alice\"]", "[White \"Bob\"]", 0),
        ("[Date \"2026.10.??\"]", "[Date \"2026.??.17\"]", 1),
        ("[Date \"2026.10.??\"]", "[Date \"2025.10.17\"]", 0),
    ] {
        let fixture = StorageImportSessionTestFixture::new()?;
        let text =
            format!("{left}\n[Black \"Carol\"]\n1. e4 *\n{right}\n[Black \"Carol\"]\n1. e4 *");
        let mut session = fixture.session(&text).await?;
        assert_eq!(session.prepare().await?.duplicates(), duplicates);
        session.cancel_retained().await?;
    }
    Ok(())
}

#[tokio::test]
async fn a_structural_digest_collision_still_requires_full_identity_equality()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::storage::import_session::disk::StorageImportSessionDiskEnumFile;
    use crate::storage::import_session::index::{
        StorageImportSessionIndex, StorageImportSessionIndexBudget, StorageImportSessionIndexEntry,
    };
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *\n1. d4 *").await?;
    session.prepare().await?;
    let summary = session
        .runtime
        .worker(|work, token, cancellation| {
            let key = work.stage.disk.record(1)?.structural_digest;
            let mut second = work.stage.disk.record(2)?;
            second.structural_digest = key;
            work.stage.disk.write_record(&second)?;
            let fs = Arc::clone(&work.stage.disk.filesystem);
            let index = work
                .stage
                .disk
                .spool(StorageImportSessionDiskEnumFile::StructuralIndex)?;
            fs.release(&mut index.file)?;
            index.length = 0;
            for ordinal in 1..=2 {
                work.stage.disk.append(
                    StorageImportSessionDiskEnumFile::StructuralIndex,
                    &StorageImportSessionIndexEntry { key, ordinal }.encoded(),
                )?;
            }
            StorageImportSessionIndex::sort(
                &mut work.stage.disk,
                StorageImportSessionDiskEnumFile::StructuralIndex,
                StorageImportSessionIndexBudget::new(160)?,
                || {
                    crate::storage::import_session::stage::StorageImportSessionStage::check(
                        token,
                        cancellation,
                    )
                },
            )?;
            work.stage.compare_staged(token, cancellation)?;
            work.stage.finish_preview(token, cancellation)
        })
        .await?;
    assert_eq!(summary.duplicates(), 0);
    assert!(session.freeze().await.is_err());
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn oversized_source_is_refused_before_any_stage_write()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *").await?;
    session
        .runtime
        .worker(|work, _token, _cancellation| {
            work.source
                .as_mut()
                .ok_or(StorageImportSessionError::State)?
                .set_len(20 * 1024 * 1024 * 1024 + 1)?;
            Ok(())
        })
        .await?;
    assert!(session.prepare().await.is_err());
    assert_eq!(fixture.filesystem.writes.load(Ordering::SeqCst), 0);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn original_source_rename_cannot_replace_the_held_file_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let (source, directory) = fixture
        .executor
        .submit_waiting(|_token| -> std::io::Result<_> {
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("source.pgn");
            std::fs::write(&path, "1. e4 *")?;
            let source = File::open(&path)?;
            std::fs::rename(&path, directory.path().join("renamed.pgn"))?;
            std::fs::write(&path, "1. e5 *")?;
            Ok((source, directory))
        })
        .await?
        .join()
        .await??;
    let mut session = StorageImportSession::new(
        source,
        StorageImportSessionResources {
            executor: Arc::clone(&fixture.executor),
            repository: Arc::clone(&fixture.repository),
            filesystem: Arc::clone(&fixture.filesystem),
            identities: Arc::new(StorageImportSessionTestIds(AtomicU64::new(1))),
        },
    )?;
    assert_eq!(session.prepare().await?.valid(), 1);
    let confirmation = session.freeze().await?;
    assert_eq!(confirmation.source_bytes(), 7);
    session.cancel_retained().await?;
    fixture
        .executor
        .submit_waiting(move |_token| directory.close())
        .await?
        .join()
        .await??;
    Ok(())
}

#[tokio::test]
async fn cancel_during_copy_observes_the_worker_before_releasing_source_and_stage()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    fixture.filesystem.pause_write.store(true, Ordering::SeqCst);
    let mut session = fixture.session("1. e4 *").await?;
    {
        let waiting = session.prepare();
        tokio::pin!(waiting);
        tokio::select! {
            result = &mut waiting => { result?; return Err("copy did not reach its barrier".into()); }
            () = fixture.filesystem.write_entered.notified() => {}
        }
    }
    {
        let cancelling = session.cancel_retained();
        tokio::pin!(cancelling);
        std::future::poll_fn(|context| {
            assert!(std::future::Future::poll(cancelling.as_mut(), context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        *fixture
            .filesystem
            .write_gate
            .0
            .try_lock()
            .map_err(|_error| "write gate unavailable")? = true;
        fixture.filesystem.write_gate.1.notify_all();
        cancelling.await?;
    }
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    session
        .runtime
        .worker(|work, _token, _cancellation| {
            assert!(work.source.is_none());
            assert!(work.stage.disk.files.is_empty());
            Ok(())
        })
        .await?;
    Ok(())
}

#[test]
fn interrupted_external_sort_returns_cancellation_and_retains_cleanup_handles()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::storage::import_session::disk::{
        StorageImportSessionDisk, StorageImportSessionDiskEnumFile,
    };
    use crate::storage::import_session::index::{
        StorageImportSessionIndex, StorageImportSessionIndexBudget, StorageImportSessionIndexEntry,
    };
    let fs = Arc::new(StorageImportSessionTestFilesystem::new());
    let mut disk = StorageImportSessionDisk::new(fs);
    disk.initialize()?;
    for ordinal in (1..=20).rev() {
        disk.append(
            StorageImportSessionDiskEnumFile::StructuralIndex,
            &StorageImportSessionIndexEntry {
                key: [0; 32],
                ordinal,
            }
            .encoded(),
        )?;
    }
    let mut checks = 0;
    let result = StorageImportSessionIndex::sort(
        &mut disk,
        StorageImportSessionDiskEnumFile::StructuralIndex,
        StorageImportSessionIndexBudget::new(160)?,
        || {
            checks += 1;
            if checks == 7 {
                Err(StorageImportSessionError::Cancelled)
            } else {
                Ok(())
            }
        },
    );
    assert!(matches!(result, Err(StorageImportSessionError::Cancelled)));
    assert!(!disk.files.is_empty());
    disk.cleanup()?;
    assert!(disk.files.is_empty());
    Ok(())
}

#[tokio::test]
async fn text_limit_termination_cannot_leave_a_committable_valid_prefix()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let text = format!("1. e4 *\n1. d4 {{{}}} *", "x".repeat(1024 * 1024 + 1));
    let mut session = fixture.session(&text).await?;
    assert!(session.prepare().await.is_err());
    assert!(session.freeze().await.is_err());
    session.cancel_retained().await?;
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn modified_projection_and_checksum_cannot_change_a_frozen_insert()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *").await?;
    session.prepare().await?;
    let confirmation = session.freeze().await?;
    fixture
        .mutate_commit(|file| {
            use sha2::{Digest, Sha256};
            file.seek(SeekFrom::Start(0))?;
            let mut header = [0; 112];
            file.read_exact(&mut header)?;
            let body_length = u64::from_le_bytes(
                header[32..40]
                    .try_into()
                    .map_err(|_error| StorageImportSessionError::Proof)?,
            );
            let projection_offset = 112_u64
                .checked_add(body_length)
                .ok_or(StorageImportSessionError::Arithmetic)?;
            file.seek(SeekFrom::Start(projection_offset))?;
            let mut projection = [0; 16];
            file.read_exact(&mut projection)?;
            projection[15] ^= 1;
            header[80..112].copy_from_slice(&Sha256::digest(projection));
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&header)?;
            file.seek(SeekFrom::Start(projection_offset))?;
            file.write_all(&projection)?;
            Ok(())
        })
        .await?;
    assert!(session.commit(confirmation).await.is_err());
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn reordered_frozen_frames_are_rejected_before_any_acknowledgement()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *\n1. d4 *").await?;
    session.prepare().await?;
    let confirmation = session.freeze().await?;
    fixture
        .mutate_commit(|file| {
            file.seek(SeekFrom::Start(0))?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            let body = u64::from_le_bytes(
                bytes
                    .get(32..40)
                    .ok_or(StorageImportSessionError::Proof)?
                    .try_into()
                    .map_err(|_error| StorageImportSessionError::Proof)?,
            );
            let projection = u64::from_le_bytes(
                bytes
                    .get(40..48)
                    .ok_or(StorageImportSessionError::Proof)?
                    .try_into()
                    .map_err(|_error| StorageImportSessionError::Proof)?,
            );
            let boundary = usize::try_from(
                112_u64
                    .checked_add(body)
                    .and_then(|value| value.checked_add(projection))
                    .ok_or(StorageImportSessionError::Arithmetic)?,
            )
            .map_err(|_error| StorageImportSessionError::Arithmetic)?;
            let (first, second) = bytes.split_at(boundary);
            file.seek(SeekFrom::Start(0))?;
            file.write_all(second)?;
            file.write_all(first)?;
            Ok(())
        })
        .await?;
    assert!(session.commit(confirmation).await.is_err());
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn forged_confirmation_counts_are_rejected_without_entering_the_backend()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let mut session = fixture.session("1. e4 *").await?;
    session.prepare().await?;
    let mut confirmation = session.freeze().await?;
    confirmation.source_bytes = confirmation
        .source_bytes
        .checked_add(1)
        .ok_or(StorageImportSessionError::Arithmetic)?;
    assert!(session.commit(confirmation).await.is_err());
    assert_eq!(fixture.repository.commits.load(Ordering::SeqCst), 0);
    session.cancel_retained().await?;
    Ok(())
}

#[tokio::test]
async fn encoding_observes_executor_cancellation_on_the_injected_single_worker()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = StorageImportSessionTestFixture::new()?;
    let entered = Arc::new(tokio::sync::Notify::new());
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let worker_entered = Arc::clone(&entered);
    let worker_gate = Arc::clone(&gate);
    let job = fixture
        .executor
        .submit_waiting(move |token| -> Result<String, StorageImportSessionError> {
            worker_entered.notify_one();
            let mut released = worker_gate
                .0
                .lock()
                .map_err(|_error| StorageImportSessionError::Poisoned)?;
            while !*released {
                released = worker_gate
                    .1
                    .wait(released)
                    .map_err(|_error| StorageImportSessionError::Poisoned)?;
            }
            drop(released);
            let record = StorageImportSessionTestFixture::notation("1. e4 {note} *", 1)?;
            let crate::storage::library_record::StorageLibraryEnumContent::Notation(game) =
                record.content()
            else {
                return Err(StorageImportSessionError::State);
            };
            Ok(
                StorageGameDocument::capture_notation_cancellable(record.id(), game, token)?
                    .json_cancellable(token)?,
            )
        })
        .await?;
    entered.notified().await;
    job.cancel();
    *gate
        .0
        .try_lock()
        .map_err(|_error| "encoding gate unavailable")? = true;
    gate.1.notify_all();
    assert!(matches!(
        job.join().await?,
        Err(StorageImportSessionError::Codec(
            crate::storage::game_document::StorageGameDocumentError::Cancelled
        ))
    ));
    Ok(())
}

#[path = "import_session_review.rs"]
mod review;

#[tokio::test]
async fn complete_identity_digest_collisions_require_full_identity_equality()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::storage::import_session::disk::StorageImportSessionDiskEnumFile;
    use crate::storage::import_session::index::StorageImportSessionIndexEntry;
    let fixture = StorageImportSessionTestFixture::new()?;
    let source = "[White \"Alice\"]\n[Black \"Carol\"]\n[Date \"2026.10.17\"]\n1. e4 *\n[White \"Bob\"]\n[Black \"Carol\"]\n[Date \"2026.10.17\"]\n1. e4 *\n[White \"Alice\"]\n[Black \"Carol\"]\n[Date \"2026.10.17\"]\n1. e4 *\n[White \"Alice\"]\n[Black \"Carol\"]\n[Date \"2026.10.17\"]\n1. d4 *\n";
    let mut session = fixture.session(source).await?;
    session.prepare().await?;
    let classes = session
        .runtime
        .worker(|work, token, cancellation| {
            for kind in [
                StorageImportSessionDiskEnumFile::CompleteIndex,
                StorageImportSessionDiskEnumFile::ClassIndex,
            ] {
                let fs = Arc::clone(&work.stage.disk.filesystem);
                let spool = work.stage.disk.spool(kind)?;
                fs.release(&mut spool.file)?;
                spool.length = 0;
            }
            for ordinal in 1..=4 {
                work.stage.disk.append(
                    StorageImportSessionDiskEnumFile::CompleteIndex,
                    &StorageImportSessionIndexEntry {
                        key: [0; 32],
                        ordinal,
                    }
                    .encoded(),
                )?;
            }
            work.stage.assign_complete_classes(token, cancellation)?;
            (1..=4)
                .map(|ordinal| {
                    work.stage
                        .class_member(ordinal)
                        .map(|(representative, _count)| representative)
                })
                .collect::<Result<Vec<_>, StorageImportSessionError>>()
        })
        .await?;
    session.cancel_retained().await?;
    assert_eq!(classes, [1, 2, 1, 4]);
    Ok(())
}

#[test]
fn candidate_query_borrows_the_canonical_validated_structural_key()
-> Result<(), StorageImportSessionError> {
    let record = StorageImportSessionTestFixture::notation("1. e4 e5 *", 1)?;
    let identity = Arc::new(record.identity()?);
    let query = StorageImportSessionCandidateQuery::new(
        StorageImportSessionCatalogStamp::new(9),
        Arc::clone(&identity),
        7,
    )?;
    let continued = query.clone();
    assert!(std::ptr::eq(
        query.structural_key(),
        identity.structural_key()
    ));
    assert!(std::ptr::eq(
        query.structural_key(),
        continued.structural_key()
    ));
    assert_eq!(
        query.structural_key().initial_fen(),
        record.start().position().fen()
    );
    assert_eq!(query.structural_key().mainline_uci(), ["e2e4", "e7e5"]);
    assert_eq!(
        query.structural_key().variant(),
        record.start().position().variant()
    );
    assert_eq!(
        query.structural_digest(),
        StorageImportSessionCandidateQuery::structural_digest_for(identity.structural_key())?
    );
    assert_eq!(
        (query.catalog_stamp().value(), query.cursor(), query.limit()),
        (9, 7, 100)
    );
    Ok(())
}
