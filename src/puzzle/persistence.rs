use std::array::TryFromSliceError;
use std::num::{NonZeroU64, TryFromIntError};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::board::enums::BoardEnumVariant;
use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::{BoardPosition, BoardPositionError};
use crate::clock::duration::ClockInstant;
use crate::puzzle::checkpoint::{
    PuzzleCheckpoint, PuzzleCheckpointEnumAdvance, PuzzleCheckpointEnumCompletion,
    PuzzleCheckpointError, PuzzleCheckpointWriteToken,
};
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::enums::{
    PuzzleEnumAssistance, PuzzleEnumCountdownMode, PuzzleEnumEvent, PuzzleEnumHint,
    PuzzleEnumPhase, PuzzleEnumPractice, PuzzleEnumPrimaryOutcome,
};
use crate::puzzle::identity::{PuzzleDefinitionIdentity, PuzzleDefinitionIdentityError};
use crate::puzzle::metadata::{
    PuzzleDefinitionDifficulty, PuzzleDefinitionMetadata, PuzzleDefinitionMetadataError,
};
use crate::puzzle::random_provider::PuzzleRandomProviderSeeded;
use crate::puzzle::run::{PuzzleRun, PuzzleRunEnumConfirmation, PuzzleRunError};
use crate::puzzle::selection::{
    PuzzleSelection, PuzzleSelectionDataset, PuzzleSelectionDifficulty, PuzzleSelectionError,
    PuzzleSelectionFilter,
};
use crate::puzzle::session::{
    PuzzleSessionConfig, PuzzleSessionConfigError, PuzzleSessionContext, PuzzleSessionId,
};
use crate::puzzle::timer::{PuzzleTimerEnumPauseReason, PuzzleTimerEnumReadiness};
use crate::puzzle::tree::PuzzleDefinitionTreeError;

