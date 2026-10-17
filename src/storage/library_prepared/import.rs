use std::io::Write;

use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::board::enums::BoardEnumVariant;
use crate::board::position::BoardPosition;
use crate::capacity::executor::CapacityExecutorCancellation;
use crate::game::enums::GameId;
use crate::game::session::{GameMetadataDate, GameMetadataOpeningCode, GameMetadataText};
use crate::package::digest::PackageDigest;
use crate::storage::document_store::StorageDocumentClient;
use crate::storage::game_document::StorageGameDocumentEnumContentKind;
use crate::storage::import_session::{
    StorageImportSessionBodyProof, StorageImportSessionPreparedFrame,
};
use crate::storage::library_prepared::{
    StorageLibraryEnumResult, StorageLibraryPreparedIdentity, StorageLibraryPreparedMemory,
    StorageLibraryProjection, StorageLibraryProjectionOptions,
};
use crate::storage::library_record::StorageLibraryRecord;
use crate::storage::library_repository::StorageLibraryError;

pub struct StorageLibraryImportPreparedProjection {
    bytes: Vec<u8>,
    _memory: StorageLibraryPreparedMemory,
}

impl AsRef<[u8]> for StorageLibraryImportPreparedProjection {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageLibraryImportProjectionWire {
    version: u32,
    origin: uuid::Uuid,
    game: String,
    body_bytes: u64,
    body_digest: [u8; 32],
    kind: String,
    mode: Option<String>,
    variant: String,
    start: String,
    result: String,
    white: Option<String>,
    white_key: Option<String>,
    black: Option<String>,
    black_key: Option<String>,
    date: Option<(Option<u16>, Option<u8>, Option<u8>)>,
    opening: Option<String>,
    opening_key: Option<String>,
    code: Option<String>,
    plies: u32,
    initial_fen: String,
    mainline: String,
}

impl StorageLibraryImportPreparedProjection {
    pub(crate) fn prepare(
        client: &StorageDocumentClient,
        record: &StorageLibraryRecord,
        proof: StorageImportSessionBodyProof<'_>,
        cancellation: &CapacityExecutorCancellation,
    ) -> Result<Self, StorageLibraryError> {
        if !proof.proves_record(record) {
            return Err(StorageLibraryError::Identity);
        }
        let body_bytes = usize::try_from(proof.byte_length())?;
        let reservation = client
            .reserve_library_memory(StorageLibraryPreparedMemory::reservation_bytes(body_bytes)?)?;
        let mut memory = StorageLibraryPreparedMemory::new(body_bytes, reservation)?;
        let projection = StorageLibraryProjection::from_record(record, cancellation)?;
        if projection.kind() != StorageGameDocumentEnumContentKind::Notation
            || projection.mode().is_some()
        {
            return Err(StorageLibraryError::ContentKind);
        }
        let identity = StorageLibraryPreparedIdentity::new(record, proof.body(), cancellation)?;
        if identity.body_digest().bytes() != &proof.digest() {
            return Err(StorageLibraryError::Identity);
        }
        let discriminants = [
            projection.kind().as_ref(),
            projection.variant().as_ref(),
            projection.start().as_ref(),
            StorageLibraryEnumResult::from(projection.result()).as_ref(),
        ]
        .into_iter()
        .try_fold(uuid::fmt::Hyphenated::LENGTH, |sum, value| {
            sum.checked_add(value.len())
                .ok_or(StorageLibraryError::TextTooLarge)
        })?;
        let capture_bytes = projection
            .transport_bytes()
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(identity.transport_bytes()))
            .and_then(|bytes| {
                bytes.checked_add(std::mem::size_of::<StorageLibraryImportProjectionWire>())
            })
            .and_then(|bytes| bytes.checked_add(discriminants))
            .ok_or(StorageLibraryError::TextTooLarge)?;
        memory.grow(capture_bytes)?;
        let wire = StorageLibraryImportProjectionWire::capture(
            client,
            record.id(),
            &proof,
            &projection,
            identity,
        );
        let mut counter = StorageLibraryImportProjectionCounter(0);
        serde_json::to_writer(&mut counter, &wire)?;
        let retained = wire.retained_bytes()?;
        let required = retained
            .checked_add(counter.0)
            .and_then(|bytes| bytes.checked_add(projection.transport_bytes()))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Self>()))
            .ok_or(StorageLibraryError::TextTooLarge)?;
        memory.grow(required)?;
        let mut writer = StorageLibraryImportProjectionWriter(Vec::with_capacity(counter.0));
        serde_json::to_writer(&mut writer, &wire)?;
        StorageLibraryError::check_cancellation(cancellation)?;
        Ok(Self {
            bytes: writer.0,
            _memory: memory,
        })
    }
}

