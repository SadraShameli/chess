use std::collections::HashSet;
use std::num::{NonZeroU64, NonZeroUsize};
use std::str::FromStr;

use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::puzzle::attempt::PuzzleAttemptId;
use chess::puzzle::definition::PuzzleDefinition;
use chess::puzzle::enums::{PuzzleEnumCountdownMode, PuzzleEnumEvent};
use chess::puzzle::identity::PuzzleDefinitionIdentity;
use chess::puzzle::metadata::{
    PuzzleDefinitionDifficulty, PuzzleDefinitionMetadata, PuzzleDefinitionTheme,
};
use chess::puzzle::random_provider::{PuzzleRandomProvider, PuzzleRandomProviderSeeded};
use chess::puzzle::selection::{
    PuzzleSelection, PuzzleSelectionDataset, PuzzleSelectionDifficulty, PuzzleSelectionEnumStatus,
    PuzzleSelectionError, PuzzleSelectionFilter, PuzzleSelectionRandomDraw,
    PuzzleSelectionSnapshot, PuzzleSelectionSnapshotOptions,
};
use chess::puzzle::session::{PuzzleSession, PuzzleSessionConfig, PuzzleSessionId};

struct PuzzleSelectionFixture;

impl PuzzleSelectionFixture {
    const SEED: u64 = 185;

    fn identity(id: &str) -> Result<PuzzleDefinitionIdentity, Box<dyn std::error::Error>> {
        Ok(PuzzleDefinitionIdentity::new(
            "local tactics".into(),
            "2026-10-05".into(),
            id.into(),
        )?)
    }