#[derive(Debug, Error)]
pub enum PuzzlePersistenceError {
    #[error("The puzzle persistence schema or random algorithm is unsupported")]
    UnsupportedSchema,
    #[error("The puzzle persistence document exceeds its bounded budget")]
    Budget,
    #[error("The saved puzzle journal, context, or countdown differs from canonical replay")]
    ReplayMismatch,
    #[error("The puzzle journal prefix does not match the requested checkpoint")]
    InvalidPrefix,
    #[error("The random provider must be freshly initialized with its declared seed")]
    RandomInitialization,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Run(#[from] PuzzleRunError),
    #[error(transparent)]
    Selection(#[from] PuzzleSelectionError),
    #[error(transparent)]
    Config(#[from] PuzzleSessionConfigError),
    #[error(transparent)]
    Identity(#[from] PuzzleDefinitionIdentityError),
    #[error(transparent)]
    Metadata(#[from] PuzzleDefinitionMetadataError),
    #[error(transparent)]
    Position(#[from] BoardPositionError),
    #[error(transparent)]
    Move(#[from] BoardMoveError),
    #[error(transparent)]
    Tree(#[from] PuzzleDefinitionTreeError),
    #[error(transparent)]
    Checkpoint(#[from] PuzzleCheckpointError),
    #[error("The puzzle journal digest has an invalid length: {0}")]
    JournalDigest(#[from] TryFromSliceError),
    #[error("The puzzle journal value exceeds its supported range: {0}")]
    Range(#[from] TryFromIntError),
}

pub enum PuzzlePersistenceEnumCommand {
    Readiness(PuzzleTimerEnumReadiness),
    Move(BoardMove),
    Hint,
    OpponentReply,
    RetryPractice,
    Pause(PuzzleTimerEnumPauseReason),
    Resume(PuzzleTimerEnumReadiness),
    Next,
    Skip,
    Reshuffle,
    End(PuzzleRunEnumConfirmation),
    Poll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PuzzlePersistenceStorageRevision(u64);

impl PuzzlePersistenceStorageRevision {
    pub const INITIAL: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PuzzlePersistenceIdentity {
    source: String,
    version: String,
    puzzle: String,
}

impl PuzzlePersistenceIdentity {
    fn capture(identity: &PuzzleDefinitionIdentity) -> Self {
        Self {
            source: identity.source().into(),
            version: identity.version().into(),
            puzzle: identity.puzzle_id().into(),
        }
    }

    fn validate(&self) -> Result<PuzzleDefinitionIdentity, PuzzlePersistenceError> {
        for value in [&self.source, &self.version, &self.puzzle] {
            if value.len() > PuzzlePersistenceCodec::MAX_TOKEN_BYTES {
                return Err(PuzzlePersistenceError::Budget);
            }
        }
        Ok(PuzzleDefinitionIdentity::new(
            self.source.clone(),
            self.version.clone(),
            self.puzzle.clone(),
        )?)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PuzzlePersistenceEnumVariant {
    Standard,
    Chess960,
}

impl PuzzlePersistenceEnumVariant {
    const fn capture(variant: BoardEnumVariant) -> Self {
        match variant {
            BoardEnumVariant::Standard => Self::Standard,
            BoardEnumVariant::Chess960 => Self::Chess960,
        }
    }

    const fn domain(&self) -> BoardEnumVariant {
        match self {
            Self::Standard => BoardEnumVariant::Standard,
            Self::Chess960 => BoardEnumVariant::Chess960,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PuzzlePersistenceDefinition {
    identity: PuzzlePersistenceIdentity,
    variant: PuzzlePersistenceEnumVariant,
    fen: String,
    lines: Vec<Vec<String>>,
    difficulty: Option<u32>,
    themes: Option<Vec<String>>,
}

impl PuzzlePersistenceDefinition {
    fn capture(definition: &PuzzleDefinition) -> Result<Self, PuzzlePersistenceError> {
        let tree = definition.solution();
        let lines = tree
            .lines()
            .iter()
            .map(|line| {
                line.nodes()
                    .iter()
                    .map(|id| {
                        tree.node(*id)
                            .and_then(|node| node.movement())
                            .map(|movement| movement.uci(definition.starting_position().variant()))
                            .ok_or(PuzzlePersistenceError::ReplayMismatch)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            identity: PuzzlePersistenceIdentity::capture(definition.identity()),
            variant: PuzzlePersistenceEnumVariant::capture(
                definition.starting_position().variant(),
            ),
            fen: definition.starting_position().fen(),
            lines,
            difficulty: definition
                .metadata()
                .difficulty()
                .map(PuzzleDefinitionDifficulty::rating),
            themes: definition
                .metadata()
                .themes()
                .map(|themes| themes.iter().map(|theme| theme.as_str().into()).collect()),
        })
    }

    fn validate(&self) -> Result<PuzzleDefinition, PuzzlePersistenceError> {
        if self.lines.len() > PuzzlePersistenceCodec::MAX_LINES
            || self
                .lines
                .iter()
                .any(|line| line.len() > PuzzlePersistenceCodec::MAX_LINE_PLIES)
        {
            return Err(PuzzlePersistenceError::Budget);
        }
        let initial = BoardPosition::from_fen(&self.fen, self.variant.domain())?;
        let lines = self
            .lines
            .iter()
            .map(|line| {
                let mut position = initial.clone();
                line.iter()
                    .map(|uci| {
                        let movement = position.parse_uci(uci)?;
                        position = position.play(movement)?;
                        Ok(movement)
                    })
                    .collect::<Result<Vec<_>, PuzzlePersistenceError>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let themes = self
            .themes
            .as_ref()
            .map(|themes| {
                themes
                    .iter()
                    .map(|theme| theme.parse())
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        Ok(PuzzleDefinition::new(
            self.identity.validate()?,
            initial,
            lines,
            PuzzleDefinitionMetadata::new(
                self.difficulty.map(PuzzleDefinitionDifficulty::new),
                themes,
            )?,
        )?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PuzzlePersistenceEnumMode {
    Session,
    PerPuzzle,
}

impl PuzzlePersistenceEnumMode {
    const fn capture(mode: PuzzleEnumCountdownMode) -> Self {
        match mode {
            PuzzleEnumCountdownMode::Session => Self::Session,
            PuzzleEnumCountdownMode::PerPuzzle => Self::PerPuzzle,
        }
    }

    const fn domain(self) -> PuzzleEnumCountdownMode {
        match self {
            Self::Session => PuzzleEnumCountdownMode::Session,
            Self::PerPuzzle => PuzzleEnumCountdownMode::PerPuzzle,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PuzzlePersistenceCollection {
    schema: u32,
    session: u64,
    mode: PuzzlePersistenceEnumMode,
    duration_nanos: u64,
    algorithm: String,
    algorithm_version: u32,
    seed: u64,
    difficulty: Option<(u32, u32)>,
    themes: Vec<String>,
    definitions: Vec<PuzzlePersistenceDefinition>,
    initial_order: Vec<PuzzlePersistenceIdentity>,
    initial_draws: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PuzzlePersistenceEnumReadiness {
    BoardNotReady,
    Hidden,
    InputBlocked,
    Ready,
}

impl PuzzlePersistenceEnumReadiness {
    const fn capture(value: PuzzleTimerEnumReadiness) -> Self {
        match value {
            PuzzleTimerEnumReadiness::BoardNotReady => Self::BoardNotReady,
            PuzzleTimerEnumReadiness::Hidden => Self::Hidden,
            PuzzleTimerEnumReadiness::InputBlocked => Self::InputBlocked,
            PuzzleTimerEnumReadiness::Ready => Self::Ready,
        }
    }
    const fn domain(self) -> PuzzleTimerEnumReadiness {
        match self {
            Self::BoardNotReady => PuzzleTimerEnumReadiness::BoardNotReady,
            Self::Hidden => PuzzleTimerEnumReadiness::Hidden,
            Self::InputBlocked => PuzzleTimerEnumReadiness::InputBlocked,
            Self::Ready => PuzzleTimerEnumReadiness::Ready,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PuzzlePersistenceEnumPauseReason {
    Manual,
    FocusLost,
    WindowHidden,
    Sleep,
    Recovery,
}

impl PuzzlePersistenceEnumPauseReason {
    const fn capture(value: PuzzleTimerEnumPauseReason) -> Self {
        match value {
            PuzzleTimerEnumPauseReason::Manual => Self::Manual,
            PuzzleTimerEnumPauseReason::FocusLost => Self::FocusLost,
            PuzzleTimerEnumPauseReason::WindowHidden => Self::WindowHidden,
            PuzzleTimerEnumPauseReason::Sleep => Self::Sleep,
            PuzzleTimerEnumPauseReason::Recovery => Self::Recovery,
        }
    }
    const fn domain(self) -> PuzzleTimerEnumPauseReason {
        match self {
            Self::Manual => PuzzleTimerEnumPauseReason::Manual,
            Self::FocusLost => PuzzleTimerEnumPauseReason::FocusLost,
            Self::WindowHidden => PuzzleTimerEnumPauseReason::WindowHidden,
            Self::Sleep => PuzzleTimerEnumPauseReason::Sleep,
            Self::Recovery => PuzzleTimerEnumPauseReason::Recovery,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum PuzzlePersistenceEnumOperation {
    Readiness(PuzzlePersistenceEnumReadiness),
    Move(String),
    Hint,
    OpponentReply,
    RetryPractice,
    Pause(PuzzlePersistenceEnumPauseReason),
    Resume(PuzzlePersistenceEnumReadiness),
    Next,
    Skip,
    Reshuffle,
    End,
    Poll,
    Recover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PuzzlePersistenceEnumOutcome {
    UnassistedSolve,
    AssistedSolve,
    Failed,
    Skipped,
    TimedOut,
}

impl PuzzlePersistenceEnumOutcome {
    const fn capture(value: PuzzleEnumPrimaryOutcome) -> Self {
        match value {
            PuzzleEnumPrimaryOutcome::Solved(PuzzleEnumAssistance::Unassisted) => {
                Self::UnassistedSolve
            }
            PuzzleEnumPrimaryOutcome::Solved(PuzzleEnumAssistance::Assisted) => Self::AssistedSolve,
            PuzzleEnumPrimaryOutcome::Failed => Self::Failed,
            PuzzleEnumPrimaryOutcome::Skipped => Self::Skipped,
            PuzzleEnumPrimaryOutcome::TimedOut => Self::TimedOut,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum PuzzlePersistenceEnumAttemptEvent {
    SolverMove(String),
    OpponentReply { movement: String, choice: usize },
    LegalUnlisted(String),
    HintSource,
    HintDestination,
    HintNextMove,
    PracticeRetry,
    PracticeCompleted,
    Skipped,
    TimedOut(PuzzlePersistenceEnumMode),
}

impl PuzzlePersistenceEnumAttemptEvent {
    fn capture(value: &PuzzleEnumEvent, variant: BoardEnumVariant) -> Self {
        match *value {
            PuzzleEnumEvent::SolverMove { movement, .. } => Self::SolverMove(movement.uci(variant)),
            PuzzleEnumEvent::OpponentReply {
                movement, choice, ..
            } => Self::OpponentReply {
                movement: movement.uci(variant),
                choice,
            },
            PuzzleEnumEvent::LegalUnlisted(movement) => Self::LegalUnlisted(movement.uci(variant)),
            PuzzleEnumEvent::Hint(hint) => match hint {
                PuzzleEnumHint::Source(_) => Self::HintSource,
                PuzzleEnumHint::Destination(_) => Self::HintDestination,
                PuzzleEnumHint::NextMove(_) => Self::HintNextMove,
            },
            PuzzleEnumEvent::PracticeRetry => Self::PracticeRetry,
            PuzzleEnumEvent::PracticeCompleted => Self::PracticeCompleted,
            PuzzleEnumEvent::Skipped => Self::Skipped,
            PuzzleEnumEvent::TimedOut(mode) => {
                Self::TimedOut(PuzzlePersistenceEnumMode::capture(mode))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PuzzlePersistenceAttempt {
    id: u64,
    elapsed_nanos: u64,
    definition: PuzzlePersistenceIdentity,
    primary: Option<PuzzlePersistenceEnumOutcome>,
    assisted: bool,
    practice: PuzzlePersistenceEnumPractice,
    phase: PuzzlePersistenceEnumPhase,
    event_count: usize,
    last_event: Option<PuzzlePersistenceEnumAttemptEvent>,
}

impl PuzzlePersistenceAttempt {
    fn capture(attempt: &crate::puzzle::attempt::PuzzleAttempt, elapsed_nanos: u64) -> Self {
        Self {
            id: attempt.id().value().get(),
            elapsed_nanos,
            definition: PuzzlePersistenceIdentity::capture(attempt.definition().identity()),
            primary: attempt
                .primary_outcome()
                .map(PuzzlePersistenceEnumOutcome::capture),
            assisted: attempt.assistance() == PuzzleEnumAssistance::Assisted,
            practice: match attempt.practice() {
                PuzzleEnumPractice::NotStarted => PuzzlePersistenceEnumPractice::NotStarted,
                PuzzleEnumPractice::InProgress => PuzzlePersistenceEnumPractice::InProgress,
                PuzzleEnumPractice::Completed => PuzzlePersistenceEnumPractice::Completed,
            },
            phase: match attempt.phase() {
                PuzzleEnumPhase::SolverMove => PuzzlePersistenceEnumPhase::SolverMove,
                PuzzleEnumPhase::OpponentReply => PuzzlePersistenceEnumPhase::OpponentReply,
                PuzzleEnumPhase::PracticeReady => PuzzlePersistenceEnumPhase::PracticeReady,
                PuzzleEnumPhase::Review => PuzzlePersistenceEnumPhase::Review,
                PuzzleEnumPhase::Finalized => PuzzlePersistenceEnumPhase::Finalized,
            },
            event_count: attempt.events().len(),
            last_event: attempt.events().last().map(|event| {
                PuzzlePersistenceEnumAttemptEvent::capture(event, attempt.position().variant())
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PuzzlePersistenceEnumPractice {
    NotStarted,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PuzzlePersistenceEnumPhase {
    SolverMove,
    OpponentReply,
    PracticeReady,
    Review,
    Finalized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PuzzlePersistenceCheckpoint {
    schema: u32,
    session: u64,
    revision: u64,
    event_count: usize,
    elapsed_nanos: u64,
    remaining_nanos: u64,
    puzzle_elapsed_nanos: u64,
    attempt: PuzzlePersistenceAttempt,
    archived_count: usize,
    run_event_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PuzzlePersistenceEvent {
    elapsed_nanos: u64,
    operation: PuzzlePersistenceEnumOperation,
    after: PuzzlePersistenceCheckpoint,
    archived: Option<PuzzlePersistenceAttempt>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct PuzzlePersistenceJournalProof([u8; 32]);

impl PuzzlePersistenceJournalProof {
    pub(crate) const VERSION: u32 = 1;
    const INITIAL_DOMAIN: &[u8] = b"chess.puzzle.journal.initial";
    const EVENT_DOMAIN: &[u8] = b"chess.puzzle.journal.event";

    pub(crate) fn initial(collection_digest: &[u8; 32]) -> Self {
        let mut hash = Sha256::new();
        hash.update(Self::INITIAL_DOMAIN);
        hash.update(Self::VERSION.to_be_bytes());
        hash.update(collection_digest);
        Self(hash.finalize().into())
    }

    pub(crate) fn restore(version: u32, digest: &[u8]) -> Result<Self, PuzzlePersistenceError> {
        if version != Self::VERSION {
            return Err(PuzzlePersistenceError::UnsupportedSchema);
        }
        Ok(Self(digest.try_into()?))
    }

    pub(crate) fn extend(self, event: &str) -> Result<Self, PuzzlePersistenceError> {
        let mut hash = Sha256::new();
        hash.update(Self::EVENT_DOMAIN);
        hash.update(Self::VERSION.to_be_bytes());
        hash.update(self.0);
        hash.update(u64::try_from(event.len())?.to_be_bytes());
        hash.update(event.as_bytes());
        Ok(Self(hash.finalize().into()))
    }

    pub(crate) const fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

struct PuzzlePersistenceJournalEvent {
    body: String,
    proof: PuzzlePersistenceJournalProof,
}

impl PuzzlePersistenceJournalEvent {
    fn proof_at(
        events: &[Arc<Self>],
        initial: PuzzlePersistenceJournalProof,
        event_count: usize,
    ) -> Result<PuzzlePersistenceJournalProof, PuzzlePersistenceError> {
        match event_count.checked_sub(1) {
            None => Ok(initial),
            Some(index) => events
                .get(index)
                .map(|event| event.proof)
                .ok_or(PuzzlePersistenceError::InvalidPrefix),
        }
    }
}

#[derive(Clone)]
pub struct PuzzlePersistenceSnapshot {
    collection: Arc<String>,
    collection_digest: [u8; 32],
    events: Vec<Arc<PuzzlePersistenceJournalEvent>>,
    base_event_count: usize,
    base_journal_proof: PuzzlePersistenceJournalProof,
    checkpoint: String,
    context: PuzzleSessionContext,
    elapsed_nanos: u64,
}

impl PuzzlePersistenceSnapshot {
    pub(super) fn rebase(
        &self,
        base_event_count: usize,
        base_journal_proof: PuzzlePersistenceJournalProof,
    ) -> Result<Self, PuzzlePersistenceError> {
        if self.journal_proof_at(base_event_count)? != base_journal_proof {
            return Err(PuzzlePersistenceError::InvalidPrefix);
        }
        let offset = base_event_count
            .checked_sub(self.base_event_count)
            .ok_or(PuzzlePersistenceError::InvalidPrefix)?;
        let events = self
            .events
            .get(offset..)
            .ok_or(PuzzlePersistenceError::InvalidPrefix)?;
        Ok(Self {
            collection: Arc::clone(&self.collection),
            collection_digest: self.collection_digest,
            events: events.iter().map(Arc::clone).collect(),
            base_event_count,
            base_journal_proof,
            checkpoint: self.checkpoint.clone(),
            context: self.context.clone(),
            elapsed_nanos: self.elapsed_nanos,
        })
    }

    pub(super) fn journal_proof_at(
        &self,
        event_count: usize,
    ) -> Result<PuzzlePersistenceJournalProof, PuzzlePersistenceError> {
        let offset = event_count
            .checked_sub(self.base_event_count)
            .ok_or(PuzzlePersistenceError::InvalidPrefix)?;
        PuzzlePersistenceJournalEvent::proof_at(&self.events, self.base_journal_proof, offset)
    }

    pub(crate) const fn base_journal_proof(&self) -> PuzzlePersistenceJournalProof {
        self.base_journal_proof
    }

    pub(crate) fn journal_proof(&self) -> PuzzlePersistenceJournalProof {
        self.events
            .last()
            .map_or(self.base_journal_proof, |event| event.proof)
    }
    pub const fn context(&self) -> &PuzzleSessionContext {
        &self.context
    }
    pub const fn base_event_count(&self) -> usize {
        self.base_event_count
    }
    pub fn event_count(&self) -> usize {
        self.base_event_count + self.events.len()
    }
    pub const fn elapsed_nanos(&self) -> u64 {
        self.elapsed_nanos
    }
    pub(crate) const fn collection_digest(&self) -> &[u8; 32] {
        &self.collection_digest
    }
    pub(crate) fn collection(&self) -> &str {
        &self.collection
    }
    pub(crate) fn events(&self) -> impl Iterator<Item = &str> {
        self.events.iter().map(|event| event.body.as_str())
    }
    pub(crate) fn checkpoint(&self) -> &str {
        &self.checkpoint
    }
    pub(crate) fn attempt_rows(&self) -> Result<Vec<(u64, String)>, PuzzlePersistenceError> {
        let mut attempts = std::collections::BTreeMap::new();
        for event in self.events() {
            let event: PuzzlePersistenceEvent = serde_json::from_str(event)?;
            if let Some(archived) = event.archived {
                attempts.insert(
                    archived.id,
                    PuzzlePersistenceCodec::bounded_json(&archived)?,
                );
            }
            attempts.insert(
                event.after.attempt.id,
                PuzzlePersistenceCodec::bounded_json(&event.after.attempt)?,
            );
        }
        let checkpoint: PuzzlePersistenceCheckpoint = serde_json::from_str(&self.checkpoint)?;
        attempts.insert(
            checkpoint.attempt.id,
            PuzzlePersistenceCodec::bounded_json(&checkpoint.attempt)?,
        );
        Ok(attempts.into_iter().collect())
    }

    pub(crate) fn transport_bytes(&self) -> Result<usize, PuzzlePersistenceError> {
        self.events.iter().try_fold(
            self.collection
                .len()
                .checked_add(self.checkpoint.len())
                .ok_or(PuzzlePersistenceError::Budget)?,
            |total, event| {
                total
                    .checked_add(event.body.len())
                    .ok_or(PuzzlePersistenceError::Budget)
            },
        )
    }
}

pub struct PuzzlePersistenceRun {
    run: PuzzleRun<PuzzleRandomProviderSeeded>,
    collection: Arc<String>,
    collection_digest: [u8; 32],
    events: Vec<Arc<PuzzlePersistenceJournalEvent>>,
    initial_journal_proof: PuzzlePersistenceJournalProof,
    journal_bytes: usize,
}

impl PuzzlePersistenceRun {
    pub fn new(
        id: PuzzleSessionId,
        config: PuzzleSessionConfig,
        definitions: Vec<PuzzleDefinition>,
        filters: PuzzleSelectionFilter,
        random: PuzzleRandomProviderSeeded,
    ) -> Result<Self, PuzzlePersistenceError> {
        if definitions.len() > PuzzlePersistenceCodec::MAX_DEFINITIONS {
            return Err(PuzzlePersistenceError::Budget);
        }
        let seed = random.seed();
        if !random.is_fresh() {
            return Err(PuzzlePersistenceError::RandomInitialization);
        }
        let dataset = PuzzleSelectionDataset::from_identity(
            definitions
                .first()
                .ok_or(PuzzleRunError::NoMatchingPuzzles)?
                .identity(),
        );
        let stored = definitions
            .iter()
            .map(PuzzlePersistenceDefinition::capture)
            .collect::<Result<Vec<_>, _>>()?;
        for definition in &stored {
            definition.validate()?;
        }
        let selection = PuzzleSelection::new(dataset, definitions, filters.clone(), random)?;
        let initial = selection.snapshot()?;
        let collection = PuzzlePersistenceCollection {
            schema: PuzzlePersistenceCodec::SCHEMA,
            session: id.value().get(),
            mode: PuzzlePersistenceEnumMode::capture(config.mode()),
            duration_nanos: config.duration().as_nanos(),
            algorithm: PuzzleRandomProviderSeeded::ALGORITHM.into(),
            algorithm_version: PuzzleRandomProviderSeeded::ALGORITHM_VERSION,
            seed,
            difficulty: filters
                .difficulty()
                .map(|range| (range.minimum().rating(), range.maximum().rating())),
            themes: filters
                .themes()
                .iter()
                .map(|theme| theme.as_str().into())
                .collect(),
            definitions: stored,
            initial_order: initial
                .persisted()
                .order
                .iter()
                .map(PuzzlePersistenceIdentity::capture)
                .collect(),
            initial_draws: initial
                .persisted()
                .randomness
                .iter()
                .map(|draw| (draw.alternatives().get(), draw.choice()))
                .collect(),
        };
        let encoded = PuzzlePersistenceCodec::bounded_json(&collection)?;
        let collection_digest = Sha256::digest(encoded.as_bytes()).into();
        Ok(Self {
            run: PuzzleRun::new(id, config, selection)?,
            collection_digest,
            collection: Arc::new(encoded),
            events: Vec::new(),
            initial_journal_proof: PuzzlePersistenceJournalProof::initial(&collection_digest),
            journal_bytes: 0,
        })
    }

    pub const fn run(&self) -> &PuzzleRun<PuzzleRandomProviderSeeded> {
        &self.run
    }
    pub(crate) fn attempt_rows(&self) -> Result<Vec<(u64, String)>, PuzzlePersistenceError> {
        let mut attempts = std::collections::BTreeMap::new();
        for entry in self.run.previous_attempts() {
            let attempt = PuzzlePersistenceAttempt::capture(
                entry.attempt(),
                entry.elapsed_active().as_nanos(),
            );
            attempts.insert(attempt.id, PuzzlePersistenceCodec::bounded_json(&attempt)?);
        }
        let attempt = self
            .projection(ClockInstant::from_nanos(0), self.events.len())?
            .attempt;
        attempts.insert(attempt.id, PuzzlePersistenceCodec::bounded_json(&attempt)?);
        Ok(attempts.into_iter().collect())
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn execute(
        &mut self,
        command: PuzzlePersistenceEnumCommand,
        now: ClockInstant,
    ) -> Result<(), PuzzlePersistenceError> {
        let operation = match command {
            PuzzlePersistenceEnumCommand::Readiness(value) => {
                PuzzlePersistenceEnumOperation::Readiness(PuzzlePersistenceEnumReadiness::capture(
                    value,
                ))
            }
            PuzzlePersistenceEnumCommand::Move(movement) => PuzzlePersistenceEnumOperation::Move(
                movement.uci(
                    self.run
                        .persistence_session()
                        .attempt()
                        .position()
                        .variant(),
                ),
            ),
            PuzzlePersistenceEnumCommand::Hint => PuzzlePersistenceEnumOperation::Hint,
            PuzzlePersistenceEnumCommand::OpponentReply => {
                PuzzlePersistenceEnumOperation::OpponentReply
            }
            PuzzlePersistenceEnumCommand::RetryPractice => {
                PuzzlePersistenceEnumOperation::RetryPractice
            }
            PuzzlePersistenceEnumCommand::Pause(value) => PuzzlePersistenceEnumOperation::Pause(
                PuzzlePersistenceEnumPauseReason::capture(value),
            ),
            PuzzlePersistenceEnumCommand::Resume(value) => PuzzlePersistenceEnumOperation::Resume(
                PuzzlePersistenceEnumReadiness::capture(value),
            ),
            PuzzlePersistenceEnumCommand::Next => PuzzlePersistenceEnumOperation::Next,
            PuzzlePersistenceEnumCommand::Skip => PuzzlePersistenceEnumOperation::Skip,
            PuzzlePersistenceEnumCommand::Reshuffle => PuzzlePersistenceEnumOperation::Reshuffle,
            PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Confirmed) => {
                PuzzlePersistenceEnumOperation::End
            }
            PuzzlePersistenceEnumCommand::End(PuzzleRunEnumConfirmation::Unconfirmed) => {
                return Err(PuzzleRunError::ConfirmationRequired.into());
            }
            PuzzlePersistenceEnumCommand::Poll => PuzzlePersistenceEnumOperation::Poll,
        };
        self.apply_and_record(operation, now)
    }

    fn apply_and_record(
        &mut self,
        operation: PuzzlePersistenceEnumOperation,
        now: ClockInstant,
    ) -> Result<(), PuzzlePersistenceError> {
        let reserved_events = if operation == PuzzlePersistenceEnumOperation::Recover {
            1
        } else {
            2
        };
        self.reserve_event(reserved_events)?;
        let elapsed_nanos = self.run.view(now)?.timer().elapsed().as_nanos();
        let previous_count = self.run.previous_attempts().len();
        self.apply(&operation, now)?;
        let event = self.event_after(operation, elapsed_nanos, now, previous_count)?;
        self.append_event(&event)?;
        Ok(())
    }

    fn reserve_event(&self, reserved_events: usize) -> Result<(), PuzzlePersistenceError> {
        let reserved = self
            .collection
            .len()
            .checked_add(self.journal_bytes)
            .and_then(|bytes| {
                bytes.checked_add(
                    PuzzlePersistenceCodec::MAX_EVENT_BYTES.checked_mul(reserved_events + 1)?,
                )
            })
            .ok_or(PuzzlePersistenceError::Budget)?;
        if self
            .events
            .len()
            .checked_add(reserved_events)
            .is_none_or(|events| events > PuzzlePersistenceCodec::MAX_EVENTS)
            || reserved > PuzzlePersistenceCodec::MAX_DOCUMENT_BYTES
        {
            return Err(PuzzlePersistenceError::Budget);
        }
        Ok(())
    }

    fn append_event(
        &mut self,
        event: &PuzzlePersistenceEvent,
    ) -> Result<(), PuzzlePersistenceError> {
        let encoded = PuzzlePersistenceCodec::bounded_json(event)?;
        if encoded.len() > PuzzlePersistenceCodec::MAX_EVENT_BYTES {
            return Err(PuzzlePersistenceError::Budget);
        }
        let proof = self.journal_proof_at(self.events.len())?.extend(&encoded)?;
        self.journal_bytes = self
            .journal_bytes
            .checked_add(encoded.len())
            .ok_or(PuzzlePersistenceError::Budget)?;
        self.events.push(Arc::new(PuzzlePersistenceJournalEvent {
            body: encoded,
            proof,
        }));
        Ok(())
    }

    pub(crate) fn journal_proof_at(
        &self,
        event_count: usize,
    ) -> Result<PuzzlePersistenceJournalProof, PuzzlePersistenceError> {
        PuzzlePersistenceJournalEvent::proof_at(
            &self.events,
            self.initial_journal_proof,
            event_count,
        )
    }

    fn archived_after(&self, previous_count: usize) -> Option<PuzzlePersistenceAttempt> {
        if self.run.previous_attempts().len() == previous_count {
            return None;
        }
        self.run.previous_attempts().last().map(|entry| {
            PuzzlePersistenceAttempt::capture(entry.attempt(), entry.elapsed_active().as_nanos())
        })
    }

    fn event_after(
        &self,
        operation: PuzzlePersistenceEnumOperation,
        elapsed_nanos: u64,
        now: ClockInstant,
        previous_count: usize,
    ) -> Result<PuzzlePersistenceEvent, PuzzlePersistenceError> {
        Ok(PuzzlePersistenceEvent {
            elapsed_nanos,
            operation,
            after: self.projection(now, self.events.len() + 1)?,
            archived: self.archived_after(previous_count),
        })
    }

    fn apply(
        &mut self,
        operation: &PuzzlePersistenceEnumOperation,
        now: ClockInstant,
    ) -> Result<(), PuzzlePersistenceError> {
        let context = self.run.context();
        match operation {
            PuzzlePersistenceEnumOperation::Readiness(value) => {
                self.run.update_readiness(&context, now, value.domain())?;
            }
            PuzzlePersistenceEnumOperation::Move(uci) => {
                let movement = self
                    .run
                    .persistence_session()
                    .attempt()
                    .position()
                    .parse_uci(uci)?;
                self.run.submit_move(&context, now, movement)?;
            }
            PuzzlePersistenceEnumOperation::Hint => {
                self.run.request_hint(&context, now)?;
            }
            PuzzlePersistenceEnumOperation::OpponentReply => {
                self.run.play_opponent_reply(&context, now)?;
            }
            PuzzlePersistenceEnumOperation::RetryPractice => {
                self.run.retry_practice(&context, now)?;
            }
            PuzzlePersistenceEnumOperation::Pause(reason) => {
                self.run.pause(&context, now, reason.domain())?;
            }
            PuzzlePersistenceEnumOperation::Resume(value) => {
                self.run.resume(&context, now, value.domain())?;
            }
            PuzzlePersistenceEnumOperation::Next => {
                self.run.next(&context, now)?;
            }
            PuzzlePersistenceEnumOperation::Skip => {
                self.run.skip(&context, now)?;
            }
            PuzzlePersistenceEnumOperation::Reshuffle => {
                self.run.reshuffle_and_continue(&context, now)?;
            }
            PuzzlePersistenceEnumOperation::End => {
                self.run
                    .end_run(&context, now, PuzzleRunEnumConfirmation::Confirmed)?;
            }
            PuzzlePersistenceEnumOperation::Poll => {
                self.run.poll(now)?;
            }
            PuzzlePersistenceEnumOperation::Recover => {
                let snapshot = self.run.snapshot(now)?;
                let collection: PuzzlePersistenceCollection =
                    serde_json::from_str(&self.collection)?;
                let definitions = collection
                    .definitions
                    .iter()
                    .map(PuzzlePersistenceDefinition::validate)
                    .collect::<Result<Vec<_>, _>>()?;
                self.run = PuzzleRun::recover(
                    snapshot,
                    definitions,
                    PuzzleRandomProviderSeeded::new(collection.seed),
                )?;
            }
        }
        Ok(())
    }

    fn projection(
        &self,
        now: ClockInstant,
        event_count: usize,
    ) -> Result<PuzzlePersistenceCheckpoint, PuzzlePersistenceError> {
        let session = self.run.persistence_session();
        let attempt = session.attempt();
        let timer = self.run.view(now)?.timer();
        let attempt = PuzzlePersistenceAttempt::capture(
            attempt,
            self.run.persistence_attempt_elapsed(timer).as_nanos(),
        );
        Ok(PuzzlePersistenceCheckpoint {
            schema: PuzzlePersistenceCodec::SCHEMA,
            session: session.id().value().get(),
            revision: session.revision().value(),
            event_count,
            elapsed_nanos: timer.elapsed().as_nanos(),
            remaining_nanos: timer.remaining().as_nanos(),
            puzzle_elapsed_nanos: timer.puzzle_elapsed().as_nanos(),
            attempt,
            archived_count: self.run.previous_attempts().len(),
            run_event_count: self.run.events().len(),
        })
    }

    pub fn capture(
        &mut self,
        now: ClockInstant,
        base_event_count: usize,
    ) -> Result<PuzzlePersistenceSnapshot, PuzzlePersistenceError> {
        if base_event_count > self.events.len() {
            return Err(PuzzlePersistenceError::InvalidPrefix);
        }
        let before = self.run.context();
        let timer = self.run.view(now)?.timer();
        if matches!(
            timer.status(),
            crate::puzzle::timer::PuzzleTimerEnumStatus::Expired { .. }
        ) && self
            .run
            .persistence_session()
            .attempt()
            .phase()
            .is_unfinished()
        {
            self.reserve_event(2)?;
        }
        let previous_count = self.run.previous_attempts().len();
        self.run.poll(now)?;
        if self.run.context() != before {
            let operation = PuzzlePersistenceEnumOperation::Poll;
            let event =
                self.event_after(operation, timer.elapsed().as_nanos(), now, previous_count)?;
            self.append_event(&event)?;
        }
        let checkpoint = self.projection(now, self.events.len())?;
        Ok(PuzzlePersistenceSnapshot {
            collection: Arc::clone(&self.collection),
            collection_digest: self.collection_digest,
            events: self.events[base_event_count..]
                .iter()
                .map(Arc::clone)
                .collect(),
            base_event_count,
            base_journal_proof: self.journal_proof_at(base_event_count)?,
            checkpoint: PuzzlePersistenceCodec::bounded_json(&checkpoint)?,
            context: self.run.context(),
            elapsed_nanos: checkpoint.elapsed_nanos,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PuzzlePersistenceDocument {
    schema: u32,
    collection: PuzzlePersistenceCollection,
    events: Vec<PuzzlePersistenceEvent>,
    checkpoint: PuzzlePersistenceCheckpoint,
}

pub struct PuzzlePersistenceCodec;

impl PuzzlePersistenceCodec {
    pub const SCHEMA: u32 = 1;
    pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
    pub const MAX_DEFINITIONS: usize = 200_000;
    pub const MAX_EVENTS: usize = 100_000;
    const MAX_TOKEN_BYTES: usize = 1_024;
    const MAX_EVENT_BYTES: usize = 64 * 1_024;
    const MAX_LINES: usize = 1_024;
    const MAX_LINE_PLIES: usize = 4_096;

    fn bounded_json<T: Serialize>(value: &T) -> Result<String, PuzzlePersistenceError> {
        let encoded = serde_json::to_string(value)?;
        if encoded.len() > Self::MAX_DOCUMENT_BYTES {
            return Err(PuzzlePersistenceError::Budget);
        }
        Ok(encoded)
    }

    pub fn encode(snapshot: &PuzzlePersistenceSnapshot) -> Result<Vec<u8>, PuzzlePersistenceError> {
        if snapshot.base_event_count != 0 {
            return Err(PuzzlePersistenceError::InvalidPrefix);
        }
        let document = PuzzlePersistenceDocument {
            schema: Self::SCHEMA,
            collection: serde_json::from_str(&snapshot.collection)?,
            events: snapshot
                .events
                .iter()
                .map(|event| serde_json::from_str(&event.body))
                .collect::<Result<Vec<_>, _>>()?,
            checkpoint: serde_json::from_str(&snapshot.checkpoint)?,
        };
        Ok(Self::bounded_json(&document)?.into_bytes())
    }

    pub fn decode(bytes: &[u8]) -> Result<PuzzlePersistenceRun, PuzzlePersistenceError> {
        if bytes.len() > Self::MAX_DOCUMENT_BYTES {
            return Err(PuzzlePersistenceError::Budget);
        }
        let document: PuzzlePersistenceDocument = serde_json::from_slice(bytes)?;
        Self::restore(document)
    }

    pub(crate) fn decode_parts(
        collection: &str,
        events: Vec<String>,
        checkpoint: &str,
    ) -> Result<PuzzlePersistenceRun, PuzzlePersistenceError> {
        let size = events.iter().try_fold(
            collection
                .len()
                .checked_add(checkpoint.len())
                .ok_or(PuzzlePersistenceError::Budget)?,
            |size, event| {
                size.checked_add(event.len())
                    .ok_or(PuzzlePersistenceError::Budget)
            },
        )?;
        if size > Self::MAX_DOCUMENT_BYTES || events.len() > Self::MAX_EVENTS {
            return Err(PuzzlePersistenceError::Budget);
        }
        Self::restore(PuzzlePersistenceDocument {
            schema: Self::SCHEMA,
            collection: serde_json::from_str(collection)?,
            events: events
                .iter()
                .map(|event| serde_json::from_str(event))
                .collect::<Result<Vec<_>, _>>()?,
            checkpoint: serde_json::from_str(checkpoint)?,
        })
    }

    fn restore(
        document: PuzzlePersistenceDocument,
    ) -> Result<PuzzlePersistenceRun, PuzzlePersistenceError> {
        let collection = document.collection;
        if document.schema != Self::SCHEMA
            || collection.schema != Self::SCHEMA
            || document.checkpoint.schema != Self::SCHEMA
            || collection.algorithm != PuzzleRandomProviderSeeded::ALGORITHM
            || collection.algorithm_version != PuzzleRandomProviderSeeded::ALGORITHM_VERSION
        {
            return Err(PuzzlePersistenceError::UnsupportedSchema);
        }
        if document.events.len() > Self::MAX_EVENTS {
            return Err(PuzzlePersistenceError::Budget);
        }
        let id = PuzzleSessionId::new(
            NonZeroU64::new(collection.session).ok_or(PuzzlePersistenceError::ReplayMismatch)?,
        );
        let config = PuzzleSessionConfig::new(
            collection.mode.domain(),
            Duration::from_nanos(collection.duration_nanos),
        )?;
        let definitions = collection
            .definitions
            .iter()
            .map(PuzzlePersistenceDefinition::validate)
            .collect::<Result<Vec<_>, _>>()?;
        let difficulty = collection
            .difficulty
            .map(|(minimum, maximum)| {
                PuzzleSelectionDifficulty::new(
                    PuzzleDefinitionDifficulty::new(minimum),
                    PuzzleDefinitionDifficulty::new(maximum),
                )
            })
            .transpose()?;
        let themes = collection
            .themes
            .iter()
            .map(|theme| theme.parse())
            .collect::<Result<Vec<_>, _>>()?;
        let mut owner = PuzzlePersistenceRun::new(
            id,
            config,
            definitions,
            PuzzleSelectionFilter::new(difficulty, themes)?,
            PuzzleRandomProviderSeeded::new(collection.seed),
        )?;
        let initial = owner.run.persistence_selection()?;
        if initial
            .persisted()
            .order
            .iter()
            .map(PuzzlePersistenceIdentity::capture)
            .ne(collection.initial_order)
            || initial
                .persisted()
                .randomness
                .iter()
                .map(|draw| (draw.alternatives().get(), draw.choice()))
                .ne(collection.initial_draws)
        {
            return Err(PuzzlePersistenceError::ReplayMismatch);
        }
        let mut elapsed_nanos = 0;
        for event in document.events {
            if event.elapsed_nanos < elapsed_nanos {
                return Err(PuzzlePersistenceError::ReplayMismatch);
            }
            elapsed_nanos = event.elapsed_nanos;
            let now = ClockInstant::from_nanos(elapsed_nanos);
            let previous_count = owner.run.previous_attempts().len();
            owner.apply(&event.operation, now)?;
            if owner.archived_after(previous_count) != event.archived
                || owner.projection(now, owner.events.len() + 1)? != event.after
            {
                return Err(PuzzlePersistenceError::ReplayMismatch);
            }
            owner.append_event(&event)?;
        }
        if document.checkpoint.elapsed_nanos < elapsed_nanos {
            return Err(PuzzlePersistenceError::ReplayMismatch);
        }
        let now = ClockInstant::from_nanos(document.checkpoint.elapsed_nanos);
        owner.run.poll(now)?;
        if owner.projection(now, owner.events.len())? != document.checkpoint {
            return Err(PuzzlePersistenceError::ReplayMismatch);
        }
        owner.apply_and_record(PuzzlePersistenceEnumOperation::Recover, now)?;
        Ok(owner)
    }
}

#[derive(Clone)]
pub struct PuzzlePersistenceSaveRequest {
    snapshot: PuzzlePersistenceSnapshot,
    expected_storage_revision: PuzzlePersistenceStorageRevision,
    token: PuzzleCheckpointWriteToken,
}

impl PuzzlePersistenceSaveRequest {
    pub fn new(
        snapshot: PuzzlePersistenceSnapshot,
        expected_storage_revision: PuzzlePersistenceStorageRevision,
        token: PuzzleCheckpointWriteToken,
    ) -> Result<Self, PuzzlePersistenceError> {
        if snapshot.context() != token.context()
            || snapshot.elapsed_nanos != token.elapsed_active().as_nanos()
        {
            return Err(PuzzlePersistenceError::ReplayMismatch);
        }
        Ok(Self {
            snapshot,
            expected_storage_revision,
            token,
        })
    }
    pub const fn snapshot(&self) -> &PuzzlePersistenceSnapshot {
        &self.snapshot
    }
    pub const fn expected_storage_revision(&self) -> PuzzlePersistenceStorageRevision {
        self.expected_storage_revision
    }
    pub const fn token(&self) -> &PuzzleCheckpointWriteToken {
        &self.token
    }
}

pub struct PuzzlePersistenceReceipt {
    request: PuzzlePersistenceSaveRequest,
    storage_revision: PuzzlePersistenceStorageRevision,
}

impl PuzzlePersistenceReceipt {
    pub(crate) const fn committed(
        request: PuzzlePersistenceSaveRequest,
        storage_revision: PuzzlePersistenceStorageRevision,
    ) -> Self {
        Self {
            request,
            storage_revision,
        }
    }
    pub const fn storage_revision(&self) -> PuzzlePersistenceStorageRevision {
        self.storage_revision
    }
    pub fn event_count(&self) -> usize {
        self.request.snapshot.event_count()
    }
    pub(super) fn journal_proof(&self) -> PuzzlePersistenceJournalProof {
        self.request.snapshot.journal_proof()
    }
    pub const fn context(&self) -> &PuzzleSessionContext {
        self.request.snapshot.context()
    }
    pub fn acknowledge<S>(
        &self,
        checkpoint: &mut PuzzleCheckpoint<S>,
        now: ClockInstant,
    ) -> Result<PuzzleCheckpointEnumAdvance, PuzzlePersistenceError> {
        Ok(checkpoint.complete(
            now,
            &self.request.token,
            PuzzleCheckpointEnumCompletion::Committed,
        )?)
    }
}
