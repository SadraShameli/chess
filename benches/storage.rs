use std::error::Error;
use std::fs::File;
use std::io::{Cursor, Write};
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chess::board::editor::{
    BoardEditorSession, BoardEditorSessionEnumEdit, BoardEditorSessionEnumOperation,
    BoardEditorSessionRequest,
};
use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::ClockInstant;
use chess::game::enums::{GameId, GameIdentityError, GameMoveId};
use chess::game::library_organization::GameLibraryOrganizationTimestamp;
use chess::game::session::GameMetadataText;
use chess::game::storage_repository::{
    GameStorageRevision, GameStorageWallTimeError, GameStorageWallTimeProvider,
};
use chess::notation::pgn_import::NotationPgnImport;
use chess::puzzle::checkpoint::{
    PuzzleCheckpointEnumTrigger, PuzzleCheckpointObservation, PuzzleCheckpointSnapshot,
};
use chess::puzzle::definition::PuzzleDefinition;
use chess::puzzle::enums::PuzzleEnumCountdownMode;
use chess::puzzle::identity::PuzzleDefinitionIdentity;
use chess::puzzle::metadata::PuzzleDefinitionMetadata;
use chess::puzzle::persistence::{
    PuzzlePersistenceEnumCommand, PuzzlePersistenceRun, PuzzlePersistenceSnapshot,
    PuzzlePersistenceStorageRevision,
};
use chess::puzzle::random_provider::PuzzleRandomProviderSeeded;
use chess::puzzle::selection::PuzzleSelectionFilter;
use chess::puzzle::session::{PuzzleSessionConfig, PuzzleSessionId};
use chess::puzzle::storage_repository::PuzzleStorageDriver;
use chess::puzzle::timer::{PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness};
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::editor_repository::{StorageEditorRepository, StorageEditorRepositoryRevision};
use chess::storage::game_document::StorageGameDocument;
use chess::storage::import_session::{
    StorageImportSession, StorageImportSessionDestination, StorageImportSessionEnumCommitOutcome,
    StorageImportSessionIdentityProvider, StorageImportSessionResources,
};
use chess::storage::library_import::StorageLibraryImportRepository;
use chess::storage::library_prepared::StorageLibraryEnumContentChange;
use chess::storage::library_query::{
    StorageLibraryEnumFilter, StorageLibraryEnumSort, StorageLibraryQuery,
    StorageLibraryQueryOptions,
};
use chess::storage::library_repository::{
    StorageLibraryEnumMutationReceipt, StorageLibraryRepository,
};
use chess::storage::puzzle_repository::StoragePuzzleRepository;

type StorageBenchResult<T = ()> = Result<T, Box<dyn Error>>;

#[derive(Clone, Copy)]
enum StorageBenchEnumWorkload {
    ImportPrepare,
    ImportFreeze,
    ImportCommit,
    ImportTotal,
    LibraryFirstPage,
    LibraryFilteredPage,
    LibraryKeysetWalk,
    DocumentSave,
    DocumentOpen,
    LibraryGameSave,
    LibraryGameOpen,
    PuzzleEventAppend,
    EditorDraftSave,
}

impl StorageBenchEnumWorkload {
    const fn name(self) -> &'static str {
        match self {
            Self::ImportPrepare => "import prepare (100 games, 500 existing)",
            Self::ImportFreeze => "import freeze (100 games, 500 existing)",
            Self::ImportCommit => "import commit (100 games, 500 existing)",
            Self::ImportTotal => "import prepare+freeze+commit",
            Self::LibraryFirstPage => "library first page (100 of 1500)",
            Self::LibraryFilteredPage => "library filtered page (white filter, date sort)",
            Self::LibraryKeysetWalk => "library keyset walk (15 pages, plies sort)",
            Self::DocumentSave => "game document save (document client)",
            Self::DocumentOpen => "game document open (document client)",
            Self::LibraryGameSave => "game document save (library write)",
            Self::LibraryGameOpen => "game document open (library load)",
            Self::PuzzleEventAppend => "puzzle event append",
            Self::EditorDraftSave => "editor draft save",
        }
    }
}

struct StorageBenchReport {
    workload: StorageBenchEnumWorkload,
    workers: usize,
    run: usize,
    samples: Vec<Duration>,
}