    fn definition(
        id: &str,
        difficulty: Option<u32>,
        themes: Option<&[&str]>,
    ) -> Result<PuzzleDefinition, Box<dyn std::error::Error>> {
        let initial = BoardPosition::from_fen(
            "6k1/5ppp/8/8/8/8/5PPP/4R1K1 w - - 0 1",
            BoardEnumVariant::Standard,
        )?;
        let themes = themes
            .map(|themes| {
                themes
                    .iter()
                    .map(|theme| PuzzleDefinitionTheme::from_str(theme))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let movement = initial.parse_uci("e1e8")?;
        Ok(PuzzleDefinition::new(
            Self::identity(id)?,
            initial,
            vec![vec![movement]],
            PuzzleDefinitionMetadata::new(difficulty.map(PuzzleDefinitionDifficulty::new), themes)?,
        )?)
    }

    fn definitions() -> Result<Vec<PuzzleDefinition>, Box<dyn std::error::Error>> {
        Ok(vec![
            Self::definition(" 001A ", Some(249), Some(&["mateIn1", "backRankMate"]))?,
            Self::definition("001a", Some(250), Some(&["mateIn1"]))?,
            Self::definition("tactic-3", Some(1500), Some(&["backRankMate"]))?,
            Self::definition("tactic-4", Some(2000), Some(&["mateIn1", "backRankMate"]))?,
            Self::definition("unrated", None, Some(&["mateIn1"]))?,
            Self::definition("unthemed", Some(u32::MAX), None)?,
        ])
    }

    fn selection(
        filters: PuzzleSelectionFilter,
    ) -> Result<PuzzleSelection<PuzzleRandomProviderSeeded>, Box<dyn std::error::Error>> {
        Ok(PuzzleSelection::new(
            PuzzleSelectionDataset::from_identity(&Self::identity("dataset pin")?),
            Self::definitions()?,
            filters,
            PuzzleRandomProviderSeeded::new(Self::SEED),
        )?)
    }

    fn difficulty(
        minimum: u32,
        maximum: u32,
    ) -> Result<PuzzleSelectionDifficulty, PuzzleSelectionError> {
        PuzzleSelectionDifficulty::new(
            PuzzleDefinitionDifficulty::new(minimum),
            PuzzleDefinitionDifficulty::new(maximum),
        )
    }

    fn drain<R: PuzzleRandomProvider>(
        selection: &mut PuzzleSelection<R>,
    ) -> Vec<PuzzleDefinitionIdentity> {
        let mut identities = Vec::new();
        while let Some(definition) = selection.current() {
            identities.push(definition.identity().clone());
            selection.advance();
        }
        identities
    }

    fn options() -> Result<PuzzleSelectionSnapshotOptions, Box<dyn std::error::Error>> {
        Ok(Self::selection(PuzzleSelectionFilter::default())?
            .snapshot()?
            .persisted()
            .clone())
    }

    fn branching() -> Result<PuzzleDefinition, Box<dyn std::error::Error>> {
        let initial = BoardPosition::standard();
        let mut lines = Vec::new();
        for replies in [["e2e4", "e7e5", "g1f3"], ["e2e4", "c7c5", "g1f3"]] {
            let mut position = initial.clone();
            let mut line = Vec::new();
            for uci in replies {
                let movement = position.parse_uci(uci)?;
                position = position.play(movement)?;
                line.push(movement);
            }
            lines.push(line);
        }
        Ok(PuzzleDefinition::new(
            Self::identity("listed-branches")?,
            initial,
            lines,
            PuzzleDefinitionMetadata::default(),
        )?)
    }
}

#[derive(Clone)]
struct PuzzleSelectionInvalidRandom;

impl PuzzleRandomProvider for PuzzleSelectionInvalidRandom {
    fn index(&mut self, alternatives: NonZeroUsize) -> usize {
        alternatives.get()
    }
}

#[derive(Clone)]
struct PuzzleSelectionFailingRandom {
    provider: PuzzleRandomProviderSeeded,
    remaining: usize,
}

impl PuzzleRandomProvider for PuzzleSelectionFailingRandom {
    fn index(&mut self, alternatives: NonZeroUsize) -> usize {
        if self.remaining == 0 {
            return alternatives.get();
        }
        self.remaining -= 1;
        self.provider.index(alternatives)
    }
}

#[test]
fn visits_every_original_identity_once_and_waits_at_exhaustion()
-> Result<(), Box<dyn std::error::Error>> {
    let expected: HashSet<_> = PuzzleSelectionFixture::definitions()?
        .into_iter()
        .map(|definition| definition.identity().clone())
        .collect();
    let mut selection = PuzzleSelectionFixture::selection(PuzzleSelectionFilter::default())?;
    assert_eq!(selection.status(), PuzzleSelectionEnumStatus::Ready);
    let visited = PuzzleSelectionFixture::drain(&mut selection);
    assert_eq!(visited.len(), expected.len());
    assert_eq!(visited.iter().cloned().collect::<HashSet<_>>(), expected);
    assert_eq!(selection.status(), PuzzleSelectionEnumStatus::Exhausted);
    assert_eq!(selection.current(), None);
    let snapshot = selection.snapshot()?;
    assert_eq!(snapshot.persisted().visited, visited);
    assert_eq!(snapshot.persisted().cursor, expected.len());
    assert_eq!(selection.advance(), PuzzleSelectionEnumStatus::Exhausted);
    assert_eq!(selection.snapshot()?, snapshot);
    assert_eq!(
        selection.reshuffle_and_continue()?,
        PuzzleSelectionEnumStatus::Ready
    );
    assert!(selection.snapshot()?.persisted().visited.is_empty());
    let repeated = PuzzleSelectionFixture::drain(&mut selection);
    assert_eq!(repeated.len(), expected.len());
    assert_eq!(repeated.into_iter().collect::<HashSet<_>>(), expected);
    Ok(())
}

#[test]
fn refuses_early_reshuffle_without_changing_progress_or_randomness()
-> Result<(), Box<dyn std::error::Error>> {
    let mut selection = PuzzleSelectionFixture::selection(PuzzleSelectionFilter::default())?;
    let before = selection.snapshot()?;
    assert!(matches!(
        selection.reshuffle_and_continue(),
        Err(PuzzleSelectionError::CollectionNotExhausted)
    ));
    assert_eq!(selection.snapshot()?, before);
    Ok(())
}

#[test]
fn failed_reshuffle_preserves_exhaustion_and_the_previous_random_stream()
-> Result<(), Box<dyn std::error::Error>> {
    let definitions = PuzzleSelectionFixture::definitions()?;
    let mut selection = PuzzleSelection::new(
        PuzzleSelectionDataset::from_identity(&PuzzleSelectionFixture::identity("pin")?),
        definitions.clone(),
        PuzzleSelectionFilter::default(),
        PuzzleSelectionFailingRandom {
            provider: PuzzleRandomProviderSeeded::new(PuzzleSelectionFixture::SEED),
            remaining: definitions.len(),
        },
    )?;
    PuzzleSelectionFixture::drain(&mut selection);
    let saved = selection.snapshot()?;
    for _ in 0..2 {
        assert!(matches!(
            selection.reshuffle_and_continue(),
            Err(PuzzleSelectionError::InvalidRandomChoice { .. })
        ));
        assert_eq!(selection.status(), PuzzleSelectionEnumStatus::Exhausted);
        assert_eq!(selection.snapshot()?, saved);
    }
    Ok(())
}

#[test]
fn random_transaction_rolls_back_rejected_valid_draws_without_copying_prior_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut selection = PuzzleSelectionFixture::selection(PuzzleSelectionFilter::default())?;
    let before = selection.snapshot()?;
    let alternatives = NonZeroUsize::new(23).ok_or("Invalid fixture alternatives")?;
    let rejected: Result<(), &str> = selection.random_mut().transaction(|random| {
        random.index(alternatives);
        Err("Rejected reply transition")
    });
    assert_eq!(rejected, Err("Rejected reply transition"));
    assert_eq!(selection.snapshot()?, before);
    let mut restored = PuzzleSelection::restore(
        PuzzleSelectionFixture::definitions()?,
        before,
        PuzzleRandomProviderSeeded::new(PuzzleSelectionFixture::SEED),
    )?;
    assert_eq!(
        selection.random_mut().index(alternatives),
        restored.random_mut().index(alternatives)
    );
    assert_eq!(selection.snapshot()?, restored.snapshot()?);
    Ok(())
}

#[test]
fn fixed_seed_reproduces_order_and_different_seeds_change_it()
-> Result<(), Box<dyn std::error::Error>> {
    let first = PuzzleSelectionFixture::selection(PuzzleSelectionFilter::default())?.snapshot()?;
    let second = PuzzleSelectionFixture::selection(PuzzleSelectionFilter::default())?.snapshot()?;
    assert_eq!(first, second);
    let mut orders = HashSet::new();
    for seed in 0..16 {
        let selection = PuzzleSelection::new(
            first.persisted().dataset.clone(),
            PuzzleSelectionFixture::definitions()?,
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(seed),
        )?;
        orders.insert(selection.snapshot()?.persisted().order.clone());
    }
    assert!(orders.len() > 1);
    Ok(())
}

#[test]
fn freezes_inclusive_difficulty_and_any_exact_theme_filters()
-> Result<(), Box<dyn std::error::Error>> {
    let filters = PuzzleSelectionFilter::new(
        Some(PuzzleSelectionFixture::difficulty(250, 2000)?),
        vec!["mateIn1".parse()?, "backRankMate".parse()?],
    )?;
    let mut selection = PuzzleSelectionFixture::selection(filters.clone())?;
    let next_filters = PuzzleSelectionFilter::new(
        Some(PuzzleSelectionFixture::difficulty(249, 249)?),
        vec!["mateIn1".parse()?],
    )?;
    assert_ne!(selection.filters(), &next_filters);
    assert_eq!(selection.filters(), &filters);
    let ids: HashSet<_> = PuzzleSelectionFixture::drain(&mut selection)
        .into_iter()
        .map(|identity| identity.puzzle_id().to_owned())
        .collect();
    assert_eq!(
        ids,
        HashSet::from(["001a".into(), "tactic-3".into(), "tactic-4".into()])
    );
    selection.reshuffle_and_continue()?;
    assert_eq!(selection.filters(), &filters);
    assert_eq!(selection.len(), 3);
    assert_eq!(PuzzleSelectionFixture::selection(next_filters)?.len(), 1);
    Ok(())
}

#[test]
fn missing_metadata_is_available_unfiltered_and_excluded_only_when_required()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        PuzzleSelectionFixture::selection(PuzzleSelectionFilter::default())?.len(),
        6
    );
    let rated = PuzzleSelectionFixture::selection(PuzzleSelectionFilter::new(
        Some(PuzzleSelectionFixture::difficulty(0, u32::MAX)?),
        Vec::new(),
    )?)?;
    assert_eq!(rated.len(), 5);
    let themed = PuzzleSelectionFixture::selection(PuzzleSelectionFilter::new(
        None,
        vec!["mateIn1".parse()?],
    )?)?;
    assert_eq!(themed.len(), 4);
    let original = PuzzleSelectionFixture::definitions()?;
    let untitled = original.last().ok_or("Missing unavailable-theme fixture")?;
    assert_eq!(untitled.metadata().themes(), None);
    assert_eq!(
        untitled.metadata().difficulty(),
        Some(PuzzleDefinitionDifficulty::new(u32::MAX))
    );
    Ok(())
}

