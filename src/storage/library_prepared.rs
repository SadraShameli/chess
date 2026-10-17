use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;
use tokio::sync::OwnedSemaphorePermit;

use crate::board::enums::{BoardEnumColor, BoardEnumVariant};
use crate::board::start::BoardStartEnumSelection;
use crate::capacity::executor::CapacityExecutorCancellation;
use crate::game::enums::{GameEnumMode, GameEnumResult, GameId};
use crate::game::library_organization::GameLibraryOrganizationTimestamp;
use crate::game::session::{GameMetadataDate, GameMetadataOpeningCode, GameMetadataText};
use crate::game::storage_repository::GameStorageRevision;
use crate::notation::game_identity::{NotationGameIdentity, NotationGameIdentityEnumResult};
use crate::package::digest::PackageDigest;
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::game_document::{StorageGameDocument, StorageGameDocumentEnumContentKind};
use crate::storage::library_record::{StorageLibraryEnumContent, StorageLibraryRecord};
use crate::storage::library_repository::StorageLibraryError;
use crate::storage::library_transaction::StorageLibraryEnumTransactionRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageLibraryEnumStartKind {
    Standard,
    Chess960,
    Dfrc,
    Custom,
}

impl StorageLibraryEnumStartKind {
    pub(crate) const fn sql(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Chess960 => "chess960",
            Self::Dfrc => "dfrc",
            Self::Custom => "custom",
        }
    }
    pub(crate) fn parse(value: &str) -> Result<Self, StorageLibraryError> {
        match value {
            "standard" => Ok(Self::Standard),
            "chess960" => Ok(Self::Chess960),
            "dfrc" => Ok(Self::Dfrc),
            "custom" => Ok(Self::Custom),
            _ => Err(StorageLibraryError::StoredDiscriminant),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum StorageLibraryEnumContentChange {
    Changed(GameLibraryOrganizationTimestamp),
    Checkpoint,
}

#[derive(Debug)]
pub(crate) struct StorageLibraryProjection {
    kind: StorageGameDocumentEnumContentKind,
    variant: BoardEnumVariant,
    start: StorageLibraryEnumStartKind,
    mode: Option<GameEnumMode>,
    result: NotationGameIdentityEnumResult,
    white: Option<GameMetadataText>,
    white_key: Option<String>,
    black: Option<GameMetadataText>,
    black_key: Option<String>,
    date: Option<GameMetadataDate>,
    opening: Option<GameMetadataText>,
    opening_key: Option<String>,
    code: Option<GameMetadataOpeningCode>,
    plies: u32,
}

pub(crate) struct StorageLibraryProjectionOptions {
    pub(crate) kind: StorageGameDocumentEnumContentKind,
    pub(crate) variant: BoardEnumVariant,
    pub(crate) start: StorageLibraryEnumStartKind,
    pub(crate) mode: Option<GameEnumMode>,
    pub(crate) result: NotationGameIdentityEnumResult,
    pub(crate) white: Option<GameMetadataText>,
    pub(crate) black: Option<GameMetadataText>,
    pub(crate) date: Option<GameMetadataDate>,
    pub(crate) opening: Option<GameMetadataText>,
    pub(crate) code: Option<GameMetadataOpeningCode>,
    pub(crate) plies: u32,
}

impl StorageLibraryProjection {
    pub(crate) const MAX_TEXT_BYTES: usize = 1024 * 1024;
    pub(crate) const MAX_PLIES: u32 = 100_000;
    pub(crate) fn new(
        options: StorageLibraryProjectionOptions,
    ) -> Result<Self, StorageLibraryError> {
        if options.plies > Self::MAX_PLIES {
            return Err(StorageLibraryError::PlyInterval);
        }
        for text in [&options.white, &options.black, &options.opening]
            .into_iter()
            .flatten()
        {
            if text.as_str().len() > Self::MAX_TEXT_BYTES {
                return Err(StorageLibraryError::TextTooLarge);
            }
        }
        match (options.kind, options.mode) {
            (StorageGameDocumentEnumContentKind::Notation, None)
            | (StorageGameDocumentEnumContentKind::Played, Some(_)) => {}
            (StorageGameDocumentEnumContentKind::Notation, Some(_))
            | (StorageGameDocumentEnumContentKind::Played, None) => {
                return Err(StorageLibraryError::StoredDiscriminant);
            }
        }
        match (options.start, options.variant) {
            (StorageLibraryEnumStartKind::Standard, BoardEnumVariant::Chess960)
            | (
                StorageLibraryEnumStartKind::Chess960 | StorageLibraryEnumStartKind::Dfrc,
                BoardEnumVariant::Standard,
            ) => return Err(StorageLibraryError::StoredDiscriminant),
            (StorageLibraryEnumStartKind::Standard, BoardEnumVariant::Standard)
            | (
                StorageLibraryEnumStartKind::Chess960 | StorageLibraryEnumStartKind::Dfrc,
                BoardEnumVariant::Chess960,
            )
            | (StorageLibraryEnumStartKind::Custom, _) => {}
        }
        let date = options
            .date
            .filter(|date| date.year().is_some() || date.month().is_some() || date.day().is_some());
        Ok(Self {
            kind: options.kind,
            variant: options.variant,
            start: options.start,
            mode: options.mode,
            result: options.result,
            white_key: NotationGameIdentity::canonical_player(options.white.as_ref()),
            black_key: NotationGameIdentity::canonical_player(options.black.as_ref()),
            opening_key: options
                .opening
                .as_ref()
                .map(|text| NotationGameIdentity::canonical_text(text.as_str()))
                .filter(|key| !key.is_empty()),
            white: options.white,
            black: options.black,
            date,
            opening: options.opening,
            code: options.code,
            plies: options.plies,
        })
    }
    fn from_record(
        record: &StorageLibraryRecord,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<Self, StorageLibraryError> {
        StorageLibraryError::check_cancellation(cancellation)?;
        let known = record.metadata().known();
        let start = match record.start().selection() {
            BoardStartEnumSelection::Standard => StorageLibraryEnumStartKind::Standard,
            BoardStartEnumSelection::Chess960 { .. } => StorageLibraryEnumStartKind::Chess960,
            BoardStartEnumSelection::Dfrc { .. } => StorageLibraryEnumStartKind::Dfrc,
            BoardStartEnumSelection::Custom { .. } => StorageLibraryEnumStartKind::Custom,
        };
        let projection = Self::new(StorageLibraryProjectionOptions {
            kind: match record.content() {
                StorageLibraryEnumContent::Played(_) => StorageGameDocumentEnumContentKind::Played,
                StorageLibraryEnumContent::Notation(_) => {
                    StorageGameDocumentEnumContentKind::Notation
                }
            },
            variant: record.start().selection().variant(),
            start,
            mode: record.mode(),
            result: record.result(),
            white: known.white.clone(),
            black: known.black.clone(),
            date: known.date,
            opening: known.opening.clone(),
            code: known.eco.clone(),
            plies: u32::try_from(record.identity_line_len()?)?,
        })?;
        StorageLibraryError::check_cancellation(cancellation)?;
        Ok(projection)
    }
    pub(crate) const fn kind(&self) -> StorageGameDocumentEnumContentKind {
        self.kind
    }
    pub(crate) const fn variant(&self) -> BoardEnumVariant {
        self.variant
    }
    pub(crate) const fn start(&self) -> StorageLibraryEnumStartKind {
        self.start
    }
    pub(crate) const fn mode(&self) -> Option<GameEnumMode> {
        self.mode
    }
    pub(crate) const fn result(&self) -> NotationGameIdentityEnumResult {
        self.result
    }
    pub(crate) fn white(&self) -> Option<&GameMetadataText> {
        self.white.as_ref()
    }
    pub(crate) fn black(&self) -> Option<&GameMetadataText> {
        self.black.as_ref()
    }
    pub(crate) fn white_key(&self) -> Option<&str> {
        self.white_key.as_deref()
    }
    pub(crate) fn black_key(&self) -> Option<&str> {
        self.black_key.as_deref()
    }
    pub(crate) const fn date(&self) -> Option<GameMetadataDate> {
        self.date
    }
    pub(crate) fn opening(&self) -> Option<&GameMetadataText> {
        self.opening.as_ref()
    }
    pub(crate) fn opening_key(&self) -> Option<&str> {
        self.opening_key.as_deref()
    }
    pub(crate) fn code(&self) -> Option<&GameMetadataOpeningCode> {
        self.code.as_ref()
    }
    pub(crate) const fn plies(&self) -> u32 {
        self.plies
    }
    pub(crate) const fn kind_sql(kind: StorageGameDocumentEnumContentKind) -> &'static str {
        match kind {
            StorageGameDocumentEnumContentKind::Played => "played",
            StorageGameDocumentEnumContentKind::Notation => "notation",
        }
    }
    pub(crate) fn parse_kind(
        value: &str,
    ) -> Result<StorageGameDocumentEnumContentKind, StorageLibraryError> {
        match value {
            "played" => Ok(StorageGameDocumentEnumContentKind::Played),
            "notation" => Ok(StorageGameDocumentEnumContentKind::Notation),
            _ => Err(StorageLibraryError::StoredDiscriminant),
        }
    }
    pub(crate) const fn variant_sql(variant: BoardEnumVariant) -> &'static str {
        match variant {
            BoardEnumVariant::Standard => "standard",
            BoardEnumVariant::Chess960 => "chess960",
        }
    }
    pub(crate) fn parse_variant(value: &str) -> Result<BoardEnumVariant, StorageLibraryError> {
        match value {
            "standard" => Ok(BoardEnumVariant::Standard),
            "chess960" => Ok(BoardEnumVariant::Chess960),
            _ => Err(StorageLibraryError::StoredDiscriminant),
        }
    }
    pub(crate) const fn mode_sql(mode: GameEnumMode) -> &'static str {
        match mode {
            GameEnumMode::HumanVersusEngine => "human_versus_engine",
            GameEnumMode::LocalTwoPlayer => "local_two_player",
        }
    }
    pub(crate) fn parse_mode(value: &str) -> Result<GameEnumMode, StorageLibraryError> {
        match value {
            "human_versus_engine" => Ok(GameEnumMode::HumanVersusEngine),
            "local_two_player" => Ok(GameEnumMode::LocalTwoPlayer),
            _ => Err(StorageLibraryError::StoredDiscriminant),
        }
    }
    pub(crate) fn parse_result(
        value: &str,
    ) -> Result<NotationGameIdentityEnumResult, StorageLibraryError> {
        match value {
            "*" => Ok(NotationGameIdentityEnumResult::Unfinished),
            "1-0" => Ok(NotationGameIdentityEnumResult::Completed(
                GameEnumResult::Victory(BoardEnumColor::White),
            )),
            "0-1" => Ok(NotationGameIdentityEnumResult::Completed(
                GameEnumResult::Victory(BoardEnumColor::Black),
            )),
            "1/2-1/2" => Ok(NotationGameIdentityEnumResult::Completed(
                GameEnumResult::Draw,
            )),
            _ => Err(StorageLibraryError::StoredDiscriminant),
        }
    }
    pub(crate) fn transport_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + [&self.white, &self.black, &self.opening]
                .into_iter()
                .flatten()
                .map(|text| text.as_str().len())
                .sum::<usize>()
            + [&self.white_key, &self.black_key, &self.opening_key]
                .into_iter()
                .flatten()
                .map(String::capacity)
                .sum::<usize>()
            + self.code.as_ref().map_or(0, |code| code.as_str().len())
    }
}