impl StorageBenchReport {
    fn nearest_rank(sorted: &[Duration], percentile: usize) -> Option<Duration> {
        let rank = (percentile * sorted.len()).div_ceil(100);
        sorted.get(rank.checked_sub(1)?).copied()
    }

    fn emit(mut self) -> StorageBenchResult {
        self.samples.sort_unstable();
        let p50 = Self::nearest_rank(&self.samples, 50).ok_or("no samples")?;
        let p95 = Self::nearest_rank(&self.samples, 95).ok_or("no samples")?;
        let max = self.samples.last().copied().ok_or("no samples")?;
        let min = self.samples.first().copied().ok_or("no samples")?;
        println!(
            "| {} | {} | {} | {} | {:.3} | {:.3} | {:.3} | {:.3} |",
            self.workload.name(),
            self.workers,
            self.run,
            self.samples.len(),
            Self::milliseconds(min),
            Self::milliseconds(p50),
            Self::milliseconds(p95),
            Self::milliseconds(max),
        );
        Ok(())
    }

    fn milliseconds(value: Duration) -> f64 {
        value.as_secs_f64() * 1_000.0
    }
}

struct StorageBenchSamples {
    warmup: usize,
    seen: usize,
    samples: Vec<Duration>,
}

impl StorageBenchSamples {
    fn new(warmup: usize, capacity: usize) -> Self {
        Self {
            warmup,
            seen: 0,
            samples: Vec::with_capacity(capacity),
        }
    }

    fn record(&mut self, elapsed: Duration) {
        if self.seen >= self.warmup {
            self.samples.push(elapsed);
        }
        self.seen += 1;
    }

    fn emit(
        self,
        workload: StorageBenchEnumWorkload,
        workers: usize,
        run: usize,
    ) -> StorageBenchResult {
        StorageBenchReport {
            workload,
            workers,
            run,
            samples: self.samples,
        }
        .emit()
    }
}

struct StorageBenchWall;

impl GameStorageWallTimeProvider for StorageBenchWall {
    fn now(&self) -> Result<GameLibraryOrganizationTimestamp, GameStorageWallTimeError> {
        Ok(GameLibraryOrganizationTimestamp::new(123_456)?)
    }
}

struct StorageBenchIds(AtomicU64);

impl StorageBenchIds {
    fn next(&self) -> Result<GameId, GameIdentityError> {
        let value = (1_u128 << 96) | u128::from(self.0.fetch_add(1, Ordering::SeqCst));
        uuid::Uuid::from_u128(value).to_string().parse()
    }
}

impl StorageImportSessionIdentityProvider for StorageBenchIds {
    fn next_game(&self) -> Result<GameId, GameIdentityError> {
        self.next()
    }

    fn next_move(&self) -> Result<GameMoveId, GameIdentityError> {
        let value = (1_u128 << 96) | u128::from(self.0.fetch_add(1, Ordering::SeqCst));
        uuid::Uuid::from_u128(value).to_string().parse()
    }
}

struct StorageBenchFixture;