#[test]
fn validates_filter_ranges_and_duplicate_theme_tokens() -> Result<(), Box<dyn std::error::Error>> {
    assert!(matches!(
        PuzzleSelectionFixture::difficulty(2000, 250),
        Err(PuzzleSelectionError::InvalidDifficultyRange { .. })
    ));
    assert!(matches!(
        PuzzleSelectionFilter::new(None, vec!["mateIn1".parse()?, "mateIn1".parse()?]),
        Err(PuzzleSelectionError::DuplicateTheme { .. })
    ));
    let selection = PuzzleSelectionFixture::selection(PuzzleSelectionFilter::new(
        None,
        vec!["MateIn1".parse()?],
    )?)?;
    assert_eq!(selection.status(), PuzzleSelectionEnumStatus::NoMatches);
    Ok(())
}

#[test]
fn empty_match_and_single_match_collections_need_no_shuffle_draws()
-> Result<(), Box<dyn std::error::Error>> {
    let mut empty = PuzzleSelectionFixture::selection(PuzzleSelectionFilter::new(
        None,
        vec!["unavailable".parse()?],
    )?)?;
    assert!(empty.is_empty());
    assert_eq!(empty.advance(), PuzzleSelectionEnumStatus::NoMatches);
    assert!(matches!(
        empty.reshuffle_and_continue(),
        Err(PuzzleSelectionError::NoMatchingPuzzles)
    ));
    assert!(empty.snapshot()?.persisted().randomness.is_empty());
    let mut single = PuzzleSelectionFixture::selection(PuzzleSelectionFilter::new(
        Some(PuzzleSelectionFixture::difficulty(249, 249)?),
        Vec::new(),
    )?)?;
    assert_eq!(
        single
            .current()
            .ok_or("Missing sole puzzle")?
            .identity()
            .puzzle_id(),
        " 001A "
    );
    assert!(single.snapshot()?.persisted().randomness.is_empty());
    assert_eq!(single.advance(), PuzzleSelectionEnumStatus::Exhausted);
    single.reshuffle_and_continue()?;
    assert!(single.snapshot()?.persisted().randomness.is_empty());
    Ok(())
}