pub(crate) struct StorageLibraryPreparedMemory {
    reservation: OwnedSemaphorePermit,
}

impl StorageLibraryPreparedMemory {
    const ENCODED_EXPANSION_BUDGET: usize = 4;

    pub(crate) fn reservation_bytes(body_bytes: usize) -> Result<usize, StorageLibraryError> {
        body_bytes
            .checked_mul(Self::ENCODED_EXPANSION_BUDGET)
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<StorageLibraryProjection>()))
            .and_then(|bytes| {
                bytes.checked_add(2 * std::mem::size_of::<StorageLibraryEnumTransactionRequest>())
            })
            .ok_or(StorageLibraryError::TextTooLarge)
    }

    pub(crate) fn new(
        body_bytes: usize,
        reservation: OwnedSemaphorePermit,
    ) -> Result<Self, StorageLibraryError> {
        let memory = Self { reservation };
        memory.check_body(body_bytes)?;
        Ok(memory)
    }

    fn check_body(&self, body_bytes: usize) -> Result<(), StorageLibraryError> {
        self.check_bytes(Self::reservation_bytes(body_bytes)?)
    }

    fn validate_owner(&self, client: &StorageDocumentClient) -> Result<(), StorageLibraryError> {
        if !client.is_library_memory_owner(&self.reservation) {
            return Err(StorageLibraryError::ForeignPreparation);
        }
        Ok(())
    }

    fn check_bytes(&self, required: usize) -> Result<(), StorageLibraryError> {
        let reserved = self.reservation.num_permits();
        if reserved < required {
            return Err(StorageLibraryError::PreparationMemory { required, reserved });
        }
        Ok(())
    }
}

