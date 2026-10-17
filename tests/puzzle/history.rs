use std::num::NonZeroU64;
use std::time::Duration;

use chess::board::position::BoardPosition;
use chess::puzzle::attempt::PuzzleAttemptId;
use chess::puzzle::definition::PuzzleDefinition;
use chess::puzzle::enums::{PuzzleEnumAssistance, PuzzleEnumCountdownMode};
use chess::puzzle::history::{
    PuzzleHistory, PuzzleHistoryEntry, PuzzleHistoryEnumConfirmation, PuzzleHistoryEnumScope,
    PuzzleHistoryError,
};
use chess::puzzle::identity::PuzzleDefinitionIdentity;
use chess::puzzle::metadata::PuzzleDefinitionMetadata;
use chess::puzzle::random_provider::PuzzleRandomProviderSeeded;
use chess::puzzle::selection::{
    PuzzleSelection, PuzzleSelectionDataset, PuzzleSelectionFilter, PuzzleSelectionSnapshot,
};
use chess::puzzle::session::{PuzzleSession, PuzzleSessionConfig, PuzzleSessionId};
use chess::puzzle::summary::{PuzzleSummary, PuzzleSummaryAttempt};

struct PuzzleHistoryFixture;

impl PuzzleHistoryFixture {
    const SEED: u64 = 179;

    fn identity(id: &str) -> Result<PuzzleDefinitionIdentity, Box<dyn std::error::Error>> {
        Ok(PuzzleDefinitionIdentity::new(
            "history".into(),
            "v1".into(),
            id.into(),
        )?)
    }

    fn definition(id: &str) -> Result<PuzzleDefinition, Box<dyn std::error::Error>> {
        let initial = BoardPosition::standard();
        let movement = initial.parse_uci("e2e4")?;
        Ok(PuzzleDefinition::new(
            Self::identity(id)?,
            initial,
            vec![vec![movement]],
            PuzzleDefinitionMetadata::default(),
        )?)
    }

    fn definitions() -> Result<Vec<PuzzleDefinition>, Box<dyn std::error::Error>> {
        ["a", "b", "c", "d", "e"]
            .into_iter()
            .map(Self::definition)
            .collect()
    }

    fn selection() -> Result<PuzzleSelection<PuzzleRandomProviderSeeded>, Box<dyn std::error::Error>>
    {
        Ok(PuzzleSelection::new(
            PuzzleSelectionDataset::from_identity(&Self::identity("pin")?),
            Self::definitions()?,
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(Self::SEED),
        )?)
    }

    fn entry(session: u64, attempt: u64) -> Result<PuzzleHistoryEntry, Box<dyn std::error::Error>> {
        let solved = Self::solved(attempt)?;
        let id = PuzzleSessionId::new(NonZeroU64::new(session).ok_or("Zero session")?);
        Ok(PuzzleHistoryEntry::new(
            id,
            solved.attempt().clone(),
            Duration::from_secs(5),
        ))
    }

    fn history() -> Result<PuzzleHistory, Box<dyn std::error::Error>> {
        Ok(PuzzleHistory::new(vec![
            Self::entry(1, 1)?,
            Self::entry(1, 2)?,
            Self::entry(2, 1)?,
        ])?)
    }

    fn session_id(value: u64) -> Result<PuzzleSessionId, Box<dyn std::error::Error>> {
        Ok(PuzzleSessionId::new(
            NonZeroU64::new(value).ok_or("Zero session")?,
        ))
    }

    fn solved(attempt: u64) -> Result<PuzzleSession, Box<dyn std::error::Error>> {
        let mut session = PuzzleSession::new(
            PuzzleSessionId::new(NonZeroU64::new(1).ok_or("Zero session")?),
            PuzzleAttemptId::new(NonZeroU64::new(attempt).ok_or("Zero attempt")?),
            Self::definition("a")?,
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
        );
        let movement = session.attempt().position().parse_uci("e2e4")?;
        session.submit_move(&session.context(), movement)?;
        Ok(session)
    }

    fn summary(sessions: &[&PuzzleSession]) -> Result<PuzzleSummary, Box<dyn std::error::Error>> {
        Ok(PuzzleSummary::from_attempts(sessions.iter().map(
            |session| {
                PuzzleSummaryAttempt::new(session.id(), session.attempt(), Duration::from_secs(5))
            },
        ))?)
    }
}

#[test]
fn persisted_shuffle_resumes_without_repeating_visited_identities_across_relaunch()
-> Result<(), Box<dyn std::error::Error>> {
    let mut live = PuzzleHistoryFixture::selection()?;
    let mut seen = Vec::new();
    for _ in 0..2 {
        seen.push(live.current().ok_or("Missing puzzle")?.identity().clone());
        live.advance();
    }
    let saved = PuzzleSelectionSnapshot::from_persisted(live.snapshot()?.persisted().clone())?;
    let mut relaunched = PuzzleSelection::restore(
        PuzzleHistoryFixture::definitions()?,
        saved,
        PuzzleRandomProviderSeeded::new(PuzzleHistoryFixture::SEED),
    )?;
    while let Some(definition) = relaunched.current() {
        let identity = definition.identity().clone();
        assert!(!seen.contains(&identity));
        seen.push(identity);
        relaunched.advance();
    }
    assert_eq!(seen.len(), 5);
    Ok(())
}