#[test]
fn rejects_duplicate_and_mixed_dataset_identities_before_filtering()
-> Result<(), Box<dyn std::error::Error>> {
    let dataset = PuzzleSelectionDataset::from_identity(&PuzzleSelectionFixture::identity("pin")?);
    let definition = PuzzleSelectionFixture::definition("duplicate", None, None)?;
    assert!(matches!(
        PuzzleSelection::new(
            dataset.clone(),
            vec![definition.clone(), definition],
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(0)
        ),
        Err(PuzzleSelectionError::DuplicateIdentity { .. })
    ));
    let initial = BoardPosition::standard();
    let movement = initial.parse_uci("e2e4")?;
    let changed = PuzzleDefinition::new(
        PuzzleDefinitionIdentity::new(dataset.source().into(), "replacement".into(), "new".into())?,
        initial,
        vec![vec![movement]],
        PuzzleDefinitionMetadata::default(),
    )?;
    assert!(matches!(
        PuzzleSelection::new(
            dataset,
            vec![changed],
            PuzzleSelectionFilter::new(None, vec!["mateIn1".parse()?])?,
            PuzzleRandomProviderSeeded::new(0)
        ),
        Err(PuzzleSelectionError::DatasetMismatch { .. })
    ));
    Ok(())
}

#[test]
fn restored_progress_and_future_reshuffles_match_after_source_order_changes()
-> Result<(), Box<dyn std::error::Error>> {
    let mut live = PuzzleSelectionFixture::selection(PuzzleSelectionFilter::default())?;
    live.advance();
    live.advance();
    let saved = live.snapshot()?;
    let persisted = PuzzleSelectionSnapshot::from_persisted(saved.persisted().clone())?;
    let mut definitions = PuzzleSelectionFixture::definitions()?;
    definitions.reverse();
    let mut recovered = PuzzleSelection::restore(
        definitions,
        persisted,
        PuzzleRandomProviderSeeded::new(PuzzleSelectionFixture::SEED),
    )?;
    assert_eq!(recovered.snapshot()?, saved);
    assert_eq!(recovered.current(), live.current());
    assert_eq!(
        PuzzleSelectionFixture::drain(&mut recovered),
        PuzzleSelectionFixture::drain(&mut live)
    );
    for _ in 0..3 {
        recovered.reshuffle_and_continue()?;
        live.reshuffle_and_continue()?;
        recovered = PuzzleSelection::restore(
            PuzzleSelectionFixture::definitions()?,
            recovered.snapshot()?,
            PuzzleRandomProviderSeeded::new(PuzzleSelectionFixture::SEED),
        )?;
        assert_eq!(recovered.snapshot()?, live.snapshot()?);
        assert_eq!(
            PuzzleSelectionFixture::drain(&mut recovered),
            PuzzleSelectionFixture::drain(&mut live)
        );
    }
    Ok(())
}

