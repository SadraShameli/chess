use std::collections::{HashMap, HashSet};

use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::board::r#move::BoardMove;
use crate::board::start::BoardStart;
use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::clock::state::{
    ClockState, ClockStateEnumMoveOutcome, ClockStateEnumStatus, ClockStateError,
    ClockStateSnapshot, ClockStateUpdate,
};
use crate::game::adjudication::{
    GameAdjudicationEnumDecision, GameAdjudicationEnumInterruption,
    GameAdjudicationEnumMateQuestion, GameAdjudicationError, GameAdjudicationHistory,
    GameAdjudicationMateProofProvider,
};
use crate::game::annotations::{
    GameAnnotationsEnumChange, GameAnnotationsEnumVariationRequest,
    GameAnnotationsSessionConfirmation, GameAnnotationsSessionPreview,
    GameAnnotationsVariationApplied, GameAnnotationsVariationPreview,
};
use crate::game::draw_claim::{
    GameDrawClaimEnumAvailability, GameDrawClaimEnumIntent, GameDrawClaimEnumKind,
    GameDrawClaimError,
};
use crate::game::engine_provider::{
    GameEngineContext, GameEngineRequest, GameEngineRequestError, GameEngineSearchConfig,
};
use crate::game::enums::{
    GameEnumEngineReadiness, GameEnumMode, GameEnumOutcomeReason, GameEnumPhase, GameEnumResult,
    GameEnumSeat, GameEnumTransitionKind, GameId, GameMoveId, GameRevision, GameRevisionError,
};
use crate::game::move_node::{
    GameMoveNode, GameMoveNodeAnnotations, GameMoveNodeMarking, GameMoveNodeNag,
};
use crate::game::outcome::{GameOutcome, GameOutcomeError};
use crate::game::pause::{
    GamePauseEnumCause, GamePauseEnumFault, GamePauseError, GamePauseRequest, GameResumeRequest,
};
use crate::game::premove::{
    GamePremove, GamePremoveEnumClearReason, GamePremoveEnumResolution, GamePremoveError,
    GamePremoveIntent,
};
use crate::game::result::{
    GameResultEnumLocalKind, GameResultError, GameResultLocalRequest, GameResultResignRequest,
};
use crate::game::save::{
    GameSaveEnumAcknowledgment, GameSaveEnumStatus, GameSaveError, GameSaveFailureRequest,
};
use crate::game::storage_repository::GameStorageReceipt;
use crate::game::takeback::{
    GameTakeback, GameTakebackEnumConsent, GameTakebackEnumResponse, GameTakebackError,
    GameTakebackPending, GameTakebackRequest, GameTakebackResponseRequest,
};
use crate::game::tree::{GameTree, GameTreeCandidateClock, GameTreeError};

pub mod lifecycle;

#[derive(Debug, Error)]
pub enum GameMetadataError {
    #[error("Game metadata text must not contain a NUL character")]
    InvalidText,
    #[error(
        "An extension tag needs an ASCII alphanumeric or underscore name outside known PGN tags"
    )]
    InvalidExtensionName,
    #[error("An extension tag name cannot occur more than once")]
    DuplicateExtensionName,
    #[error("A partial PGN date must use a valid year, month and calendar day")]
    InvalidDate,
    #[error("An opening code must contain A..E followed by two decimal digits")]
    InvalidOpeningCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameMetadataText(String);

