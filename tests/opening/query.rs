use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::game::enums::GameEnumResult;
use chess::game::session::{GameMetadataDate, GameMetadataText};
use chess::notation::game_identity::NotationGameIdentityEnumResult;
use chess::opening::corpus::{OpeningCorpus, OpeningCorpusMetadataSupport};
use chess::opening::query::{
    OpeningQuery, OpeningQueryDateInterval, OpeningQueryEnumPlayer,
    OpeningQueryEnumRatingCriterion, OpeningQueryFilters, OpeningQueryPlayer, OpeningQueryRating,
};
use chess::opening::statistics::OpeningStatistics;

use crate::opening_fixture::OpeningFixtureGame;

#[test]
fn rating_default_requires_both_players_and_side_to_move_uses_query_turn()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tags = OpeningFixtureGame::tags()?;
    tags.black_elo = None;
    let game = OpeningFixtureGame::new(&["e2e4", "e7e5"], tags, None)?;
    let source = OpeningFixtureGame::source("masters")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![game.record(1)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let start = BoardPosition::standard();
    let after_white = start.play_uci("e2e4")?;
    assert_eq!(
        OpeningQueryEnumRatingCriterion::default(),
        OpeningQueryEnumRatingCriterion::BothPlayers
    );
    for (position, criterion, expected) in [
        (&start, OpeningQueryEnumRatingCriterion::BothPlayers, 0),
        (&start, OpeningQueryEnumRatingCriterion::White, 1),
        (&start, OpeningQueryEnumRatingCriterion::Black, 0),
        (&start, OpeningQueryEnumRatingCriterion::SideToMove, 1),
        (&after_white, OpeningQueryEnumRatingCriterion::SideToMove, 0),
    ] {
        let query = OpeningQuery::new(
            position,
            OpeningQueryFilters {
                rating: Some(OpeningQueryRating::new(2000, 2300, criterion)?),
                ..OpeningQueryFilters::default()
            },
        )?;
        assert_eq!(
            OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?
                .game_count(),
            expected
        );
    }
    Ok(())
}

