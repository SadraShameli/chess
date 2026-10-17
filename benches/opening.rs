use std::error::Error;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use chess::board::position::BoardPosition;
use chess::board::r#move::BoardMove;
use chess::capacity::budget::CapacityBudget;
use chess::opening::corpus::OpeningCorpusGameId;
use chess::opening::index::{OpeningIndexDepth, OpeningIndexGameInput, OpeningIndexKey, OpeningIndexer};
use chess::package::digest::PackageDigest;
use chess::storage::connection::StorageConnection;
use chess::storage::dataset_package::{
    StorageDatasetPackageEnumFormat, StorageDatasetPackageEnumValidation,
};
use chess::storage::dataset_registry::{
    StorageDatasetRegistry, StorageDatasetRegistryEnumHolder, StorageDatasetRegistryEnumKind,
    StorageDatasetRegistryEnumOrigin, StorageDatasetRegistryNew,
};
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::migration::StorageMigrationRunner;
use chess::storage::opening_repository::{StorageOpeningRepository, StorageOpeningSource};

type OpeningBenchResult<T = ()> = Result<T, Box<dyn Error>>;

struct OpeningBenchFixture;

impl OpeningBenchFixture {
    const RUNS: usize = 5;
    const DEFAULT_GAMES: usize = 2_000;
    const QUERIES: usize = 200;
    const REPLACEMENTS: usize = 50;
    const GAMES_VARIABLE: &'static str = "CHESS_BENCH_OPENING_GAMES";

    fn games() -> usize {
        std::env::var(Self::GAMES_VARIABLE)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(Self::DEFAULT_GAMES)
    }

    fn line(seed: u64) -> OpeningBenchResult<Vec<BoardMove>> {
        let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_add(1);
        let mut board = BoardPosition::standard();
        let mut line = Vec::new();
        for _ply in 0..OpeningIndexDepth::BASE_PLIES {
            let moves = board.legal_moves();
            if moves.is_empty() {
                break;
            }
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let index = usize::try_from(state % u64::try_from(moves.len())?)?;
            let movement = *moves.get(index).ok_or("missing generated move")?;
            board = board.play(movement)?;
            line.push(movement);
        }
        Ok(line)
    }

    fn nearest_rank(sorted: &[Duration], percentile: usize) -> Option<Duration> {
        let rank = (percentile * sorted.len()).div_ceil(100);
        sorted.get(rank.checked_sub(1)?).copied()
    }

    fn milliseconds(value: Duration) -> f64 {
        value.as_secs_f64() * 1_000.0
    }

    fn emit(workload: &str, workers: usize, run: usize, mut samples: Vec<Duration>) -> OpeningBenchResult {
        samples.sort_unstable();
        let p50 = Self::nearest_rank(&samples, 50).ok_or("no samples")?;
        let p95 = Self::nearest_rank(&samples, 95).ok_or("no samples")?;
        let min = samples.first().copied().ok_or("no samples")?;
        let max = samples.last().copied().ok_or("no samples")?;
        println!(
            "| {workload} | {workers} | {run} | {} | {:.3} | {:.3} | {:.3} | {:.3} |",
            samples.len(),
            Self::milliseconds(min),
            Self::milliseconds(p50),
            Self::milliseconds(p95),
            Self::milliseconds(max)
        );
        Ok(())
    }
}

struct OpeningBenchRunner;

