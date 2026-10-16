use std::collections::HashMap;
use std::fmt::{Formatter, Result as FormatResult};
use std::io::{Error as IoError, Write};
use std::num::NonZeroU32;

use serde::de::{Error as DeserializeError, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;
use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::board::r#move::BoardMoveError;
use crate::board::square::BoardSquare;
use crate::board::start::BoardStart;
use crate::capacity::executor::{
    CapacityExecutor, CapacityExecutorCancellation, CapacityExecutorError,
};
use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::clock::stage::{ClockStage, ClockStageEnumBonus, ClockStageEnumQuota};
use crate::clock::state::{
    ClockState, ClockStateEnumStatus, ClockStateError, ClockStateSideOptions, ClockStateSnapshot,
    ClockStateSnapshotOptions,
};
use crate::clock::time_control::{ClockTimeControl, ClockTimeControlError};
use crate::game::adjudication::{
    GameAdjudicationEnumInterruption, GameAdjudicationEnumMateQuestion,
};
use crate::game::configuration::GameConfigurationOptions;
use crate::game::engine_provider::{GameEngineRequestError, GameEngineSearchConfig};
use crate::game::enums::{
    GameEnumMode, GameEnumOutcomeReason, GameEnumResult, GameEnumSeat, GameId, GameMoveId,
    GameRevision,
};
use crate::game::move_node::{
    GameMoveNodeAnnotationError, GameMoveNodeAnnotations, GameMoveNodeMarking, GameMoveNodeNag,
};
use crate::game::outcome::{GameOutcome, GameOutcomeError};
use crate::game::pause::{GamePauseEnumCause, GamePauseEnumFault};
use crate::game::session::{
    GameMetadata, GameMetadataDate, GameMetadataError, GameMetadataExtension,
    GameMetadataKnownTags, GameMetadataOpeningCode, GameMetadataText, GameSeats, GameSession,
    GameSessionEnumInterruption, GameSessionEnumState, GameSessionError, GameSessionOptions,
    GameSessionRestoreOptions, GameSessionRestoredMove,
};
use crate::game::storage_repository::{
    GameStorageReceipt, GameStorageRecovered, GameStorageRepository, GameStorageRevision,
    GameStorageSaveRequest,
};
use crate::game::takeback::GameTakebackEnumConsent;
use crate::game::tree::GameTreeError;
use crate::storage::document_store::{
    StorageDocumentClient, StorageDocumentError, StorageDocumentStore,
};

#[derive(Debug, Error)]
pub enum StorageGameDocumentError {
    #[error("Game schema {found} is newer than supported schema {supported}")]
    FutureSchema { found: u32, supported: u32 },
    #[error("Game schema {0} has no supported migration")]
    UnsupportedSchema(u32),
    #[error("The game document exceeds the document-store byte budget")]
    TooLarge,
    #[error("A game document cannot contain more than 100000 move nodes")]
    TooManyMoves,
    #[error("Game notes and metadata fields cannot exceed one MiB")]
    TextTooLarge,
    #[error("The saved game identity does not match its database key")]
    Identity,
    #[error("The saved game has no required per-game play options")]
    MissingPlayOptions,
    #[error("Game serialization or recovery was cancelled")]
    Cancelled,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error(transparent)]
    Store(#[from] StorageDocumentError),
    #[error(transparent)]
    Executor(#[from] CapacityExecutorError),
    #[error(transparent)]
    Session(#[from] GameSessionError),
    #[error(transparent)]
    EngineRequest(#[from] GameEngineRequestError),
    #[error(transparent)]
    Clock(#[from] ClockStateError),
    #[error(transparent)]
    TimeControl(#[from] ClockTimeControlError),
    #[error(transparent)]
    Stage(#[from] crate::clock::stage::ClockStageError),
    #[error(transparent)]
    Move(#[from] BoardMoveError),
    #[error(transparent)]
    Tree(#[from] GameTreeError),
    #[error(transparent)]
    Annotation(#[from] GameMoveNodeAnnotationError),
    #[error(transparent)]
    Metadata(#[from] GameMetadataError),
    #[error(transparent)]
    Outcome(#[from] GameOutcomeError),
}

impl StorageGameDocumentError {
    fn check_cancellation(cancellation: Option<&CapacityExecutorCancellation>) -> Result<(), Self> {
        if cancellation.is_some_and(CapacityExecutorCancellation::is_cancelled) {
            Err(Self::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentEnvelope<'a> {
    schema_version: u32,
    #[serde(borrow)]
    game: &'a RawValue,
}

#[derive(Serialize)]
struct StorageGameDocumentEncoded<'a> {
    schema_version: u32,
    game: &'a StorageGameDocumentRecord,
}

pub struct StorageGameDocument {
    game: StorageGameDocumentRecord,
}

impl StorageGameDocument {
    pub const SCHEMA_VERSION: u32 = 2;
    const LEGACY_SCHEMA_VERSION: u32 = 1;
    pub const MAX_MOVES: usize = 100_000;
    const MAX_TEXT_BYTES: usize = 1024 * 1024;

    pub fn capture(
        session: &GameSession,
        now: ClockInstant,
    ) -> Result<Self, StorageGameDocumentError> {
        Self::from_snapshot(session.restoration(now)?, None)
    }

    fn from_snapshot(
        snapshot: GameSessionRestoreOptions,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<Self, StorageGameDocumentError> {
        StorageGameDocumentError::check_cancellation(cancellation)?;
        Ok(Self {
            game: StorageGameDocumentRecord::from_snapshot(snapshot, cancellation)?,
        })
    }

    pub fn json(&self) -> Result<String, StorageGameDocumentError> {
        self.encode(None)
    }

    fn encode(
        &self,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<String, StorageGameDocumentError> {
        let mut writer = StorageGameDocumentWriter {
            bytes: Vec::new(),
            failure: None,
            cancellation,
        };
        let result = serde_json::to_writer(
            &mut writer,
            &StorageGameDocumentEncoded {
                schema_version: Self::SCHEMA_VERSION,
                game: &self.game,
            },
        );
        if let Some(failure) = writer.failure {
            return Err(failure.into_error());
        }
        result?;
        Ok(String::from_utf8(writer.bytes)?)
    }

    pub fn restore(json: &str) -> Result<GameSession, StorageGameDocumentError> {
        Self::recover(json, None)
    }

    fn recover(
        json: &str,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<GameSession, StorageGameDocumentError> {
        if json.len() > StorageDocumentStore::MEMORY_BUDGET {
            return Err(StorageGameDocumentError::TooLarge);
        }
        StorageGameDocumentError::check_cancellation(cancellation)?;
        let envelope: StorageGameDocumentEnvelope<'_> = serde_json::from_str(json)?;
        if envelope.schema_version > Self::SCHEMA_VERSION {
            return Err(StorageGameDocumentError::FutureSchema {
                found: envelope.schema_version,
                supported: Self::SCHEMA_VERSION,
            });
        }
        if envelope.schema_version != Self::SCHEMA_VERSION
            && envelope.schema_version != Self::LEGACY_SCHEMA_VERSION
        {
            return Err(StorageGameDocumentError::UnsupportedSchema(
                envelope.schema_version,
            ));
        }
        let record: StorageGameDocumentRecord = serde_json::from_str(envelope.game.get())?;
        record.restore(envelope.schema_version, cancellation)
    }
}

enum StorageGameDocumentEnumWriteFailure {
    Size,
    Cancelled,
}

impl StorageGameDocumentEnumWriteFailure {
    fn into_error(self) -> StorageGameDocumentError {
        match self {
            Self::Size => StorageGameDocumentError::TooLarge,
            Self::Cancelled => StorageGameDocumentError::Cancelled,
        }
    }
}

struct StorageGameDocumentWriter<'a> {
    bytes: Vec<u8>,
    failure: Option<StorageGameDocumentEnumWriteFailure>,
    cancellation: Option<&'a CapacityExecutorCancellation>,
}

impl Write for StorageGameDocumentWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let failure = if self
            .cancellation
            .is_some_and(CapacityExecutorCancellation::is_cancelled)
        {
            Some(StorageGameDocumentEnumWriteFailure::Cancelled)
        } else if bytes.len() > StorageDocumentStore::MEMORY_BUDGET.saturating_sub(self.bytes.len())
        {
            Some(StorageGameDocumentEnumWriteFailure::Size)
        } else {
            None
        };
        if let Some(failure) = failure {
            self.failure = Some(failure);
            return Err(IoError::other("Game document writing was refused"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "ClockStateEnumStatus", rename_all = "snake_case")]
enum StorageGameDocumentEnumClockStatus {
    Paused,
    Running,
    Flagged(BoardEnumColor),
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "ClockStageEnumQuota", rename_all = "snake_case")]
enum StorageGameDocumentEnumQuota {
    Moves(NonZeroU32),
    Unlimited,
    Cyclic(NonZeroU32),
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "GamePauseEnumFault", rename_all = "snake_case")]
enum StorageGameDocumentEnumFault {
    Engine,
    Graphics,
    Save,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "GamePauseEnumCause", rename_all = "snake_case")]
enum StorageGameDocumentEnumPauseCause {
    Manual,
    ComputerSleep,
    Relaunch,
    TabClosed,
    ApplicationQuit,
    UpdateInstall,
    AutosaveFailure,
    Fault(#[serde(with = "StorageGameDocumentEnumFault")] GamePauseEnumFault),
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "GameAdjudicationEnumMateQuestion", rename_all = "snake_case")]
enum StorageGameDocumentEnumMateQuestion {
    AnySide,
    Side(BoardEnumColor),
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "GameAdjudicationEnumInterruption", rename_all = "snake_case")]
enum StorageGameDocumentEnumProofInterruption {
    TimeLimit,
    StateLimit,
    MemoryLimit,
    Cancelled,
    Unresolved,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "GameSessionEnumInterruption", rename_all = "snake_case")]
enum StorageGameDocumentEnumInterruption {
    Adjudication {
        #[serde(with = "StorageGameDocumentEnumMateQuestion")]
        question: GameAdjudicationEnumMateQuestion,
        #[serde(with = "StorageGameDocumentEnumProofInterruption")]
        cause: GameAdjudicationEnumInterruption,
    },
    Paused(#[serde(with = "StorageGameDocumentEnumPauseCause")] GamePauseEnumCause),
    RecoveryRequired(#[serde(with = "StorageGameDocumentEnumFault")] GamePauseEnumFault),
    Recovered(#[serde(with = "StorageGameDocumentEnumFault")] GamePauseEnumFault),
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "GameTakebackEnumConsent", rename_all = "snake_case")]
enum StorageGameDocumentEnumConsent {
    Required,
    NotRequired,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StorageGameDocumentEnumBonus {
    None,
    Fischer(u64),
    SimpleDelay(u64),
    BronsteinDelay(u64),
}

impl From<ClockStageEnumBonus> for StorageGameDocumentEnumBonus {
    fn from(bonus: ClockStageEnumBonus) -> Self {
        match bonus {
            ClockStageEnumBonus::None => Self::None,
            ClockStageEnumBonus::Fischer(duration) => Self::Fischer(duration.as_nanos()),
            ClockStageEnumBonus::SimpleDelay(duration) => Self::SimpleDelay(duration.as_nanos()),
            ClockStageEnumBonus::BronsteinDelay(duration) => {
                Self::BronsteinDelay(duration.as_nanos())
            }
        }
    }
}

impl From<StorageGameDocumentEnumBonus> for ClockStageEnumBonus {
    fn from(bonus: StorageGameDocumentEnumBonus) -> Self {
        match bonus {
            StorageGameDocumentEnumBonus::None => Self::None,
            StorageGameDocumentEnumBonus::Fischer(nanos) => {
                Self::Fischer(ClockDuration::from_nanos(nanos))
            }
            StorageGameDocumentEnumBonus::SimpleDelay(nanos) => {
                Self::SimpleDelay(ClockDuration::from_nanos(nanos))
            }
            StorageGameDocumentEnumBonus::BronsteinDelay(nanos) => {
                Self::BronsteinDelay(ClockDuration::from_nanos(nanos))
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentStage {
    #[serde(with = "StorageGameDocumentEnumQuota")]
    quota: ClockStageEnumQuota,
    base_nanos: u64,
    bonus: StorageGameDocumentEnumBonus,
}

impl From<ClockStage> for StorageGameDocumentStage {
    fn from(stage: ClockStage) -> Self {
        Self {
            quota: stage.quota(),
            base_nanos: stage.base_time().as_nanos(),
            bonus: stage.bonus().into(),
        }
    }
}

impl StorageGameDocumentStage {
    fn into_stage(self) -> Result<ClockStage, StorageGameDocumentError> {
        Ok(ClockStage::new(
            self.quota,
            ClockDuration::from_nanos(self.base_nanos),
            self.bonus.into(),
        )?)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentClockSide {
    remaining_nanos: u64,
    completed_moves: u64,
    stage_index: usize,
    stage_moves: u64,
}

impl From<ClockStateSideOptions> for StorageGameDocumentClockSide {
    fn from(side: ClockStateSideOptions) -> Self {
        Self {
            remaining_nanos: side.remaining.as_nanos(),
            completed_moves: side.completed_moves,
            stage_index: side.stage_index,
            stage_moves: side.stage_moves,
        }
    }
}

impl From<StorageGameDocumentClockSide> for ClockStateSideOptions {
    fn from(side: StorageGameDocumentClockSide) -> Self {
        Self {
            remaining: ClockDuration::from_nanos(side.remaining_nanos),
            completed_moves: side.completed_moves,
            stage_index: side.stage_index,
            stage_moves: side.stage_moves,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentClock {
    sides: [StorageGameDocumentClockSide; 2],
    active: BoardEnumColor,
    elapsed_nanos: u64,
    #[serde(with = "StorageGameDocumentEnumClockStatus")]
    status: ClockStateEnumStatus,
}

impl StorageGameDocumentClock {
    fn capture(
        clock: &ClockStateSnapshot,
        control: &ClockTimeControl,
    ) -> Result<Self, StorageGameDocumentError> {
        if clock.control() != control {
            return Err(ClockStateError::DifferentControl.into());
        }
        let clock = clock.persisted();
        Ok(Self {
            sides: clock.sides.map(StorageGameDocumentClockSide::from),
            active: clock.active,
            elapsed_nanos: clock.elapsed.as_nanos(),
            status: clock.status,
        })
    }

    fn restore(
        self,
        control: &ClockTimeControl,
    ) -> Result<ClockStateSnapshot, StorageGameDocumentError> {
        Ok(ClockStateSnapshot::from_persisted(
            ClockStateSnapshotOptions {
                control: control.clone(),
                sides: self.sides.map(ClockStateSideOptions::from),
                active: self.active,
                elapsed: ClockDuration::from_nanos(self.elapsed_nanos),
                status: self.status,
            },
        )?)
    }
}

#[derive(Serialize)]
#[serde(transparent)]
struct StorageGameDocumentText(String);

impl StorageGameDocumentText {
    fn copy(value: &str) -> Result<Self, StorageGameDocumentError> {
        if value.len() > StorageGameDocument::MAX_TEXT_BYTES {
            return Err(StorageGameDocumentError::TextTooLarge);
        }
        Ok(Self(value.to_owned()))
    }
}

impl<'de> Deserialize<'de> for StorageGameDocumentText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl TryFrom<String> for StorageGameDocumentText {
    type Error = StorageGameDocumentError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() > StorageGameDocument::MAX_TEXT_BYTES {
            return Err(StorageGameDocumentError::TextTooLarge);
        }
        Ok(Self(value))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentMarking {
    color: crate::game::enums::GameEnumMarkingColor,
    origin: BoardSquare,
    destination: Option<BoardSquare>,
}

impl From<GameMoveNodeMarking> for StorageGameDocumentMarking {
    fn from(marking: GameMoveNodeMarking) -> Self {
        Self {
            color: marking.color(),
            origin: marking.origin(),
            destination: marking.destination(),
        }
    }
}

impl StorageGameDocumentMarking {
    fn into_marking(self) -> Result<GameMoveNodeMarking, StorageGameDocumentError> {
        Ok(match self.destination {
            Some(destination) => GameMoveNodeMarking::arrow(self.color, self.origin, destination)?,
            None => GameMoveNodeMarking::circle(self.color, self.origin),
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentAnnotations {
    notes: StorageGameDocumentText,
    nags: Vec<u8>,
    markings: Vec<StorageGameDocumentMarking>,
}

impl StorageGameDocumentAnnotations {
    fn capture(annotations: &GameMoveNodeAnnotations) -> Result<Self, StorageGameDocumentError> {
        Ok(Self {
            notes: StorageGameDocumentText::copy(annotations.notes())?,
            nags: annotations.nags().iter().map(|nag| nag.value()).collect(),
            markings: annotations
                .markings()
                .iter()
                .copied()
                .map(StorageGameDocumentMarking::from)
                .collect(),
        })
    }

    fn restore(self) -> Result<GameMoveNodeAnnotations, StorageGameDocumentError> {
        Ok(GameMoveNodeAnnotations::new(
            self.notes.0,
            self.nags.into_iter().map(GameMoveNodeNag::new).collect(),
            self.markings
                .into_iter()
                .map(StorageGameDocumentMarking::into_marking)
                .collect::<Result<_, _>>()?,
        )?)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentMove {
    id: GameMoveId,
    parent: GameMoveId,
    uci: String,
    annotations: StorageGameDocumentAnnotations,
    before_clock: Option<StorageGameDocumentClock>,
    clock: Option<StorageGameDocumentClock>,
}

#[derive(Serialize)]
#[serde(transparent)]
struct StorageGameDocumentMoves(Vec<StorageGameDocumentMove>);

struct StorageGameDocumentMovesVisitor;

impl<'de> Visitor<'de> for StorageGameDocumentMovesVisitor {
    type Value = StorageGameDocumentMoves;
    fn expecting(&self, formatter: &mut Formatter<'_>) -> FormatResult {
        formatter.write_str("a bounded game move-node sequence")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        if sequence
            .size_hint()
            .is_some_and(|length| length > StorageGameDocument::MAX_MOVES)
        {
            return Err(A::Error::custom(StorageGameDocumentError::TooManyMoves));
        }
        let mut moves = Vec::new();
        while moves.len() < StorageGameDocument::MAX_MOVES {
            match sequence.next_element()? {
                Some(movement) => moves.push(movement),
                None => return Ok(StorageGameDocumentMoves(moves)),
            }
        }
        if sequence.next_element::<IgnoredAny>()?.is_some() {
            return Err(A::Error::custom(StorageGameDocumentError::TooManyMoves));
        }
        Ok(StorageGameDocumentMoves(moves))
    }
}

impl<'de> Deserialize<'de> for StorageGameDocumentMoves {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_seq(StorageGameDocumentMovesVisitor)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentDate {
    year: Option<u16>,
    month: Option<u8>,
    day: Option<u8>,
}

impl From<GameMetadataDate> for StorageGameDocumentDate {
    fn from(date: GameMetadataDate) -> Self {
        Self {
            year: date.year(),
            month: date.month(),
            day: date.day(),
        }
    }
}

impl StorageGameDocumentDate {
    fn restore(self) -> Result<GameMetadataDate, StorageGameDocumentError> {
        Ok(GameMetadataDate::new(self.year, self.month, self.day)?)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentExtension {
    name: StorageGameDocumentText,
    value: StorageGameDocumentText,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentMetadata {
    event: Option<StorageGameDocumentText>,
    site: Option<StorageGameDocumentText>,
    date: Option<StorageGameDocumentDate>,
    round: Option<StorageGameDocumentText>,
    white: Option<StorageGameDocumentText>,
    black: Option<StorageGameDocumentText>,
    white_elo: Option<u32>,
    black_elo: Option<u32>,
    opening: Option<StorageGameDocumentText>,
    variation: Option<StorageGameDocumentText>,
    eco: Option<StorageGameDocumentText>,
    annotator: Option<StorageGameDocumentText>,
    extensions: Vec<StorageGameDocumentExtension>,
}

impl StorageGameDocumentMetadata {
    fn text(
        value: &Option<GameMetadataText>,
    ) -> Result<Option<StorageGameDocumentText>, StorageGameDocumentError> {
        value
            .as_ref()
            .map(|text| StorageGameDocumentText::copy(text.as_str()))
            .transpose()
    }
    fn restored_text(
        value: Option<StorageGameDocumentText>,
    ) -> Result<Option<GameMetadataText>, StorageGameDocumentError> {
        value
            .map(|text| GameMetadataText::new(text.0).map_err(StorageGameDocumentError::Metadata))
            .transpose()
    }
    fn capture(metadata: &GameMetadata) -> Result<Self, StorageGameDocumentError> {
        let known = metadata.known();
        Ok(Self {
            event: Self::text(&known.event)?,
            site: Self::text(&known.site)?,
            date: known.date.map(StorageGameDocumentDate::from),
            round: Self::text(&known.round)?,
            white: Self::text(&known.white)?,
            black: Self::text(&known.black)?,
            white_elo: known.white_elo,
            black_elo: known.black_elo,
            opening: Self::text(&known.opening)?,
            variation: Self::text(&known.variation)?,
            eco: known
                .eco
                .as_ref()
                .map(|code| StorageGameDocumentText::copy(code.as_str()))
                .transpose()?,
            annotator: Self::text(&known.annotator)?,
            extensions: metadata
                .extensions()
                .iter()
                .map(|extension| {
                    Ok(StorageGameDocumentExtension {
                        name: StorageGameDocumentText::copy(extension.name())?,
                        value: StorageGameDocumentText::copy(extension.value().as_str())?,
                    })
                })
                .collect::<Result<_, StorageGameDocumentError>>()?,
        })
    }
    fn restore(self) -> Result<GameMetadata, StorageGameDocumentError> {
        let known = GameMetadataKnownTags {
            event: Self::restored_text(self.event)?,
            site: Self::restored_text(self.site)?,
            date: self
                .date
                .map(StorageGameDocumentDate::restore)
                .transpose()?,
            round: Self::restored_text(self.round)?,
            white: Self::restored_text(self.white)?,
            black: Self::restored_text(self.black)?,
            white_elo: self.white_elo,
            black_elo: self.black_elo,
            opening: Self::restored_text(self.opening)?,
            variation: Self::restored_text(self.variation)?,
            eco: self
                .eco
                .map(|code| GameMetadataOpeningCode::new(code.0))
                .transpose()?,
            annotator: Self::restored_text(self.annotator)?,
        };
        let extensions = self
            .extensions
            .into_iter()
            .map(|extension| {
                GameMetadataExtension::new(
                    extension.name.0,
                    GameMetadataText::new(extension.value.0)?,
                )
            })
            .collect::<Result<_, GameMetadataError>>()?;
        Ok(GameMetadata::new(known, extensions)?)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentOutcome {
    result: GameEnumResult,
    reason: GameEnumOutcomeReason,
    origin: GameMoveId,
}

impl From<GameOutcome> for StorageGameDocumentOutcome {
    fn from(outcome: GameOutcome) -> Self {
        Self {
            result: outcome.result(),
            reason: outcome.reason(),
            origin: outcome.origin(),
        }
    }
}

impl StorageGameDocumentOutcome {
    fn restore(self) -> Result<GameOutcome, StorageGameDocumentError> {
        Ok(GameOutcome::new(self.result, self.reason, self.origin)?)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StorageGameDocumentEnumState {
    Setup,
    Active,
    Interrupted(#[serde(with = "StorageGameDocumentEnumInterruption")] GameSessionEnumInterruption),
    Completed(StorageGameDocumentOutcome),
    Analysis(Option<StorageGameDocumentOutcome>),
}

impl From<GameSessionEnumState> for StorageGameDocumentEnumState {
    fn from(state: GameSessionEnumState) -> Self {
        match state {
            GameSessionEnumState::Setup => Self::Setup,
            GameSessionEnumState::Active => Self::Active,
            GameSessionEnumState::Interrupted(interruption) => Self::Interrupted(interruption),
            GameSessionEnumState::Completed(outcome) => Self::Completed(outcome.into()),
            GameSessionEnumState::Analysis(outcome) => {
                Self::Analysis(outcome.map(StorageGameDocumentOutcome::from))
            }
        }
    }
}

impl StorageGameDocumentEnumState {
    fn restore(self) -> Result<GameSessionEnumState, StorageGameDocumentError> {
        Ok(match self {
            Self::Setup => GameSessionEnumState::Setup,
            Self::Active => GameSessionEnumState::Active,
            Self::Interrupted(interruption) => GameSessionEnumState::Interrupted(interruption),
            Self::Completed(outcome) => GameSessionEnumState::Completed(outcome.restore()?),
            Self::Analysis(outcome) => GameSessionEnumState::Analysis(
                outcome
                    .map(StorageGameDocumentOutcome::restore)
                    .transpose()?,
            ),
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentPlayOptions {
    untimed_move_time_nanos: u64,
    move_overhead_nanos: u64,
    is_auto_flip_enabled: bool,
}

impl StorageGameDocumentPlayOptions {
    fn capture(engine_search: GameEngineSearchConfig, is_auto_flip_enabled: bool) -> Self {
        Self {
            untimed_move_time_nanos: engine_search.untimed_move_time().as_nanos(),
            move_overhead_nanos: engine_search.move_overhead().as_nanos(),
            is_auto_flip_enabled,
        }
    }

    fn initial_default() -> Self {
        let options = GameConfigurationOptions::initial_default();
        Self::capture(options.engine_search(), options.is_auto_flip_enabled())
    }

    fn engine_search_config(&self) -> Result<GameEngineSearchConfig, GameEngineRequestError> {
        GameEngineSearchConfig::new(
            ClockDuration::from_nanos(self.untimed_move_time_nanos),
            ClockDuration::from_nanos(self.move_overhead_nanos),
        )
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentRecord {
    id: GameId,
    revision: GameRevision,
    root: GameMoveId,
    start: BoardStart,
    mode: GameEnumMode,
    white: GameEnumSeat,
    black: GameEnumSeat,
    time_control: Vec<StorageGameDocumentStage>,
    root_annotations: StorageGameDocumentAnnotations,
    root_clock: Option<StorageGameDocumentClock>,
    moves: StorageGameDocumentMoves,
    live: GameMoveId,
    view: GameMoveId,
    clock: StorageGameDocumentClock,
    warnings_used: [bool; 2],
    state: StorageGameDocumentEnumState,
    metadata: StorageGameDocumentMetadata,
    #[serde(with = "StorageGameDocumentEnumConsent")]
    takeback_consent: GameTakebackEnumConsent,
    premoves_enabled: bool,
    #[serde(default)]
    play_options: Option<StorageGameDocumentPlayOptions>,
}

impl StorageGameDocumentRecord {
    fn from_snapshot(
        snapshot: GameSessionRestoreOptions,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<Self, StorageGameDocumentError> {
        if snapshot.moves.len() > StorageGameDocument::MAX_MOVES {
            return Err(StorageGameDocumentError::TooManyMoves);
        }
        let control = snapshot.clock.control();
        let mut moves = Vec::with_capacity(snapshot.moves.len());
        for movement in snapshot.moves {
            StorageGameDocumentError::check_cancellation(cancellation)?;
            moves.push(StorageGameDocumentMove {
                id: movement.id,
                parent: movement.parent,
                uci: movement
                    .movement
                    .uci(snapshot.session.start.position().variant()),
                annotations: StorageGameDocumentAnnotations::capture(&movement.annotations)?,
                before_clock: movement
                    .before_clock
                    .as_ref()
                    .map(|clock| StorageGameDocumentClock::capture(clock, control))
                    .transpose()?,
                clock: movement
                    .clock
                    .as_ref()
                    .map(|clock| StorageGameDocumentClock::capture(clock, control))
                    .transpose()?,
            });
        }
        Ok(Self {
            id: snapshot.session.id,
            revision: snapshot.revision,
            root: snapshot.session.root,
            start: snapshot.session.start,
            mode: snapshot.session.seats.mode(),
            white: snapshot.session.seats.seat(BoardEnumColor::White),
            black: snapshot.session.seats.seat(BoardEnumColor::Black),
            time_control: control
                .stages()
                .iter()
                .copied()
                .map(StorageGameDocumentStage::from)
                .collect(),
            root_annotations: StorageGameDocumentAnnotations::capture(&snapshot.root_annotations)?,
            root_clock: snapshot
                .root_clock
                .as_ref()
                .map(|clock| StorageGameDocumentClock::capture(clock, control))
                .transpose()?,
            moves: StorageGameDocumentMoves(moves),
            live: snapshot.live,
            view: snapshot.view,
            clock: StorageGameDocumentClock::capture(&snapshot.clock, control)?,
            warnings_used: snapshot.warnings_used,
            state: snapshot.state.into(),
            metadata: StorageGameDocumentMetadata::capture(&snapshot.session.metadata)?,
            takeback_consent: snapshot.takeback_consent,
            premoves_enabled: snapshot.premoves_enabled,
            play_options: Some(StorageGameDocumentPlayOptions::capture(
                snapshot.engine_search_config,
                snapshot.is_auto_flip_enabled,
            )),
        })
    }

    fn restore(
        self,
        schema_version: u32,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<GameSession, StorageGameDocumentError> {
        let play_options = match self.play_options {
            Some(options) => options,
            None if schema_version == StorageGameDocument::LEGACY_SCHEMA_VERSION => {
                StorageGameDocumentPlayOptions::initial_default()
            }
            None => return Err(StorageGameDocumentError::MissingPlayOptions),
        };
        let engine_search_config = play_options.engine_search_config()?;
        let control = if self.time_control.is_empty() {
            ClockTimeControl::untimed()
        } else {
            ClockTimeControl::staged(
                self.time_control
                    .into_iter()
                    .map(StorageGameDocumentStage::into_stage)
                    .collect::<Result<_, _>>()?,
            )?
        };
        let mut positions = HashMap::with_capacity(self.moves.0.len().saturating_add(1));
        positions.insert(self.root, self.start.position().clone());
        let mut moves = Vec::with_capacity(self.moves.0.len());
        for record in self.moves.0 {
            StorageGameDocumentError::check_cancellation(cancellation)?;
            let parent = positions
                .get(&record.parent)
                .ok_or(GameTreeError::BrokenRelationship)?;
            let movement = parent.parse_uci(&record.uci)?;
            let position = parent.play(movement)?;
            if positions.insert(record.id, position).is_some() {
                return Err(GameTreeError::DuplicateIdentity.into());
            }
            moves.push(GameSessionRestoredMove {
                id: record.id,
                parent: record.parent,
                movement,
                annotations: record.annotations.restore()?,
                before_clock: record
                    .before_clock
                    .map(|clock| clock.restore(&control))
                    .transpose()?,
                clock: record
                    .clock
                    .map(|clock| clock.restore(&control))
                    .transpose()?,
            });
        }
        let initial_side = self.start.position().turn();
        let snapshot = GameSessionRestoreOptions {
            session: GameSessionOptions {
                id: self.id,
                root: self.root,
                start: self.start,
                seats: GameSeats::new(self.mode, self.white, self.black)?,
                clock: ClockState::new(control.clone(), initial_side),
                metadata: self.metadata.restore()?,
            },
            revision: self.revision,
            live: self.live,
            view: self.view,
            moves,
            root_annotations: self.root_annotations.restore()?,
            root_clock: self
                .root_clock
                .map(|clock| clock.restore(&control))
                .transpose()?,
            clock: self.clock.restore(&control)?,
            warnings_used: self.warnings_used,
            state: self.state.restore()?,
            takeback_consent: self.takeback_consent,
            premoves_enabled: self.premoves_enabled,
            engine_search_config,
            is_auto_flip_enabled: play_options.is_auto_flip_enabled,
        };
        StorageGameDocumentError::check_cancellation(cancellation)?;
        Ok(GameSession::restore(snapshot)?)
    }
}

pub struct StorageGameDocumentRepository<'a> {
    client: StorageDocumentClient,
    executor: &'a CapacityExecutor,
}

impl<'a> StorageGameDocumentRepository<'a> {
    pub const fn new(client: StorageDocumentClient, executor: &'a CapacityExecutor) -> Self {
        Self { client, executor }
    }
}

impl GameStorageRepository for StorageGameDocumentRepository<'_> {
    type Error = StorageGameDocumentError;

    async fn save(
        &self,
        request: GameStorageSaveRequest,
    ) -> Result<GameStorageReceipt, Self::Error> {
        let game = request.game();
        let game_revision = request.game_revision();
        let expected_storage_revision = request.expected_storage_revision();
        let json = self
            .executor
            .submit_waiting(move |cancellation| {
                StorageGameDocument::from_snapshot(request.into_snapshot(), Some(cancellation))?
                    .encode(Some(cancellation))
            })
            .await?
            .join()
            .await??;
        let saved = self
            .client
            .save(game.as_uuid(), expected_storage_revision.value(), json)
            .await?;
        Ok(GameStorageReceipt::new(
            game,
            game_revision,
            GameStorageRevision::new(saved.revision()),
        ))
    }

    async fn load(&self, game: GameId) -> Result<Option<GameStorageRecovered>, Self::Error> {
        let Some(document) = self.client.read(game.as_uuid()).await? else {
            return Ok(None);
        };
        let storage_revision = GameStorageRevision::new(document.revision());
        let session = self
            .executor
            .submit_waiting(move |cancellation| {
                StorageGameDocument::recover(document.body(), Some(cancellation))
            })
            .await?
            .join()
            .await??;
        if session.id() != game {
            return Err(StorageGameDocumentError::Identity);
        }
        Ok(Some(GameStorageRecovered::new(session, storage_revision)))
    }
}