impl StorageBenchFixture {
    const WARMUP: usize = 20;
    const SAMPLES: usize = 200;
    const IMPORT_WARMUP: usize = 3;
    const IMPORT_SAMPLES: usize = 20;
    const WALK_WARMUP: usize = 3;
    const WALK_SAMPLES: usize = 50;
    const RUNS: usize = 5;
    const FILES: [u8; 8] = *b"abcdefgh";
    const EXISTING_GAMES: u64 = 500;
    const INCOMING_GAMES: u64 = 100;
    const CORPUS_GAMES: u128 = 1_500;
    const OPERA: &'static str = "1. e4 e5 2. Nf3 d6 3. d4 Bg4 4. dxe5 Bxf3 5. Qxf3 dxe5 6. Bc4 Nf6 7. Qb3 Qe7 8. Nc3 c6 9. Bg5 b5 10. Nxb5 cxb5 11. Bxb5+ Nbd7 12. O-O-O Rd8 13. Rxd7 Rxd7 14. Rd1 Qe6 15. Bxd7+ Nxd7 16. Qb8+ Nxb8 17. Rd8# 1-0";
    const OPENING_TOKENS: [&'static str; 6] = ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bb5", "a6"];
    const RESULTS: [&'static str; 4] = ["1-0", "0-1", "1/2-1/2", "*"];

    fn game_id(value: u128) -> StorageBenchResult<GameId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn body(id: GameId, pgn: &str) -> StorageBenchResult<String> {
        let mut next = 1_000_u128;
        let mut parser = NotationPgnImport::new(Cursor::new(pgn.as_bytes()), move || {
            next += 1;
            uuid::Uuid::from_u128(next).to_string().parse()
        });
        let game = parser
            .next_game(&|| false)?
            .ok_or("missing parsed fixture game")?;
        Ok(StorageGameDocument::capture_notation(id, &game)?.json()?)
    }

    fn opera_pgn(white: &str) -> String {
        format!(
            "[White \"{white}\"]\n[Black \"Fixed opponent\"]\n[Date \"2026.01.01\"]\n[Result \"1-0\"]\n\n{}\n\n",
            Self::OPERA
        )
    }

    fn pawn_movetext(white_rank: char, black_rank: char, index: u64) -> StorageBenchResult<String> {
        let file = |position: u64| -> StorageBenchResult<char> {
            Ok(char::from(
                *Self::FILES
                    .get(usize::try_from(position)?)
                    .ok_or("file out of range")?,
            ))
        };
        let second = |position: u64, taken: u64| {
            if position < taken {
                position
            } else {
                position + 1
            }
        };
        let white = index % 8;
        let black = (index / 8) % 8;
        let white_second = second((index / 64) % 7, white);
        let black_second = second((index / 448) % 7, black);
        Ok(format!(
            "1. {}{white_rank} {}{black_rank} 2. {}{white_rank} {}{black_rank}",
            file(white)?,
            file(black)?,
            file(white_second)?,
            file(black_second)?,
        ))
    }

    fn existing_pgn(index: u64) -> StorageBenchResult<String> {
        Ok(format!(
            "[White \"Existing {index:05}\"]\n[Black \"Fixed opponent\"]\n[Date \"2026.01.01\"]\n[Result \"*\"]\n\n{} {{existing λ {index:05}}} *\n\n",
            Self::pawn_movetext('4', '5', index)?
        ))
    }

    fn incoming_pgn(run: usize, sample: usize) -> StorageBenchResult<String> {
        let mut games = String::new();
        for game in 0..Self::INCOMING_GAMES {
            let index = u64::try_from(sample)? * Self::INCOMING_GAMES + game;
            games.push_str(&format!(
                "[White \"Incoming {run} {sample} {game:04}\"]\n[Black \"Fixed opponent\"]\n[Date \"2026.01.01\"]\n[Result \"*\"]\n\n{} {{incoming λ {index:05}}} *\n\n",
                Self::pawn_movetext('3', '6', index)?
            ));
        }
        Ok(games)
    }

    fn corpus_pgn(index: u128) -> StorageBenchResult<String> {
        let plies = 1 + usize::try_from(index % 6)?;
        let mut movetext = Self::OPENING_TOKENS[..plies].join(" ");
        let result = Self::RESULTS[usize::try_from(index % 4)?];
        movetext.push(' ');
        movetext.push_str(result);
        Ok(format!(
            "[White \"Group {}\"]\n[Black \"Opponent {}\"]\n[Date \"{}.01.{:02}\"]\n[Opening \"Opening {}\"]\n[Result \"{result}\"]\n\n{movetext}\n\n",
            index % 20,
            index % 7,
            2020 + index % 5,
            1 + index % 28,
            index % 9,
        ))
    }

    async fn insert(
        library: &StorageLibraryRepository<'_>,
        id: GameId,
        expected: GameStorageRevision,
        body: String,
        timestamp: i64,
    ) -> StorageBenchResult<GameStorageRevision> {
        let prepared = library
            .prepare_write(
                id,
                expected,
                body,
                StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(
                    timestamp,
                )?),
            )
            .await?;
        let mut pending = library.begin_save(prepared)?;
        match pending.wait().await? {
            StorageLibraryEnumMutationReceipt::Saved(receipt) => Ok(receipt.content_revision()),
            StorageLibraryEnumMutationReceipt::Organized(_)
            | StorageLibraryEnumMutationReceipt::DestructionApplied(_)
            | StorageLibraryEnumMutationReceipt::Restored(_) => {
                Err("unexpected library receipt".into())
            }
        }
    }
}

struct StorageBenchEnvironment {
    directory: tempfile::TempDir,
    executor: Arc<CapacityExecutor>,
    store: StorageDocumentStore,
}

impl StorageBenchEnvironment {
    async fn open(workers: NonZeroUsize) -> StorageBenchResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(workers)?;
        let executor = Arc::new(CapacityExecutor::new(budget.clone())?);
        let store =
            StorageDocumentStore::open(directory.path().join("bench.sqlite3"), budget).await?;
        Ok(Self {
            directory,
            executor,
            store,
        })
    }