pub(crate) struct StorageLibraryPreparedWriteOptions {
    pub(crate) id: GameId,
    pub(crate) expected: GameStorageRevision,
    pub(crate) body: String,
    pub(crate) change: StorageLibraryEnumContentChange,
    pub(crate) memory: StorageLibraryPreparedMemory,
}

pub struct StorageLibraryPreparedWrite {
    id: GameId,
    expected: GameStorageRevision,
    body: String,
    change: StorageLibraryEnumContentChange,
    projection: Box<StorageLibraryProjection>,
    identity: StorageLibraryPreparedIdentity,
    memory: StorageLibraryPreparedMemory,
}

impl StorageLibraryPreparedWrite {
    pub(crate) fn validate_memory_owner(
        &self,
        client: &StorageDocumentClient,
    ) -> Result<(), StorageLibraryError> {
        self.memory.validate_owner(client)
    }

    pub(crate) fn from_encoded(
        options: StorageLibraryPreparedWriteOptions,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<Self, StorageLibraryError> {
        let StorageLibraryPreparedWriteOptions {
            id,
            expected,
            body,
            change,
            memory,
        } = options;
        memory.check_body(body.capacity())?;
        let record = StorageGameDocument::recover_record_cancellable(&body, cancellation)?;
        if record.id() != id {
            return Err(StorageLibraryError::Identity);
        }
        let projection = StorageLibraryProjection::from_record(&record, cancellation)?;
        let identity = StorageLibraryPreparedIdentity::new(&record, &body, cancellation)?;
        let prepared = Self {
            id,
            expected,
            body,
            change,
            projection: Box::new(projection),
            identity,
            memory,
        };
        prepared.memory.check_bytes(
            prepared.transport_bytes()
                + std::mem::size_of::<StorageLibraryEnumTransactionRequest>(),
        )?;
        StorageLibraryError::check_cancellation(cancellation)?;
        Ok(prepared)
    }
    pub(crate) const fn id(&self) -> GameId {
        self.id
    }
    pub(crate) const fn expected(&self) -> GameStorageRevision {
        self.expected
    }
    pub(crate) fn body(&self) -> &str {
        &self.body
    }
    pub(crate) const fn change(&self) -> StorageLibraryEnumContentChange {
        self.change
    }
    pub(crate) fn projection(&self) -> &StorageLibraryProjection {
        &self.projection
    }
    pub(crate) fn identity(&self) -> &StorageLibraryPreparedIdentity {
        &self.identity
    }
    pub(crate) fn transport_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.body.capacity()
            + self.projection.transport_bytes()
            + self.identity.transport_bytes()
    }
}

