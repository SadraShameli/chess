use std::collections::TryReserveError;
use std::io::{Error as IoError, Write};
use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;
use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::board::r#move::BoardMoveError;
use crate::board::square::BoardSquare;
use crate::board::start::BoardStart;
use crate::capacity::executor::{
    CapacityExecutor, CapacityExecutorCancellation, CapacityExecutorError,
};
use crate::clock::duration::ClockInstant;
use crate::clock::stage::{ClockStage, ClockStageEnumBonus, ClockStageEnumQuota};
use crate::clock::state::{
    ClockState, ClockStateEnumStatus, ClockStateError, ClockStateSideOptions, ClockStateSnapshot,
    ClockStateSnapshotOptions,
};
use crate::clock::time_control::{ClockTimeControl, ClockTimeControlError};
use crate::game::adjudication::{
    GameAdjudicationEnumInterruption, GameAdjudicationEnumMateQuestion,
};
use crate::game::engine_provider::{GameEngineRequestError, GameEngineSearchConfig};
use crate::game::enums::{
    GameEnumMarkingColor, GameEnumMode, GameEnumOutcomeReason, GameEnumResult, GameEnumSeat,
    GameId, GameMoveId, GameRevision,
};
use crate::game::move_node::{GameMoveNodeAnnotationError, GameMoveNodeAnnotations};
use crate::game::outcome::{GameOutcome, GameOutcomeError};
use crate::game::pause::{GamePauseEnumCause, GamePauseEnumFault};
use crate::game::session::{
    GameMetadata, GameMetadataError, GameSeats, GameSession, GameSessionEnumInterruption,
    GameSessionEnumState, GameSessionError, GameSessionOptions, GameSessionRestoreOptions,
    GameSessionRestoredMove,
};
use crate::game::storage_repository::{
    GameStorageReceipt, GameStorageRecovered, GameStorageRepository, GameStorageRevision,
    GameStorageSaveRequest,
};
use crate::game::takeback::GameTakebackEnumConsent;
use crate::game::tree::{GameTree, GameTreeError};
use crate::notation::game::{NotationGame, NotationGameError};
use crate::storage::document_store::{
    StorageDocumentClient, StorageDocumentError, StorageDocumentStore,
};
use crate::storage::game_document::notation::StorageGameDocumentNotation;
use crate::storage::game_document::records::{
    StorageGameDocumentAnnotations, StorageGameDocumentDuration, StorageGameDocumentMetadata,
    StorageGameDocumentMove, StorageGameDocumentMoveSource, StorageGameDocumentMoves,
};
use crate::storage::library_record::{
    StorageLibraryEnumContent, StorageLibraryRecord, StorageLibraryRecordError,
};

mod notation;
mod records;

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
    #[error("Schema {schema_version} cannot use content kind {kind:?}")]
    SchemaKind {
        schema_version: u32,
        kind: Option<StorageGameDocumentEnumContentKind>,
    },
    #[error("Expected {expected:?} content, found {found:?}")]
    ContentKind {
        expected: StorageGameDocumentEnumContentKind,
        found: StorageGameDocumentEnumContentKind,
    },
    #[error("The saved played game has no explicit study mainline tip")]
    MissingStudyTip,
    #[error("Clockless notation cannot contain runtime clock snapshots")]
    NotationRuntimeClock,
    #[error("The saved game has no required per-game play options")]
    MissingPlayOptions,
    #[error("Game serialization or recovery was cancelled")]
    Cancelled,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error(transparent)]
    Allocation(#[from] TryReserveError),
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
    #[error(transparent)]
    Notation(#[from] NotationGameError),
    #[error(transparent)]
    Record(#[from] StorageLibraryRecordError),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageGameDocumentEnumContentKind {
    Played,
    Notation,
}

enum StorageGameDocumentEnumField<T> {
    Absent,
    Present(T),
}

impl<T> StorageGameDocumentEnumField<T> {
    fn absent() -> Self {
        Self::Absent
    }

    fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for StorageGameDocumentEnumField<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self::Present)
    }
}

impl<T: Serialize> Serialize for StorageGameDocumentEnumField<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Absent => serializer.serialize_none(),
            Self::Present(value) => value.serialize(serializer),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentEnvelope<'a> {
    schema_version: u32,
    #[serde(default = "StorageGameDocumentEnumField::absent", borrow)]
    kind: StorageGameDocumentEnumField<&'a RawValue>,
    #[serde(borrow)]
    game: &'a RawValue,
}