    fn library(&self) -> StorageLibraryRepository<'_> {
        StorageLibraryRepository::new(self.store.client(), &self.executor)
    }

    async fn close(self) -> StorageBenchResult {
        self.store.close().await?;
        Arc::try_unwrap(self.executor)
            .map_err(|_retained| "the bench retained the executor")?
            .shutdown()
            .await?;
        self.directory.close()?;
        Ok(())
    }
}

struct StorageBenchRunner {
    workers: usize,
    run: usize,
}

impl StorageBenchRunner {
    async fn import(&self, environment: &StorageBenchEnvironment) -> StorageBenchResult {
        let library = environment.library();
        for index in 0..StorageBenchFixture::EXISTING_GAMES {
            let id = StorageBenchFixture::game_id(10_000 + u128::from(index))?;
            let body = StorageBenchFixture::body(id, &StorageBenchFixture::existing_pgn(index)?)?;
            StorageBenchFixture::insert(
                &library,
                id,
                GameStorageRevision::INITIAL,
                body,
                1_000 + i64::try_from(index)?,
            )
            .await?;
        }
        let anchor = File::open(environment.directory.path())?;
        let filesystem = Arc::new(
            chess::storage::import_session::StorageImportSessionFilesystem::new(
                anchor.try_clone()?,
                anchor,
                chess::storage::import_session::StorageImportSessionUnixSpace,
            ),
        );
        let port = Arc::new(StorageLibraryImportRepository::new(
            environment.store.client(),
            StorageImportSessionDestination::unfiled(),
            Arc::new(StorageBenchWall),
        ));
        let total = StorageBenchFixture::IMPORT_WARMUP + StorageBenchFixture::IMPORT_SAMPLES;
        let mut prepare = StorageBenchSamples::new(
            StorageBenchFixture::IMPORT_WARMUP,
            StorageBenchFixture::IMPORT_SAMPLES,
        );
        let mut freeze = StorageBenchSamples::new(
            StorageBenchFixture::IMPORT_WARMUP,
            StorageBenchFixture::IMPORT_SAMPLES,
        );
        let mut commit = StorageBenchSamples::new(
            StorageBenchFixture::IMPORT_WARMUP,
            StorageBenchFixture::IMPORT_SAMPLES,
        );
        let mut whole = StorageBenchSamples::new(
            StorageBenchFixture::IMPORT_WARMUP,
            StorageBenchFixture::IMPORT_SAMPLES,
        );
        for sample in 0..total {
            let mut source = tempfile::tempfile()?;
            source.write_all(StorageBenchFixture::incoming_pgn(self.run, sample)?.as_bytes())?;
            let ids = StorageBenchIds(AtomicU64::new(1_000_000 * (u64::try_from(sample)? + 1)));
            let mut session = StorageImportSession::new(
                source,
                StorageImportSessionResources {
                    executor: Arc::clone(&environment.executor),
                    repository: Arc::clone(&port),
                    filesystem: Arc::clone(&filesystem),
                    identities: Arc::new(ids),
                },
            )?;
            let started = Instant::now();
            let summary = session.prepare().await?;
            let prepared = started.elapsed();
            if summary.valid() != StorageBenchFixture::INCOMING_GAMES {
                return Err("import did not validate every incoming game".into());
            }
            let frozen_started = Instant::now();
            let confirmation = session.freeze().await?;
            let frozen = frozen_started.elapsed();
            let commit_started = Instant::now();
            let outcome = session.commit(confirmation).await?;
            let committed = commit_started.elapsed();
            match outcome.as_ref() {
                StorageImportSessionEnumCommitOutcome::Committed(receipt)
                    if receipt.count() == StorageBenchFixture::INCOMING_GAMES => {}
                StorageImportSessionEnumCommitOutcome::Committed(_)
                | StorageImportSessionEnumCommitOutcome::CommittedCleanupFailed { .. } => {
                    return Err("import commit did not land every game".into());
                }
            }
            prepare.record(prepared);
            freeze.record(frozen);
            commit.record(committed);
            whole.record(started.elapsed());
            drop(session);
        }
        prepare.emit(
            StorageBenchEnumWorkload::ImportPrepare,
            self.workers,
            self.run,
        )?;
        freeze.emit(
            StorageBenchEnumWorkload::ImportFreeze,
            self.workers,
            self.run,
        )?;
        commit.emit(
            StorageBenchEnumWorkload::ImportCommit,
            self.workers,
            self.run,
        )?;
        whole.emit(
            StorageBenchEnumWorkload::ImportTotal,
            self.workers,
            self.run,
        )
    }

