use std::collections::HashSet;
use std::num::NonZeroUsize;

use chess::board::enums::BoardEnumVariant;
use chess::board::r#move::BoardMove;
use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::opening::corpus::OpeningCorpusGameId;
use chess::opening::index::{
    OpeningIndexBatcher, OpeningIndexDepth, OpeningIndexError, OpeningIndexGameInput,
    OpeningIndexKey, OpeningIndexRun, OpeningIndexer,
};
use chess::storage::opening_repository::{StorageOpeningGameIndex, StorageOpeningSource};

type OpeningIndexTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct OpeningIndexFixture {
    initial: BoardPosition,
    mainline: Vec<BoardMove>,
    source: StorageOpeningSource,
}

impl OpeningIndexFixture {
    fn from_uci(moves: &[&str]) -> OpeningIndexTestResult<Self> {
        let initial = BoardPosition::standard();
        let mut position = initial.clone();
        let mut mainline = Vec::new();
        for value in moves {
            let movement = position.parse_uci(value)?;
            position = position.play(movement)?;
            mainline.push(movement);
        }
        Ok(Self {
            initial,
            mainline,
            source: StorageOpeningSource::new("fixture\nv1\n00".to_owned())?,
        })
    }

    fn walk(plies: usize) -> OpeningIndexTestResult<Self> {
        let initial = BoardPosition::standard();
        let mut position = initial.clone();
        let mut mainline = Vec::new();
        for _ in 0..plies {
            let movement = position
                .legal_moves()
                .into_iter()
                .max_by_key(|movement| movement.uci(BoardEnumVariant::Standard))
                .ok_or("the walk ended early")?;
            position = position.play(movement)?;
            mainline.push(movement);
        }
        Ok(Self {
            initial,
            mainline,
            source: StorageOpeningSource::new("walk\nv1\n00".to_owned())?,
        })
    }

    fn input(
        &self,
        id: u64,
        recorded: Option<OpeningIndexDepth>,
    ) -> OpeningIndexTestResult<OpeningIndexGameInput<'_>> {
        Ok(OpeningIndexGameInput {
            source: &self.source,
            id: OpeningCorpusGameId::new(id)?,
            initial: &self.initial,
            mainline: &self.mainline,
            recorded,
        })
    }

    fn first_occurrences(&self, limit: usize) -> OpeningIndexTestResult<Vec<(usize, String)>> {
        let mut board = self.initial.clone();
        let mut seen = HashSet::new();
        let mut expected = Vec::new();
        for ply in 0..=self.mainline.len().min(limit) {
            if seen.insert(board.identity()) {
                expected.push((
                    ply,
                    self.mainline.get(ply).map_or_else(String::new, |movement| {
                        movement.uci(BoardEnumVariant::Standard)
                    }),
                ));
            }
            if let Some(&movement) = self.mainline.get(ply) {
                board = board.play(movement)?;
            }
        }
        Ok(expected)
    }
}