impl StorageLibraryImportProjectionWire {
    const VERSION: u32 = 1;

    fn capture(
        client: &StorageDocumentClient,
        game: GameId,
        proof: &StorageImportSessionBodyProof<'_>,
        projection: &StorageLibraryProjection,
        identity: StorageLibraryPreparedIdentity,
    ) -> Self {
        Self {
            version: Self::VERSION,
            origin: client.library_preparation_origin(),
            game: game.as_uuid().to_string(),
            body_bytes: proof.byte_length(),
            body_digest: proof.digest(),
            kind: projection.kind().as_ref().to_owned(),
            mode: projection.mode().map(|mode| mode.as_ref().to_owned()),
            variant: projection.variant().as_ref().to_owned(),
            start: projection.start().as_ref().to_owned(),
            result: StorageLibraryEnumResult::from(projection.result())
                .as_ref()
                .to_owned(),
            white: projection.white().map(|text| text.as_str().to_owned()),
            white_key: projection.white_key().map(str::to_owned),
            black: projection.black().map(|text| text.as_str().to_owned()),
            black_key: projection.black_key().map(str::to_owned),
            date: projection
                .date()
                .map(|date| (date.year(), date.month(), date.day())),
            opening: projection.opening().map(|text| text.as_str().to_owned()),
            opening_key: projection.opening_key().map(str::to_owned),
            code: projection.code().map(|code| code.as_str().to_owned()),
            plies: projection.plies(),
            initial_fen: identity.initial_fen,
            mainline: identity.mainline,
        }
    }

    fn retained_bytes(&self) -> Result<usize, StorageLibraryError> {
        let fixed = [
            &self.game,
            &self.kind,
            &self.variant,
            &self.start,
            &self.result,
            &self.initial_fen,
            &self.mainline,
        ]
        .into_iter()
        .try_fold(std::mem::size_of::<Self>(), |sum, text| {
            sum.checked_add(text.capacity())
                .ok_or(StorageLibraryError::TextTooLarge)
        })?;
        [
            &self.mode,
            &self.white,
            &self.white_key,
            &self.black,
            &self.black_key,
            &self.opening,
            &self.opening_key,
            &self.code,
        ]
        .into_iter()
        .flatten()
        .try_fold(fixed, |sum, text| {
            sum.checked_add(text.capacity())
                .ok_or(StorageLibraryError::TextTooLarge)
        })
    }
}

pub(crate) struct StorageLibraryImportRecoveredProjection {
    projection: StorageLibraryProjection,
    identity: StorageLibraryPreparedIdentity,
}