    async fn library_query(&self, environment: &StorageBenchEnvironment) -> StorageBenchResult {
        let library = environment.library();
        for index in 0..StorageBenchFixture::CORPUS_GAMES {
            let id = StorageBenchFixture::game_id(100_000 + index)?;
            let body = StorageBenchFixture::body(id, &StorageBenchFixture::corpus_pgn(index)?)?;
            StorageBenchFixture::insert(
                &library,
                id,
                GameStorageRevision::INITIAL,
                body,
                1_000 + i64::try_from(index)?,
            )
            .await?;
        }
        let first = StorageLibraryQueryOptions::default();
        let filtered = StorageLibraryQueryOptions {
            white: StorageLibraryEnumFilter::Value(GameMetadataText::new("Group 3".to_owned())?),
            sort: StorageLibraryEnumSort::DateDescending,
            ..StorageLibraryQueryOptions::default()
        };
        let walk = StorageLibraryQueryOptions {
            sort: StorageLibraryEnumSort::PliesDescending,
            ..StorageLibraryQueryOptions::default()
        };
        let mut first_samples =
            StorageBenchSamples::new(StorageBenchFixture::WARMUP, StorageBenchFixture::SAMPLES);
        let mut filtered_samples =
            StorageBenchSamples::new(StorageBenchFixture::WARMUP, StorageBenchFixture::SAMPLES);
        let mut walk_samples = StorageBenchSamples::new(
            StorageBenchFixture::WALK_WARMUP,
            StorageBenchFixture::WALK_SAMPLES,
        );
        for _ in 0..StorageBenchFixture::WARMUP + StorageBenchFixture::SAMPLES {
            let started = Instant::now();
            let page = library
                .query(StorageLibraryQuery::new(first.clone())?)
                .await?;
            first_samples.record(started.elapsed());
            if page.entries().len() != StorageLibraryQuery::PAGE_SIZE {
                return Err("first page did not fill".into());
            }
            let started = Instant::now();
            let page = library
                .query(StorageLibraryQuery::new(filtered.clone())?)
                .await?;
            filtered_samples.record(started.elapsed());
            if page.entries().is_empty() {
                return Err("filtered page was empty".into());
            }
        }
        for _ in 0..StorageBenchFixture::WALK_WARMUP + StorageBenchFixture::WALK_SAMPLES {
            let started = Instant::now();
            let mut seen = 0_usize;
            let mut cursor = None;
            loop {
                let mut query = StorageLibraryQuery::new(walk.clone())?;
                if let Some(next) = cursor.take() {
                    query = query.with_cursor(next)?;
                }
                let page = library.query(query).await?;
                seen += page.entries().len();
                match page.next_cursor() {
                    Some(next) => cursor = Some(next.clone()),
                    None => break,
                }
            }
            walk_samples.record(started.elapsed());
            if seen != usize::try_from(StorageBenchFixture::CORPUS_GAMES)? {
                return Err("keyset walk missed a record".into());
            }
        }
        first_samples.emit(
            StorageBenchEnumWorkload::LibraryFirstPage,
            self.workers,
            self.run,
        )?;
        filtered_samples.emit(
            StorageBenchEnumWorkload::LibraryFilteredPage,
            self.workers,
            self.run,
        )?;
        walk_samples.emit(
            StorageBenchEnumWorkload::LibraryKeysetWalk,
            self.workers,
            self.run,
        )
    }

