use std::collections::HashSet;

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
use crate::game::draw_claim::{
    GameDrawClaimEnumAvailability, GameDrawClaimEnumIntent, GameDrawClaimEnumKind,
    GameDrawClaimError,
};
use crate::game::engine_provider::{
    GameEngineContext, GameEngineRequest, GameEngineRequestError, GameEngineSearchConfig,
};
use crate::game::enums::{
    GameEnumEngineReadiness, GameEnumMode, GameEnumPhase, GameEnumResult, GameEnumSeat,
    GameEnumTransitionKind, GameId, GameMoveId, GameRevision, GameRevisionError,
};
use crate::game::move_node::GameMoveNode;
use crate::game::outcome::{GameOutcome, GameOutcomeError};
use crate::game::pause::{
    GamePauseEnumCause, GamePauseEnumFault, GamePauseError, GamePauseRequest, GameResumeRequest,
};
use crate::game::premove::{
    GamePremove, GamePremoveEnumClearReason, GamePremoveEnumResolution, GamePremoveError,
    GamePremoveIntent,
};
use crate::game::takeback::{
    GameTakeback, GameTakebackEnumConsent, GameTakebackEnumResponse, GameTakebackError,
    GameTakebackPending, GameTakebackRequest, GameTakebackResponseRequest,
};
use crate::game::tree::{GameTree, GameTreeCandidateClock, GameTreeError};

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
}