#[test]
fn restoring_requires_exact_frozen_membership_and_dataset_version()
-> Result<(), Box<dyn std::error::Error>> {
    let saved = PuzzleSelectionFixture::selection(PuzzleSelectionFilter::default())?.snapshot()?;
    let mut removed = PuzzleSelectionFixture::definitions()?;
    removed.pop().ok_or("Missing removed fixture")?;
    assert!(matches!(
        PuzzleSelection::restore(
            removed,
            saved.clone(),
            PuzzleRandomProviderSeeded::new(PuzzleSelectionFixture::SEED)
        ),
        Err(PuzzleSelectionError::CollectionMismatch)
    ));
    let mut added = PuzzleSelectionFixture::definitions()?;
    added.push(PuzzleSelectionFixture::definition("added", None, None)?);
    assert!(matches!(
        PuzzleSelection::restore(
            added,
            saved.clone(),
            PuzzleRandomProviderSeeded::new(PuzzleSelectionFixture::SEED)
        ),
        Err(PuzzleSelectionError::CollectionMismatch)
    ));
    let initial = BoardPosition::standard();
    let movement = initial.parse_uci("e2e4")?;
    let replacement = PuzzleDefinition::new(
        PuzzleDefinitionIdentity::new("local tactics".into(), "new version".into(), "001a".into())?,
        initial,
        vec![vec![movement]],
        PuzzleDefinitionMetadata::default(),
    )?;
    assert!(matches!(
        PuzzleSelection::restore(
            vec![replacement],
            saved,
            PuzzleRandomProviderSeeded::new(PuzzleSelectionFixture::SEED),
        ),
        Err(PuzzleSelectionError::DatasetMismatch { .. })
    ));
    Ok(())
}