#[derive(Serialize)]
struct StorageGameDocumentEncoded<'a> {
    schema_version: u32,
    #[serde(flatten)]
    content: &'a StorageGameDocumentEnumContent,
}

#[derive(Serialize)]
#[serde(tag = "kind", content = "game", rename_all = "snake_case")]
enum StorageGameDocumentEnumContent {
    Played(Box<StorageGameDocumentPlayed>),
    Notation(Box<StorageGameDocumentNotation>),
}

pub struct StorageGameDocument {
    content: StorageGameDocumentEnumContent,
}

impl StorageGameDocument {
    pub const SCHEMA_VERSION: u32 = 1;
    pub const MAX_MOVES: usize = 100_000;
    const MAX_TEXT_BYTES: usize = 1024 * 1024;

    pub fn capture(
        session: &GameSession,
        now: ClockInstant,
    ) -> Result<Self, StorageGameDocumentError> {
        Self::from_snapshot(&session.restoration(now)?, None)
    }

    pub(crate) fn from_snapshot(
        snapshot: &GameSessionRestoreOptions,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<Self, StorageGameDocumentError> {
        StorageGameDocumentError::check_cancellation(cancellation)?;
        Ok(Self {
            content: StorageGameDocumentEnumContent::Played(Box::new(
                StorageGameDocumentPlayed::from_snapshot(snapshot, cancellation)?,
            )),
        })
    }

    pub(crate) fn snapshot_memory_bytes(
        snapshot: &GameSessionRestoreOptions,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<usize, StorageGameDocumentError> {
        StorageGameDocumentMemory::played(snapshot, cancellation)
    }

    pub(crate) fn notation_memory_bytes(
        game: &NotationGame,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<usize, StorageGameDocumentError> {
        StorageGameDocumentNotation::memory_bytes(game, cancellation)
    }

    pub fn capture_notation(
        id: GameId,
        game: &NotationGame,
    ) -> Result<Self, StorageGameDocumentError> {
        Self::from_notation(id, game, None)
    }

    pub fn capture_notation_cancellable(
        id: GameId,
        game: &NotationGame,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<Self, StorageGameDocumentError> {
        Self::from_notation(id, game, Some(cancellation))
    }

    fn from_notation(
        id: GameId,
        game: &NotationGame,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<Self, StorageGameDocumentError> {
        StorageGameDocumentError::check_cancellation(cancellation)?;
        Ok(Self {
            content: StorageGameDocumentEnumContent::Notation(Box::new(
                StorageGameDocumentNotation::capture(id, game, cancellation)?,
            )),
        })
    }

    pub fn json(&self) -> Result<String, StorageGameDocumentError> {
        self.encode(None)
    }

    pub fn json_cancellable(
        &self,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<String, StorageGameDocumentError> {
        self.encode(Some(cancellation))
    }

    fn encode(
        &self,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<String, StorageGameDocumentError> {
        self.encode_with_memory(cancellation, 0, |_required| Ok(()))
    }

    pub(crate) fn json_with_memory(
        &self,
        cancellation: &CapacityExecutorCancellation,
        retained_bytes: usize,
        reserve: impl FnMut(usize) -> Result<(), StorageDocumentError>,
    ) -> Result<String, StorageGameDocumentError> {
        self.encode_with_memory(Some(cancellation), retained_bytes, reserve)
    }

    fn encode_with_memory(
        &self,
        cancellation: Option<&CapacityExecutorCancellation>,
        retained_bytes: usize,
        reserve: impl FnMut(usize) -> Result<(), StorageDocumentError>,
    ) -> Result<String, StorageGameDocumentError> {
        let mut writer = StorageGameDocumentWriter {
            bytes: Vec::new(),
            failure: None,
            cancellation,
            retained_bytes,
            reserve,
        };
        let result = serde_json::to_writer(
            &mut writer,
            &StorageGameDocumentEncoded {
                schema_version: Self::SCHEMA_VERSION,
                content: &self.content,
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
        match Self::decode(json, cancellation)?.into_content() {
            StorageLibraryEnumContent::Played(session) => Ok(*session),
            StorageLibraryEnumContent::Notation(_) => Err(StorageGameDocumentError::ContentKind {
                expected: StorageGameDocumentEnumContentKind::Played,
                found: StorageGameDocumentEnumContentKind::Notation,
            }),
        }
    }

    pub fn recover_record(json: &str) -> Result<StorageLibraryRecord, StorageGameDocumentError> {
        Self::decode(json, None)
    }

    pub fn recover_record_cancellable(
        json: &str,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<StorageLibraryRecord, StorageGameDocumentError> {
        Self::decode(json, Some(cancellation))
    }

    fn decode(
        json: &str,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<StorageLibraryRecord, StorageGameDocumentError> {
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
        if envelope.schema_version != Self::SCHEMA_VERSION {
            return Err(StorageGameDocumentError::UnsupportedSchema(
                envelope.schema_version,
            ));
        }
        let kind = match (envelope.schema_version, envelope.kind) {
            (Self::SCHEMA_VERSION, StorageGameDocumentEnumField::Present(kind)) => {
                serde_json::from_str(kind.get())?
            }
            (schema_version, kind) => {
                return Err(StorageGameDocumentError::SchemaKind {
                    schema_version,
                    kind: match kind {
                        StorageGameDocumentEnumField::Absent => None,
                        StorageGameDocumentEnumField::Present(raw) => {
                            Some(serde_json::from_str(raw.get())?)
                        }
                    },
                });
            }
        };
        let record = match kind {
            StorageGameDocumentEnumContentKind::Played => {
                let record: StorageGameDocumentPlayed = serde_json::from_str(envelope.game.get())?;
                let id = record.id;
                StorageLibraryRecord::new(
                    id,
                    StorageLibraryEnumContent::Played(Box::new(record.restore(cancellation)?)),
                )?
            }
            StorageGameDocumentEnumContentKind::Notation => {
                let record: StorageGameDocumentNotation =
                    serde_json::from_str(envelope.game.get())?;
                record.restore(cancellation)?
            }
        };
        StorageGameDocumentError::check_cancellation(cancellation)?;
        Ok(record)
    }
}

struct StorageGameDocumentMemory {
    bytes: usize,
}

impl StorageGameDocumentMemory {
    const UCI_CAPACITY: usize = 8;
    const MIN_EXTENSION_CAPACITY: usize = 4;

    fn played(
        snapshot: &GameSessionRestoreOptions,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<usize, StorageGameDocumentError> {
        StorageGameDocumentError::check_cancellation(Some(cancellation))?;
        let mut memory = Self {
            bytes: std::mem::size_of::<StorageGameDocumentPlayed>(),
        };
        memory.start(&snapshot.session.start)?;
        memory.moves(snapshot.moves.len())?;
        memory.add_items::<StorageGameDocumentStage>(snapshot.clock.control().stages().len())?;
        memory.add_items::<ClockStage>(snapshot.clock.control().stages().len())?;
        memory.annotations(&snapshot.root_annotations)?;
        for movement in &snapshot.moves {
            StorageGameDocumentError::check_cancellation(Some(cancellation))?;
            memory.annotations(&movement.annotations)?;
        }
        memory.metadata(&snapshot.session.metadata, cancellation)?;
        StorageGameDocumentError::check_cancellation(Some(cancellation))?;
        Ok(memory.bytes)
    }

    fn start(&mut self, start: &BoardStart) -> Result<(), StorageGameDocumentError> {
        self.add_items::<u8>(uuid::fmt::Hyphenated::LENGTH)?;
        self.add_items::<[u8; 2]>(start.initial_fen().len())
    }

    fn moves(&mut self, count: usize) -> Result<(), StorageGameDocumentError> {
        if count > StorageGameDocument::MAX_MOVES {
            return Err(StorageGameDocumentError::TooManyMoves);
        }
        self.add_items::<StorageGameDocumentMove>(count)?;
        self.add_items::<[u8; Self::UCI_CAPACITY]>(count)
    }

    fn metadata(
        &mut self,
        metadata: &GameMetadata,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<(), StorageGameDocumentError> {
        let known = metadata.known();
        for text in [
            &known.event,
            &known.site,
            &known.round,
            &known.white,
            &known.black,
            &known.opening,
            &known.variation,
            &known.annotator,
        ]
        .into_iter()
        .flatten()
        {
            self.text(text.as_str())?;
        }
        if let Some(code) = &known.eco {
            self.text(code.as_str())?;
        }
        let extensions = metadata.extensions();
        if !extensions.is_empty() {
            let capacity = extensions
                .len()
                .checked_next_power_of_two()
                .ok_or(StorageGameDocumentError::TooLarge)?
                .max(Self::MIN_EXTENSION_CAPACITY);
            let peak = capacity
                .checked_add(capacity / 2)
                .ok_or(StorageGameDocumentError::TooLarge)?;
            self.add_items::<(String, String)>(peak)?;
        }
        for extension in extensions {
            StorageGameDocumentError::check_cancellation(Some(cancellation))?;
            self.text(extension.name())?;
            self.text(extension.value().as_str())?;
        }
        StorageGameDocumentError::check_cancellation(Some(cancellation))?;
        Ok(())
    }

    fn add_items<T>(&mut self, count: usize) -> Result<(), StorageGameDocumentError> {
        self.bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .and_then(|bytes| self.bytes.checked_add(bytes))
            .ok_or(StorageGameDocumentError::TooLarge)?;
        Ok(())
    }

    fn text(&mut self, text: &str) -> Result<(), StorageGameDocumentError> {
        if text.len() > StorageGameDocument::MAX_TEXT_BYTES {
            return Err(StorageGameDocumentError::TextTooLarge);
        }
        self.add_items::<u8>(text.len())
    }

    fn annotations(
        &mut self,
        annotations: &GameMoveNodeAnnotations,
    ) -> Result<(), StorageGameDocumentError> {
        self.text(annotations.notes())?;
        self.add_items::<u8>(annotations.nags().len())?;
        self.add_items::<(GameEnumMarkingColor, BoardSquare, Option<BoardSquare>)>(
            annotations.markings().len(),
        )
    }
}

enum StorageGameDocumentEnumWriteFailure {
    Size,
    Cancelled,
    Memory(StorageDocumentError),
    Allocation(TryReserveError),
}

impl StorageGameDocumentEnumWriteFailure {
    fn into_error(self) -> StorageGameDocumentError {
        match self {
            Self::Size => StorageGameDocumentError::TooLarge,
            Self::Cancelled => StorageGameDocumentError::Cancelled,
            Self::Memory(error) => error.into(),
            Self::Allocation(error) => error.into(),
        }
    }
}

struct StorageGameDocumentWriter<'a, F> {
    bytes: Vec<u8>,
    failure: Option<StorageGameDocumentEnumWriteFailure>,
    cancellation: Option<&'a CapacityExecutorCancellation>,
    retained_bytes: usize,
    reserve: F,
}

impl<F> StorageGameDocumentWriter<'_, F>
where
    F: FnMut(usize) -> Result<(), StorageDocumentError>,
{
    const MIN_CAPACITY: usize = 8;

    fn reserve(&mut self, additional: usize) -> Result<(), StorageGameDocumentEnumWriteFailure> {
        let required = self
            .bytes
            .len()
            .checked_add(additional)
            .filter(|required| *required <= StorageDocumentStore::MEMORY_BUDGET)
            .ok_or(StorageGameDocumentEnumWriteFailure::Size)?;
        if required <= self.bytes.capacity() {
            return Ok(());
        }
        let capacity = self
            .bytes
            .capacity()
            .checked_mul(2)
            .ok_or(StorageGameDocumentEnumWriteFailure::Size)?
            .max(required)
            .clamp(Self::MIN_CAPACITY, StorageDocumentStore::MEMORY_BUDGET);
        let peak = self
            .retained_bytes
            .checked_add(self.bytes.capacity())
            .and_then(|bytes| bytes.checked_add(capacity))
            .ok_or(StorageGameDocumentEnumWriteFailure::Size)?;
        (self.reserve)(peak).map_err(StorageGameDocumentEnumWriteFailure::Memory)?;
        self.bytes
            .try_reserve_exact(capacity - self.bytes.len())
            .map_err(StorageGameDocumentEnumWriteFailure::Allocation)?;
        Ok(())
    }
}

impl<F> Write for StorageGameDocumentWriter<'_, F>
where
    F: FnMut(usize) -> Result<(), StorageDocumentError>,
{
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let failure = if self
            .cancellation
            .is_some_and(CapacityExecutorCancellation::is_cancelled)
        {
            Some(StorageGameDocumentEnumWriteFailure::Cancelled)
        } else {
            self.reserve(bytes.len()).err()
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
    #[serde(skip)]
    PendingResult,
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
    Fischer(StorageGameDocumentDuration),
    SimpleDelay(StorageGameDocumentDuration),
    BronsteinDelay(StorageGameDocumentDuration),
}

impl From<ClockStageEnumBonus> for StorageGameDocumentEnumBonus {
    fn from(bonus: ClockStageEnumBonus) -> Self {
        match bonus {
            ClockStageEnumBonus::None => Self::None,
            ClockStageEnumBonus::Fischer(duration) => Self::Fischer(duration.into()),
            ClockStageEnumBonus::SimpleDelay(duration) => Self::SimpleDelay(duration.into()),
            ClockStageEnumBonus::BronsteinDelay(duration) => Self::BronsteinDelay(duration.into()),
        }
    }
}

impl From<StorageGameDocumentEnumBonus> for ClockStageEnumBonus {
    fn from(bonus: StorageGameDocumentEnumBonus) -> Self {
        match bonus {
            StorageGameDocumentEnumBonus::None => Self::None,
            StorageGameDocumentEnumBonus::Fischer(nanos) => Self::Fischer(nanos.into()),
            StorageGameDocumentEnumBonus::SimpleDelay(nanos) => Self::SimpleDelay(nanos.into()),
            StorageGameDocumentEnumBonus::BronsteinDelay(nanos) => {
                Self::BronsteinDelay(nanos.into())
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentStage {
    #[serde(with = "StorageGameDocumentEnumQuota")]
    quota: ClockStageEnumQuota,
    base_nanos: StorageGameDocumentDuration,
    bonus: StorageGameDocumentEnumBonus,
}

impl From<ClockStage> for StorageGameDocumentStage {
    fn from(stage: ClockStage) -> Self {
        Self {
            quota: stage.quota(),
            base_nanos: stage.base_time().into(),
            bonus: stage.bonus().into(),
        }
    }
}

impl StorageGameDocumentStage {
    fn into_stage(self) -> Result<ClockStage, StorageGameDocumentError> {
        Ok(ClockStage::new(
            self.quota,
            self.base_nanos.into(),
            self.bonus.into(),
        )?)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentClockSide {
    remaining_nanos: StorageGameDocumentDuration,
    completed_moves: u64,
    stage_index: usize,
    stage_moves: u64,
}

impl From<ClockStateSideOptions> for StorageGameDocumentClockSide {
    fn from(side: ClockStateSideOptions) -> Self {
        Self {
            remaining_nanos: side.remaining.into(),
            completed_moves: side.completed_moves,
            stage_index: side.stage_index,
            stage_moves: side.stage_moves,
        }
    }
}

impl From<StorageGameDocumentClockSide> for ClockStateSideOptions {
    fn from(side: StorageGameDocumentClockSide) -> Self {
        Self {
            remaining: side.remaining_nanos.into(),
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
    elapsed_nanos: StorageGameDocumentDuration,
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
        Ok(Self {
            sides: clock.sides().map(StorageGameDocumentClockSide::from),
            active: clock.active(),
            elapsed_nanos: clock.elapsed().into(),
            status: clock.status(),
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
                elapsed: self.elapsed_nanos.into(),
                status: self.status,
            },
        )?)
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
    untimed_move_time_nanos: StorageGameDocumentDuration,
    move_overhead_nanos: StorageGameDocumentDuration,
    is_auto_flip_enabled: bool,
}

impl StorageGameDocumentPlayOptions {
    fn capture(engine_search: GameEngineSearchConfig, is_auto_flip_enabled: bool) -> Self {
        Self {
            untimed_move_time_nanos: engine_search.untimed_move_time().into(),
            move_overhead_nanos: engine_search.move_overhead().into(),
            is_auto_flip_enabled,
        }
    }

    fn engine_search_config(&self) -> Result<GameEngineSearchConfig, GameEngineRequestError> {
        GameEngineSearchConfig::new(
            self.untimed_move_time_nanos.into(),
            self.move_overhead_nanos.into(),
        )
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentPlayed {
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
    #[serde(
        default = "StorageGameDocumentEnumField::absent",
        skip_serializing_if = "StorageGameDocumentEnumField::is_absent"
    )]
    study_mainline_tip: StorageGameDocumentEnumField<GameMoveId>,
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

impl StorageGameDocumentPlayed {
    fn from_snapshot(
        snapshot: &GameSessionRestoreOptions,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<Self, StorageGameDocumentError> {
        if snapshot.moves.len() > StorageGameDocument::MAX_MOVES {
            return Err(StorageGameDocumentError::TooManyMoves);
        }
        let control = snapshot.clock.control();
        let mut moves = Vec::with_capacity(snapshot.moves.len());
        for movement in &snapshot.moves {
            StorageGameDocumentError::check_cancellation(cancellation)?;
            moves.push(StorageGameDocumentMove::capture(
                StorageGameDocumentMoveSource {
                    id: movement.id,
                    parent: movement.parent,
                    movement: movement.movement,
                    annotations: &movement.annotations,
                    before_clock: movement.before_clock.as_ref(),
                    clock: movement.clock.as_ref(),
                },
                snapshot.session.start.position(),
                Some(control),
            )?);
        }
        Ok(Self {
            id: snapshot.session.id,
            revision: snapshot.revision,
            root: snapshot.session.root,
            start: snapshot.session.start.clone(),
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
            study_mainline_tip: StorageGameDocumentEnumField::Present(snapshot.study_mainline_tip),
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
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<GameSession, StorageGameDocumentError> {
        let play_options = match self.play_options {
            Some(options) => options,
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
        let mut tree = GameTree::new(
            self.root,
            self.start.position().clone(),
            self.root_clock
                .map(|clock| clock.restore(&control))
                .transpose()?,
        );
        tree.set_annotations(self.root, self.root_annotations.restore()?)?;
        let tree = self.moves.restore(tree, Some(&control), cancellation)?;
        let study_mainline_tip = match self.study_mainline_tip {
            StorageGameDocumentEnumField::Present(tip) => {
                tree.validate_study_tip(tip)?;
                tip
            }
            StorageGameDocumentEnumField::Absent => {
                return Err(StorageGameDocumentError::MissingStudyTip);
            }
        };
        let mut moves = Vec::with_capacity(tree.len().saturating_sub(1));
        for node in tree.nodes() {
            StorageGameDocumentError::check_cancellation(cancellation)?;
            if let (Some(parent), Some(movement)) = (node.parent(), node.movement()) {
                moves.push(GameSessionRestoredMove {
                    id: node.id(),
                    parent,
                    movement,
                    annotations: node.annotations().clone(),
                    before_clock: node.before_clock().cloned(),
                    clock: node.clock().cloned(),
                });
            }
        }
        let root = tree.node(self.root)?;
        let root_annotations = root.annotations().clone();
        let root_clock = root.clock().cloned();
        drop(tree);
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
            study_mainline_tip,
            moves,
            root_annotations,
            root_clock,
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
                StorageGameDocument::from_snapshot(request.snapshot(), Some(cancellation))?
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

#[cfg(test)]
mod memory_tests {
    use std::io::Write;

    use crate::storage::document_store::StorageDocumentError;
    use crate::storage::game_document::{
        StorageGameDocumentEnumWriteFailure, StorageGameDocumentWriter,
    };

    #[test]
    fn refused_growth_preserves_existing_encoded_bytes() -> Result<(), std::io::Error> {
        let mut requests = Vec::new();
        let mut writer = StorageGameDocumentWriter {
            bytes: Vec::new(),
            failure: None,
            cancellation: None,
            retained_bytes: 128,
            reserve: |required| {
                requests.push(required);
                if requests.len() == 1 {
                    Ok(())
                } else {
                    Err(StorageDocumentError::Saturated)
                }
            },
        };
        writer.write_all(b"abc")?;
        let capacity = writer.bytes.capacity();
        assert!(writer.write_all(b"defghijkl").is_err());
        assert_eq!(writer.bytes, b"abc");
        assert_eq!(writer.bytes.capacity(), capacity);
        assert!(matches!(
            writer.failure,
            Some(StorageGameDocumentEnumWriteFailure::Memory(
                StorageDocumentError::Saturated
            ))
        ));
        drop(writer);
        assert_eq!(requests, [136, 152]);
        Ok(())
    }

    #[test]
    fn overflowing_growth_refuses_before_requesting_memory() {
        let mut requested = false;
        let mut writer = StorageGameDocumentWriter {
            bytes: vec![b'a'],
            failure: None,
            cancellation: None,
            retained_bytes: 0,
            reserve: |_required| {
                requested = true;
                Ok(())
            },
        };
        assert!(matches!(
            writer.reserve(usize::MAX),
            Err(StorageGameDocumentEnumWriteFailure::Size)
        ));
        assert_eq!(writer.bytes, b"a");
        drop(writer);
        assert!(!requested);
    }
}