fn summary(index: &StorageOpeningGameIndex) -> Vec<(usize, String)> {
    index
        .occurrences()
        .iter()
        .map(|occurrence| {
            (
                usize::from(occurrence.ply()),
                occurrence.next_move().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

#[test]
fn keys_agree_with_full_position_identity() -> OpeningIndexTestResult {
    let first = OpeningIndexFixture::from_uci(&["g1f3", "g8f6", "b1c3", "b8c6"])?;
    let second = OpeningIndexFixture::from_uci(&["b1c3", "b8c6", "g1f3", "g8f6"])?;
    let mut left = first.initial.clone();
    for &movement in &first.mainline {
        left = left.play(movement)?;
    }
    let mut right = second.initial.clone();
    for &movement in &second.mainline {
        right = right.play(movement)?;
    }
    assert_eq!(left.identity(), right.identity());
    assert_eq!(OpeningIndexKey::of(&left)?, OpeningIndexKey::of(&right)?);
    let counters = BoardPosition::from_fen(
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
        BoardEnumVariant::Standard,
    )?;
    let moved = BoardPosition::from_fen(
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 5 9",
        BoardEnumVariant::Standard,
    )?;
    assert_eq!(counters.identity(), moved.identity());
    assert_eq!(
        OpeningIndexKey::of(&counters)?,
        OpeningIndexKey::of(&moved)?
    );
    Ok(())
}

#[test]
fn keys_distinguish_legal_en_passant_castling_turn_and_variant() -> OpeningIndexTestResult {
    let capture = BoardPosition::from_fen(
        "rnbqkbnr/1pp1pppp/p7/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3",
        BoardEnumVariant::Standard,
    )?;
    let silent = BoardPosition::from_fen(
        "rnbqkbnr/1pp1pppp/p7/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq - 0 3",
        BoardEnumVariant::Standard,
    )?;
    assert_ne!(capture.identity(), silent.identity());
    assert_ne!(
        OpeningIndexKey::of(&capture)?,
        OpeningIndexKey::of(&silent)?
    );
    let flipped = BoardPosition::from_fen(
        "rnbqkbnr/1pp1pppp/p7/3pP3/8/8/PPPP1PPP/RNBQKBNR b KQkq - 0 3",
        BoardEnumVariant::Standard,
    )?;
    let no_rights = BoardPosition::from_fen(
        "rnbqkbnr/1pp1pppp/p7/3pP3/8/8/PPPP1PPP/RNBQKBNR w - - 0 3",
        BoardEnumVariant::Standard,
    )?;
    let chess960 = BoardPosition::from_fen(
        "rnbqkbnr/1pp1pppp/p7/3pP3/8/8/PPPP1PPP/RNBQKBNR w HAha - 0 3",
        BoardEnumVariant::Chess960,
    )?;
    let keys = HashSet::from([
        OpeningIndexKey::of(&silent)?,
        OpeningIndexKey::of(&flipped)?,
        OpeningIndexKey::of(&no_rights)?,
        OpeningIndexKey::of(&chess960)?,
    ]);
    assert_eq!(keys.len(), 4);
    Ok(())
}

#[test]
fn indexes_only_first_occurrences_of_repeated_positions() -> OpeningIndexTestResult {
    let moves: Vec<&str> = ["g1f3", "g8f6", "f3g1", "f6g8"].repeat(12);
    let fixture = OpeningIndexFixture::from_uci(&moves)?;
    let index = OpeningIndexer::index(&fixture.input(7, None)?, OpeningIndexDepth::base(), || {
        false
    })?
    .ok_or("a new game must be indexed")?;
    assert_eq!(summary(&index), fixture.first_occurrences(40)?);
    assert_eq!(index.occurrences().len(), 4);
    assert_eq!(index.indexed_plies(), 40);
    assert_eq!(index.game(), 7);
    Ok(())
}

#[test]
fn legal_indexing_stops_at_the_base_ply_and_records_the_next_move() -> OpeningIndexTestResult {
    let fixture = OpeningIndexFixture::walk(70)?;
    let index = OpeningIndexer::index(&fixture.input(1, None)?, OpeningIndexDepth::base(), || {
        false
    })?
    .ok_or("a new game must be indexed")?;
    let expected = fixture.first_occurrences(40)?;
    assert_eq!(summary(&index), expected);
    let last = index
        .occurrences()
        .iter()
        .map(|occurrence| occurrence.ply())
        .max()
        .ok_or("no occurrences")?;
    assert!(last <= 40);
    assert!(
        index
            .occurrences()
            .iter()
            .all(|occurrence| occurrence.next_move().is_some())
    );
    Ok(())
}

#[test]
fn a_short_game_ends_with_an_unrecorded_next_move() -> OpeningIndexTestResult {
    let fixture = OpeningIndexFixture::from_uci(&["e2e4", "e7e5"])?;
    let index = OpeningIndexer::index(&fixture.input(2, None)?, OpeningIndexDepth::base(), || {
        false
    })?
    .ok_or("a new game must be indexed")?;
    assert_eq!(index.occurrences().len(), 3);
    assert_eq!(index.occurrences()[2].next_move(), None);
    assert_eq!(index.occurrences()[0].next_move(), Some("e2e4"));
    Ok(())
}

#[test]
fn deeper_extension_adds_only_positions_beyond_the_recorded_depth() -> OpeningIndexTestResult {
    let fixture = OpeningIndexFixture::walk(100)?;
    let deeper = OpeningIndexDepth::extended(30)?;
    let base = OpeningIndexer::index(&fixture.input(3, None)?, OpeningIndexDepth::base(), || {
        false
    })?
    .ok_or("base must index")?;
    let extension = OpeningIndexer::index(
        &fixture.input(3, Some(OpeningIndexDepth::base()))?,
        deeper,
        || false,
    )?
    .ok_or("extension must index")?;
    assert_eq!(extension.indexed_plies(), 70);
    assert!(
        extension
            .occurrences()
            .iter()
            .all(|occurrence| occurrence.ply() > 40)
    );
    let mut combined = summary(&base);
    combined.extend(summary(&extension));
    assert_eq!(combined, fixture.first_occurrences(70)?);
    assert!(OpeningIndexer::index(&fixture.input(3, Some(deeper))?, deeper, || false)?.is_none());
    Ok(())
}

#[test]
fn depth_is_bounded() {
    assert!(matches!(
        OpeningIndexDepth::new(39),
        Err(OpeningIndexError::Depth { .. })
    ));
    assert!(matches!(
        OpeningIndexDepth::extended(OpeningIndexDepth::MAX_EXTENSION_PLIES + 1),
        Err(OpeningIndexError::Depth { .. })
    ));
    assert_eq!(
        OpeningIndexDepth::extended(OpeningIndexDepth::MAX_EXTENSION_PLIES)
            .map(OpeningIndexDepth::plies)
            .ok(),
        Some(120)
    );
}

#[test]
fn cancellation_stops_indexing() -> OpeningIndexTestResult {
    let fixture = OpeningIndexFixture::walk(20)?;
    let mut checks = 0;
    let outcome =
        OpeningIndexer::index(&fixture.input(1, None)?, OpeningIndexDepth::base(), || {
            checks += 1;
            checks > 5
        });
    assert!(matches!(outcome, Err(OpeningIndexError::Cancelled)));
    Ok(())
}

#[test]
fn batches_never_exceed_their_byte_limit() -> OpeningIndexTestResult {
    let fixture = OpeningIndexFixture::walk(40)?;
    let one = OpeningIndexer::index(&fixture.input(1, None)?, OpeningIndexDepth::base(), || {
        false
    })?
    .ok_or("must index")?;
    let size = one.transport_bytes();
    let limit = NonZeroUsize::new(size * 2 + size / 2).ok_or("limit")?;
    let mut batcher = OpeningIndexBatcher::new(limit)?;
    let mut batches = Vec::new();
    for id in 1..=5 {
        let index =
            OpeningIndexer::index(&fixture.input(id, None)?, OpeningIndexDepth::base(), || {
                false
            })?
            .ok_or("must index")?;
        batches.extend(batcher.push(index)?);
    }
    batches.extend(batcher.finish());
    assert_eq!(
        batches
            .iter()
            .map(|batch| batch.games().len())
            .collect::<Vec<_>>(),
        vec![2, 2, 1]
    );
    assert!(batches.iter().all(|batch| batch.bytes() <= limit.get()));
    let small = OpeningIndexBatcher::new(NonZeroUsize::new(size - 1).ok_or("limit")?);
    let mut small = small?;
    assert!(matches!(
        small.push(one),
        Err(OpeningIndexError::GameExceedsBatch { .. })
    ));
    Ok(())
}

#[test]
fn batch_limit_cannot_exceed_one_hundred_twenty_eight_mebibytes() -> OpeningIndexTestResult {
    assert_eq!(OpeningIndexBatcher::MAX_BATCH_BYTES, 128 * 1024 * 1024);
    assert!(OpeningIndexBatcher::new(NonZeroUsize::new(128 * 1024 * 1024).ok_or("limit")?).is_ok());
    assert!(matches!(
        OpeningIndexBatcher::new(NonZeroUsize::new(128 * 1024 * 1024 + 1).ok_or("limit")?),
        Err(OpeningIndexError::BatchBytes(_))
    ));
    Ok(())
}

#[tokio::test]
async fn a_run_executes_on_leased_capacity_and_honors_cancellation() -> OpeningIndexTestResult {
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let job = executor.submit(|cancellation| {
        let fixture = OpeningIndexFixture::walk(50).map_err(|error| error.to_string())?;
        let batcher = OpeningIndexBatcher::new(
            NonZeroUsize::new(OpeningIndexBatcher::MAX_BATCH_BYTES).ok_or("limit")?,
        )
        .map_err(|error| error.to_string())?;
        let ids = (1..=3)
            .map(OpeningCorpusGameId::new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let games = ids.into_iter().map(|id| OpeningIndexGameInput {
            source: &fixture.source,
            id,
            initial: &fixture.initial,
            mainline: &fixture.mainline,
            recorded: None,
        });
        let mut batches = 0;
        let indexed = OpeningIndexRun::execute(
            games,
            OpeningIndexDepth::base(),
            batcher,
            || cancellation.is_cancelled(),
            |_batch| {
                batches += 1;
                Ok(())
            },
        )
        .map_err(|error| error.to_string())?;
        Ok::<_, String>((indexed, batches))
    })?;
    assert_eq!(job.join().await??, (3, 1));
    executor.shutdown().await?;
    Ok(())
}