    async fn documents(&self, environment: &StorageBenchEnvironment) -> StorageBenchResult {
        let client = environment.store.client();
        let library = environment.library();
        let raw_id = StorageBenchFixture::game_id(1)?;
        let library_id = StorageBenchFixture::game_id(2)?;
        let bodies = [
            StorageBenchFixture::body(raw_id, &StorageBenchFixture::opera_pgn("Raw A"))?,
            StorageBenchFixture::body(raw_id, &StorageBenchFixture::opera_pgn("Raw B"))?,
        ];
        let library_bodies = [
            StorageBenchFixture::body(library_id, &StorageBenchFixture::opera_pgn("Library A"))?,
            StorageBenchFixture::body(library_id, &StorageBenchFixture::opera_pgn("Library B"))?,
        ];
        let mut revision = client
            .save(raw_id.as_uuid(), 0, bodies[0].clone())
            .await?
            .revision();
        let mut library_revision = StorageBenchFixture::insert(
            &library,
            library_id,
            GameStorageRevision::INITIAL,
            library_bodies[0].clone(),
            1,
        )
        .await?;
        let total = StorageBenchFixture::WARMUP + StorageBenchFixture::SAMPLES;
        let mut save =
            StorageBenchSamples::new(StorageBenchFixture::WARMUP, StorageBenchFixture::SAMPLES);
        let mut open =
            StorageBenchSamples::new(StorageBenchFixture::WARMUP, StorageBenchFixture::SAMPLES);
        let mut library_save =
            StorageBenchSamples::new(StorageBenchFixture::WARMUP, StorageBenchFixture::SAMPLES);
        let mut library_open =
            StorageBenchSamples::new(StorageBenchFixture::WARMUP, StorageBenchFixture::SAMPLES);
        for sample in 0..total {
            let body = bodies[(sample + 1) % 2].clone();
            let started = Instant::now();
            revision = client
                .save(raw_id.as_uuid(), revision, body)
                .await?
                .revision();
            save.record(started.elapsed());
            let started = Instant::now();
            let document = client
                .read(raw_id.as_uuid())
                .await?
                .ok_or("missing saved document")?;
            open.record(started.elapsed());
            if document.revision() != revision {
                return Err("opened document revision differs from the saved revision".into());
            }
            drop(document);
            let body = library_bodies[(sample + 1) % 2].clone();
            let started = Instant::now();
            library_revision = StorageBenchFixture::insert(
                &library,
                library_id,
                library_revision,
                body,
                2 + i64::try_from(sample)?,
            )
            .await?;
            library_save.record(started.elapsed());
            let started = Instant::now();
            let loaded = library
                .load(library_id)
                .await?
                .ok_or("missing saved library record")?;
            library_open.record(started.elapsed());
            if loaded.content_revision() != library_revision {
                return Err("loaded library revision differs from the saved revision".into());
            }
        }
        save.emit(
            StorageBenchEnumWorkload::DocumentSave,
            self.workers,
            self.run,
        )?;
        open.emit(
            StorageBenchEnumWorkload::DocumentOpen,
            self.workers,
            self.run,
        )?;
        library_save.emit(
            StorageBenchEnumWorkload::LibraryGameSave,
            self.workers,
            self.run,
        )?;
        library_open.emit(
            StorageBenchEnumWorkload::LibraryGameOpen,
            self.workers,
            self.run,
        )
    }

    fn puzzle_owner() -> StorageBenchResult<PuzzlePersistenceRun> {
        let initial = BoardPosition::standard();
        let movement = initial.parse_uci("e2e4")?;
        let definition = PuzzleDefinition::new(
            PuzzleDefinitionIdentity::new("bench".into(), "v1".into(), "first".into())?,
            initial,
            vec![vec![movement]],
            PuzzleDefinitionMetadata::default(),
        )?;
        Ok(PuzzlePersistenceRun::new(
            PuzzleSessionId::new(NonZeroU64::MIN),
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
            vec![definition],
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(100),
        )?)
    }

    fn puzzle_snapshot(
        owner: &mut PuzzlePersistenceRun,
        now: ClockInstant,
        base: usize,
    ) -> StorageBenchResult<PuzzleCheckpointSnapshot<PuzzlePersistenceSnapshot>> {
        let payload = owner.capture(now, base)?;
        Ok(PuzzleCheckpointSnapshot::new(
            PuzzleCheckpointObservation::from_run(owner.run(), now)?,
            payload,
        ))
    }