#[derive(Debug, Clone, Copy)]
enum GameSessionEnumState {
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

pub struct GameSessionOptions {
    pub id: GameId,
    pub root: GameMoveId,
    pub start: BoardStart,
    pub seats: GameSeats,
    pub clock: ClockState,
    pub metadata: GameMetadata,
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
    clock: ClockState,
    state: GameSessionEnumState,
    metadata: GameMetadata,
    history: GameAdjudicationHistory,
    premove: GamePremove,
    takeback: GameTakeback,
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
            start: options.start,
            seats: options.seats,
            clock: options.clock,
            metadata: options.metadata,
            state: GameSessionEnumState::Setup,
            premove: GamePremove::default(),
            takeback: GameTakeback::default(),
        })
    }

    pub fn id(&self) -> GameId {
        self.id
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

    pub fn set_premoves_enabled(&mut self, enabled: bool) {
        self.premove.set_enabled(enabled);
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

    pub fn configure_takeback_consent(
        &mut self,
        expected: GameRevision,
        consent: GameTakebackEnumConsent,
    ) -> Result<GameRevision, GameSessionError> {
        self.revision.check_expected(expected)?;
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

    pub fn pause<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        request: GamePauseRequest,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.revision.check_expected(request.expected_revision)?;
        if self.phase() != GameEnumPhase::Active {
            return Err(GameSessionError::WrongPhase(self.phase()));
        }
        request.engine.validate(
            self.id,
            self.revision,
            self.seats.mode(),
            Some(request.cause),
        )?;
        let revision = self.revision.next()?;
        let mut clock = self.clock.clone();
        let update = clock.pause(request.received_at)?;
        if let ClockStateEnumStatus::Flagged(flagged) = update.view().status() {
            return self.finish_decision(GameSessionDecisionTransition {
                revision,
                kind: GameEnumTransitionKind::FlagFall,
                clock,
                decision: self.history.flag_fall(flagged, proof),
                now: request.received_at,
            });
        }
        let interruption = match request.cause {
            GamePauseEnumCause::Fault(fault) => {
                GameSessionEnumInterruption::RecoveryRequired(fault)
            }
            cause @ (GamePauseEnumCause::Manual
            | GamePauseEnumCause::ComputerSleep
            | GamePauseEnumCause::Relaunch
            | GamePauseEnumCause::TabClosed
            | GamePauseEnumCause::ApplicationQuit
            | GamePauseEnumCause::UpdateInstall) => GameSessionEnumInterruption::Paused(cause),
        };
        self.finish_state(GameSessionStateTransition {
            revision,
            kind: GameEnumTransitionKind::Interrupted,
            clock,
            state: GameSessionEnumState::Interrupted(interruption),
            now: request.received_at,
        })
    }

    pub fn mark_recovered(
        &mut self,
        expected: GameRevision,
        fault: GamePauseEnumFault,
        now: ClockInstant,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.revision.check_expected(expected)?;
        if self.interruption() != Some(GameSessionEnumInterruption::RecoveryRequired(fault)) {
            return Err(GamePauseError::WrongRecovery.into());
        }
        let revision = self.revision.next()?;
        self.finish_state(GameSessionStateTransition {
            revision,
            kind: GameEnumTransitionKind::RecoveryAccepted,
            clock: self.clock.clone(),
            state: GameSessionEnumState::Interrupted(GameSessionEnumInterruption::Recovered(fault)),
            now,
        })
    }

    pub fn resume<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        request: GameResumeRequest,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.revision.check_expected(request.expected_revision)?;
        let Some(interruption) = self.interruption() else {
            return Err(GameSessionError::WrongPhase(self.phase()));
        };
        if let GameSessionEnumInterruption::RecoveryRequired(fault) = interruption {
            return Err(GamePauseError::RecoveryRequired(fault).into());
        }
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
        match self.state {
            GameSessionEnumState::Active => Ok(()),
            GameSessionEnumState::Interrupted(
                GameSessionEnumInterruption::Paused(_) | GameSessionEnumInterruption::Recovered(_),
            ) => Ok(()),
            GameSessionEnumState::Interrupted(GameSessionEnumInterruption::RecoveryRequired(
                fault,
            )) => Err(GamePauseError::RecoveryRequired(fault).into()),
            GameSessionEnumState::Setup
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

    pub fn start<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        expected_revision: GameRevision,
        now: ClockInstant,
        readiness: GameEnumEngineReadiness,
        proof: &P,
    ) -> Result<GameSessionTransition, GameSessionError> {
        self.revision.check_expected(expected_revision)?;
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
        self.history.commit(candidate)?;
        let node = insertion.commit();
        let follow_live = self.view == self.live;
        let previous_revision = self.revision;
        self.live = node.id();
        if follow_live {
            self.view = self.live;
        }
        self.clock = clock;
        self.state = state;
        self.revision = revision;
        self.takeback.clear();
        if self.phase() != GameEnumPhase::Active {
            self.premove.clear(GamePremoveEnumClearReason::Fault);
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
        self.clock = clock;
        self.state = state;
        self.revision = revision;
        self.takeback.clear();
        self.premove.clear(GamePremoveEnumClearReason::Fault);
        Ok(transition)
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

    pub fn poll_deadline<P: GameAdjudicationMateProofProvider + ?Sized>(
        &mut self,
        expected_revision: GameRevision,
        now: ClockInstant,
        proof: &P,
    ) -> Result<GameSessionClockPoll, GameSessionError> {
        self.revision.check_expected(expected_revision)?;
        if self.phase() != GameEnumPhase::Active {
            return Err(GameSessionError::WrongPhase(self.phase()));
        }
        let mut clock = self.clock.clone();
        let update = clock.poll(now)?;
        let transition = match update.view().status() {
            ClockStateEnumStatus::Flagged(flagged) => {
                let revision = self.revision.next()?;
                let decision = self.history.flag_fall(flagged, proof);
                Some(self.finish_decision(GameSessionDecisionTransition {
                    revision,
                    kind: GameEnumTransitionKind::FlagFall,
                    clock,
                    decision,
                    now,
                })?)
            }
            ClockStateEnumStatus::Running => {
                self.clock = clock;
                None
            }
            ClockStateEnumStatus::Paused => return Err(GameSessionError::WrongPhase(self.phase())),
        };
        Ok(GameSessionClockPoll { update, transition })
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
