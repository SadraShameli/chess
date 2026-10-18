use std::hash::{BuildHasherDefault, Hasher};
use std::num::NonZeroUsize;

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::game::enums::GameEnumResult;
use chess::game::session::{GameMetadataDate, GameMetadataText};
use chess::notation::game_identity::NotationGameIdentityEnumResult;
use chess::opening::corpus::{
    OpeningCorpus, OpeningCorpusError, OpeningCorpusLimits, OpeningCorpusMetadataSupport,
};
use chess::opening::query::{OpeningQuery, OpeningQueryFilters};
use chess::opening::statistics::OpeningStatistics;

use crate::opening_fixture::OpeningFixtureGame;

#[derive(Default)]
struct OpeningStatisticsCollisionHasher;

impl Hasher for OpeningStatisticsCollisionHasher {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, _bytes: &[u8]) {}
}

#[test]
fn independent_counts_separate_outcome_frequency_and_ending_denominators()
-> Result<(), Box<dyn std::error::Error>> {
    let white = Some(NotationGameIdentityEnumResult::Completed(
        GameEnumResult::Victory(BoardEnumColor::White),
    ));
    let black = Some(NotationGameIdentityEnumResult::Completed(
        GameEnumResult::Victory(BoardEnumColor::Black),
    ));
    let draw = Some(NotationGameIdentityEnumResult::Completed(
        GameEnumResult::Draw,
    ));
    let mut missing_ratings = OpeningFixtureGame::tags()?;
    missing_ratings.white_elo = None;
    let games = [
        OpeningFixtureGame::new(&["e2e4"], OpeningFixtureGame::tags()?, white)?,
        OpeningFixtureGame::new(&["e2e4", "e7e5"], missing_ratings, draw)?,
        OpeningFixtureGame::new(&["d2d4"], OpeningFixtureGame::tags()?, black)?,
        OpeningFixtureGame::new(
            &["e2e4", "c7c5"],
            OpeningFixtureGame::tags()?,
            Some(NotationGameIdentityEnumResult::Unfinished),
        )?,
        OpeningFixtureGame::new(&[], OpeningFixtureGame::tags()?, None)?,
    ];
    let source = OpeningFixtureGame::source("masters")?;
    let records = games
        .iter()
        .enumerate()
        .map(|(index, game)| game.record(u64::try_from(index)? + 1))
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        records,
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    let result =
        OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?;
    assert_eq!(result.game_count(), 5);
    assert_eq!(result.next_move_games(), 4);
    assert_eq!(result.ending_games(), 1);
    assert_eq!(result.outcomes().known(), 3);
    assert_eq!(result.outcomes().unknown(), 2);
    assert_eq!(result.outcomes().white(), 1);
    assert_eq!(result.outcomes().draws(), 1);
    assert_eq!(result.outcomes().black(), 1);
    assert_eq!(
        result
            .outcomes()
            .white_share()
            .map(|share| (share.numerator(), share.denominator().get())),
        Some((1, 3))
    );
    let e4 = &result.continuations()[0];
    assert_eq!(e4.movement().uci(position.variant()), "e2e4");
    assert_eq!(e4.game_count(), 3);
    assert_eq!(
        (
            e4.frequency().numerator(),
            e4.frequency().denominator().get()
        ),
        (3, 4)
    );
    assert_eq!(e4.outcomes().known(), 2);
    assert_eq!(e4.outcomes().unknown(), 1);
    assert_eq!(
        e4.white_rating()
            .map(|average| (average.sum(), average.samples().get())),
        Some((4400, 2))
    );
    assert_eq!(
        e4.black_rating()
            .map(|average| (average.sum(), average.samples().get())),
        Some((6000, 3))
    );
    assert!(position.play(e4.movement()).is_ok());
    Ok(())
}

