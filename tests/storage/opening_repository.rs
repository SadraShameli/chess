use std::num::NonZeroUsize;

use chess::capacity::budget::CapacityBudget;
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::opening_repository::{
    StorageOpeningCursor, StorageOpeningError, StorageOpeningGameIndex, StorageOpeningOccurrence,
    StorageOpeningPositionKey, StorageOpeningRepository, StorageOpeningSource,
};

type StorageOpeningTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct StorageOpeningFixture {
    _directory: tempfile::TempDir,
    store: StorageDocumentStore,
    repository: StorageOpeningRepository,
}

impl StorageOpeningFixture {
    async fn new() -> StorageOpeningTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
        let store =
            StorageDocumentStore::open(directory.path().join("openings.sqlite"), budget).await?;
        let repository = StorageOpeningRepository::new(store.client());
        Ok(Self {
            _directory: directory,
            store,
            repository,
        })
    }

    fn source(name: &str) -> StorageOpeningTestResult<StorageOpeningSource> {
        Ok(StorageOpeningSource::new(name.to_owned())?)
    }

    fn key(name: &str) -> StorageOpeningTestResult<StorageOpeningPositionKey> {
        Ok(StorageOpeningPositionKey::new(name.to_owned())?)
    }

    fn occurrence(
        key: &str,
        ply: u16,
        next: Option<&str>,
    ) -> StorageOpeningTestResult<StorageOpeningOccurrence> {
        Ok(StorageOpeningOccurrence::new(
            Self::key(key)?,
            ply,
            next.map(str::to_owned),
        )?)
    }

    fn game(
        source: &str,
        game: u64,
        plies: u16,
        occurrences: Vec<StorageOpeningOccurrence>,
    ) -> StorageOpeningTestResult<StorageOpeningGameIndex> {
        Ok(StorageOpeningGameIndex::new(
            Self::source(source)?,
            game,
            plies,
            occurrences,
        )?)
    }
}

#[tokio::test]
async fn indexed_games_report_hits_and_exact_continuation_counts() -> StorageOpeningTestResult {
    let fixture = StorageOpeningFixture::new().await?;
    let games = vec![
        StorageOpeningFixture::game(
            "a",
            1,
            40,
            vec![
                StorageOpeningFixture::occurrence("root", 0, Some("e2e4"))?,
                StorageOpeningFixture::occurrence("after-e4", 1, None)?,
            ],
        )?,
        StorageOpeningFixture::game(
            "a",
            2,
            40,
            vec![StorageOpeningFixture::occurrence("root", 0, Some("d2d4"))?],
        )?,
        StorageOpeningFixture::game(
            "b",
            1,
            40,
            vec![StorageOpeningFixture::occurrence("root", 0, Some("e2e4"))?],
        )?,
    ];
    assert_eq!(fixture.repository.index(games).await?, 3);
    let continuations = fixture
        .repository
        .continuations(StorageOpeningFixture::key("root")?)
        .await?;
    let counts: Vec<_> = continuations
        .iter()
        .map(|continuation| {
            (
                continuation.next_move().map(str::to_owned),
                continuation.games(),
            )
        })
        .collect();
    assert_eq!(
        counts,
        vec![(Some("d2d4".to_owned()), 1), (Some("e2e4".to_owned()), 2)]
    );
    let ending = fixture
        .repository
        .continuations(StorageOpeningFixture::key("after-e4")?)
        .await?;
    assert_eq!(ending.len(), 1);
    assert_eq!(ending[0].next_move(), None);
    assert!(
        fixture
            .repository
            .continuations(StorageOpeningFixture::key("absent")?)
            .await?
            .is_empty()
    );
    fixture.store.close().await?;
    Ok(())
}

#[tokio::test]
async fn hits_page_in_source_then_game_order() -> StorageOpeningTestResult {
    let fixture = StorageOpeningFixture::new().await?;
    let mut games = Vec::new();
    for game in (1..=150).rev() {
        games.push(StorageOpeningFixture::game(
            "a",
            game,
            40,
            vec![StorageOpeningFixture::occurrence("root", 0, Some("e2e4"))?],
        )?);
    }
    games.push(StorageOpeningFixture::game(
        "b",
        1,
        40,
        vec![StorageOpeningFixture::occurrence("root", 0, None)?],
    )?);
    fixture.repository.index(games).await?;
    let first = fixture
        .repository
        .hits(StorageOpeningFixture::key("root")?, None)
        .await?;
    assert_eq!(first.len(), 100);
    assert_eq!(first[0].game(), 1);
    let cursor = first.last().ok_or("empty page")?.cursor();
    let second = fixture
        .repository
        .hits(StorageOpeningFixture::key("root")?, Some(cursor))
        .await?;
    assert_eq!(second.len(), 51);
    assert_eq!(second[0].game(), 101);
    let last = second.last().ok_or("empty page")?;
    assert_eq!(last.source().as_str(), "b");
    assert_eq!(last.next_move(), None);
    let after_last = fixture
        .repository
        .hits(
            StorageOpeningFixture::key("root")?,
            Some(StorageOpeningCursor::new(
                StorageOpeningFixture::source("b")?,
                1,
            )),
        )
        .await?;
    assert!(after_last.is_empty());
    fixture.store.close().await?;
    Ok(())
}