impl StorageLibraryImportRecoveredProjection {
    pub(crate) fn recover(
        frame: &StorageImportSessionPreparedFrame<'_>,
        origin: uuid::Uuid,
    ) -> Result<Self, StorageLibraryError> {
        let wire: StorageLibraryImportProjectionWire = serde_json::from_slice(frame.projection())?;
        if wire.origin != origin {
            return Err(StorageLibraryError::ForeignPreparation);
        }
        if wire.version != StorageLibraryImportProjectionWire::VERSION
            || wire.game.parse::<GameId>()? != frame.game()
            || wire.body_bytes != u64::try_from(frame.body().len())?
            || wire.body_digest != frame.body_digest()
            || PackageDigest::hash_slice(frame.body()).bytes() != &wire.body_digest
            || PackageDigest::hash_slice(frame.projection()).bytes() != &frame.projection_digest()
        {
            return Err(StorageLibraryError::ImportProof);
        }
        let kind = wire
            .kind
            .parse::<StorageGameDocumentEnumContentKind>()
            .map_err(|_error| StorageLibraryError::StoredDiscriminant)?;
        if kind != StorageGameDocumentEnumContentKind::Notation || wire.mode.is_some() {
            return Err(StorageLibraryError::ContentKind);
        }
        let variant = wire
            .variant
            .parse::<BoardEnumVariant>()
            .map_err(|_error| StorageLibraryError::StoredDiscriminant)?;
        let position = BoardPosition::from_fen(&wire.initial_fen, variant)
            .map_err(|_position| StorageLibraryError::ImportProof)?;
        if position.fen() != wire.initial_fen {
            return Err(StorageLibraryError::ImportProof);
        }
        let line: StorageLibraryImportMainline = serde_json::from_str(&wire.mainline)?;
        if line.plies != wire.plies || line.bytes != wire.mainline.len() {
            return Err(StorageLibraryError::ImportProof);
        }
        let projection = StorageLibraryProjection::new(StorageLibraryProjectionOptions {
            kind,
            variant,
            start: wire
                .start
                .parse()
                .map_err(|_error| StorageLibraryError::StoredDiscriminant)?,
            mode: None,
            result: wire
                .result
                .parse::<StorageLibraryEnumResult>()
                .map_err(|_error| StorageLibraryError::StoredDiscriminant)?
                .into(),
            white: wire.white.map(GameMetadataText::new).transpose()?,
            black: wire.black.map(GameMetadataText::new).transpose()?,
            date: wire
                .date
                .map(|(year, month, day)| GameMetadataDate::new(year, month, day))
                .transpose()?,
            opening: wire.opening.map(GameMetadataText::new).transpose()?,
            code: wire.code.map(GameMetadataOpeningCode::new).transpose()?,
            plies: wire.plies,
        })?;
        if projection.white_key() != wire.white_key.as_deref()
            || projection.black_key() != wire.black_key.as_deref()
            || projection.opening_key() != wire.opening_key.as_deref()
            || projection
                .date()
                .map(|date| (date.year(), date.month(), date.day()))
                != wire.date
        {
            return Err(StorageLibraryError::ImportProof);
        }
        Ok(Self {
            projection,
            identity: StorageLibraryPreparedIdentity {
                initial_fen: wire.initial_fen,
                mainline: wire.mainline,
                body_digest: PackageDigest::hash_slice(frame.body()),
            },
        })
    }
    pub(crate) const fn projection(&self) -> &StorageLibraryProjection {
        &self.projection
    }
    pub(crate) const fn identity(&self) -> &StorageLibraryPreparedIdentity {
        &self.identity
    }
}

struct StorageLibraryImportMainline {
    plies: u32,
    bytes: usize,
}

impl<'de> Deserialize<'de> for StorageLibraryImportMainline {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_seq(StorageLibraryImportMainlineVisitor)
    }
}

struct StorageLibraryImportMainlineVisitor;

impl<'de> Visitor<'de> for StorageLibraryImportMainlineVisitor {
    type Value = StorageLibraryImportMainline;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a canonical bounded UCI sequence")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut line = StorageLibraryImportMainline { plies: 0, bytes: 2 };
        while let Some(token) = sequence.next_element::<&str>()? {
            let movement: shakmaty::uci::UciMove =
                token.parse().map_err(serde::de::Error::custom)?;
            if movement.to_string() != token {
                return Err(serde::de::Error::custom("noncanonical UCI"));
            }
            line.bytes = line
                .bytes
                .checked_add(token.len() + 2 + usize::from(line.plies != 0))
                .ok_or_else(|| serde::de::Error::custom("UCI length overflow"))?;
            line.plies = line
                .plies
                .checked_add(1)
                .filter(|plies| *plies <= StorageLibraryProjection::MAX_PLIES)
                .ok_or_else(|| serde::de::Error::custom("UCI ply overflow"))?;
        }
        Ok(line)
    }
}

struct StorageLibraryImportProjectionCounter(usize);

impl Write for StorageLibraryImportProjectionCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or(std::io::ErrorKind::OutOfMemory)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct StorageLibraryImportProjectionWriter(Vec<u8>);

impl Write for StorageLibraryImportProjectionWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.0.capacity() - self.0.len() {
            return Err(std::io::ErrorKind::OutOfMemory.into());
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
