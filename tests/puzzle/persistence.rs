use std::num::NonZeroU64;

use chess::board::position::BoardPosition;
use chess::clock::duration::ClockInstant;
use chess::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumPrimaryOutcome,
};
use chess::puzzle::persistence::{
    PuzzlePersistenceCodec, PuzzlePersistenceEnumCommand, PuzzlePersistenceRun,
};
use chess::puzzle::random_provider::PuzzleRandomProviderSeeded;
use chess::puzzle::run::PuzzleRunEnumStatus;
use chess::puzzle::selection::PuzzleSelectionFilter;
use chess::puzzle::session::{PuzzleSessionConfig, PuzzleSessionId};
use chess::puzzle::timer::PuzzleTimerEnumReadiness;

use crate::puzzle_fixture::PuzzleFixture;

#[test]
fn canonical_roundtrip_preserves_failed_primary_and_requires_resume()
-> Result<(), Box<dyn std::error::Error>> {
    let initial = BoardPosition::standard();
    let definition = PuzzleFixture::definition(
        initial.clone(),
        &[&["e2e4", "e7e5", "g1f3"], &["e2e4", "c7c5", "g1f3"]],
    )?;
    let mut owner = PuzzlePersistenceRun::new(
        PuzzleSessionId::new(NonZeroU64::MIN),
        PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
        vec![definition],
        PuzzleSelectionFilter::default(),
        PuzzleRandomProviderSeeded::new(81),
    )?;
    owner.execute(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        ClockInstant::from_nanos(100),
    )?;
    owner.execute(
        PuzzlePersistenceEnumCommand::Move(initial.parse_uci("d2d4")?),
        ClockInstant::from_nanos(200),
    )?;
    owner.execute(
        PuzzlePersistenceEnumCommand::RetryPractice,
        ClockInstant::from_nanos(300),
    )?;
    owner.execute(
        PuzzlePersistenceEnumCommand::Hint,
        ClockInstant::from_nanos(400),
    )?;
    owner.execute(
        PuzzlePersistenceEnumCommand::Move(initial.parse_uci("e2e4")?),
        ClockInstant::from_nanos(500),
    )?;
    owner.execute(
        PuzzlePersistenceEnumCommand::OpponentReply,
        ClockInstant::from_nanos(600),
    )?;
    let position = match owner.run().view(ClockInstant::from_nanos(600))?.content() {
        chess::puzzle::run::PuzzleRunEnumContent::Board(attempt) => attempt.position().clone(),
        chess::puzzle::run::PuzzleRunEnumContent::Hidden
        | chess::puzzle::run::PuzzleRunEnumContent::Review(_) => {
            return Err("expected active board".into());
        }
    };
    owner.execute(
        PuzzlePersistenceEnumCommand::Move(position.parse_uci("g1f3")?),
        ClockInstant::from_nanos(700),
    )?;
    let saved = owner.capture(ClockInstant::from_nanos(700), 0)?;
    let bytes = PuzzlePersistenceCodec::encode(&saved)?;
    let recovered = PuzzlePersistenceCodec::decode(&bytes)?;
    assert!(matches!(
        recovered.run().view(ClockInstant::from_nanos(0))?.status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    let summary = recovered.run().summary(ClockInstant::from_nanos(0))?;
    assert_eq!(summary.primary_count(PuzzleEnumPrimaryOutcome::Failed), 1);
    assert_eq!(
        summary.primary_count(PuzzleEnumPrimaryOutcome::Solved(
            PuzzleEnumAssistance::Assisted
        )),
        0
    );
    assert_eq!(summary.practice_completions(), 1);
    assert_eq!(summary.hints(), 1);
    Ok(())
}

#[test]
fn corrupt_schema_and_payload_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    assert!(PuzzlePersistenceCodec::decode(b"not-json").is_err());
    assert!(PuzzlePersistenceCodec::decode(br#"{"schema":999}"#).is_err());
    assert!(
        PuzzlePersistenceCodec::decode(&vec![b' '; PuzzlePersistenceCodec::MAX_DOCUMENT_BYTES + 1])
            .is_err()
    );
    Ok(())
}

#[test]
fn listed_underpromotions_survive_recovery_with_the_full_source_tree()
-> Result<(), Box<dyn std::error::Error>> {
    let initial = BoardPosition::from_fen(
        "7k/P7/8/8/8/8/8/7K w - - 0 1",
        chess::board::enums::BoardEnumVariant::Standard,
    )?;
    let definition = PuzzleFixture::definition(
        initial.clone(),
        &[&["a7a8n"], &["a7a8b"], &["a7a8r"], &["a7a8q"]],
    )?;
    let mut owner = PuzzlePersistenceRun::new(
        PuzzleSessionId::new(NonZeroU64::MIN),
        PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::PerPuzzle),
        vec![definition.clone()],
        PuzzleSelectionFilter::default(),
        PuzzleRandomProviderSeeded::new(7),
    )?;
    owner.execute(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        ClockInstant::from_nanos(0),
    )?;
    owner.execute(
        PuzzlePersistenceEnumCommand::Move(initial.parse_uci("a7a8n")?),
        ClockInstant::from_nanos(10),
    )?;
    let encoded = PuzzlePersistenceCodec::encode(&owner.capture(ClockInstant::from_nanos(10), 0)?)?;
    let mut restored = PuzzlePersistenceCodec::decode(&encoded)?;
    restored.execute(
        PuzzlePersistenceEnumCommand::Resume(PuzzleTimerEnumReadiness::Ready),
        ClockInstant::from_nanos(0),
    )?;
    let chess::puzzle::run::PuzzleRunEnumContent::Review(attempt) =
        restored.run().view(ClockInstant::from_nanos(0))?.content()
    else {
        return Err("expected restored review".into());
    };
    assert_eq!(attempt.definition(), &definition);
    assert_eq!(attempt.position(), &initial.play_uci("a7a8n")?);
    assert_eq!(
        attempt.primary_outcome(),
        Some(PuzzleEnumPrimaryOutcome::Solved(
            PuzzleEnumAssistance::Unassisted
        ))
    );
    Ok(())
}

#[test]
fn repeated_relaunch_uses_active_durations_and_rejects_corrupted_balances_and_counters()
-> Result<(), Box<dyn std::error::Error>> {
    let initial = BoardPosition::standard();
    let definition = PuzzleFixture::definition(initial, &[&["e2e4"]])?;
    let mut owner = PuzzlePersistenceRun::new(
        PuzzleSessionId::new(NonZeroU64::MIN),
        PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
        vec![definition],
        PuzzleSelectionFilter::default(),
        PuzzleRandomProviderSeeded::new(42),
    )?;
    owner.execute(
        PuzzlePersistenceEnumCommand::Readiness(PuzzleTimerEnumReadiness::Ready),
        ClockInstant::from_nanos(500),
    )?;
    let bytes =
        PuzzlePersistenceCodec::encode(&owner.capture(ClockInstant::from_nanos(1_500), 0)?)?;
    let document = String::from_utf8(bytes.clone())?;
    for (original, corrupt) in [
        ("\"algorithm_version\":1", "\"algorithm_version\":999"),
        ("\"event_count\":1", "\"event_count\":999"),
        ("\"remaining_nanos\":299999999000", "\"remaining_nanos\":1"),
        ("\"phase\":\"solver_move\"", "\"phase\":\"unknown\""),
    ] {
        let malformed = document.replace(original, corrupt);
        assert_ne!(document, malformed);
        assert!(PuzzlePersistenceCodec::decode(malformed.as_bytes()).is_err());
    }
    let mut restored = PuzzlePersistenceCodec::decode(&bytes)?;
    restored.execute(
        PuzzlePersistenceEnumCommand::Resume(PuzzleTimerEnumReadiness::Ready),
        ClockInstant::from_nanos(0),
    )?;
    let again =
        PuzzlePersistenceCodec::encode(&restored.capture(ClockInstant::from_nanos(250), 0)?)?;
    let restored = PuzzlePersistenceCodec::decode(&again)?;
    assert_eq!(
        restored
            .run()
            .view(ClockInstant::from_nanos(0))?
            .timer()
            .elapsed()
            .as_nanos(),
        1_250
    );
    assert!(matches!(
        restored.run().view(ClockInstant::from_nanos(0))?.status(),
        PuzzleRunEnumStatus::Paused { .. }
    ));
    Ok(())
}