#[tokio::test]
async fn extension_raises_the_depth_and_never_shrinks_it() -> StorageOpeningTestResult {
    let fixture = StorageOpeningFixture::new().await?;
    let source = StorageOpeningFixture::source("a")?;
    assert_eq!(
        fixture.repository.indexed_plies(source.clone(), 1).await?,
        None
    );
    fixture
        .repository
        .index(vec![StorageOpeningFixture::game(
            "a",
            1,
            40,
            vec![StorageOpeningFixture::occurrence("root", 0, Some("e2e4"))?],
        )?])
        .await?;
    fixture
        .repository
        .index(vec![StorageOpeningFixture::game(
            "a",
            1,
            80,
            vec![StorageOpeningFixture::occurrence("deep", 60, None)?],
        )?])
        .await?;
    assert_eq!(
        fixture.repository.indexed_plies(source.clone(), 1).await?,
        Some(80)
    );
    let regression = fixture
        .repository
        .index(vec![StorageOpeningFixture::game("a", 1, 50, Vec::new())?])
        .await;
    assert!(matches!(
        regression,
        Err(StorageOpeningError::DepthRegression {
            recorded: 80,
            requested: 50
        })
    ));
    assert_eq!(fixture.repository.indexed_plies(source, 1).await?, Some(80));
    fixture.store.close().await?;
    Ok(())
}

#[tokio::test]
async fn a_failed_chunk_leaves_no_partial_games() -> StorageOpeningTestResult {
    let fixture = StorageOpeningFixture::new().await?;
    fixture
        .repository
        .index(vec![StorageOpeningFixture::game(
            "a",
            1,
            40,
            vec![StorageOpeningFixture::occurrence("root", 0, None)?],
        )?])
        .await?;
    let outcome = fixture
        .repository
        .index(vec![
            StorageOpeningFixture::game(
                "a",
                2,
                40,
                vec![StorageOpeningFixture::occurrence("root", 0, None)?],
            )?,
            StorageOpeningFixture::game(
                "a",
                1,
                40,
                vec![StorageOpeningFixture::occurrence("root", 0, None)?],
            )?,
        ])
        .await;
    assert!(matches!(
        outcome,
        Err(StorageOpeningError::DuplicateOccurrence)
    ));
    assert_eq!(
        fixture
            .repository
            .indexed_plies(StorageOpeningFixture::source("a")?, 2)
            .await?,
        None
    );
    fixture.store.close().await?;
    Ok(())
}

#[tokio::test]
async fn removing_a_source_deletes_its_games_and_orphan_positions() -> StorageOpeningTestResult {
    let fixture = StorageOpeningFixture::new().await?;
    fixture
        .repository
        .index(vec![
            StorageOpeningFixture::game(
                "a",
                1,
                40,
                vec![
                    StorageOpeningFixture::occurrence("root", 0, Some("e2e4"))?,
                    StorageOpeningFixture::occurrence("only-a", 1, None)?,
                ],
            )?,
            StorageOpeningFixture::game(
                "b",
                1,
                40,
                vec![StorageOpeningFixture::occurrence("root", 0, Some("d2d4"))?],
            )?,
        ])
        .await?;
    assert_eq!(
        fixture
            .repository
            .remove_source(StorageOpeningFixture::source("a")?)
            .await?,
        1
    );
    assert!(
        fixture
            .repository
            .hits(StorageOpeningFixture::key("only-a")?, None)
            .await?
            .is_empty()
    );
    let remaining = fixture
        .repository
        .hits(StorageOpeningFixture::key("root")?, None)
        .await?;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].source().as_str(), "b");
    fixture.store.close().await?;
    Ok(())
}

#[test]
fn records_reject_invalid_shapes() -> StorageOpeningTestResult {
    assert!(StorageOpeningSource::new(String::new()).is_err());
    assert!(StorageOpeningPositionKey::new("has space".to_owned()).is_err());
    assert!(
        StorageOpeningOccurrence::new(StorageOpeningFixture::key("k")?, 0, Some("e".to_owned()))
            .is_err()
    );
    assert!(
        StorageOpeningGameIndex::new(StorageOpeningFixture::source("a")?, 0, 40, Vec::new())
            .is_err()
    );
    assert!(
        StorageOpeningGameIndex::new(StorageOpeningFixture::source("a")?, 1, 39, Vec::new())
            .is_err()
    );
    assert!(
        StorageOpeningGameIndex::new(StorageOpeningFixture::source("a")?, 1, 121, Vec::new())
            .is_err()
    );
    assert!(
        StorageOpeningGameIndex::new(
            StorageOpeningFixture::source("a")?,
            u64::from(u32::MAX),
            40,
            Vec::new()
        )
        .is_err()
    );
    let beyond = vec![StorageOpeningFixture::occurrence("k", 41, None)?];
    assert!(
        StorageOpeningGameIndex::new(StorageOpeningFixture::source("a")?, 1, 40, beyond).is_err()
    );
    let repeated = vec![
        StorageOpeningFixture::occurrence("k", 0, None)?,
        StorageOpeningFixture::occurrence("k", 2, None)?,
    ];
    assert!(matches!(
        StorageOpeningGameIndex::new(StorageOpeningFixture::source("a")?, 1, 40, repeated),
        Err(StorageOpeningError::RepeatedPosition)
    ));
    Ok(())
}