impl GameMetadataText {
    pub fn new(value: String) -> Result<Self, GameMetadataError> {
        if value.contains('\0') {
            return Err(GameMetadataError::InvalidText);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameMetadataDate {
    year: Option<u16>,
    month: Option<u8>,
    day: Option<u8>,
}

impl GameMetadataDate {
    pub fn new(
        year: Option<u16>,
        month: Option<u8>,
        day: Option<u8>,
    ) -> Result<Self, GameMetadataError> {
        if year.is_some_and(|year| year == 0 || year > 9999)
            || month.is_some_and(|month| !(1..=12).contains(&month))
            || day.is_some_and(|day| day == 0 || day > Self::maximum_day(year, month))
        {
            return Err(GameMetadataError::InvalidDate);
        }
        Ok(Self { year, month, day })
    }

    fn maximum_day(year: Option<u16>, month: Option<u8>) -> u8 {
        match month {
            Some(4 | 6 | 9 | 11) => 30,
            Some(2) => {
                if year.is_none_or(|year| {
                    year.is_multiple_of(400)
                        || (year.is_multiple_of(4) && !year.is_multiple_of(100))
                }) {
                    29
                } else {
                    28
                }
            }
            Some(1 | 3 | 5 | 7 | 8 | 10 | 12) | None => 31,
            Some(_) => 0,
        }
    }

    pub fn year(self) -> Option<u16> {
        self.year
    }
    pub fn month(self) -> Option<u8> {
        self.month
    }
    pub fn day(self) -> Option<u8> {
        self.day
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameMetadataOpeningCode(String);

impl GameMetadataOpeningCode {
    pub fn new(value: String) -> Result<Self, GameMetadataError> {
        let bytes = value.as_bytes();
        if bytes.len() != 3
            || !(b'A'..=b'E').contains(&bytes[0])
            || !bytes[1..].iter().all(u8::is_ascii_digit)
        {
            return Err(GameMetadataError::InvalidOpeningCode);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameMetadataKnownTags {
    pub event: Option<GameMetadataText>,
    pub site: Option<GameMetadataText>,
    pub date: Option<GameMetadataDate>,
    pub round: Option<GameMetadataText>,
    pub white: Option<GameMetadataText>,
    pub black: Option<GameMetadataText>,
    pub white_elo: Option<u32>,
    pub black_elo: Option<u32>,
    pub opening: Option<GameMetadataText>,
    pub variation: Option<GameMetadataText>,
    pub eco: Option<GameMetadataOpeningCode>,
    pub annotator: Option<GameMetadataText>,
}

impl GameMetadataKnownTags {
    const RESERVED_NAMES: [&'static str; 17] = [
        "Event",
        "Site",
        "Date",
        "Round",
        "White",
        "Black",
        "WhiteElo",
        "BlackElo",
        "Opening",
        "Variation",
        "ECO",
        "Annotator",
        "Result",
        "FEN",
        "SetUp",
        "Variant",
        "TimeControl",
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameMetadataExtension {
    name: String,
    value: GameMetadataText,
}

impl GameMetadataExtension {
    pub fn new(name: String, value: GameMetadataText) -> Result<Self, GameMetadataError> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || GameMetadataKnownTags::RESERVED_NAMES.contains(&name.as_str())
        {
            return Err(GameMetadataError::InvalidExtensionName);
        }
        Ok(Self { name, value })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn value(&self) -> &GameMetadataText {
        &self.value
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameMetadata {
    known: GameMetadataKnownTags,
    extensions: Vec<GameMetadataExtension>,
}

impl GameMetadata {
    pub fn new(
        known: GameMetadataKnownTags,
        extensions: Vec<GameMetadataExtension>,
    ) -> Result<Self, GameMetadataError> {
        let mut names = HashSet::with_capacity(extensions.len());
        for extension in &extensions {
            if !names.insert(extension.name()) {
                return Err(GameMetadataError::DuplicateExtensionName);
            }
        }
        Ok(Self { known, extensions })
    }

    pub fn known(&self) -> &GameMetadataKnownTags {
        &self.known
    }
    pub fn extensions(&self) -> &[GameMetadataExtension] {
        &self.extensions
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameSeats {
    mode: GameEnumMode,
    white: GameEnumSeat,
    black: GameEnumSeat,
}

impl GameSeats {
    pub fn new(
        mode: GameEnumMode,
        white: GameEnumSeat,
        black: GameEnumSeat,
    ) -> Result<Self, GameSessionError> {
        let is_valid = match mode {
            GameEnumMode::HumanVersusEngine => white != black,
            GameEnumMode::LocalTwoPlayer => {
                white == GameEnumSeat::Human && black == GameEnumSeat::Human
            }
        };
        if !is_valid {
            return Err(GameSessionError::InvalidSeats);
        }
        Ok(Self { mode, white, black })
    }

    pub fn mode(self) -> GameEnumMode {
        self.mode
    }

    pub fn seat(self, color: BoardEnumColor) -> GameEnumSeat {
        match color {
            BoardEnumColor::White => self.white,
            BoardEnumColor::Black => self.black,
        }
    }

    fn validate_readiness(
        self,
        readiness: GameEnumEngineReadiness,
    ) -> Result<(), GameSessionError> {
        if self.mode == GameEnumMode::HumanVersusEngine
            && readiness != GameEnumEngineReadiness::Ready
        {
            return Err(GameSessionError::EngineUnavailable);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameSessionEnumInterruption {
    Adjudication {
        question: GameAdjudicationEnumMateQuestion,
        cause: GameAdjudicationEnumInterruption,
    },
    Paused(GamePauseEnumCause),
    RecoveryRequired(GamePauseEnumFault),
    Recovered(GamePauseEnumFault),
    PendingResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameSessionEnumState {
    Setup,
    Active,
    Interrupted(GameSessionEnumInterruption),
    Completed(GameOutcome),
    Analysis(Option<GameOutcome>),
}

impl GameSessionEnumState {
    fn from_decision(
        decision: GameAdjudicationEnumDecision,
        origin: GameMoveId,
    ) -> Result<Self, GameOutcomeError> {
        match decision {
            GameAdjudicationEnumDecision::Ongoing => Ok(Self::Active),
            GameAdjudicationEnumDecision::Complete { result, reason } => {
                Ok(Self::Completed(GameOutcome::new(result, reason, origin)?))
            }
            GameAdjudicationEnumDecision::Interrupted { question, cause } => Ok(Self::Interrupted(
                GameSessionEnumInterruption::Adjudication { question, cause },
            )),
        }
    }

    fn phase(&self) -> GameEnumPhase {
        match self {
            Self::Setup => GameEnumPhase::Setup,
            Self::Active => GameEnumPhase::Active,
            Self::Interrupted(_) => GameEnumPhase::Interrupted,
            Self::Completed(_) => GameEnumPhase::Completed,
            Self::Analysis(_) => GameEnumPhase::Analysis,
        }
    }

    fn outcome(&self) -> Option<GameOutcome> {
        match self {
            Self::Completed(outcome) | Self::Analysis(Some(outcome)) => Some(*outcome),
            Self::Setup | Self::Active | Self::Interrupted(_) | Self::Analysis(None) => None,
        }
    }

    fn interruption(&self) -> Option<GameSessionEnumInterruption> {
        match self {
            Self::Interrupted(interruption) => Some(*interruption),
            Self::Setup | Self::Active | Self::Completed(_) | Self::Analysis(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GameSessionOptions {
    pub id: GameId,
    pub root: GameMoveId,
    pub start: BoardStart,
    pub seats: GameSeats,
    pub clock: ClockState,
    pub metadata: GameMetadata,
}

#[derive(Debug, Clone)]
pub struct GameSessionRestoredMove {
    pub id: GameMoveId,
    pub parent: GameMoveId,
    pub movement: BoardMove,
    pub annotations: GameMoveNodeAnnotations,
    pub before_clock: Option<ClockStateSnapshot>,
    pub clock: Option<ClockStateSnapshot>,
}

#[derive(Debug, Clone)]
pub struct GameSessionRestoreOptions {
    pub session: GameSessionOptions,
    pub root_annotations: GameMoveNodeAnnotations,
    pub root_clock: Option<ClockStateSnapshot>,
    pub revision: GameRevision,
    pub live: GameMoveId,
    pub view: GameMoveId,
    pub study_mainline_tip: GameMoveId,
    pub moves: Vec<GameSessionRestoredMove>,
    pub clock: ClockStateSnapshot,
    pub warnings_used: [bool; 2],
    pub state: GameSessionEnumState,
    pub takeback_consent: GameTakebackEnumConsent,
    pub premoves_enabled: bool,
    pub engine_search_config: GameEngineSearchConfig,
    pub is_auto_flip_enabled: bool,
}

#[derive(Debug, Error)]
pub enum GameSessionRestoreError {
    #[error("Stored clock snapshots must match the game control, position and move counters")]
    ClockMismatch,
    #[error("An accepted move needs both before and after snapshots or neither")]
    UnpairedClocks,
    #[error("A restored move's after snapshot does not follow from its before snapshot")]
    RewrittenMoveClock,
    #[error("The stored game phase, result or clock status is inconsistent")]
    StateMismatch,
    #[error("The authoritative deadline must be adjudicated before capturing this game")]
    UnacceptedDeadline,
    #[error("Stored move counters exceed their representable capacity")]
    MoveOverflow,
    #[error("The recorded live line continues after an established terminal result")]
    MovesAfterResult,
}

#[derive(Clone, Copy)]
struct GameSessionMoveCounts([u64; 2]);

impl GameSessionMoveCounts {
    const ZERO: Self = Self([0; 2]);

    fn after(self, side: BoardEnumColor) -> Result<Self, GameSessionRestoreError> {
        let mut counts = self.0;
        let index = match side {
            BoardEnumColor::White => 0,
            BoardEnumColor::Black => 1,
        };
        counts[index] = counts[index]
            .checked_add(1)
            .ok_or(GameSessionRestoreError::MoveOverflow)?;
        Ok(Self(counts))
    }

    fn matches(self, clock: &ClockStateSnapshot) -> bool {
        self.0
            == [
                clock.completed_moves(BoardEnumColor::White),
                clock.completed_moves(BoardEnumColor::Black),
            ]
    }
}

impl GameSessionRestoredMove {
    fn validate_clocks(
        &self,
        initial: &ClockState,
        before_counts: GameSessionMoveCounts,
        after_counts: GameSessionMoveCounts,
    ) -> Result<(), GameSessionError> {
        let (before, after) = match (&self.before_clock, &self.clock) {
            (None, None) => return Ok(()),
            (Some(before), Some(after)) => (before, after),
            (Some(_), None) | (None, Some(_)) => {
                return Err(GameSessionRestoreError::UnpairedClocks.into());
            }
        };
        if before.control() != initial.control()
            || after.control() != initial.control()
            || !before_counts.matches(before)
            || !after_counts.matches(after)
            || before.status() != ClockStateEnumStatus::Running
            || before.active() == after.active()
        {
            return Err(GameSessionRestoreError::ClockMismatch.into());
        }
        let now = ClockInstant::from_nanos(0);
        let mut replayed = ClockState::recover(before, [false; 2])?;
        replayed.resume(now)?;
        if !matches!(
            replayed.complete_move(now)?,
            ClockStateEnumMoveOutcome::Accepted(_)
        ) {
            return Err(GameSessionRestoreError::RewrittenMoveClock.into());
        }
        match after.status() {
            ClockStateEnumStatus::Running => {}
            ClockStateEnumStatus::Paused => {
                replayed.pause(now)?;
            }
            ClockStateEnumStatus::Flagged(_) => {
                return Err(GameSessionRestoreError::RewrittenMoveClock.into());
            }
        }
        if replayed.snapshot(now)? != *after {
            return Err(GameSessionRestoreError::RewrittenMoveClock.into());
        }
        Ok(())
    }
}

pub struct GameSessionMoveRequest {
    pub expected_revision: GameRevision,
    pub seat: GameEnumSeat,
    pub color: BoardEnumColor,
    pub node: GameMoveId,
    pub movement: BoardMove,
    pub received_at: ClockInstant,
}

#[derive(Debug)]
pub struct GameSessionTransition {
    game: GameId,
    previous_revision: GameRevision,
    revision: GameRevision,
    kind: GameEnumTransitionKind,
    live: GameMoveId,
    phase: GameEnumPhase,
    outcome: Option<GameOutcome>,
    interruption: Option<GameSessionEnumInterruption>,
    clock: ClockStateSnapshot,
    before_clock: Option<ClockStateSnapshot>,
    accepted_node: Option<GameMoveNode>,
}

impl GameSessionTransition {
    pub fn game(&self) -> GameId {
        self.game
    }
    pub fn previous_revision(&self) -> GameRevision {
        self.previous_revision
    }
    pub fn revision(&self) -> GameRevision {
        self.revision
    }
    pub fn kind(&self) -> GameEnumTransitionKind {
        self.kind
    }
    pub fn live(&self) -> GameMoveId {
        self.live
    }
    pub fn phase(&self) -> GameEnumPhase {
        self.phase
    }
    pub fn outcome(&self) -> Option<GameOutcome> {
        self.outcome
    }
    pub fn interruption(&self) -> Option<GameSessionEnumInterruption> {
        self.interruption
    }
    pub fn clock(&self) -> &ClockStateSnapshot {
        &self.clock
    }
    pub fn before_clock(&self) -> Option<&ClockStateSnapshot> {
        self.before_clock.as_ref()
    }
    pub fn accepted_node(&self) -> Option<&GameMoveNode> {
        self.accepted_node.as_ref()
    }
}

#[derive(Debug, Error)]
pub enum GameSessionError {
    #[error("Game mode and player seats are inconsistent")]
    InvalidSeats,
    #[error(
        "The initial clock must be paused at the initial position's turn with no completed moves"
    )]
    InvalidInitialClock,
    #[error("Reckless must be ready before this game starts")]
    EngineUnavailable,
    #[error("This operation is unavailable during game phase {0:?}")]
    WrongPhase(GameEnumPhase),
    #[error("The request does not belong to the seat whose turn it is")]
    WrongSeat,
    #[error("This draw claim must be explicitly requested by the human seat")]
    HumanClaimRequired,
    #[error("The annotation confirmation belongs to game {received:?}, expected {expected:?}")]
    ForeignAnnotationConfirmation { expected: GameId, received: GameId },
    #[error("The lifecycle preparation belongs to game {received:?}, expected {expected:?}")]
    ForeignLifecyclePreparation { expected: GameId, received: GameId },
    #[error("Only a newly prepared terminal result can be deferred")]
    LifecycleResultRequired,
    #[error("Retry Save must accept the retained pending result before this operation")]
    PendingResult,
    #[error("A study edit cannot delete the played live path")]
    ProtectedLivePath,
    #[error(transparent)]
    Revision(#[from] GameRevisionError),
    #[error(transparent)]
    Clock(#[from] ClockStateError),
    #[error(transparent)]
    Tree(#[from] GameTreeError),
    #[error(transparent)]
    Adjudication(#[from] GameAdjudicationError),
    #[error(transparent)]
    Claim(#[from] GameDrawClaimError),
    #[error(transparent)]
    Outcome(#[from] GameOutcomeError),
    #[error(transparent)]
    Pause(#[from] GamePauseError),
    #[error(transparent)]
    Takeback(#[from] GameTakebackError),
    #[error(transparent)]
    Premove(#[from] GamePremoveError),
    #[error(transparent)]
    EngineRequest(#[from] GameEngineRequestError),
    #[error(transparent)]
    Restore(#[from] GameSessionRestoreError),
    #[error(transparent)]
    Result(#[from] GameResultError),
    #[error(transparent)]
    Save(#[from] GameSaveError),
}

struct GameSessionDecisionTransition {
    revision: GameRevision,
    kind: GameEnumTransitionKind,
    clock: ClockState,
    decision: GameAdjudicationEnumDecision,
    now: ClockInstant,
}

struct GameSessionStateTransition {
    revision: GameRevision,
    kind: GameEnumTransitionKind,
    clock: ClockState,
    state: GameSessionEnumState,
    now: ClockInstant,
}

pub struct GameSession {
    id: GameId,
    revision: GameRevision,
    start: BoardStart,
    seats: GameSeats,
    tree: GameTree,
    live: GameMoveId,
    view: GameMoveId,
    study_mainline_tip: GameMoveId,
    clock: ClockState,
    state: GameSessionEnumState,
    metadata: GameMetadata,
    history: GameAdjudicationHistory,
    premove: GamePremove,
    takeback: GameTakeback,
    save_status: GameSaveEnumStatus,
    saved_revision: GameRevision,
    engine_search_config: GameEngineSearchConfig,
    is_auto_flip_enabled: bool,
}

impl GameSession {
    pub fn new(options: GameSessionOptions) -> Result<Self, GameSessionError> {
        let snapshot = options.clock.snapshot(ClockInstant::from_nanos(0))?;
        if options.clock.active() != options.start.position().turn()
            || snapshot.status() != ClockStateEnumStatus::Paused
            || snapshot.elapsed() != ClockDuration::ZERO
            || snapshot.completed_moves(BoardEnumColor::White) != 0
            || snapshot.completed_moves(BoardEnumColor::Black) != 0
        {
            return Err(GameSessionError::InvalidInitialClock);
        }
        Ok(Self {
            id: options.id,
            revision: GameRevision::ZERO,
            history: GameAdjudicationHistory::new(options.start.position().clone()),
            tree: GameTree::new(
                options.root,
                options.start.position().clone(),
                Some(snapshot),
            ),
            live: options.root,
            view: options.root,
            study_mainline_tip: options.root,
            start: options.start,
            seats: options.seats,
            clock: options.clock,
            metadata: options.metadata,
            state: GameSessionEnumState::Setup,
            save_status: GameSaveEnumStatus::Current,
            saved_revision: GameRevision::ZERO,
            premove: GamePremove::default(),
            takeback: GameTakeback::default(),
            engine_search_config: GameEngineSearchConfig::initial_default(),
            is_auto_flip_enabled: false,
        })
    }

    pub fn id(&self) -> GameId {
        self.id
    }

    pub fn state(&self) -> GameSessionEnumState {
        self.state
    }

    fn validate_no_pending_result(&self) -> Result<(), GameSessionError> {
        if self.interruption() == Some(GameSessionEnumInterruption::PendingResult) {
            Err(GameSessionError::PendingResult)
        } else {
            Ok(())
        }
    }

    pub const fn engine_search_config(&self) -> GameEngineSearchConfig {
        self.engine_search_config
    }

    pub const fn is_auto_flip_enabled(&self) -> bool {
        self.is_auto_flip_enabled
    }

    pub fn configure_play_options(
        &mut self,
        expected: GameRevision,
        engine_search_config: GameEngineSearchConfig,
        is_auto_flip_enabled: bool,
    ) -> Result<GameRevision, GameSessionError> {
        self.revision.check_expected(expected)?;
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        if self.phase() != GameEnumPhase::Setup {
            return Err(GameSessionError::WrongPhase(self.phase()));
        }
        let revision = self.revision.next()?;
        self.engine_search_config = engine_search_config;
        self.is_auto_flip_enabled = is_auto_flip_enabled;
        self.revision = revision;
        Ok(revision)
    }

    pub fn save_status(&self) -> GameSaveEnumStatus {
        self.save_status
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.save_status == GameSaveEnumStatus::Required || self.revision > self.saved_revision
    }

    pub fn acknowledge_save(
        &mut self,
        receipt: GameStorageReceipt,
    ) -> Result<GameSaveEnumAcknowledgment, GameSessionError> {
        if receipt.game() != self.id {
            return Err(GameSaveError::ForeignGame {
                expected: self.id,
                received: receipt.game(),
            }
            .into());
        }
        if receipt.game_revision() > self.revision {
            return Err(GameSaveError::FutureRevision {
                current: self.revision,
                received: receipt.game_revision(),
            }
            .into());
        }
        if receipt.game_revision() < self.revision {
            return Ok(GameSaveEnumAcknowledgment::Older);
        }
        self.validate_no_pending_result()?;
        self.save_status = GameSaveEnumStatus::Current;
        self.saved_revision = receipt.game_revision();
        Ok(GameSaveEnumAcknowledgment::Current)
    }

    pub fn interrupt_for_save_failure<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        request: GameSaveFailureRequest,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.revision.check_expected(request.expected_revision)?;
        let transition = match self.state {
            GameSessionEnumState::Active
            | GameSessionEnumState::Interrupted(GameSessionEnumInterruption::PendingResult) => self
                .pause(
                    GamePauseRequest {
                        expected_revision: request.expected_revision,
                        cause: GamePauseEnumCause::AutosaveFailure,
                        received_at: request.received_at,
                        engine: request.engine,
                    },
                    proof,
                )?,
            GameSessionEnumState::Setup
            | GameSessionEnumState::Interrupted(_)
            | GameSessionEnumState::Completed(_)
            | GameSessionEnumState::Analysis(_) => {
                self.finish_state(GameSessionStateTransition {
                    revision: self.revision.next()?,
                    kind: GameEnumTransitionKind::Interrupted,
                    clock: self.clock.clone(),
                    state: self.state,
                    now: request.received_at,
                })?
            }
        };
        self.save_status = GameSaveEnumStatus::Required;
        Ok(transition)
    }

    pub fn restoration(
        &self,
        now: ClockInstant,
    ) -> Result<GameSessionRestoreOptions, GameSessionError> {
        self.validate_no_pending_result()?;
        let clock = self.clock.snapshot(now)?;
        if self.phase() == GameEnumPhase::Active && clock.status() != ClockStateEnumStatus::Running
        {
            return Err(GameSessionRestoreError::UnacceptedDeadline.into());
        }
        let root = self.tree.node(self.tree.root())?;
        let mut pending = vec![root.id()];
        let mut moves = Vec::with_capacity(self.tree.len().saturating_sub(1));
        while let Some(id) = pending.pop() {
            let node = self.tree.node(id)?;
            if let (Some(parent), Some(movement)) = (node.parent(), node.movement()) {
                moves.push(GameSessionRestoredMove {
                    id,
                    parent,
                    movement,
                    annotations: node.annotations().clone(),
                    before_clock: node.before_clock().cloned(),
                    clock: node.clock().cloned(),
                });
            }
            pending.extend(node.children().iter().rev().copied());
        }
        Ok(GameSessionRestoreOptions {
            session: GameSessionOptions {
                id: self.id,
                root: root.id(),
                start: self.start.clone(),
                seats: self.seats,
                clock: ClockState::new(self.clock.control().clone(), self.start.position().turn()),
                metadata: self.metadata.clone(),
            },
            root_annotations: root.annotations().clone(),
            root_clock: root.clock().cloned(),
            revision: self.revision,
            live: self.live,
            view: self.view,
            study_mainline_tip: self.study_mainline_tip,
            moves,
            clock,
            warnings_used: [
                self.clock.warning_used(BoardEnumColor::White),
                self.clock.warning_used(BoardEnumColor::Black),
            ],
            state: self.state,
            takeback_consent: self.takeback.consent(),
            premoves_enabled: self.premove.is_enabled(),
            engine_search_config: self.engine_search_config,
            is_auto_flip_enabled: self.is_auto_flip_enabled,
        })
    }

    pub fn restore(options: GameSessionRestoreOptions) -> Result<Self, GameSessionError> {
        if options.state.interruption() == Some(GameSessionEnumInterruption::PendingResult) {
            return Err(GameSessionError::PendingResult);
        }
        let mut session = Self::new(options.session)?;
        if options.clock.control() != session.clock.control() {
            return Err(GameSessionRestoreError::ClockMismatch.into());
        }
        if let Some(root_clock) = &options.root_clock
            && (root_clock.control() != session.clock.control()
                || root_clock.active() != session.start.position().turn()
                || root_clock.status() != ClockStateEnumStatus::Paused
                || root_clock.elapsed() != ClockDuration::ZERO
                || !GameSessionMoveCounts::ZERO.matches(root_clock))
        {
            return Err(GameSessionRestoreError::ClockMismatch.into());
        }
        session.tree = GameTree::new(
            session.tree.root(),
            session.start.position().clone(),
            options.root_clock,
        );
        session
            .tree
            .set_annotations(session.tree.root(), options.root_annotations)?;
        let mut counts = HashMap::with_capacity(options.moves.len().saturating_add(1));
        counts.insert(session.tree.root(), GameSessionMoveCounts::ZERO);
        for movement in options.moves {
            if counts.contains_key(&movement.id) {
                return Err(GameTreeError::DuplicateIdentity.into());
            }
            let before_counts = *counts
                .get(&movement.parent)
                .ok_or(GameTreeError::UnknownNode)?;
            let mover = session.tree.node(movement.parent)?.position().turn();
            let after_counts = before_counts.after(mover)?;
            movement.validate_clocks(&session.clock, before_counts, after_counts)?;
            if movement
                .before_clock
                .as_ref()
                .is_some_and(|clock| clock.active() != mover)
            {
                return Err(GameSessionRestoreError::ClockMismatch.into());
            }
            let clock = movement
                .before_clock
                .zip(movement.clock)
                .map(|(before, after)| GameTreeCandidateClock { before, after });
            session
                .tree
                .restore_move(movement.parent, movement.id, movement.movement, clock)?;
            session
                .tree
                .set_annotations(movement.id, movement.annotations)?;
            counts.insert(movement.id, after_counts);
        }
        let live_counts = *counts
            .get(&options.live)
            .ok_or(GameTreeError::UnknownNode)?;
        session.tree.node(options.view)?;
        session
            .tree
            .validate_study_tip(options.study_mainline_tip)?;
        session.live = options.live;
        session.view = options.view;
        session.study_mainline_tip = options.study_mainline_tip;
        session.history = session.restored_live_history()?;
        if options.clock.active() != session.history.position().turn()
            || !live_counts.matches(&options.clock)
        {
            return Err(GameSessionRestoreError::ClockMismatch.into());
        }
        if let Some(live_clock) = session.tree.node(session.live)?.clock()
            && live_clock.persisted().sides != options.clock.persisted().sides
        {
            return Err(GameSessionRestoreError::ClockMismatch.into());
        }
        session.validate_restored_state(options.state, &options.clock)?;
        if matches!(
            options.state,
            GameSessionEnumState::Active | GameSessionEnumState::Interrupted(_)
        ) {
            options.revision.next()?;
            for id in session.tree.path_to(session.live)? {
                let node = session.tree.node(id)?;
                if node.parent().is_some() && node.before_clock().is_none() {
                    return Err(GameSessionRestoreError::ClockMismatch.into());
                }
            }
        }
        session.state = match options.state {
            GameSessionEnumState::Active => GameSessionEnumState::Interrupted(
                GameSessionEnumInterruption::Paused(GamePauseEnumCause::Relaunch),
            ),
            state @ (GameSessionEnumState::Setup
            | GameSessionEnumState::Interrupted(_)
            | GameSessionEnumState::Completed(_)
            | GameSessionEnumState::Analysis(_)) => state,
        };
        session.clock = ClockState::recover(&options.clock, options.warnings_used)?;
        session.revision = options.revision;
        session.saved_revision = options.revision;
        session.takeback.configure(options.takeback_consent);
        session.premove.set_enabled(options.premoves_enabled);
        session.premove.clear(GamePremoveEnumClearReason::Relaunch);
        session.engine_search_config = options.engine_search_config;
        session.is_auto_flip_enabled = options.is_auto_flip_enabled;
        Ok(session)
    }

    fn restored_live_history(&self) -> Result<GameAdjudicationHistory, GameSessionError> {
        let mut history = GameAdjudicationHistory::new(self.start.position().clone());
        for movement in self.live_moves()? {
            if history.direct_terminal().is_some() {
                return Err(GameSessionRestoreError::MovesAfterResult.into());
            }
            let candidate = history.prepare(movement)?;
            history.commit(candidate)?;
        }
        Ok(history)
    }

    fn validate_restored_state(
        &self,
        state: GameSessionEnumState,
        clock: &ClockStateSnapshot,
    ) -> Result<(), GameSessionError> {
        let valid = match state {
            GameSessionEnumState::Interrupted(GameSessionEnumInterruption::PendingResult) => {
                return Err(GameSessionError::PendingResult);
            }
            GameSessionEnumState::Setup => {
                self.tree.len() == 1
                    && clock.status() == ClockStateEnumStatus::Paused
                    && clock.elapsed() == ClockDuration::ZERO
            }
            GameSessionEnumState::Active => {
                clock.status() == ClockStateEnumStatus::Running
                    && self.history.direct_terminal().is_none()
            }
            GameSessionEnumState::Interrupted(GameSessionEnumInterruption::Adjudication {
                question,
                ..
            }) => match (question, clock.status()) {
                (GameAdjudicationEnumMateQuestion::AnySide, ClockStateEnumStatus::Paused) => {
                    self.history.direct_terminal().is_none()
                }
                (
                    GameAdjudicationEnumMateQuestion::Side(side),
                    ClockStateEnumStatus::Flagged(flagged),
                ) => side == flagged.opposite() && self.history.direct_terminal().is_none(),
                (
                    GameAdjudicationEnumMateQuestion::AnySide,
                    ClockStateEnumStatus::Running | ClockStateEnumStatus::Flagged(_),
                )
                | (
                    GameAdjudicationEnumMateQuestion::Side(_),
                    ClockStateEnumStatus::Paused | ClockStateEnumStatus::Running,
                ) => false,
            },
            GameSessionEnumState::Interrupted(GameSessionEnumInterruption::Paused(
                GamePauseEnumCause::Fault(_),
            )) => false,
            GameSessionEnumState::Interrupted(
                GameSessionEnumInterruption::Paused(_)
                | GameSessionEnumInterruption::RecoveryRequired(_)
                | GameSessionEnumInterruption::Recovered(_),
            ) => {
                clock.status() == ClockStateEnumStatus::Paused
                    && self.history.direct_terminal().is_none()
            }
            GameSessionEnumState::Completed(outcome)
            | GameSessionEnumState::Analysis(Some(outcome)) => {
                return self.validate_restored_outcome(outcome, clock);
            }
            GameSessionEnumState::Analysis(None) => clock.status() == ClockStateEnumStatus::Paused,
        };
        if valid {
            Ok(())
        } else {
            Err(GameSessionRestoreError::StateMismatch.into())
        }
    }

    fn validate_restored_outcome(
        &self,
        outcome: GameOutcome,
        clock: &ClockStateSnapshot,
    ) -> Result<(), GameSessionError> {
        if outcome.origin() != self.live || clock.status() == ClockStateEnumStatus::Running {
            return Err(GameSessionRestoreError::StateMismatch.into());
        }
        if let Some(GameAdjudicationEnumDecision::Complete { result, reason }) =
            self.history.direct_terminal()
        {
            if result == outcome.result()
                && reason == outcome.reason()
                && clock.status() == ClockStateEnumStatus::Paused
            {
                return Ok(());
            }
            return Err(GameSessionRestoreError::StateMismatch.into());
        }
        let valid = match outcome.reason() {
            GameEnumOutcomeReason::Checkmate
            | GameEnumOutcomeReason::Stalemate
            | GameEnumOutcomeReason::FivefoldRepetition
            | GameEnumOutcomeReason::SeventyFiveMove => false,
            GameEnumOutcomeReason::DeadPosition => clock.status() == ClockStateEnumStatus::Paused,
            GameEnumOutcomeReason::Resignation => clock.status() != ClockStateEnumStatus::Running,
            GameEnumOutcomeReason::ConsensualAdjudication => {
                self.seats.mode() == GameEnumMode::LocalTwoPlayer
            }
            GameEnumOutcomeReason::Agreement => {
                self.seats.mode() == GameEnumMode::LocalTwoPlayer
                    && clock.status() == ClockStateEnumStatus::Paused
            }
            GameEnumOutcomeReason::ThreefoldRepetition => {
                clock.status() == ClockStateEnumStatus::Paused
                    && self.has_restored_claim(GameDrawClaimEnumKind::ThreefoldRepetition)?
            }
            GameEnumOutcomeReason::FiftyMove => {
                clock.status() == ClockStateEnumStatus::Paused
                    && self.has_restored_claim(GameDrawClaimEnumKind::FiftyMove)?
            }
            GameEnumOutcomeReason::Flagfall => match clock.status() {
                ClockStateEnumStatus::Flagged(flagged) => {
                    outcome.result() == GameEnumResult::Draw
                        || outcome.result() == GameEnumResult::Victory(flagged.opposite())
                }
                ClockStateEnumStatus::Paused | ClockStateEnumStatus::Running => false,
            },
        };
        if valid {
            Ok(())
        } else {
            Err(GameSessionRestoreError::StateMismatch.into())
        }
    }

    fn has_restored_claim(&self, kind: GameDrawClaimEnumKind) -> Result<bool, GameSessionError> {
        if self.history.claimable().contains(kind) {
            return Ok(true);
        }
        for movement in self.history.position().legal_moves() {
            if self.history.prepare(movement)?.claimable().contains(kind) {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn revision(&self) -> GameRevision {
        self.revision
    }
    pub fn start_position(&self) -> &BoardStart {
        &self.start
    }
    pub fn seats(&self) -> GameSeats {
        self.seats
    }
    pub fn tree(&self) -> &GameTree {
        &self.tree
    }
    pub fn live(&self) -> GameMoveId {
        self.live
    }
    pub fn view(&self) -> GameMoveId {
        self.view
    }
    pub const fn study_mainline_tip(&self) -> GameMoveId {
        self.study_mainline_tip
    }
    pub fn clock(&self) -> &ClockState {
        &self.clock
    }
    pub fn phase(&self) -> GameEnumPhase {
        self.state.phase()
    }
    pub fn outcome(&self) -> Option<GameOutcome> {
        self.state.outcome()
    }
    pub fn interruption(&self) -> Option<GameSessionEnumInterruption> {
        self.state.interruption()
    }
    pub fn metadata(&self) -> &GameMetadata {
        &self.metadata
    }

    pub fn premove(&self) -> &GamePremove {
        &self.premove
    }

    pub fn set_premoves_enabled(
        &mut self,
        expected: GameRevision,
        enabled: bool,
    ) -> Result<GameRevision, GameSessionError> {
        self.revision.check_expected(expected)?;
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        let revision = self.revision.next()?;
        self.premove.set_enabled(enabled);
        self.revision = revision;
        Ok(revision)
    }

    fn human_color(&self) -> Result<BoardEnumColor, GameTakebackError> {
        match self.seats.mode() {
            GameEnumMode::HumanVersusEngine => Ok(
                if self.seats.seat(BoardEnumColor::White) == GameEnumSeat::Human {
                    BoardEnumColor::White
                } else {
                    BoardEnumColor::Black
                },
            ),
            GameEnumMode::LocalTwoPlayer => Err(GameTakebackError::HumanRequired),
        }
    }

    pub fn queue_premove(
        &mut self,
        expected: GameRevision,
        intent: GamePremoveIntent,
    ) -> Result<Option<GamePremoveIntent>, GameSessionError> {
        self.revision.check_expected(expected)?;
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        if self.phase() != GameEnumPhase::Active {
            return Err(GameSessionError::WrongPhase(self.phase()));
        }
        let human = self.human_color()?;
        Ok(self
            .premove
            .queue(self.history.position(), self.seats.mode(), human, intent)?)
    }

    pub fn clear_premove(
        &mut self,
        expected: GameRevision,
        reason: GamePremoveEnumClearReason,
    ) -> Result<Option<GamePremoveIntent>, GameSessionError> {
        self.revision.check_expected(expected)?;
        Ok(self.premove.clear(reason))
    }

    pub fn resolve_premove(
        &mut self,
        expected: GameRevision,
    ) -> Result<GamePremoveEnumResolution, GameSessionError> {
        self.revision.check_expected(expected)?;
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        if self.phase() != GameEnumPhase::Active {
            return Err(GameSessionError::WrongPhase(self.phase()));
        }
        let human = self.human_color()?;
        Ok(self.premove.resolve(self.history.position(), human)?)
    }

    pub fn engine_request(
        &self,
        config: GameEngineSearchConfig,
    ) -> Result<GameEngineRequest, GameSessionError> {
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        if self.phase() != GameEnumPhase::Active {
            return Err(GameSessionError::WrongPhase(self.phase()));
        }
        if self.seats.seat(self.history.position().turn()) != GameEnumSeat::Reckless {
            return Err(GameSessionError::WrongSeat);
        }
        Ok(GameEngineRequest::new(
            GameEngineContext::new(self.id, self.revision),
            self.start.position().clone(),
            self.live_moves()?,
            self.clock.clone(),
            config,
        )?)
    }

    pub fn takeback(&self) -> &GameTakeback {
        &self.takeback
    }

    pub fn claimable(&self) -> GameDrawClaimEnumAvailability {
        self.history.claimable()
    }

    fn validate_result_phase(&self) -> Result<(), GameSessionError> {
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        match self.phase() {
            GameEnumPhase::Active | GameEnumPhase::Interrupted => Ok(()),
            GameEnumPhase::Setup | GameEnumPhase::Completed | GameEnumPhase::Analysis => {
                Err(GameSessionError::WrongPhase(self.phase()))
            }
        }
    }

    fn finish_result<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        requested: GameAdjudicationEnumDecision,
        now: ClockInstant,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        let revision = self.revision.next()?;
        let mut clock = self.clock.clone();
        let update = if clock.is_running() {
            clock.pause(now)?
        } else {
            clock.poll(now)?
        };
        let (kind, decision) = match (self.phase(), update.view().status()) {
            (GameEnumPhase::Active, ClockStateEnumStatus::Flagged(flagged)) => (
                GameEnumTransitionKind::FlagFall,
                self.history.flag_fall(flagged, proof),
            ),
            (
                GameEnumPhase::Active,
                ClockStateEnumStatus::Paused | ClockStateEnumStatus::Running,
            )
            | (
                GameEnumPhase::Interrupted,
                ClockStateEnumStatus::Paused
                | ClockStateEnumStatus::Running
                | ClockStateEnumStatus::Flagged(_),
            ) => (GameEnumTransitionKind::ResultAccepted, requested),
            (
                GameEnumPhase::Setup | GameEnumPhase::Completed | GameEnumPhase::Analysis,
                ClockStateEnumStatus::Paused
                | ClockStateEnumStatus::Running
                | ClockStateEnumStatus::Flagged(_),
            ) => return Err(GameSessionError::WrongPhase(self.phase())),
        };
        self.finish_decision(GameSessionDecisionTransition {
            revision,
            kind,
            clock,
            decision,
            now,
        })
    }

    pub fn resign<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        request: GameResultResignRequest,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.revision.check_expected(request.expected_revision)?;
        self.validate_result_phase()?;
        if self.seats.seat(request.resigning) != GameEnumSeat::Human {
            return Err(GameSessionError::WrongSeat);
        }
        request
            .engine
            .validate(self.id, self.revision, self.seats.mode(), None)?;
        self.finish_result(
            self.history.resign(request.resigning),
            request.received_at,
            proof,
        )
    }

    pub fn finish_local<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        request: GameResultLocalRequest,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.revision.check_expected(request.expected_revision)?;
        self.validate_result_phase()?;
        if self.seats.mode() != GameEnumMode::LocalTwoPlayer {
            return Err(GameResultError::LocalPlayersRequired.into());
        }
        request
            .consent
            .validate(self.id, self.revision, self.live)?;
        let decision = match request.consent.kind() {
            GameResultEnumLocalKind::DrawAgreement => {
                if matches!(self.clock.status(), ClockStateEnumStatus::Flagged(_)) {
                    return Err(GameResultError::ConsensualResultRequired.into());
                }
                GameAdjudicationEnumDecision::Complete {
                    result: GameEnumResult::Draw,
                    reason: GameEnumOutcomeReason::Agreement,
                }
            }
            GameResultEnumLocalKind::ConsensualAdjudication(result) => {
                if !matches!(
                    self.interruption(),
                    Some(GameSessionEnumInterruption::Adjudication { .. })
                ) {
                    return Err(GameResultError::AdjudicationInterruptionRequired.into());
                }
                GameAdjudicationEnumDecision::Complete {
                    result,
                    reason: GameEnumOutcomeReason::ConsensualAdjudication,
                }
            }
        };
        self.finish_result(decision, request.received_at, proof)
    }

    pub fn configure_takeback_consent(
        &mut self,
        expected: GameRevision,
        consent: GameTakebackEnumConsent,
    ) -> Result<GameRevision, GameSessionError> {
        self.revision.check_expected(expected)?;
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        match self.phase() {
            GameEnumPhase::Setup | GameEnumPhase::Active | GameEnumPhase::Interrupted => {}
            GameEnumPhase::Completed | GameEnumPhase::Analysis => {
                return Err(GameSessionError::WrongPhase(self.phase()));
            }
        }
        let revision = self.revision.next()?;
        self.takeback.configure(consent);
        self.revision = revision;
        Ok(revision)
    }

    pub(crate) fn validate_recovery(
        &self,
        expected: GameRevision,
        fault: GamePauseEnumFault,
    ) -> Result<(), GameSessionError> {
        self.revision.check_expected(expected)?;
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        if self.interruption() != Some(GameSessionEnumInterruption::RecoveryRequired(fault)) {
            return Err(GamePauseError::WrongRecovery.into());
        }
        Ok(())
    }

    pub fn mark_recovered(
        &mut self,
        expected: GameRevision,
        fault: GamePauseEnumFault,
        now: ClockInstant,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.validate_recovery(expected, fault)?;
        let revision = self.revision.next()?;
        self.finish_state(GameSessionStateTransition {
            revision,
            kind: GameEnumTransitionKind::RecoveryAccepted,
            clock: self.clock.clone(),
            state: GameSessionEnumState::Interrupted(GameSessionEnumInterruption::Recovered(fault)),
            now,
        })
    }

    pub(crate) fn validate_resume(&self, expected: GameRevision) -> Result<(), GameSessionError> {
        self.revision.check_expected(expected)?;
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        let Some(interruption) = self.interruption() else {
            return Err(GameSessionError::WrongPhase(self.phase()));
        };
        if let GameSessionEnumInterruption::RecoveryRequired(fault) = interruption {
            return Err(GamePauseError::RecoveryRequired(fault).into());
        }
        Ok(())
    }

    pub fn resume<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        request: GameResumeRequest,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.validate_resume(request.expected_revision)?;
        self.seats.validate_readiness(request.readiness)?;
        let revision = self.revision.next()?;
        let decision = match self.clock.status() {
            ClockStateEnumStatus::Flagged(flagged) => self.history.flag_fall(flagged, proof),
            ClockStateEnumStatus::Paused => self.claim_for_reckless(
                self.history.adjudicate(proof),
                self.history.claimable(),
                self.history.position().turn(),
            ),
            ClockStateEnumStatus::Running => {
                return Err(GameSessionError::WrongPhase(self.phase()));
            }
        };
        let state = GameSessionEnumState::from_decision(decision, self.live)?;
        let mut clock = self.clock.clone();
        if state.phase() == GameEnumPhase::Active {
            clock.resume(request.received_at)?;
        }
        self.finish_state(GameSessionStateTransition {
            revision,
            kind: GameEnumTransitionKind::Resumed,
            clock,
            state,
            now: request.received_at,
        })
    }

    fn validate_takeback_phase(&self) -> Result<(), GameSessionError> {
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        match self.state {
            GameSessionEnumState::Active => Ok(()),
            GameSessionEnumState::Interrupted(
                GameSessionEnumInterruption::Paused(_) | GameSessionEnumInterruption::Recovered(_),
            ) => Ok(()),
            GameSessionEnumState::Interrupted(GameSessionEnumInterruption::RecoveryRequired(
                fault,
            )) => Err(GamePauseError::RecoveryRequired(fault).into()),
            GameSessionEnumState::Setup
            | GameSessionEnumState::Interrupted(GameSessionEnumInterruption::PendingResult)
            | GameSessionEnumState::Interrupted(GameSessionEnumInterruption::Adjudication {
                ..
            })
            | GameSessionEnumState::Completed(_)
            | GameSessionEnumState::Analysis(_) => Err(GameSessionError::WrongPhase(self.phase())),
        }
    }

    pub fn takeback_target(&self) -> Result<GameMoveId, GameSessionError> {
        self.validate_takeback_phase()?;
        let mut parent = self.tree.node(self.live)?.parent();
        while let Some(id) = parent {
            let node = self.tree.node(id)?;
            if self.seats.mode() == GameEnumMode::LocalTwoPlayer
                || self.seats.seat(node.position().turn()) == GameEnumSeat::Human
            {
                return Ok(id);
            }
            parent = node.parent();
        }
        Err(GameTakebackError::NoTarget.into())
    }

    fn takeback_clock(&self, target: GameMoveId) -> Result<ClockStateSnapshot, GameSessionError> {
        let path = self.tree.path_to(self.live)?;
        let index = path
            .iter()
            .position(|id| *id == target)
            .and_then(|index| index.checked_add(1))
            .ok_or(GameTakebackError::NoTarget)?;
        let undone = path.get(index).ok_or(GameTakebackError::NoTarget)?;
        self.tree
            .node(*undone)?
            .before_clock()
            .cloned()
            .ok_or_else(|| GameTakebackError::MissingClockSnapshot.into())
    }

    fn observed_takeback_clock<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        now: ClockInstant,
        proof: &P,
    ) -> Result<Result<ClockState, GameSessionTransition>, GameSessionError> {
        let mut clock = self.clock.clone();
        let update = clock.poll(now)?;
        if let ClockStateEnumStatus::Flagged(flagged) = update.view().status() {
            let revision = self.revision.next()?;
            return Ok(Err(self.finish_decision(
                GameSessionDecisionTransition {
                    revision,
                    kind: GameEnumTransitionKind::FlagFall,
                    clock,
                    decision: self.history.flag_fall(flagged, proof),
                    now,
                },
            )?));
        }
        Ok(Ok(clock))
    }

    pub fn request_takeback<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        request: GameTakebackRequest,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.revision.check_expected(request.expected_revision)?;
        self.validate_takeback_phase()?;
        if self.seats.seat(request.requester) != GameEnumSeat::Human {
            return Err(GameTakebackError::HumanRequired.into());
        }
        if self.takeback.pending().is_some() {
            return Err(GameTakebackError::AlreadyPending.into());
        }
        let target = self.takeback_target()?;
        request
            .engine
            .validate(self.id, self.revision, self.seats.mode(), None)?;
        let clock = match self.observed_takeback_clock(request.received_at, proof)? {
            Ok(clock) => clock,
            Err(transition) => return Ok(transition),
        };
        let revision = self.revision.next()?;
        if self.seats.mode() == GameEnumMode::LocalTwoPlayer
            && self.takeback.consent() == GameTakebackEnumConsent::Required
        {
            let transition = self.finish_state(GameSessionStateTransition {
                revision,
                kind: GameEnumTransitionKind::TakebackRequested,
                clock,
                state: self.state,
                now: request.received_at,
            })?;
            self.takeback.request(GameTakebackPending::new(
                request.requester,
                self.live,
                target,
                revision,
            ));
            return Ok(transition);
        }
        self.apply_takeback(target, clock, request.received_at)
    }

    pub fn respond_takeback<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        request: GameTakebackResponseRequest,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.revision.check_expected(request.expected_revision)?;
        self.validate_takeback_phase()?;
        let pending = self
            .takeback
            .pending()
            .ok_or(GameTakebackError::NotPending)?;
        if pending.revision() != self.revision || pending.live() != self.live {
            return Err(GameTakebackError::StaleRequest.into());
        }
        pending.validate_response(request.responder, request.response)?;
        let clock = match self.observed_takeback_clock(request.received_at, proof)? {
            Ok(clock) => clock,
            Err(transition) => return Ok(transition),
        };
        match request.response {
            GameTakebackEnumResponse::Accept => {
                self.apply_takeback(pending.target(), clock, request.received_at)
            }
            GameTakebackEnumResponse::Decline | GameTakebackEnumResponse::Cancel => {
                let kind = match request.response {
                    GameTakebackEnumResponse::Decline => GameEnumTransitionKind::TakebackDeclined,
                    GameTakebackEnumResponse::Cancel => GameEnumTransitionKind::TakebackCancelled,
                    GameTakebackEnumResponse::Accept => {
                        return Err(GameTakebackError::StaleRequest.into());
                    }
                };
                self.finish_state(GameSessionStateTransition {
                    revision: self.revision.next()?,
                    kind,
                    clock,
                    state: self.state,
                    now: request.received_at,
                })
            }
        }
    }

    fn apply_takeback(
        &mut self,
        target: GameMoveId,
        mut clock: ClockState,
        now: ClockInstant,
    ) -> Result<GameSessionTransition, GameSessionError> {
        let revision = self.revision.next()?;
        let before = self.takeback_clock(target)?;
        let movements = self
            .tree
            .path_to(target)?
            .into_iter()
            .filter_map(|id| self.tree.node(id).map(|node| node.movement()).transpose())
            .collect::<Result<Vec<_>, _>>()?;
        let history =
            GameAdjudicationHistory::from_moves(self.start.position().clone(), movements)?;
        clock.restore(&before)?;
        if self.phase() == GameEnumPhase::Active {
            clock.resume(now)?;
        }
        let snapshot = clock.snapshot(now)?;
        let mut transition = self.transition(
            revision,
            GameEnumTransitionKind::TakebackAccepted,
            &self.state,
            snapshot,
        );
        transition.live = target;
        transition.before_clock = Some(before);
        self.history = history;
        self.live = target;
        self.view = target;
        self.clock = clock;
        self.revision = revision;
        self.takeback.clear();
        self.premove.clear(GamePremoveEnumClearReason::Takeback);
        Ok(transition)
    }

    pub fn live_moves(&self) -> Result<Vec<BoardMove>, GameSessionError> {
        self.tree
            .path_to(self.live)?
            .into_iter()
            .filter_map(|id| self.tree.node(id).map(|node| node.movement()).transpose())
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn select_view(&mut self, node: GameMoveId) -> Result<(), GameSessionError> {
        self.tree.node(node)?;
        self.view = node;
        Ok(())
    }

    pub fn return_to_live(&mut self) {
        self.view = self.live;
    }

    pub fn save_notes_and_nags(
        &mut self,
        expected: GameRevision,
        node: GameMoveId,
        notes: String,
        nags: Vec<GameMoveNodeNag>,
    ) -> Result<GameRevision, GameSessionError> {
        self.validate_study_edit(expected)?;
        let prepared = self.tree.prepare_notes_and_nags(node, notes, nags)?;
        let revision = Self::annotation_revision(self.revision, prepared.change())?;
        prepared.commit();
        self.revision = revision;
        Ok(revision)
    }

    pub fn save_markings(
        &mut self,
        expected: GameRevision,
        node: GameMoveId,
        markings: Vec<GameMoveNodeMarking>,
    ) -> Result<GameRevision, GameSessionError> {
        self.validate_study_edit(expected)?;
        let prepared = self.tree.prepare_markings(node, markings)?;
        let revision = Self::annotation_revision(self.revision, prepared.change())?;
        prepared.commit();
        self.revision = revision;
        Ok(revision)
    }

    pub fn preview_variation(
        &self,
        expected: GameRevision,
        request: GameAnnotationsEnumVariationRequest,
    ) -> Result<GameAnnotationsSessionPreview, GameSessionError> {
        self.validate_study_edit(expected)?;
        let preview = self
            .tree
            .preview_variation(request, self.study_mainline_tip)?;
        self.validate_variation_policy(&preview)?;
        Ok(GameAnnotationsSessionPreview::new(
            self.id,
            self.revision,
            preview,
        ))
    }

    pub fn apply_variation(
        &mut self,
        confirmation: GameAnnotationsSessionConfirmation,
    ) -> Result<GameRevision, GameSessionError> {
        if confirmation.game() != self.id {
            return Err(GameSessionError::ForeignAnnotationConfirmation {
                expected: self.id,
                received: confirmation.game(),
            });
        }
        self.validate_study_edit(confirmation.revision())?;
        self.validate_variation_policy(confirmation.variation().preview())?;
        let prepared = self.tree.prepare_variation(
            confirmation.variation(),
            self.study_mainline_tip,
            self.view,
        )?;
        let revision = Self::annotation_revision(self.revision, prepared.change())?;
        let tip = prepared.tip();
        let view = prepared.view();
        prepared.commit();
        self.study_mainline_tip = tip;
        self.view = view;
        self.revision = revision;
        Ok(revision)
    }

    fn annotation_revision(
        revision: GameRevision,
        change: GameAnnotationsEnumChange,
    ) -> Result<GameRevision, GameRevisionError> {
        match change {
            GameAnnotationsEnumChange::Unchanged => Ok(revision),
            GameAnnotationsEnumChange::Changed => revision.next(),
        }
    }

    fn validate_variation_policy(
        &self,
        preview: &GameAnnotationsVariationPreview,
    ) -> Result<(), GameSessionError> {
        if preview.deleted_nodes().contains(&self.live) {
            return Err(GameSessionError::ProtectedLivePath);
        }
        if preview.parent() != Some(preview.resulting_tip()) {
            self.tree.validate_study_tip(preview.resulting_tip())?;
        }
        Ok(())
    }

    fn validate_study_edit(&self, expected: GameRevision) -> Result<(), GameSessionError> {
        self.revision.check_expected(expected)?;
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        match self.state {
            GameSessionEnumState::Completed(_) | GameSessionEnumState::Analysis(_) => Ok(()),
            GameSessionEnumState::Setup
            | GameSessionEnumState::Active
            | GameSessionEnumState::Interrupted(_) => {
                Err(GameSessionError::WrongPhase(self.phase()))
            }
        }
    }

    pub fn append_study_variation(
        &mut self,
        expected: GameRevision,
        node: GameMoveId,
        movement: BoardMove,
    ) -> Result<GameAnnotationsVariationApplied, GameSessionError> {
        self.validate_study_edit(expected)?;
        let revision = self.revision.next()?;
        let advances_study = self.view == self.study_mainline_tip;
        let insertion = self.tree.prepare_study(self.view, node, movement)?;
        let study_mainline_tip = if advances_study {
            insertion.ordered_leaf()?
        } else {
            self.study_mainline_tip
        };
        let change = if insertion.insertion_change() == GameAnnotationsEnumChange::Changed
            || study_mainline_tip != self.study_mainline_tip
        {
            GameAnnotationsEnumChange::Changed
        } else {
            GameAnnotationsEnumChange::Unchanged
        };
        let inserted = insertion.commit().id();
        self.view = inserted;
        self.study_mainline_tip = study_mainline_tip;
        self.revision = revision;
        Ok(GameAnnotationsVariationApplied::new(
            change,
            study_mainline_tip,
            inserted,
        ))
    }

    pub fn start<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        expected_revision: GameRevision,
        now: ClockInstant,
        readiness: GameEnumEngineReadiness,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.revision.check_expected(expected_revision)?;
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        if self.phase() != GameEnumPhase::Setup {
            return Err(GameSessionError::WrongPhase(self.phase()));
        }
        self.seats.validate_readiness(readiness)?;
        let revision = self.revision.next()?;
        let decision = self.claim_for_reckless(
            self.history.adjudicate(proof),
            self.history.claimable(),
            self.history.position().turn(),
        );
        let state = GameSessionEnumState::from_decision(decision, self.live)?;
        let mut clock = self.clock.clone();
        if state.phase() == GameEnumPhase::Active {
            clock.resume(now)?;
        }
        let snapshot = clock.snapshot(now)?;
        let transition =
            self.transition(revision, GameEnumTransitionKind::Started, &state, snapshot);
        self.clock = clock;
        self.state = state;
        self.revision = revision;
        Ok(transition)
    }

    pub fn accept_move<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        request: GameSessionMoveRequest,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.validate_turn(request.expected_revision, request.seat, request.color)?;
        let revision = self.revision.next()?;
        let mut clock = self.clock.clone();
        let receipt = match clock.complete_move(request.received_at)? {
            ClockStateEnumMoveOutcome::Accepted(receipt) => receipt,
            ClockStateEnumMoveOutcome::Expired(_) => {
                let decision = self.history.flag_fall(request.color, proof);
                return self.finish_decision(GameSessionDecisionTransition {
                    revision,
                    kind: GameEnumTransitionKind::FlagFall,
                    clock,
                    decision,
                    now: request.received_at,
                });
            }
        };
        let candidate = self.history.prepare(request.movement)?;
        if request.seat == GameEnumSeat::Reckless {
            let available = self
                .history
                .claimable()
                .preferred()
                .or_else(|| candidate.claimable().preferred());
            if let Some(kind) = available {
                let mut claim_clock = self.clock.clone();
                claim_clock.pause(request.received_at)?;
                return self.finish_decision(GameSessionDecisionTransition {
                    revision,
                    kind: GameEnumTransitionKind::ResultAccepted,
                    clock: claim_clock,
                    decision: GameAdjudicationEnumDecision::Complete {
                        result: GameEnumResult::Draw,
                        reason: kind.reason(),
                    },
                    now: request.received_at,
                });
            }
        }
        let decision = self.claim_for_reckless(
            candidate.adjudicate(proof),
            candidate.claimable(),
            candidate.position().turn(),
        );
        let origin = request.node;
        let mut state = GameSessionEnumState::from_decision(decision, origin)?;
        if state.phase() != GameEnumPhase::Active {
            clock.pause(request.received_at)?;
        }
        let snapshot = clock.snapshot(request.received_at)?;
        let insertion = self.tree.prepare_candidate(
            self.live,
            request.node,
            &candidate,
            GameTreeCandidateClock {
                before: receipt.before().clone(),
                after: snapshot.clone(),
            },
        )?;
        let accepted_id = insertion.id();
        if accepted_id != origin {
            state = GameSessionEnumState::from_decision(decision, accepted_id)?;
        }
        let study_mainline_tip = insertion.ordered_leaf()?;
        self.history.commit(candidate)?;
        let node = insertion.commit();
        let follow_live = self.view == self.live;
        let previous_revision = self.revision;
        self.live = node.id();
        self.study_mainline_tip = study_mainline_tip;
        if follow_live {
            self.view = self.live;
        }
        self.clock = clock;
        self.state = state;
        self.revision = revision;
        self.takeback.clear();
        if self.phase() != GameEnumPhase::Active {
            self.premove.clear(GamePremoveEnumClearReason::Fault);
        } else if request.seat == GameEnumSeat::Human {
            self.premove.clear(GamePremoveEnumClearReason::Explicit);
        }
        Ok(GameSessionTransition {
            game: self.id,
            previous_revision,
            revision,
            kind: GameEnumTransitionKind::MoveAccepted,
            live: self.live,
            phase: self.state.phase(),
            outcome: self.state.outcome(),
            interruption: self.state.interruption(),
            clock: snapshot,
            before_clock: Some(receipt.before().clone()),
            accepted_node: Some(node),
        })
    }

    fn validate_turn(
        &self,
        expected: GameRevision,
        seat: GameEnumSeat,
        color: BoardEnumColor,
    ) -> Result<(), GameSessionError> {
        self.revision.check_expected(expected)?;
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        if self.phase() != GameEnumPhase::Active {
            return Err(GameSessionError::WrongPhase(self.phase()));
        }
        if color != self.history.position().turn() || self.seats.seat(color) != seat {
            return Err(GameSessionError::WrongSeat);
        }
        Ok(())
    }

    fn claim_for_reckless(
        &self,
        decision: GameAdjudicationEnumDecision,
        available: GameDrawClaimEnumAvailability,
        turn: BoardEnumColor,
    ) -> GameAdjudicationEnumDecision {
        if decision == GameAdjudicationEnumDecision::Ongoing
            && self.seats.seat(turn) == GameEnumSeat::Reckless
            && let Some(kind) = available.preferred()
        {
            return GameAdjudicationEnumDecision::Complete {
                result: GameEnumResult::Draw,
                reason: kind.reason(),
            };
        }
        decision
    }

    pub fn claim_draw<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        request: GameSessionDrawClaimRequest,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.validate_turn(
            request.expected_revision,
            GameEnumSeat::Human,
            request.color,
        )?;
        let revision = self.revision.next()?;
        let mut clock = self.clock.clone();
        let update = clock.poll(request.received_at)?;
        let decision = match update.view().status() {
            ClockStateEnumStatus::Flagged(flagged) => self.history.flag_fall(flagged, proof),
            ClockStateEnumStatus::Running => {
                let decision = self
                    .history
                    .claim(request.color, request.kind, request.intent)?;
                clock.pause(request.received_at)?;
                decision
            }
            ClockStateEnumStatus::Paused => return Err(GameSessionError::WrongPhase(self.phase())),
        };
        self.finish_decision(GameSessionDecisionTransition {
            revision,
            kind: GameEnumTransitionKind::ResultAccepted,
            clock,
            decision,
            now: request.received_at,
        })
    }

    fn finish_decision(
        &mut self,
        transition: GameSessionDecisionTransition,
    ) -> Result<GameSessionTransition, GameSessionError> {
        let GameSessionDecisionTransition {
            revision,
            kind,
            clock,
            decision,
            now,
        } = transition;
        let state = GameSessionEnumState::from_decision(decision, self.live)?;
        self.finish_state(GameSessionStateTransition {
            revision,
            kind,
            clock,
            state,
            now,
        })
    }

    fn finish_state(
        &mut self,
        transition: GameSessionStateTransition,
    ) -> Result<GameSessionTransition, GameSessionError> {
        let GameSessionStateTransition {
            revision,
            kind,
            clock,
            state,
            now,
        } = transition;
        let snapshot = clock.snapshot(now)?;
        let transition = self.transition(revision, kind, &state, snapshot);
        Ok(self.accept_state(transition, clock, state))
    }

    fn accept_state(
        &mut self,
        transition: GameSessionTransition,
        clock: ClockState,
        state: GameSessionEnumState,
    ) -> GameSessionTransition {
        self.clock = clock;
        self.state = state;
        self.revision = transition.revision;
        self.takeback.clear();
        self.premove.clear(GamePremoveEnumClearReason::Fault);
        transition
    }

    fn transition(
        &self,
        revision: GameRevision,
        kind: GameEnumTransitionKind,
        state: &GameSessionEnumState,
        clock: ClockStateSnapshot,
    ) -> GameSessionTransition {
        GameSessionTransition {
            game: self.id,
            previous_revision: self.revision,
            revision,
            kind,
            live: self.live,
            phase: state.phase(),
            outcome: state.outcome(),
            interruption: state.interruption(),
            clock,
            before_clock: None,
            accepted_node: None,
        }
    }

    pub fn begin_analysis(
        &mut self,
        expected_revision: GameRevision,
        now: ClockInstant,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.revision.check_expected(expected_revision)?;
        self.validate_no_pending_result()?;
        self.save_status.validate()?;
        match self.phase() {
            GameEnumPhase::Setup | GameEnumPhase::Completed | GameEnumPhase::Analysis => {}
            GameEnumPhase::Active | GameEnumPhase::Interrupted => {
                return Err(GameSessionError::WrongPhase(self.phase()));
            }
        }
        let revision = self.revision.next()?;
        let state = GameSessionEnumState::Analysis(self.outcome());
        let snapshot = self.clock.snapshot(now)?;
        let transition = self.transition(
            revision,
            GameEnumTransitionKind::AnalysisStarted,
            &state,
            snapshot,
        );
        self.state = state;
        self.revision = revision;
        Ok(transition)
    }
}

pub struct GameSessionDrawClaimRequest {
    pub expected_revision: GameRevision,
    pub color: BoardEnumColor,
    pub kind: GameDrawClaimEnumKind,
    pub intent: GameDrawClaimEnumIntent,
    pub received_at: ClockInstant,
}

#[derive(Debug)]
pub struct GameSessionClockPoll {
    update: ClockStateUpdate,
    transition: Option<GameSessionTransition>,
}

impl GameSessionClockPoll {
    pub fn update(&self) -> &ClockStateUpdate {
        &self.update
    }
    pub fn transition(&self) -> Option<&GameSessionTransition> {
        self.transition.as_ref()
    }
    pub fn into_transition(self) -> Option<GameSessionTransition> {
        self.transition
    }
}

#[cfg(test)]
#[path = "../../tests/game/session_lifecycle.rs"]
mod lifecycle_tests;