#[test]
fn repeated_position_counts_only_the_first_recorded_continuation()
-> Result<(), Box<dyn std::error::Error>> {
    let game = OpeningFixtureGame::new(
        &["g1f3", "g8f6", "f3g1", "f6g8", "e2e4"],
        OpeningFixtureGame::tags()?,
        None,
    )?;
    let source = OpeningFixtureGame::source("masters")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![game.record(1)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    let result =
        OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?;
    assert_eq!(result.game_count(), 1);
    assert_eq!(result.continuations().len(), 1);
    assert_eq!(
        result.continuations()[0].movement().uci(position.variant()),
        "g1f3"
    );
    assert_eq!(result.ending_games(), 0);
    Ok(())
}

#[test]
fn combined_exact_duplicates_keep_all_source_game_ids_and_are_order_independent()
-> Result<(), Box<dyn std::error::Error>> {
    let game = OpeningFixtureGame::new(
        &["e2e4"],
        OpeningFixtureGame::tags()?,
        Some(NotationGameIdentityEnumResult::Completed(
            GameEnumResult::Draw,
        )),
    )?;
    let left_source = OpeningFixtureGame::source("a-masters")?;
    let right_source = OpeningFixtureGame::source("z-online")?;
    let left = OpeningCorpus::new(
        &left_source,
        OpeningCorpusMetadataSupport::ALL,
        vec![game.record(11)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let right = OpeningCorpus::new(
        &right_source,
        OpeningCorpusMetadataSupport::ALL,
        vec![game.record(29)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    for selected in [[&left, &right], [&right, &left]] {
        let result =
            OpeningStatistics::query(&selected, &query, OpeningFixtureGame::limits()?, || false)?;
        assert_eq!(result.game_count(), 1);
        assert_eq!(result.games()[0].provenance().len(), 2);
        assert_eq!(result.games()[0].provenance()[0].source(), &left_source);
        assert_eq!(result.games()[0].provenance()[0].game().value(), 11);
        assert_eq!(result.games()[0].provenance()[1].source(), &right_source);
        assert_eq!(result.games()[0].provenance()[1].game().value(), 29);
    }
    let single =
        OpeningStatistics::query(&[&left], &query, OpeningFixtureGame::limits()?, || false)?;
    assert_eq!(single.games()[0].provenance().len(), 1);
    Ok(())
}

#[test]
fn hash_collisions_never_merge_different_games_or_positions()
-> Result<(), Box<dyn std::error::Error>> {
    let games = [
        OpeningFixtureGame::new(&["e2e4"], OpeningFixtureGame::tags()?, None)?,
        OpeningFixtureGame::new(&["d2d4"], OpeningFixtureGame::tags()?, None)?,
    ];
    let source = OpeningFixtureGame::source("masters")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![games[0].record(1)?, games[1].record(2)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    let result = OpeningStatistics::query_with_hasher(
        &[&corpus],
        &query,
        OpeningFixtureGame::limits()?,
        || false,
        BuildHasherDefault::<OpeningStatisticsCollisionHasher>::default(),
    )?;
    assert_eq!(result.game_count(), 2);
    assert_eq!(
        result.continuations()[0].movement().uci(position.variant()),
        "d2d4"
    );
    assert_eq!(
        result.continuations()[1].movement().uci(position.variant()),
        "e2e4"
    );
    let unrelated = position.play_uci("c2c4")?;
    let absent = OpeningQuery::new(&unrelated, OpeningQueryFilters::default())?;
    assert_eq!(
        OpeningStatistics::query_with_hasher(
            &[&corpus],
            &absent,
            OpeningFixtureGame::limits()?,
            || false,
            BuildHasherDefault::<OpeningStatisticsCollisionHasher>::default()
        )?
        .game_count(),
        0
    );
    Ok(())
}

#[test]
fn missing_identity_metadata_warns_without_automatic_merge()
-> Result<(), Box<dyn std::error::Error>> {
    let complete = OpeningFixtureGame::new(
        &["e2e4"],
        OpeningFixtureGame::tags()?,
        Some(NotationGameIdentityEnumResult::Completed(
            GameEnumResult::Draw,
        )),
    )?;
    let mut tags = OpeningFixtureGame::tags()?;
    tags.white = None;
    let partial = OpeningFixtureGame::new(&["e2e4"], tags, complete.result)?;
    let source = OpeningFixtureGame::source("masters")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![complete.record(1)?, partial.record(2)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    let result =
        OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?;
    assert_eq!(result.game_count(), 2);
    assert_eq!(result.potential_duplicate_pairs(), 1);
    Ok(())
}

#[test]
fn sample_pages_are_newest_first_then_canonical_identity_and_bounded_to_fifty()
-> Result<(), Box<dyn std::error::Error>> {
    let mut games = Vec::new();
    for day in 1..=51_u16 {
        let mut tags = OpeningFixtureGame::tags()?;
        tags.date = Some(GameMetadataDate::new(Some(2000 + day), Some(1), Some(1))?);
        tags.white = Some(GameMetadataText::new(format!("Player {day:02}"))?);
        games.push(OpeningFixtureGame::new(&["e2e4"], tags, None)?);
    }
    let source = OpeningFixtureGame::source("masters")?;
    let records = games
        .iter()
        .enumerate()
        .map(|(index, game)| game.record(u64::try_from(index)? + 1))
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        records,
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    let result =
        OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?;
    let first = result.sample_page(0)?;
    assert_eq!(first.len(), 50);
    assert_eq!(
        first
            .games()
            .next()
            .map(|game| game.record().identity().white()),
        Some(Some("player 51"))
    );
    assert_eq!(first.next_offset(), Some(50));
    assert_eq!(result.sample_page(50)?.len(), 1);
    assert_eq!(result.sample_page(51)?.next_offset(), None);
    assert!(result.sample_page(52).is_err());
    Ok(())
}

#[test]
fn cancellation_and_work_memory_limits_refuse_partial_success()
-> Result<(), Box<dyn std::error::Error>> {
    let game = OpeningFixtureGame::new(
        &["g1f3", "g8f6", "f3g1", "f6g8", "e2e4"],
        OpeningFixtureGame::tags()?,
        None,
    )?;
    let source = OpeningFixtureGame::source("masters")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![game.record(1)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard().play_uci("e2e4")?;
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    let limits = OpeningFixtureGame::limits()?;
    let mut polls = 0;
    assert!(matches!(
        OpeningStatistics::query(&[&corpus], &query, limits, || {
            polls += 1;
            polls >= 8
        }),
        Err(OpeningCorpusError::Cancelled)
    ));
    assert_eq!(polls, 8);
    let one = NonZeroUsize::new(1).ok_or("one")?;
    let work = OpeningCorpusLimits::new(
        limits.max_records(),
        limits.max_plies(),
        limits.max_bytes(),
        one,
    );
    assert!(matches!(
        OpeningStatistics::query(&[&corpus], &query, work, || false),
        Err(OpeningCorpusError::WorkLimit)
    ));
    let memory = OpeningCorpusLimits::new(
        limits.max_records(),
        limits.max_plies(),
        one,
        limits.max_steps(),
    );
    assert!(matches!(
        OpeningStatistics::query(&[&corpus], &query, memory, || false),
        Err(OpeningCorpusError::MemoryLimit { .. })
    ));
    Ok(())
}

#[test]
fn unavailable_results_and_ratings_never_become_draws_or_zero_averages()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tags = OpeningFixtureGame::tags()?;
    tags.white_elo = None;
    tags.black_elo = None;
    let game = OpeningFixtureGame::new(&[], tags, None)?;
    let source = OpeningFixtureGame::source("personal")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![game.record(1)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    let result =
        OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?;
    assert_eq!(result.outcomes().unknown(), 1);
    assert_eq!(result.outcomes().known(), 0);
    assert_eq!(result.outcomes().white_share(), None);
    assert_eq!(result.outcomes().draw_share(), None);
    assert_eq!(result.outcomes().black_share(), None);
    assert_eq!(result.white_rating(), None);
    assert_eq!(result.black_rating(), None);
    assert!(result.continuations().is_empty());
    assert_eq!(result.ending_games(), 1);
    Ok(())
}

#[test]
fn zero_and_maximum_source_ratings_remain_available_exact_values()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tags = OpeningFixtureGame::tags()?;
    tags.white_elo = Some(0);
    tags.black_elo = Some(u32::MAX);
    let game = OpeningFixtureGame::new(&["e2e4"], tags, None)?;
    let source = OpeningFixtureGame::source("personal")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![game.record(1)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    let result =
        OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?;
    assert_eq!(
        result
            .white_rating()
            .map(|value| (value.sum(), value.samples().get())),
        Some((0, 1))
    );
    assert_eq!(
        result.black_rating().map(|value| value.sum()),
        Some(4_294_967_295)
    );
    Ok(())
}

#[test]
fn same_date_samples_use_canonical_identity_order_and_ignore_source_input_order()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tags = OpeningFixtureGame::tags()?;
    tags.white = Some(GameMetadataText::new("Zed".to_owned())?);
    let zed = OpeningFixtureGame::new(&["e2e4"], tags, None)?;
    let alice = OpeningFixtureGame::new(&["e2e4"], OpeningFixtureGame::tags()?, None)?;
    let source = OpeningFixtureGame::source("masters")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![zed.record(1)?, alice.record(2)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    let result =
        OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?;
    let names = result
        .sample_page(0)?
        .games()
        .map(|game| game.record().identity().white())
        .collect::<Vec<_>>();
    assert_eq!(names, [Some("alice"), Some("zed")]);
    Ok(())
}

#[test]
fn rejects_repeated_dataset_selection_and_conflicting_version_binding()
-> Result<(), Box<dyn std::error::Error>> {
    let source = OpeningFixtureGame::source("masters")?;
    let conflicting = chess::storage::dataset_package::StorageDatasetPackageSourceIdentity::new(
        "masters".to_owned(),
        "v1".to_owned(),
        chess::package::digest::PackageDigest::hash_slice(b"changed"),
    )?;
    let left = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        Vec::new(),
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let right = OpeningCorpus::new(
        &conflicting,
        OpeningCorpusMetadataSupport::ALL,
        Vec::new(),
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    for selected in [[&left, &left], [&left, &right]] {
        assert!(matches!(
            OpeningStatistics::query(&selected, &query, OpeningFixtureGame::limits()?, || false),
            Err(OpeningCorpusError::SourceBinding)
        ));
    }
    Ok(())
}

#[test]
fn counter_differences_share_position_identity_but_variants_do_not()
-> Result<(), Box<dyn std::error::Error>> {
    let game = OpeningFixtureGame::new(&["e2e4"], OpeningFixtureGame::tags()?, None)?;
    let source = OpeningFixtureGame::source("masters")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![game.record(1)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 7 42";
    for (variant, expected) in [
        (chess::board::enums::BoardEnumVariant::Standard, 1),
        (chess::board::enums::BoardEnumVariant::Chess960, 0),
    ] {
        let position = BoardPosition::from_fen(fen, variant)?;
        let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
        assert_eq!(
            OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?
                .game_count(),
            expected
        );
    }
    Ok(())
}

#[test]
fn maximum_selected_fullmove_counter_does_not_reject_a_legal_corpus_continuation()
-> Result<(), Box<dyn std::error::Error>> {
    let game = OpeningFixtureGame::new(&["e2e4", "e7e5"], OpeningFixtureGame::tags()?, None)?;
    let source = OpeningFixtureGame::source("masters")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![game.record(1)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::from_fen(
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1000000",
        chess::board::enums::BoardEnumVariant::Standard,
    )?;
    assert_eq!(
        position.identity(),
        BoardPosition::standard().play_uci("e2e4")?.identity()
    );
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    let result =
        OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?;
    assert_eq!(result.game_count(), 1);
    assert_eq!(result.next_move_games(), 1);
    assert_eq!(result.continuations().len(), 1);
    assert_eq!(
        result.continuations()[0].movement(),
        position.parse_uci("e7e5")?
    );
    Ok(())
}

#[test]
fn eligible_duplicate_supplies_statistics_while_all_selected_provenance_survives()
-> Result<(), Box<dyn std::error::Error>> {
    let outcome = Some(NotationGameIdentityEnumResult::Completed(
        GameEnumResult::Draw,
    ));
    let mut tags = OpeningFixtureGame::tags()?;
    tags.black_elo = None;
    let missing = OpeningFixtureGame::new(&["e2e4"], tags, outcome)?;
    let complete = OpeningFixtureGame::new(&["e2e4"], OpeningFixtureGame::tags()?, outcome)?;
    let left_source = OpeningFixtureGame::source("a-missing")?;
    let right_source = OpeningFixtureGame::source("b-complete")?;
    let left = OpeningCorpus::new(
        &left_source,
        OpeningCorpusMetadataSupport::ALL,
        vec![missing.record(1)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let right = OpeningCorpus::new(
        &right_source,
        OpeningCorpusMetadataSupport::ALL,
        vec![complete.record(2)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(
        &position,
        OpeningQueryFilters {
            rating: Some(chess::opening::query::OpeningQueryRating::new(
                1900,
                2300,
                chess::opening::query::OpeningQueryEnumRatingCriterion::default(),
            )?),
            ..OpeningQueryFilters::default()
        },
    )?;
    let result = OpeningStatistics::query(
        &[&left, &right],
        &query,
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    assert_eq!(result.game_count(), 1);
    assert_eq!(result.games()[0].provenance().len(), 2);
    assert_eq!(
        result.black_rating().map(|average| average.sum()),
        Some(2000)
    );
    Ok(())
}

#[test]
fn combined_duplicate_uses_available_per_side_ratings_without_counting_game_twice()
-> Result<(), Box<dyn std::error::Error>> {
    let outcome = Some(NotationGameIdentityEnumResult::Completed(
        GameEnumResult::Draw,
    ));
    let mut white_tags = OpeningFixtureGame::tags()?;
    white_tags.black_elo = None;
    let mut black_tags = OpeningFixtureGame::tags()?;
    black_tags.white_elo = None;
    let white = OpeningFixtureGame::new(&["e2e4"], white_tags, outcome)?;
    let black = OpeningFixtureGame::new(&["e2e4"], black_tags, outcome)?;
    let left_source = OpeningFixtureGame::source("a-white")?;
    let right_source = OpeningFixtureGame::source("b-black")?;
    let left = OpeningCorpus::new(
        &left_source,
        OpeningCorpusMetadataSupport::ALL,
        vec![white.record(1)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let right = OpeningCorpus::new(
        &right_source,
        OpeningCorpusMetadataSupport::ALL,
        vec![black.record(2)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    for selected in [[&left, &right], [&right, &left]] {
        let result =
            OpeningStatistics::query(&selected, &query, OpeningFixtureGame::limits()?, || false)?;
        assert_eq!(result.game_count(), 1);
        assert_eq!(
            result
                .white_rating()
                .map(|average| (average.sum(), average.samples().get())),
            Some((2200, 1))
        );
        assert_eq!(
            result
                .black_rating()
                .map(|average| (average.sum(), average.samples().get())),
            Some((2000, 1))
        );
        assert_eq!(
            result.games()[0].provenance()[1]
                .record()
                .metadata()
                .known()
                .white_elo,
            None
        );
    }
    Ok(())
}