#[test]
fn recomputed_summaries_follow_removed_attempts_and_end_with_an_unavailable_rate()
-> Result<(), Box<dyn std::error::Error>> {
    let first = PuzzleHistoryFixture::solved(1)?;
    let second = PuzzleHistoryFixture::solved(2)?;
    let both = PuzzleHistoryFixture::summary(&[&first, &second])?;
    let one = PuzzleHistoryFixture::summary(&[&second])?;
    let none = PuzzleHistoryFixture::summary(&[])?;
    assert_eq!(both.finalized(), 2);
    assert_eq!(one.finalized(), 1);
    assert_eq!(one.elapsed_active(), Duration::from_secs(5));
    assert_eq!(none.finalized(), 0);
    assert_eq!(none.elapsed_active(), Duration::ZERO);
    let rate = one
        .solve_rate(PuzzleEnumAssistance::Unassisted)
        .ok_or("Unavailable rate")?;
    assert_eq!((rate.numerator(), rate.denominator().get()), (1, 1));
    assert_eq!(none.solve_rate(PuzzleEnumAssistance::Unassisted), None);
    Ok(())
}

#[test]
fn confirmed_deletion_removes_the_scope_and_recomputes_the_summary()
-> Result<(), Box<dyn std::error::Error>> {
    let mut history = PuzzleHistoryFixture::history()?;
    assert_eq!(history.summary()?.finalized(), 3);
    let one = history.delete(
        PuzzleHistoryEnumScope::Attempt {
            session: PuzzleHistoryFixture::session_id(1)?,
            attempt: PuzzleAttemptId::new(NonZeroU64::new(2).ok_or("Zero attempt")?),
        },
        PuzzleHistoryEnumConfirmation::Confirmed,
        None,
    )?;
    assert_eq!((one.removed(), one.summary().finalized()), (1, 2));
    let session = history.delete(
        PuzzleHistoryEnumScope::Session(PuzzleHistoryFixture::session_id(1)?),
        PuzzleHistoryEnumConfirmation::Confirmed,
        None,
    )?;
    assert_eq!((session.removed(), session.summary().finalized()), (1, 1));
    let all = history.delete(
        PuzzleHistoryEnumScope::All,
        PuzzleHistoryEnumConfirmation::Confirmed,
        None,
    )?;
    assert_eq!(all.removed(), 1);
    assert_eq!(
        all.summary().solve_rate(PuzzleEnumAssistance::Unassisted),
        None
    );
    assert!(history.entries().is_empty());
    assert!(matches!(
        history.delete(
            PuzzleHistoryEnumScope::All,
            PuzzleHistoryEnumConfirmation::Confirmed,
            None
        ),
        Err(PuzzleHistoryError::NothingToDelete)
    ));
    Ok(())
}

#[test]
fn unconfirmed_deletion_changes_nothing() -> Result<(), Box<dyn std::error::Error>> {
    let mut history = PuzzleHistoryFixture::history()?;
    let before = history.clone();
    assert!(matches!(
        history.delete(
            PuzzleHistoryEnumScope::All,
            PuzzleHistoryEnumConfirmation::Unconfirmed,
            None
        ),
        Err(PuzzleHistoryError::ConfirmationRequired)
    ));
    assert_eq!(history, before);
    Ok(())
}

#[test]
fn active_run_deletion_is_refused_without_changing_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut history = PuzzleHistoryFixture::history()?;
    let before = history.clone();
    let active = Some(PuzzleHistoryFixture::session_id(1)?);
    for scope in [
        PuzzleHistoryEnumScope::Session(PuzzleHistoryFixture::session_id(1)?),
        PuzzleHistoryEnumScope::Attempt {
            session: PuzzleHistoryFixture::session_id(1)?,
            attempt: PuzzleAttemptId::new(NonZeroU64::MIN),
        },
        PuzzleHistoryEnumScope::All,
    ] {
        assert!(matches!(
            history.delete(scope, PuzzleHistoryEnumConfirmation::Confirmed, active),
            Err(PuzzleHistoryError::ActiveRunRefused { .. })
        ));
        assert_eq!(history, before);
    }
    let other = history.delete(
        PuzzleHistoryEnumScope::Session(PuzzleHistoryFixture::session_id(2)?),
        PuzzleHistoryEnumConfirmation::Confirmed,
        active,
    )?;
    assert_eq!(other.removed(), 1);
    Ok(())
}