pub struct StorageLibraryPreparedAdoption {
    id: GameId,
    observed: GameStorageRevision,
    projection: Box<StorageLibraryProjection>,
    identity: StorageLibraryPreparedIdentity,
    memory: StorageLibraryPreparedMemory,
}

impl StorageLibraryPreparedAdoption {
    pub(crate) fn validate_memory_owner(
        &self,
        client: &StorageDocumentClient,
    ) -> Result<(), StorageLibraryError> {
        self.memory.validate_owner(client)
    }

    pub(crate) fn from_encoded(
        id: GameId,
        observed: GameStorageRevision,
        body: &str,
        memory: StorageLibraryPreparedMemory,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<Self, StorageLibraryError> {
        memory.check_body(body.len())?;
        let record = StorageGameDocument::recover_record_cancellable(body, cancellation)?;
        if record.id() != id {
            return Err(StorageLibraryError::Identity);
        }
        let prepared = Self {
            id,
            observed,
            projection: Box::new(StorageLibraryProjection::from_record(
                &record,
                cancellation,
            )?),
            identity: StorageLibraryPreparedIdentity::new(&record, body, cancellation)?,
            memory,
        };
        prepared.memory.check_bytes(
            prepared.transport_bytes()
                + std::mem::size_of::<StorageLibraryEnumTransactionRequest>(),
        )?;
        StorageLibraryError::check_cancellation(cancellation)?;
        Ok(prepared)
    }
    pub(crate) const fn id(&self) -> GameId {
        self.id
    }
    pub(crate) const fn observed(&self) -> GameStorageRevision {
        self.observed
    }
    pub(crate) fn projection(&self) -> &StorageLibraryProjection {
        &self.projection
    }
    pub(crate) fn identity(&self) -> &StorageLibraryPreparedIdentity {
        &self.identity
    }
    pub(crate) fn transport_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.projection.transport_bytes()
            + self.identity.transport_bytes()
    }
}