#[test]
fn partial_dates_overlap_and_are_disclosed_without_accepting_impossible_dates()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tags = OpeningFixtureGame::tags()?;
    tags.date = Some(GameMetadataDate::new(Some(2024), None, Some(31))?);
    let game = OpeningFixtureGame::new(&["e2e4"], tags, None)?;
    let source = OpeningFixtureGame::source("masters")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![game.record(1)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    for (month, last, expected) in [(2, 29, 0), (3, 31, 1)] {
        let query = OpeningQuery::new(
            &position,
            OpeningQueryFilters {
                date: Some(OpeningQueryDateInterval::new(
                    GameMetadataDate::new(Some(2024), Some(month), Some(1))?,
                    GameMetadataDate::new(Some(2024), Some(month), Some(last))?,
                )?),
                ..OpeningQueryFilters::default()
            },
        )?;
        let result =
            OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?;
        assert_eq!(result.game_count(), expected);
        assert_eq!(result.partial_date_matches(), expected);
    }
    assert!(
        OpeningQueryDateInterval::new(
            GameMetadataDate::new(Some(2024), None, None)?,
            GameMetadataDate::new(Some(2024), Some(12), Some(31))?
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn player_filters_reuse_unicode_identity_normalization() -> Result<(), Box<dyn std::error::Error>> {
    let mut tags = OpeningFixtureGame::tags()?;
    tags.white = Some(GameMetadataText::new("  Straße  ".to_owned())?);
    let game = OpeningFixtureGame::new(&[], tags, None)?;
    let source = OpeningFixtureGame::source("masters")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![game.record(1)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    let query = OpeningQuery::new(
        &position,
        OpeningQueryFilters {
            player: Some(OpeningQueryPlayer::new(
                OpeningQueryEnumPlayer::White,
                GameMetadataText::new("STRASSE".to_owned())?,
                OpeningFixtureGame::limits()?,
            )?),
            ..OpeningQueryFilters::default()
        },
    )?;
    assert_eq!(
        OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?
            .game_count(),
        1
    );
    assert!(
        OpeningQueryPlayer::new(
            OpeningQueryEnumPlayer::Either,
            GameMetadataText::new(" ? ".to_owned())?,
            OpeningFixtureGame::limits()?
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn active_metadata_filters_exclude_missing_and_unsupported_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tags = OpeningFixtureGame::tags()?;
    tags.date = None;
    let mut game = OpeningFixtureGame::new(&[], tags, None)?;
    game.time_control = None;
    let source = OpeningFixtureGame::source("personal")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::NONE,
        vec![game.record(1)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    for filters in [
        OpeningQueryFilters {
            time_control: Some(GameMetadataText::new("300+3".to_owned())?),
            ..OpeningQueryFilters::default()
        },
        OpeningQueryFilters {
            result: Some(NotationGameIdentityEnumResult::Completed(
                GameEnumResult::Victory(BoardEnumColor::White),
            )),
            ..OpeningQueryFilters::default()
        },
        OpeningQueryFilters {
            player: Some(OpeningQueryPlayer::new(
                OpeningQueryEnumPlayer::Either,
                GameMetadataText::new("Alice".to_owned())?,
                OpeningFixtureGame::limits()?,
            )?),
            ..OpeningQueryFilters::default()
        },
    ] {
        let query = OpeningQuery::new(&position, filters)?;
        assert_eq!(
            OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?
                .game_count(),
            0
        );
    }
    Ok(())
}

#[test]
fn matches_exact_raw_time_control_and_explicit_result() -> Result<(), Box<dyn std::error::Error>> {
    let outcome = NotationGameIdentityEnumResult::Completed(GameEnumResult::Draw);
    let game = OpeningFixtureGame::new(&["e2e4"], OpeningFixtureGame::tags()?, Some(outcome))?;
    let source = OpeningFixtureGame::source("masters")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![game.record(1)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    for (control, expected) in [("300+3", 1), ("180+0", 0)] {
        let query = OpeningQuery::new(
            &position,
            OpeningQueryFilters {
                time_control: Some(GameMetadataText::new(control.to_owned())?),
                result: Some(outcome),
                ..OpeningQueryFilters::default()
            },
        )?;
        assert_eq!(
            OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?
                .game_count(),
            expected
        );
    }
    assert!(
        OpeningQueryRating::new(2300, 2200, OpeningQueryEnumRatingCriterion::default()).is_err()
    );
    Ok(())
}

#[test]
fn date_filters_exclude_completely_unknown_dates_but_match_unknown_year_leap_days()
-> Result<(), Box<dyn std::error::Error>> {
    let mut unknown = OpeningFixtureGame::tags()?;
    unknown.date = Some(GameMetadataDate::new(None, None, None)?);
    let mut leap = OpeningFixtureGame::tags()?;
    leap.date = Some(GameMetadataDate::new(None, Some(2), Some(29))?);
    let games = [
        OpeningFixtureGame::new(&[], unknown, None)?,
        OpeningFixtureGame::new(&[], leap, None)?,
    ];
    let source = OpeningFixtureGame::source("personal")?;
    let corpus = OpeningCorpus::new(
        &source,
        OpeningCorpusMetadataSupport::ALL,
        vec![games[0].record(1)?, games[1].record(2)?],
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    let position = BoardPosition::standard();
    for (year, expected) in [(2023, 0), (2024, 1)] {
        let query = OpeningQuery::new(
            &position,
            OpeningQueryFilters {
                date: Some(OpeningQueryDateInterval::new(
                    GameMetadataDate::new(Some(year), Some(1), Some(1))?,
                    GameMetadataDate::new(Some(year), Some(12), Some(31))?,
                )?),
                ..OpeningQueryFilters::default()
            },
        )?;
        assert_eq!(
            OpeningStatistics::query(&[&corpus], &query, OpeningFixtureGame::limits()?, || false)?
                .game_count(),
            expected
        );
    }
    Ok(())
}
