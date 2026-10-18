use std::collections::BTreeSet;

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};

use crate::notation::game_identity::NotationGameIdentity;
use crate::package::digest::PackageDigest;
use crate::puzzle::identity::PuzzleDefinitionIdentity;
use crate::storage::dataset_package::{
    StorageDatasetPackageError, StorageDatasetPackageSourceIdentity,
};
use crate::storage::dataset_selection::{
    StorageDatasetSelection, StorageDatasetSelectionEnumStratum,
    StorageDatasetSelectionGameClassification, StorageDatasetSelectionIdentity,
    StorageDatasetSelectionPuzzleClassification,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StorageDatasetPackageRawFieldRecord")]
pub struct StorageDatasetPackageRawField {
    name: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageDatasetPackageRawFieldRecord {
    name: String,
    value: String,
}

impl TryFrom<StorageDatasetPackageRawFieldRecord> for StorageDatasetPackageRawField {
    type Error = StorageDatasetPackageError;
    fn try_from(record: StorageDatasetPackageRawFieldRecord) -> Result<Self, Self::Error> {
        Self::new(record.name, record.value)
    }
}

impl StorageDatasetPackageRawField {
    pub fn new(name: String, value: String) -> Result<Self, StorageDatasetPackageError> {
        StorageDatasetPackageSourceIdentity::validate_text(&name)?;
        if value.contains('\0') {
            return Err(StorageDatasetPackageError::InvalidText);
        }
        Ok(Self { name, value })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StorageDatasetPackageSourceRecordInput")]
pub struct StorageDatasetPackageSourceRecord {
    source: StorageDatasetPackageSourceIdentity,
    record_id: String,
    raw_checksum: PackageDigest,
    raw_fields: Vec<StorageDatasetPackageRawField>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageDatasetPackageSourceRecordInput {
    source: StorageDatasetPackageSourceIdentity,
    record_id: String,
    raw_checksum: PackageDigest,
    raw_fields: Vec<StorageDatasetPackageRawField>,
}

impl TryFrom<StorageDatasetPackageSourceRecordInput> for StorageDatasetPackageSourceRecord {
    type Error = StorageDatasetPackageError;
    fn try_from(record: StorageDatasetPackageSourceRecordInput) -> Result<Self, Self::Error> {
        Self::new(
            record.source,
            record.record_id,
            record.raw_checksum,
            record.raw_fields,
        )
    }
}

impl StorageDatasetPackageSourceRecord {
    pub fn new(
        source: StorageDatasetPackageSourceIdentity,
        record_id: String,
        raw_checksum: PackageDigest,
        raw_fields: Vec<StorageDatasetPackageRawField>,
    ) -> Result<Self, StorageDatasetPackageError> {
        PuzzleDefinitionIdentity::new(
            source.source().to_owned(),
            source.version().to_owned(),
            record_id.clone(),
        )?;
        Ok(Self {
            source,
            record_id,
            raw_checksum,
            raw_fields,
        })
    }
    pub const fn source(&self) -> &StorageDatasetPackageSourceIdentity {
        &self.source
    }
    pub fn record_id(&self) -> &str {
        &self.record_id
    }
    pub const fn raw_checksum(&self) -> PackageDigest {
        self.raw_checksum
    }
    pub fn raw_fields(&self) -> &[StorageDatasetPackageRawField] {
        &self.raw_fields
    }
}

#[derive(strum::AsRefStr)]
enum StorageDatasetPackageEnumRawField {
    #[strum(serialize = "WhiteElo")]
    WhiteElo,
    #[strum(serialize = "BlackElo")]
    BlackElo,
    #[strum(serialize = "TimeControl")]
    TimeControl,
    #[strum(serialize = "Rating")]
    Rating,
    #[strum(serialize = "Themes")]
    Themes,
}

impl StorageDatasetPackageEnumRawField {
    fn read(
        self,
        fields: &[StorageDatasetPackageRawField],
    ) -> Result<Option<&str>, StorageDatasetPackageError> {
        let name = self.as_ref();
        let mut values = fields
            .iter()
            .filter(|field| field.name == name)
            .map(|field| field.value.as_str());
        let value = values.next();
        if values.any(|other| Some(other) != value) {
            return Err(StorageDatasetPackageError::AmbiguousMetadata);
        }
        Ok(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatasetPackageEnumClassification {
    Game(StorageDatasetSelectionGameClassification),
    Puzzle(StorageDatasetSelectionPuzzleClassification),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StorageDatasetPackageEnumRecordIdentity {
    Game(NotationGameIdentity),
    Puzzle(PuzzleDefinitionIdentity),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDatasetPackageRecordProvenance {
    identity: StorageDatasetPackageEnumRecordIdentity,
    selection_identity: StorageDatasetSelectionIdentity,
    stratum: StorageDatasetSelectionEnumStratum,
    classification: StorageDatasetPackageEnumClassification,
    representative_index: usize,
    source_records: Vec<StorageDatasetPackageSourceRecord>,
}

impl StorageDatasetPackageRecordProvenance {
    pub fn game(
        identity: NotationGameIdentity,
        representative_index: usize,
        source_records: Vec<StorageDatasetPackageSourceRecord>,
    ) -> Result<Self, StorageDatasetPackageError> {
        let representative = Self::validate_records(&source_records, representative_index)?;
        let raw_fields = representative.raw_fields();
        let classification = StorageDatasetSelection::classify_game(
            StorageDatasetPackageEnumRawField::WhiteElo.read(raw_fields)?,
            StorageDatasetPackageEnumRawField::BlackElo.read(raw_fields)?,
            StorageDatasetPackageEnumRawField::TimeControl.read(raw_fields)?,
        )?;
        let selection_identity = StorageDatasetSelectionIdentity::game(&identity)?;
        Ok(Self {
            identity: StorageDatasetPackageEnumRecordIdentity::Game(identity),
            selection_identity,
            stratum: StorageDatasetSelectionEnumStratum::Game(classification.stratum().clone()),
            classification: StorageDatasetPackageEnumClassification::Game(classification),
            representative_index,
            source_records,
        })
    }

    pub fn puzzle(
        identity: PuzzleDefinitionIdentity,
        representative_index: usize,
        source_records: Vec<StorageDatasetPackageSourceRecord>,
    ) -> Result<Self, StorageDatasetPackageError> {
        let representative = Self::validate_records(&source_records, representative_index)?;
        if representative.source.source() != identity.source()
            || representative.source.version() != identity.version()
            || source_records
                .iter()
                .any(|record| record.record_id != identity.puzzle_id())
        {
            return Err(StorageDatasetPackageError::SourceBinding);
        }
        if source_records
            .windows(2)
            .any(|pair| pair[0].raw_checksum != pair[1].raw_checksum)
        {
            return Err(StorageDatasetPackageError::SourceBinding);
        }
        let raw_fields = representative.raw_fields();
        let classification = StorageDatasetSelection::classify_puzzle(
            StorageDatasetPackageEnumRawField::Rating.read(raw_fields)?,
            StorageDatasetPackageEnumRawField::Themes.read(raw_fields)?,
        )?;
        let selection_identity = StorageDatasetSelectionIdentity::puzzle(&identity);
        Ok(Self {
            identity: StorageDatasetPackageEnumRecordIdentity::Puzzle(identity),
            selection_identity,
            stratum: StorageDatasetSelectionEnumStratum::Puzzle(classification.stratum().clone()),
            classification: StorageDatasetPackageEnumClassification::Puzzle(classification),
            representative_index,
            source_records,
        })
    }

    fn validate_records(
        records: &[StorageDatasetPackageSourceRecord],
        representative_index: usize,
    ) -> Result<&StorageDatasetPackageSourceRecord, StorageDatasetPackageError> {
        StorageDatasetPackageSourceIdentity::validate_record_bindings(
            records
                .iter()
                .map(StorageDatasetPackageSourceRecord::source),
        )?;
        let representative = records
            .get(representative_index)
            .ok_or(StorageDatasetPackageError::SourceBinding)?;
        let mut identities = BTreeSet::new();
        for record in records {
            if !identities.insert((&record.source, record.record_id.as_str())) {
                return Err(StorageDatasetPackageError::DuplicateSource);
            }
        }
        Ok(representative)
    }

    pub fn game_identity(&self) -> Option<&NotationGameIdentity> {
        match &self.identity {
            StorageDatasetPackageEnumRecordIdentity::Game(identity) => Some(identity),
            StorageDatasetPackageEnumRecordIdentity::Puzzle(_) => None,
        }
    }
    pub fn puzzle_identity(&self) -> Option<&PuzzleDefinitionIdentity> {
        match &self.identity {
            StorageDatasetPackageEnumRecordIdentity::Game(_) => None,
            StorageDatasetPackageEnumRecordIdentity::Puzzle(identity) => Some(identity),
        }
    }
    pub const fn selection_identity(&self) -> &StorageDatasetSelectionIdentity {
        &self.selection_identity
    }
    pub const fn stratum(&self) -> &StorageDatasetSelectionEnumStratum {
        &self.stratum
    }
    pub const fn classification(&self) -> &StorageDatasetPackageEnumClassification {
        &self.classification
    }
    pub const fn representative_index(&self) -> usize {
        self.representative_index
    }
    pub fn representative(&self) -> &StorageDatasetPackageSourceRecord {
        &self.source_records[self.representative_index]
    }
    pub fn source_records(&self) -> &[StorageDatasetPackageSourceRecord] {
        &self.source_records
    }
}

impl Serialize for StorageDatasetPackageRecordProvenance {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("StorageDatasetPackageRecordProvenance", 6)?;
        record.serialize_field("selection_identity", self.selection_identity.bytes())?;
        record.serialize_field("identity_digest", &self.selection_identity.digest())?;
        record.serialize_field("stratum", &self.stratum)?;
        record.serialize_field("classification", &self.classification)?;
        record.serialize_field("representative_index", &self.representative_index)?;
        record.serialize_field("source_records", &self.source_records)?;
        record.end()
    }
}