    async fn puzzle(&self, environment: &StorageBenchEnvironment) -> StorageBenchResult {
        let repository = StoragePuzzleRepository::new(environment.store.client());
        let mut owner = Self::puzzle_owner()?;
        let origin = ClockInstant::from_nanos(0);
        let mut driver = PuzzleStorageDriver::new(
            repository,
            Self::puzzle_snapshot(&mut owner, origin, 0)?,
            PuzzlePersistenceStorageRevision::INITIAL,
            0,
        )?;
        driver.begin_write(origin)?;
        driver.wait().await?;
        driver.settle(origin)?;
        owner.execute(
            PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
            origin,
        )?;
        driver.record(
            Self::puzzle_snapshot(&mut owner, origin, 0)?,
            PuzzleCheckpointEnumTrigger::AcceptedTransition,
        )?;
        driver.begin_write(origin)?;
        driver.wait().await?;
        driver.settle(origin)?;
        let mut samples =
            StorageBenchSamples::new(StorageBenchFixture::WARMUP, StorageBenchFixture::SAMPLES);
        for sample in 0..StorageBenchFixture::WARMUP + StorageBenchFixture::SAMPLES {
            let now = ClockInstant::from_millis(u64::try_from(sample)? + 1)?;
            let command = if sample % 2 == 0 {
                PuzzlePersistenceEnumCommand::Pause(PuzzleTimerEnumPauseReason::Manual)
            } else {
                PuzzlePersistenceEnumCommand::Resume(PuzzleTimerEnumReadiness::Ready)
            };
            owner.execute(command, now)?;
            driver.record(
                Self::puzzle_snapshot(&mut owner, now, driver.persisted_event_count())?,
                PuzzleCheckpointEnumTrigger::AcceptedTransition,
            )?;
            let started = Instant::now();
            driver.begin_write(now)?;
            driver.wait().await?;
            samples.record(started.elapsed());
            driver.settle(now)?;
            if driver.persisted_event_count() != owner.event_count() {
                return Err("puzzle driver did not persist every event".into());
            }
        }
        samples.emit(
            StorageBenchEnumWorkload::PuzzleEventAppend,
            self.workers,
            self.run,
        )
    }

    async fn editor(&self, environment: &StorageBenchEnvironment) -> StorageBenchResult {
        let repository = StorageEditorRepository::new(environment.store.client());
        let mut session =
            BoardEditorSession::new(uuid::Uuid::from_u128(1).to_string().parse()?, None);
        let mut revision = StorageEditorRepositoryRevision::ABSENT;
        let mut samples =
            StorageBenchSamples::new(StorageBenchFixture::WARMUP, StorageBenchFixture::SAMPLES);
        for sample in 0..StorageBenchFixture::WARMUP + StorageBenchFixture::SAMPLES {
            session.apply(BoardEditorSessionRequest {
                draft: session.draft().id(),
                expected_revision: session.draft().revision(),
                operation: BoardEditorSessionEnumOperation::Edit(
                    BoardEditorSessionEnumEdit::Counters {
                        halfmoves: i64::try_from(sample)?,
                        fullmoves: i64::try_from(sample)? + 1,
                    },
                ),
            })?;
            let started = Instant::now();
            let receipt = repository.save(session.draft(), revision)?.wait().await?;
            samples.record(started.elapsed());
            revision = receipt.storage_revision();
        }
        samples.emit(
            StorageBenchEnumWorkload::EditorDraftSave,
            self.workers,
            self.run,
        )
    }

    async fn execute(workers: NonZeroUsize, run: usize) -> StorageBenchResult {
        let runner = Self {
            workers: workers.get(),
            run,
        };
        let environment = StorageBenchEnvironment::open(workers).await?;
        runner.import(&environment).await?;
        environment.close().await?;
        let environment = StorageBenchEnvironment::open(workers).await?;
        runner.library_query(&environment).await?;
        environment.close().await?;
        let environment = StorageBenchEnvironment::open(workers).await?;
        runner.documents(&environment).await?;
        runner.puzzle(&environment).await?;
        runner.editor(&environment).await?;
        environment.close().await
    }
}

fn main() -> StorageBenchResult {
    let detected = std::thread::available_parallelism()?;
    let mut budgets = vec![NonZeroUsize::MIN];
    if detected > NonZeroUsize::MIN {
        budgets.push(detected);
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    println!("detected hardware threads: {detected}");
    println!(
        "| workload | workers | run | samples | min ms | p50 ms | p95 ms (nearest rank) | max ms |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- |");
    for workers in budgets {
        for run in 1..=StorageBenchFixture::RUNS {
            runtime.block_on(StorageBenchRunner::execute(workers, run))?;
        }
    }
    Ok(())
}