impl OpeningBenchRunner {
    async fn execute(workers: NonZeroUsize, run: usize) -> OpeningBenchResult {
        let directory = tempfile::tempdir()?;
        let store = StorageDocumentStore::open(
            directory.path().join("opening.sqlite3"),
            CapacityBudget::new(workers)?,
        )
        .await?;
        let repository = StorageOpeningRepository::new(store.client());
        let source = StorageOpeningSource::new("bench".to_owned())?;
        let initial = BoardPosition::standard();
        let count = OpeningBenchFixture::games();
        let mut lines = Vec::with_capacity(count);
        for game in 0..count {
            lines.push(OpeningBenchFixture::line(u64::try_from(game)?)?);
        }
        let started = Instant::now();
        let mut indexed = Vec::with_capacity(count);
        for (game, line) in lines.iter().enumerate() {
            let input = OpeningIndexGameInput {
                source: &source,
                id: OpeningCorpusGameId::new(u64::try_from(game)?.saturating_add(1))?,
                initial: &initial,
                mainline: line,
                recorded: None,
            };
            if let Some(index) = OpeningIndexer::index(&input, OpeningIndexDepth::base(), || false)? {
                indexed.push(index);
            }
        }
        let written = repository.index(indexed).await?;
        let elapsed = started.elapsed();
        println!(
            "indexed {written} games in {:.3} s ({:.1} games/s) with {} worker(s), run {run}",
            elapsed.as_secs_f64(),
            f64::from(u32::try_from(written)?) / elapsed.as_secs_f64().max(f64::EPSILON),
            workers
        );
        let key = OpeningIndexKey::of(&initial)?;
        let mut queries = Vec::with_capacity(OpeningBenchFixture::QUERIES);
        for _query in 0..OpeningBenchFixture::QUERIES {
            let started = Instant::now();
            repository.continuations(key.clone()).await?;
            queries.push(started.elapsed());
        }
        OpeningBenchFixture::emit("indexed continuation query", workers.get(), run, queries)?;
        let cancelled_after = Instant::now();
        let mut checks = 0_usize;
        let outcome = OpeningIndexer::index(
            &OpeningIndexGameInput {
                source: &source,
                id: OpeningCorpusGameId::new(u64::MAX >> 33)?,
                initial: &initial,
                mainline: lines.first().map(Vec::as_slice).unwrap_or_default(),
                recorded: None,
            },
            OpeningIndexDepth::base(),
            || {
                checks += 1;
                checks > 3
            },
        );
        println!(
            "cancellation observed after {:.3} ms ({} checks, cancelled: {})",
            OpeningBenchFixture::milliseconds(cancelled_after.elapsed()),
            checks,
            outcome.is_err()
        );
        store.close().await?;
        directory.close()?;
        Ok(())
    }

    fn replacement(run: usize) -> OpeningBenchResult {
        let directory = tempfile::tempdir()?;
        let mut connection = StorageConnection::open(&directory.path().join("datasets.sqlite3"))?;
        StorageMigrationRunner::run_embedded(connection.diesel())?;
        let dataset = |version: usize| StorageDatasetRegistryNew {
            kind: StorageDatasetRegistryEnumKind::Corpus,
            origin: StorageDatasetRegistryEnumOrigin::Personal,
            name: "bench".to_owned(),
            version: version.to_string(),
            format: StorageDatasetPackageEnumFormat::Pgn,
            license: "Personal use".to_owned(),
            checksum: PackageDigest::hash_slice(version.to_string().as_bytes()),
            location: "/bench.pgn".to_owned(),
            validation: StorageDatasetPackageEnumValidation::IntegrityVerified,
            records: Some(1),
        };
        let mut current = StorageDatasetRegistry::register(connection.diesel(), &dataset(0))?;
        let mut samples = Vec::with_capacity(OpeningBenchFixture::REPLACEMENTS);
        for version in 1..=OpeningBenchFixture::REPLACEMENTS {
            let lease = StorageDatasetRegistry::lease(
                connection.diesel(),
                current,
                StorageDatasetRegistryEnumHolder::Query,
                "bench",
            )?;
            let started = Instant::now();
            let next = StorageDatasetRegistry::replace(connection.diesel(), current, &dataset(version))?;
            samples.push(started.elapsed());
            StorageDatasetRegistry::release(connection.diesel(), lease)?;
            StorageDatasetRegistry::purge_released(connection.diesel())?;
            current = next;
        }
        OpeningBenchFixture::emit("dataset replacement", 1, run, samples)?;
        directory.close()?;
        Ok(())
    }
}

fn main() -> OpeningBenchResult {
    let detected = std::thread::available_parallelism()?;
    let mut budgets = vec![NonZeroUsize::MIN];
    if detected > NonZeroUsize::MIN {
        budgets.push(detected);
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    println!("detected hardware threads: {detected}");
    println!("games per run: {} (set {})", OpeningBenchFixture::games(), OpeningBenchFixture::GAMES_VARIABLE);
    println!("| workload | workers | run | samples | min ms | p50 ms | p95 ms (nearest rank) | max ms |");
    println!("| --- | --- | --- | --- | --- | --- | --- | --- |");
    for workers in budgets {
        for run in 1..=OpeningBenchFixture::RUNS {
            runtime.block_on(OpeningBenchRunner::execute(workers, run))?;
        }
    }
    for run in 1..=OpeningBenchFixture::RUNS {
        OpeningBenchRunner::replacement(run)?;
    }
    println!("peak memory is not captured by this harness; record it with the platform process monitor");
    Ok(())
}