#[test]
fn persisted_boundaries_reject_invalid_order_progress_pins_and_draw_offsets()
-> Result<(), Box<dyn std::error::Error>> {
    let valid = PuzzleSelectionFixture::options()?;
    let mut invalid = valid.clone();
    invalid.order[0] = invalid.order[1].clone();
    assert!(matches!(
        PuzzleSelectionSnapshot::from_persisted(invalid),
        Err(PuzzleSelectionError::InvalidOrder)
    ));
    let mut invalid = valid.clone();
    invalid.cursor = invalid.order.len() + 1;
    assert!(matches!(
        PuzzleSelectionSnapshot::from_persisted(invalid),
        Err(PuzzleSelectionError::InvalidCursor)
    ));
    let mut invalid = valid.clone();
    invalid.visited.push(invalid.order[0].clone());
    assert!(matches!(
        PuzzleSelectionSnapshot::from_persisted(invalid),
        Err(PuzzleSelectionError::InvalidVisited)
    ));
    let mut invalid = valid.clone();
    invalid.dataset = PuzzleSelectionDataset::from_identity(&PuzzleDefinitionIdentity::new(
        "different".into(),
        "v2".into(),
        "pin".into(),
    )?);
    assert!(matches!(
        PuzzleSelectionSnapshot::from_persisted(invalid),
        Err(PuzzleSelectionError::DatasetMismatch { .. })
    ));
    let mut invalid = valid.clone();
    invalid.shuffle_start = usize::MAX;
    assert!(matches!(
        PuzzleSelectionSnapshot::from_persisted(invalid),
        Err(PuzzleSelectionError::InvalidShuffleRandomness)
    ));
    let mut invalid = valid;
    invalid.order.swap(0, 1);
    assert!(matches!(
        PuzzleSelectionSnapshot::from_persisted(invalid),
        Err(PuzzleSelectionError::InvalidShuffleRandomness)
    ));
    Ok(())
}

#[test]
fn rejects_invalid_random_choices_and_incompatible_replay_provider()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(matches!(
        PuzzleSelectionRandomDraw::new(NonZeroUsize::MIN, 1),
        Err(PuzzleSelectionError::InvalidRandomChoice { .. })
    ));
    let options = PuzzleSelectionFixture::options()?;
    assert!(matches!(
        PuzzleSelection::new(
            options.dataset.clone(),
            PuzzleSelectionFixture::definitions()?,
            PuzzleSelectionFilter::default(),
            PuzzleSelectionInvalidRandom
        ),
        Err(PuzzleSelectionError::InvalidRandomChoice { .. })
    ));
    assert!(matches!(
        PuzzleSelection::restore(
            PuzzleSelectionFixture::definitions()?,
            PuzzleSelectionSnapshot::from_persisted(options)?,
            PuzzleSelectionInvalidRandom
        ),
        Err(PuzzleSelectionError::RandomReplayMismatch { .. })
    ));
    Ok(())
}

#[test]
fn listed_replies_use_the_recorded_injected_stream_and_keep_existing_attempt_events()
-> Result<(), Box<dyn std::error::Error>> {
    let mut selection = PuzzleSelectionFixture::selection(PuzzleSelectionFilter::default())?;
    let definition = PuzzleSelectionFixture::branching()?;
    let mut session = PuzzleSession::new(
        PuzzleSessionId::new(NonZeroU64::MIN),
        PuzzleAttemptId::new(NonZeroU64::MIN),
        definition,
        PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
    );
    let movement = session.attempt().position().parse_uci("e2e4")?;
    session.submit_move(&session.context(), movement)?;
    let before = selection.snapshot()?.persisted().randomness.len();
    let reply = session.play_opponent_reply(&session.context(), selection.random_mut())?;
    let snapshot = selection.snapshot()?;
    let draw = snapshot
        .persisted()
        .randomness
        .last()
        .ok_or("Missing reply draw")?;
    assert_eq!(snapshot.persisted().randomness.len(), before + 1);
    assert_eq!(draw.alternatives().get(), 2);
    assert!(
        matches!(session.attempt().events().last(), Some(PuzzleEnumEvent::OpponentReply { movement, choice, .. }) if *movement == reply && *choice == draw.choice())
    );
    let mut recovered = PuzzleSelection::restore(
        PuzzleSelectionFixture::definitions()?,
        snapshot,
        PuzzleRandomProviderSeeded::new(PuzzleSelectionFixture::SEED),
    )?;
    let alternatives = NonZeroUsize::new(17).ok_or("Invalid fixture alternatives")?;
    assert_eq!(
        selection.random_mut().index(alternatives),
        recovered.random_mut().index(alternatives)
    );
    assert_eq!(selection.snapshot()?, recovered.snapshot()?);
    Ok(())
}