pub(crate) struct StorageLibraryPreparedIdentity {
    initial_fen: String,
    mainline: String,
    body_digest: PackageDigest,
}

impl StorageLibraryPreparedIdentity {
    fn new(
        record: &StorageLibraryRecord,
        body: &str,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<Self, StorageLibraryError> {
        StorageLibraryError::check_cancellation(cancellation)?;
        let identity = record.identity()?;
        let structural = identity.structural_key();
        let prepared = Self {
            initial_fen: structural.initial_fen().to_owned(),
            mainline: serde_json::to_string(structural.mainline_uci())?,
            body_digest: PackageDigest::hash_slice(body.as_bytes()),
        };
        StorageLibraryError::check_cancellation(cancellation)?;
        Ok(prepared)
    }
    pub(crate) fn initial_fen(&self) -> &str {
        &self.initial_fen
    }
    pub(crate) fn mainline(&self) -> &str {
        &self.mainline
    }
    pub(crate) fn body_digest(&self) -> PackageDigest {
        self.body_digest
    }
    fn transport_bytes(&self) -> usize {
        self.initial_fen.capacity() + self.mainline.capacity()
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct StorageLibraryEnvelopeProbe<'a> {
    schema_version: StorageLibraryEnvelopePresence,
    game: StorageLibraryEnvelopePresence,
    #[serde(borrow)]
    kind: Option<&'a RawValue>,
}

#[derive(Default)]
struct StorageLibraryEnvelopePresence(bool);

impl<'de> Deserialize<'de> for StorageLibraryEnvelopePresence {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        IgnoredAny::deserialize(deserializer)?;
        Ok(Self(true))
    }
}

impl StorageLibraryEnvelopeProbe<'_> {
    pub(crate) fn recognizes(body: &str) -> Result<bool, StorageLibraryError> {
        let probe: StorageLibraryEnvelopeProbe<'_> = serde_json::from_str(body)?;
        let declared = probe
            .kind
            .and_then(|kind| {
                serde_json::from_str::<StorageGameDocumentEnumContentKind>(kind.get()).ok()
            })
            .is_some();
        Ok((probe.schema_version.0 && probe.game.0) || declared)
    }
}

pub(crate) struct StorageLibraryAdoptionPreparation;

impl StorageLibraryAdoptionPreparation {
    pub(crate) fn recognizes(body: &str, associated: bool) -> Result<bool, StorageLibraryError> {
        if associated {
            return Ok(true);
        }
        let first = body.trim_start().as_bytes().first().copied();
        if first != Some(b'{') {
            serde_json::from_str::<IgnoredAny>(body)?;
            return Ok(false);
        }
        StorageLibraryEnvelopeProbe::recognizes(body)
    }
}
