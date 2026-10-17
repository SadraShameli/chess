use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::game::enums::GameEnumResult;
use chess::game::session::{GameMetadataDate, GameMetadataText};
use chess::notation::game_identity::NotationGameIdentityEnumResult;
use chess::opening::corpus::{OpeningCorpus, OpeningCorpusMetadataSupport};
use chess::opening::explorer::{OpeningExplorerDefinition, OpeningExplorerNames};
use chess::opening::query::{
    OpeningQuery, OpeningQueryDateInterval, OpeningQueryEnumRatingCriterion, OpeningQueryFilters,
    OpeningQueryRating,
};
use chess::opening::statistics::OpeningStatistics;

use crate::opening_fixture::OpeningFixtureGame;

struct OpeningExplorerOutcome;

impl OpeningExplorerOutcome {
    fn win(color: BoardEnumColor) -> Option<NotationGameIdentityEnumResult> {
        Some(NotationGameIdentityEnumResult::Completed(
            GameEnumResult::Victory(color),
        ))
    }

    fn draw() -> Option<NotationGameIdentityEnumResult> {
        Some(NotationGameIdentityEnumResult::Completed(
            GameEnumResult::Draw,
        ))
    }
}

#[test]
fn transpositions_reach_one_position_once_per_game_and_split_by_next_move()
-> Result<(), Box<dyn std::error::Error>> {
    let games = [
        OpeningFixtureGame::new(
            &["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"],
            OpeningFixtureGame::tags()?,
            OpeningExplorerOutcome::win(BoardEnumColor::White),
        )?,
        OpeningFixtureGame::new(
            &["g1f3", "b8c6", "e2e4", "e7e5", "f1c4"],
            OpeningFixtureGame::tags()?,
            OpeningExplorerOutcome::draw(),
        )?,
        OpeningFixtureGame::new(
            &["g1f3", "b8c6", "e2e4", "e7e5"],
            OpeningFixtureGame::tags()?,
            OpeningExplorerOutcome::win(BoardEnumColor::Black),
        )?,
        OpeningFixtureGame::new(
            &["d2d4", "d7d5"],
            OpeningFixtureGame::tags()?,
            OpeningExplorerOutcome::draw(),
        )?,
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
    let mut position = BoardPosition::standard();
    for movement in ["e2e4", "e7e5", "g1f3", "b8c6"] {
        position = position.play_uci(movement)?;
    }
    let query = OpeningQuery::new(&position, OpeningQueryFilters::default())?;
    let result =
        OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?;
    assert_eq!(result.game_count(), 3);
    assert_eq!(result.next_move_games(), 2);
    assert_eq!(result.ending_games(), 1);
    let moves: Vec<(String, u64)> = result
        .continuations()
        .iter()
        .map(|continuation| {
            (
                continuation.movement().uci(position.variant()),
                continuation.game_count(),
            )
        })
        .collect();
    assert_eq!(moves, [("f1b5".to_owned(), 1), ("f1c4".to_owned(), 1)]);
    for continuation in result.continuations() {
        assert_eq!(
            (
                continuation.frequency().numerator(),
                continuation.frequency().denominator().get()
            ),
            (1, 2)
        );
    }
    assert_eq!(
        (
            result.outcomes().white(),
            result.outcomes().draws(),
            result.outcomes().black()
        ),
        (1, 1, 1)
    );
    Ok(())
}

#[test]
fn combined_filters_exclude_missing_metadata_and_disclose_partial_dates_in_denominators()
-> Result<(), Box<dyn std::error::Error>> {
    let full = OpeningFixtureGame::new(
        &["e2e4"],
        OpeningFixtureGame::tags()?,
        OpeningExplorerOutcome::win(BoardEnumColor::White),
    )?;
    let mut partial_tags = OpeningFixtureGame::tags()?;
    partial_tags.white = Some(GameMetadataText::new("Carol".to_owned())?);
    partial_tags.date = Some(GameMetadataDate::new(Some(2026), Some(10), None)?);
    let partial = OpeningFixtureGame::new(
        &["d2d4"],
        partial_tags,
        OpeningExplorerOutcome::win(BoardEnumColor::Black),
    )?;
    let mut unrated_tags = OpeningFixtureGame::tags()?;
    unrated_tags.white = Some(GameMetadataText::new("Dave".to_owned())?);
    unrated_tags.white_elo = None;
    let unrated = OpeningFixtureGame::new(&["e2e4"], unrated_tags, OpeningExplorerOutcome::draw())?;
    let mut undated_tags = OpeningFixtureGame::tags()?;
    undated_tags.white = Some(GameMetadataText::new("Erin".to_owned())?);
    undated_tags.date = None;
    let undated = OpeningFixtureGame::new(&["e2e4"], undated_tags, OpeningExplorerOutcome::draw())?;
    let left_source = OpeningFixtureGame::source("a-masters")?;
    let right_source = OpeningFixtureGame::source("b-online")?;
    let left = OpeningCorpus::new(
        &left_source,
        OpeningCorpusMetadataSupport::ALL,
        vec![full.record(1)?, unrated.record(2)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let right = OpeningCorpus::new(
        &right_source,
        OpeningCorpusMetadataSupport::ALL,
        vec![partial.record(1)?, undated.record(2)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let unfiltered = OpeningStatistics::query(
        &[&left, &right],
        &OpeningQuery::new(&position, OpeningQueryFilters::default())?,
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    assert_eq!(unfiltered.game_count(), 4);
    let query = OpeningQuery::new(
        &position,
        OpeningQueryFilters {
            date: Some(OpeningQueryDateInterval::new(
                GameMetadataDate::new(Some(2026), Some(10), Some(10))?,
                GameMetadataDate::new(Some(2026), Some(10), Some(20))?,
            )?),
            rating: Some(OpeningQueryRating::new(
                1500,
                2500,
                OpeningQueryEnumRatingCriterion::default(),
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
    assert_eq!(result.game_count(), 2);
    assert_eq!(result.partial_date_matches(), 1);
    assert_eq!(result.sources().len(), 2);
    assert_eq!(result.next_move_games(), 2);
    assert_eq!(result.ending_games(), 0);
    assert_eq!(
        (
            result.outcomes().white(),
            result.outcomes().draws(),
            result.outcomes().black(),
            result.outcomes().unknown()
        ),
        (1, 0, 1, 0)
    );
    let moves: Vec<(String, u64)> = result
        .continuations()
        .iter()
        .map(|continuation| {
            (
                continuation.movement().uci(position.variant()),
                continuation.game_count(),
            )
        })
        .collect();
    assert_eq!(moves, [("d2d4".to_owned(), 1), ("e2e4".to_owned(), 1)]);
    Ok(())
}

#[test]
fn deduplication_across_sources_counts_once_under_a_filter_and_orders_samples_stably()
-> Result<(), Box<dyn std::error::Error>> {
    let shared = OpeningFixtureGame::new(
        &["e2e4"],
        OpeningFixtureGame::tags()?,
        OpeningExplorerOutcome::draw(),
    )?;
    let mut older_tags = OpeningFixtureGame::tags()?;
    older_tags.white = Some(GameMetadataText::new("Zed".to_owned())?);
    older_tags.date = Some(GameMetadataDate::new(Some(2020), Some(1), Some(1))?);
    let older = OpeningFixtureGame::new(&["e2e4"], older_tags, OpeningExplorerOutcome::draw())?;
    let left_source = OpeningFixtureGame::source("a-masters")?;
    let right_source = OpeningFixtureGame::source("b-online")?;
    let left = OpeningCorpus::new(
        &left_source,
        OpeningCorpusMetadataSupport::ALL,
        vec![older.record(7)?, shared.record(9)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let right = OpeningCorpus::new(
        &right_source,
        OpeningCorpusMetadataSupport::ALL,
        vec![shared.record(3)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(
        &position,
        OpeningQueryFilters {
            rating: Some(OpeningQueryRating::new(
                1900,
                2300,
                OpeningQueryEnumRatingCriterion::BothPlayers,
            )?),
            ..OpeningQueryFilters::default()
        },
    )?;
    for selected in [[&left, &right], [&right, &left]] {
        let result =
            OpeningStatistics::query(&selected, &query, OpeningFixtureGame::limits()?, || false)?;
        assert_eq!(result.game_count(), 2);
        let page = result.sample_page(0)?;
        let provenance: Vec<usize> = page.games().map(|game| game.provenance().len()).collect();
        assert_eq!(provenance, [2, 1]);
        assert_eq!(
            page.games()
                .next()
                .and_then(|game| game.record().metadata().known().date),
            Some(GameMetadataDate::new(Some(2026), Some(10), Some(17))?)
        );
    }
    Ok(())
}

#[test]
fn deepest_known_position_names_the_opening_with_code_then_name_tie_break()
-> Result<(), Box<dyn std::error::Error>> {
    let definitions = [
        OpeningExplorerDefinition::new("C20", "King's Pawn Game", &["e2e4", "e7e5"])?,
        OpeningExplorerDefinition::new("C44", "King's Knight Opening", &["e2e4", "e7e5", "g1f3"])?,
        OpeningExplorerDefinition::new("C45", "Zeta", &["e2e4", "e7e5", "g1f3"])?,
        OpeningExplorerDefinition::new("C44", "Alpha", &["e2e4", "e7e5", "g1f3"])?,
        OpeningExplorerDefinition::new("C44", "Transposed", &["g1f3", "e7e5", "e2e4"])?,
    ];
    let names = OpeningExplorerNames::new(&definitions)?;
    let mut line = vec![BoardPosition::standard()];
    assert!(names.deepest_along(&line).is_none());
    let mut labels = Vec::new();
    for movement in ["e2e4", "e7e5", "g1f3", "b8c6"] {
        let next = line[line.len() - 1].play_uci(movement)?;
        line.push(next);
        labels.push(
            names
                .deepest_along(&line)
                .map(|label| (label.code().to_owned(), label.name().to_owned())),
        );
    }
    assert_eq!(labels[0], None);
    assert_eq!(
        labels[1],
        Some(("C20".to_owned(), "King's Pawn Game".to_owned()))
    );
    assert_eq!(labels[2], Some(("C44".to_owned(), "Alpha".to_owned())));
    assert_eq!(labels[3], Some(("C44".to_owned(), "Alpha".to_owned())));
    let transposed = ["g1f3", "e7e5", "e2e4"].iter().try_fold(
        vec![BoardPosition::standard()],
        |mut line, movement| {
            let next = line[line.len() - 1].play_uci(movement)?;
            line.push(next);
            Ok::<_, Box<dyn std::error::Error>>(line)
        },
    )?;
    assert_eq!(
        names.deepest_along(&transposed).map(|label| label.name()),
        Some("Alpha")
    );
    assert!(OpeningExplorerDefinition::new("", "x", &["e2e4"]).is_err());
    assert!(
        OpeningExplorerDefinition::new("A00", "x", &["e2e5"])
            .and_then(|definition| OpeningExplorerNames::new(&[definition]))
            .is_err()
    );
    Ok(())
}
