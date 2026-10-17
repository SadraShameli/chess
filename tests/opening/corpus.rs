use std::num::NonZeroUsize;

use chess::board::position::BoardPosition;
use chess::opening::corpus::{
    OpeningCorpus, OpeningCorpusEnumValidation, OpeningCorpusError, OpeningCorpusGameId,
    OpeningCorpusLimits, OpeningCorpusMetadataSupport, OpeningCorpusRecord,
};

use crate::opening_fixture::OpeningFixtureGame;

#[test]
fn retains_source_version_original_metadata_and_validated_identity()
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
    assert_eq!(corpus.source(), &source);
    assert_eq!(corpus.source().version(), "v1");
    assert_eq!(
        corpus.validation(),
        OpeningCorpusEnumValidation::LegalMainlines
    );
    assert_eq!(corpus.records()[0].metadata(), &game.metadata);
    assert_eq!(corpus.records()[0].identity().white(), Some("alice"));
    assert_eq!(corpus.records()[0].mainline().len(), 2);
    assert_eq!(
        corpus.records()[0].time_control(),
        game.time_control.as_ref()
    );
    Ok(())
}

#[test]
fn refuses_duplicate_source_game_ids_and_zero_identifiers() -> Result<(), Box<dyn std::error::Error>>
{
    let game = OpeningFixtureGame::new(&[], OpeningFixtureGame::tags()?, None)?;
    let source = OpeningFixtureGame::source("masters")?;
    assert!(matches!(
        OpeningCorpusGameId::new(0),
        Err(OpeningCorpusError::GameId)
    ));
    assert!(matches!(
        OpeningCorpus::new(
            &source,
            OpeningCorpusMetadataSupport::ALL,
            vec![game.record(1)?, game.record(1)?],
            OpeningFixtureGame::limits()?,
            || false,
        ),
        Err(OpeningCorpusError::DuplicateGameId(_))
    ));
    Ok(())
}

#[test]
fn validates_moves_against_the_actual_preceding_board() -> Result<(), Box<dyn std::error::Error>> {
    let mut game = OpeningFixtureGame::new(&[], OpeningFixtureGame::tags()?, None)?;
    game.mainline
        .push(BoardPosition::standard().parse_uci("e2e4")?);
    game.mainline
        .push(BoardPosition::standard().parse_uci("d2d4")?);
    assert!(game.record(1).is_err());
    Ok(())
}

#[test]
fn observes_cancellation_before_identity_validation_and_corpus_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let game = OpeningFixtureGame::new(&[], OpeningFixtureGame::tags()?, None)?;
    assert!(matches!(
        OpeningCorpusRecord::new(game.input(1)?, OpeningFixtureGame::limits()?, || true),
        Err(OpeningCorpusError::Cancelled)
    ));
    let source = OpeningFixtureGame::source("masters")?;
    assert!(matches!(
        OpeningCorpus::new(
            &source,
            OpeningCorpusMetadataSupport::ALL,
            Vec::new(),
            OpeningFixtureGame::limits()?,
            || true,
        ),
        Err(OpeningCorpusError::Cancelled)
    ));
    Ok(())
}

#[test]
fn enforces_injected_record_ply_and_memory_limits() -> Result<(), Box<dyn std::error::Error>> {
    let game = OpeningFixtureGame::new(&["e2e4", "e7e5"], OpeningFixtureGame::tags()?, None)?;
    let one = NonZeroUsize::new(1).ok_or("one")?;
    let limits = OpeningFixtureGame::limits()?;
    let ply_limit = OpeningCorpusLimits::new(one, one, limits.max_bytes(), limits.max_steps());
    assert!(matches!(
        OpeningCorpusRecord::new(game.input(1)?, ply_limit, || false),
        Err(OpeningCorpusError::PlyLimit { .. })
    ));
    let memory_limit = OpeningCorpusLimits::new(one, limits.max_plies(), one, limits.max_steps());
    assert!(matches!(
        OpeningCorpusRecord::new(game.input(1)?, memory_limit, || false),
        Err(OpeningCorpusError::MemoryLimit { .. })
    ));
    let source = OpeningFixtureGame::source("masters")?;
    let record_limit = OpeningCorpusLimits::new(
        one,
        limits.max_plies(),
        limits.max_bytes(),
        limits.max_steps(),
    );
    assert!(matches!(
        OpeningCorpus::new(
            &source,
            OpeningCorpusMetadataSupport::ALL,
            vec![game.record(1)?, game.record(2)?],
            record_limit,
            || false,
        ),
        Err(OpeningCorpusError::RecordLimit { .. })
    ));
    Ok(())
}

#[test]
fn empty_validated_corpus_retains_declared_metadata_availability()
-> Result<(), Box<dyn std::error::Error>> {
    let source = OpeningFixtureGame::source("personal")?;
    let supported = OpeningCorpusMetadataSupport::NONE
        .with(chess::opening::corpus::OpeningCorpusEnumMetadata::Players);
    let corpus = OpeningCorpus::new(
        &source,
        supported,
        Vec::new(),
        OpeningFixtureGame::limits()?,
        || false,
    )?;
    assert_eq!(corpus.validation(), OpeningCorpusEnumValidation::Empty);
    assert_eq!(corpus.supported_metadata(), supported);
    assert!(corpus.records().is_empty());
    Ok(())
}
