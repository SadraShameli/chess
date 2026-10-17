use std::io::Cursor;
use std::num::NonZeroUsize;

use chess::board::position::BoardPosition;
use chess::capacity::budget::{CapacityBudget, CapacityBudgetError};
use chess::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use chess::game::session::{GameMetadata, GameMetadataKnownTags, GameMetadataText};
use chess::notation::game_identity::{NotationGameIdentity, NotationGameIdentityEnumResult};
use chess::package::digest::PackageDigest;
use chess::puzzle::identity::PuzzleDefinitionIdentity;
use chess::storage::dataset_package::{
    StorageDatasetPackage, StorageDatasetPackageEnumFormat, StorageDatasetPackageEnumValidation,
    StorageDatasetPackageError,
};
use chess::storage::dataset_package::{
    StorageDatasetPackageArtifact, StorageDatasetPackageArtifactInput,
    StorageDatasetPackageArtifactReference, StorageDatasetPackageCounts,
    StorageDatasetPackageEnumArtifactRole, StorageDatasetPackageEnumClassification,
    StorageDatasetPackageEnumLicense, StorageDatasetPackageEnumSemanticValidation,
    StorageDatasetPackageEnumSourceKind, StorageDatasetPackageEnumUnavailableCategory,
    StorageDatasetPackageEnumUnavailableReason, StorageDatasetPackageLicense,
    StorageDatasetPackageManifest, StorageDatasetPackageManifestInput,
    StorageDatasetPackageObservation, StorageDatasetPackageObservationInput,
    StorageDatasetPackageRawField, StorageDatasetPackageRecordProvenance,
    StorageDatasetPackageSource, StorageDatasetPackageSourceIdentity,
    StorageDatasetPackageSourceRecord, StorageDatasetPackageUnavailable,
};
use chess::storage::dataset_selection::{
    StorageDatasetSelection, StorageDatasetSelectionCandidate,
    StorageDatasetSelectionEnumComponent, StorageDatasetSelectionEnumDuplicate,
    StorageDatasetSelectionEnumGameRatingBand, StorageDatasetSelectionEnumRating,
    StorageDatasetSelectionEnumStratum, StorageDatasetSelectionEnumSubset,
    StorageDatasetSelectionEnumTimeClass, StorageDatasetSelectionIdentity,
    StorageDatasetSelectionPrefixes, StorageDatasetSelectionSequence,
};

type StorageDatasetPackageTestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn review_puzzle_provenance_refuses_an_unrelated_retained_puzzle_id()
-> StorageDatasetPackageTestResult {
    let first = StorageDatasetPackageSourceIdentity::new(
        "A".into(),
        "v1".into(),
        PackageDigest::hash_slice(b"manifest A"),
    )?;
    let second = StorageDatasetPackageSourceIdentity::new(
        "B".into(),
        "v1".into(),
        PackageDigest::hash_slice(b"manifest B"),
    )?;
    let checksum = PackageDigest::hash_slice(b"one original record");
    let record = |source, id: &str| {
        StorageDatasetPackageSourceRecord::new(source, id.into(), checksum, vec![])
    };
    let identity = PuzzleDefinitionIdentity::new("A".into(), "v1".into(), "p1".into())?;
    let valid = StorageDatasetPackageRecordProvenance::puzzle(
        identity.clone(),
        0,
        vec![record(first.clone(), "p1")?, record(second.clone(), "p1")?],
    );
    assert!(valid.is_ok());
    let invalid = StorageDatasetPackageRecordProvenance::puzzle(
        identity,
        0,
        vec![record(first, "p1")?, record(second, "p2")?],
    );
    assert!(matches!(
        invalid,
        Err(StorageDatasetPackageError::SourceBinding)
    ));
    Ok(())
}

#[test]
fn review_record_provenance_refuses_conflicting_frozen_source_versions()
-> StorageDatasetPackageTestResult {
    let first = StorageDatasetPackageSourceIdentity::new(
        "games".into(),
        "v1".into(),
        PackageDigest::hash_slice(b"manifest A"),
    )?;
    let conflicting = StorageDatasetPackageSourceIdentity::new(
        "games".into(),
        "v1".into(),
        PackageDigest::hash_slice(b"manifest B"),
    )?;
    let checksum = PackageDigest::hash_slice(b"record");
    let record = |source, id: &str| {
        StorageDatasetPackageSourceRecord::new(source, id.into(), checksum, vec![])
    };
    let valid = StorageDatasetPackageRecordProvenance::game(
        StorageDatasetPackageManifestFixture::game()?,
        0,
        vec![
            record(first.clone(), "record1")?,
            record(first.clone(), "record2")?,
        ],
    );
    assert!(valid.is_ok());
    let invalid = StorageDatasetPackageRecordProvenance::game(
        StorageDatasetPackageManifestFixture::game()?,
        0,
        vec![record(first, "record1")?, record(conflicting, "record2")?],
    );
    assert!(matches!(
        invalid,
        Err(StorageDatasetPackageError::SourceBinding)
    ));
    Ok(())
}

#[test]
fn review_game_provenance_retains_copy_metadata_and_binds_the_selected_representative()
-> StorageDatasetPackageTestResult {
    let identity = NotationGameIdentity::new(
        &BoardPosition::standard(),
        &[],
        &GameMetadata::new(
            GameMetadataKnownTags {
                white_elo: Some(2400),
                black_elo: Some(2400),
                ..GameMetadataKnownTags::default()
            },
            vec![],
        )?,
        Some(NotationGameIdentityEnumResult::Unfinished),
    )?;
    let duplicate_identity = NotationGameIdentity::new(
        &BoardPosition::standard(),
        &[],
        &GameMetadata::new(
            GameMetadataKnownTags {
                white_elo: Some(1100),
                black_elo: Some(1100),
                ..GameMetadataKnownTags::default()
            },
            vec![],
        )?,
        Some(NotationGameIdentityEnumResult::Unfinished),
    )?;
    assert_eq!(identity, duplicate_identity);
    let selection_identity = StorageDatasetSelectionIdentity::game(&identity)?;
    let duplicate_selection_identity = StorageDatasetSelectionIdentity::game(&duplicate_identity)?;
    let first_raw = "[WhiteElo \"2400\"]\n[BlackElo \"2400\"]\n[TimeControl \"300+0\"]\n[Custom \"retained first\"]\n*";
    let second_raw = "[WhiteElo \"1100\"]\n[BlackElo \"1100\"]\n[TimeControl \"60+0\"]\n[Custom \"retained second\"]\n*";
    assert_eq!(
        StorageDatasetSelection::compare_duplicate(
            &selection_identity,
            first_raw,
            &duplicate_selection_identity,
            second_raw,
        )?,
        StorageDatasetSelectionEnumDuplicate::Replace
    );
    let first = StorageDatasetPackageSourceRecord::new(
        StorageDatasetPackageSourceIdentity::new(
            "A".into(),
            "v1".into(),
            PackageDigest::hash_slice(b"manifest A"),
        )?,
        "first".into(),
        PackageDigest::hash_slice(first_raw.as_bytes()),
        StorageDatasetPackageManifestFixture::raw_fields(&[
            ("WhiteElo", "2400"),
            ("BlackElo", "2400"),
            ("TimeControl", "300+0"),
            ("Custom", "retained first"),
        ])?,
    )?;
    let second = StorageDatasetPackageSourceRecord::new(
        StorageDatasetPackageSourceIdentity::new(
            "Z".into(),
            "v1".into(),
            PackageDigest::hash_slice(b"manifest Z"),
        )?,
        "second".into(),
        PackageDigest::hash_slice(second_raw.as_bytes()),
        StorageDatasetPackageManifestFixture::raw_fields(&[
            ("WhiteElo", "1100"),
            ("BlackElo", "1100"),
            ("TimeControl", "60+0"),
            ("Custom", "retained second"),
        ])?,
    )?;
    for (representative_index, source_records) in [
        (1, vec![first.clone(), second.clone()]),
        (0, vec![second.clone(), first]),
    ] {
        let provenance = StorageDatasetPackageRecordProvenance::game(
            identity.clone(),
            representative_index,
            source_records.clone(),
        )?;
        assert_eq!(provenance.game_identity(), Some(&identity));
        assert_eq!(provenance.selection_identity(), &selection_identity);
        assert_eq!(provenance.representative_index(), representative_index);
        assert_eq!(provenance.representative(), &second);
        assert_eq!(provenance.source_records(), source_records);
        let StorageDatasetPackageEnumClassification::Game(classification) =
            provenance.classification()
        else {
            return Err("game provenance must retain a game classification".into());
        };
        assert_eq!(
            classification.stratum().rating_band(),
            StorageDatasetSelectionEnumGameRatingBand::Below1200
        );
        assert_eq!(
            classification.stratum().time_class(),
            StorageDatasetSelectionEnumTimeClass::Bullet
        );
        assert_eq!(
            classification.white_rating(),
            StorageDatasetSelectionEnumRating::Known(1100)
        );
        assert_eq!(
            classification.black_rating(),
            StorageDatasetSelectionEnumRating::Known(1100)
        );
        let serialized = serde_json::to_string(&provenance)?;
        let retained: StorageDatasetPackageProvenanceFixtureRecord =
            serde_json::from_str(&serialized)?;
        assert_eq!(retained.selection_identity, selection_identity.bytes());
        assert_eq!(retained.identity_digest, selection_identity.digest());
        assert_eq!(&retained.stratum, provenance.stratum());
        assert_eq!(
            retained.classification.get(),
            r#"{"game":{"stratum":{"rating_band":"below1200","time_class":"bullet"},"white_rating":{"known":1100},"black_rating":{"known":1100},"time_reason":null}}"#
        );
        assert_eq!(retained.representative_index, representative_index);
        assert_eq!(retained.source_records, source_records);
        assert_eq!(
            retained.source_records.get(retained.representative_index),
            Some(&second)
        );
    }
    Ok(())
}

#[test]
fn review_provenance_refuses_a_missing_representative() -> StorageDatasetPackageTestResult {
    let source = StorageDatasetPackageSourceIdentity::new(
        "games".into(),
        "v1".into(),
        PackageDigest::hash_slice(b"manifest"),
    )?;
    let record = StorageDatasetPackageSourceRecord::new(
        source,
        "record".into(),
        PackageDigest::hash_slice(b"raw"),
        vec![],
    )?;
    for representative_index in [1, usize::MAX] {
        assert!(matches!(
            StorageDatasetPackageRecordProvenance::game(
                StorageDatasetPackageManifestFixture::game()?,
                representative_index,
                vec![record.clone()],
            ),
            Err(StorageDatasetPackageError::SourceBinding)
        ));
    }
    assert!(matches!(
        StorageDatasetPackageRecordProvenance::game(
            StorageDatasetPackageManifestFixture::game()?,
            0,
            vec![],
        ),
        Err(StorageDatasetPackageError::SourceBinding)
    ));
    Ok(())
}

#[test]
fn review_puzzle_representative_binds_its_retained_source_identity()
-> StorageDatasetPackageTestResult {
    let records = ["A", "B"]
        .into_iter()
        .map(|source| {
            StorageDatasetPackageSourceRecord::new(
                StorageDatasetPackageSourceIdentity::new(
                    source.into(),
                    "v1".into(),
                    PackageDigest::hash_slice(source.as_bytes()),
                )?,
                "p1".into(),
                PackageDigest::hash_slice(b"same puzzle record"),
                vec![],
            )
        })
        .collect::<Result<Vec<_>, StorageDatasetPackageError>>()?;
    let provenance = StorageDatasetPackageRecordProvenance::puzzle(
        PuzzleDefinitionIdentity::new("B".into(), "v1".into(), "p1".into())?,
        1,
        records.clone(),
    )?;
    assert_eq!(provenance.representative().source().source(), "B");
    assert_eq!(provenance.source_records(), records);
    assert!(matches!(
        StorageDatasetPackageRecordProvenance::puzzle(
            PuzzleDefinitionIdentity::new("A".into(), "v1".into(), "p1".into())?,
            1,
            records,
        ),
        Err(StorageDatasetPackageError::SourceBinding)
    ));
    Ok(())
}

#[test]
fn review_source_record_metadata_roundtrips_and_validates_each_external_field()
-> StorageDatasetPackageTestResult {
    let source = StorageDatasetPackageManifestFixture::source(
        StorageDatasetPackageEnumSourceKind::StandardGames,
    )?;
    let record = StorageDatasetPackageSourceRecord::new(
        source.identity().clone(),
        "record".into(),
        PackageDigest::hash_slice(b"original"),
        StorageDatasetPackageManifestFixture::raw_fields(&[
            ("WhiteElo", " invalid "),
            ("Custom", "retained"),
            ("Custom", "second"),
        ])?,
    )?;
    let encoded = serde_json::to_string(&record)?;
    let decoded: StorageDatasetPackageSourceRecord = serde_json::from_str(&encoded)?;
    assert_eq!(decoded, record);
    for (before, after) in [
        (r#""WhiteElo""#, r#"" WhiteElo""#),
        (r#""retained""#, r#""\u0000""#),
    ] {
        assert!(encoded.contains(before));
        assert!(
            serde_json::from_str::<StorageDatasetPackageSourceRecord>(
                &encoded.replacen(before, after, 1)
            )
            .is_err()
        );
    }
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageDatasetPackageProvenanceFixtureRecord {
    selection_identity: Vec<u8>,
    identity_digest: PackageDigest,
    stratum: StorageDatasetSelectionEnumStratum,
    classification: Box<serde_json::value::RawValue>,
    representative_index: usize,
    source_records: Vec<StorageDatasetPackageSourceRecord>,
}

use chess::storage::dataset_package::{
    StorageDatasetPackageLicenseId, StorageDatasetPackageMeasurement,
    StorageDatasetPackageMeasurements, StorageDatasetPackageReduction,
    StorageDatasetPackageSemanticReceipt,
};

#[test]
fn license_identifiers_validate_at_the_external_boundary() -> StorageDatasetPackageTestResult {
    let id = StorageDatasetPackageLicenseId::new("CC-BY-4.0".into())?;
    let license = StorageDatasetPackageLicense::new(
        StorageDatasetPackageEnumLicense::Other(id),
        StorageDatasetPackageManifestFixture::reference("licenses/terms.txt")?,
    );
    let decoded: StorageDatasetPackageLicense =
        serde_json::from_str(&serde_json::to_string(&license)?)?;
    assert_eq!(decoded, license);
    assert!(serde_json::from_str::<StorageDatasetPackageLicenseId>("\" \"").is_err());
    Ok(())
}

#[test]
fn installed_inventory_refuses_portable_path_aliases_and_file_directory_conflicts()
-> StorageDatasetPackageTestResult {
    for path in ["LICENSES/CC0.TXT", "licenses/cc0.txt/child", "licenses"] {
        let mut input = StorageDatasetPackageManifestFixture::input()?;
        input
            .artifacts
            .push(StorageDatasetPackageManifestFixture::artifact(
                path,
                StorageDatasetSelectionEnumComponent::Labels,
                StorageDatasetPackageEnumArtifactRole::Index,
                vec![input.sources[0].identity().clone()],
            )?);
        assert!(StorageDatasetPackageManifest::new(input).is_err());
    }
    Ok(())
}

#[test]
fn reproduction_refuses_changed_membership_even_when_stratum_counts_match()
-> StorageDatasetPackageTestResult {
    let manifest = StorageDatasetPackageManifestFixture::manifest()?;
    let changed = StorageDatasetSelectionSequence::new(
        StorageDatasetSelectionEnumSubset::Puzzles,
        vec![StorageDatasetSelectionCandidate::new(
            0,
            StorageDatasetSelectionIdentity::puzzle(&PuzzleDefinitionIdentity::new(
                "Puzzles".into(),
                "frozen-fixture".into(),
                "changed".into(),
            )?),
            StorageDatasetSelectionEnumStratum::Puzzle(
                StorageDatasetSelection::classify_puzzle(Some("249"), Some("mate"))?
                    .stratum()
                    .clone(),
            ),
        )?],
    )?;
    assert!(
        manifest
            .validate_reproduction(StorageDatasetSelectionEnumSubset::Puzzles, &changed)
            .is_err()
    );
    Ok(())
}

#[test]
fn malformed_references_digests_strata_and_fixed_measurements_are_rejected()
-> StorageDatasetPackageTestResult {
    let manifest = StorageDatasetPackageManifestFixture::manifest()?;
    let encoded = String::from_utf8(manifest.to_json()?)?;
    let digest = manifest.sources()[0]
        .identity()
        .manifest_digest()
        .to_string();
    for (before, after) in [
        ("\"available\":1", "\"available\":0"),
        ("\"selected_prefix\":1", "\"selected_prefix\":2"),
        ("\"fixed_bytes\":0", "\"fixed_bytes\":18"),
        ("\"stratum\":\"master\"", "\"stratum\":\"unrecognized\""),
        ("Master/provenance.bin", "Master/missing.bin"),
        (digest.as_str(), "bad"),
    ] {
        assert!(encoded.contains(before));
        assert!(
            StorageDatasetPackageManifest::from_json(encoded.replacen(before, after, 1).as_bytes())
                .is_err(),
            "{before}"
        );
    }
    Ok(())
}

#[test]
fn reduction_record_uses_core_transition_and_refuses_forged_history()
-> StorageDatasetPackageTestResult {
    let measurements = StorageDatasetPackageMeasurements::new(
        StorageDatasetSelectionEnumComponent::ORDER
            .into_iter()
            .map(|component| {
                StorageDatasetPackageMeasurement::new(
                    component,
                    if component == StorageDatasetSelectionEnumComponent::Book {
                        component.ceiling_bytes() + 1
                    } else {
                        1
                    },
                    0,
                )
            })
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    let before = StorageDatasetSelectionPrefixes::new(10, 10, 10)?;
    let reduction = StorageDatasetPackageReduction::new(measurements, before)?;
    assert_eq!(reduction.decision().before(), before);
    assert_eq!(
        reduction.decision().after(),
        StorageDatasetSelectionPrefixes::new(9, 10, 10)?
    );
    let json = serde_json::to_string(&reduction)?;
    let decoded: StorageDatasetPackageReduction = serde_json::from_str(&json)?;
    assert_eq!(decoded, reduction);
    assert!(
        serde_json::from_str::<StorageDatasetPackageReduction>(&json.replacen(
            "\"master\":9",
            "\"master\":8",
            1
        ))
        .is_err()
    );
    Ok(())
}

#[test]
fn semantic_receipt_must_bind_descriptor_bytes_and_format() -> StorageDatasetPackageTestResult {
    let directory = tempfile::tempdir()?;
    let descriptor = StorageDatasetPackageFixture::descriptor(directory.path().join("source.pgn"))?;
    let receipt = StorageDatasetPackageSemanticReceipt::new(
        StorageDatasetPackageEnumFormat::Pgn,
        descriptor.checksum(),
        1,
        0,
    )?;
    let validated = descriptor.clone().with_semantic_receipt(receipt)?;
    assert_eq!(
        validated.validation(),
        StorageDatasetPackageEnumValidation::Unchecked
    );
    assert_eq!(
        validated.semantic_validation(),
        StorageDatasetPackageEnumSemanticValidation::Validated
    );
    assert!(
        descriptor
            .with_semantic_receipt(StorageDatasetPackageSemanticReceipt::new(
                StorageDatasetPackageEnumFormat::Polyglot,
                PackageDigest::hash_slice(b"other bytes"),
                1,
                0
            )?)
            .is_err()
    );
    Ok(())
}

#[test]
fn prepared_catalog_format_is_distinct_from_interchange_csv() {
    assert_ne!(
        StorageDatasetPackageEnumFormat::PreparedPuzzleCatalogV1,
        StorageDatasetPackageEnumFormat::PuzzleCsv
    );
}

#[test]
fn puzzle_record_identity_preserves_exact_nonempty_source_identifier_bytes()
-> StorageDatasetPackageTestResult {
    let source =
        StorageDatasetPackageManifestFixture::source(StorageDatasetPackageEnumSourceKind::Puzzles)?;
    let identity = PuzzleDefinitionIdentity::new(
        source.identity().source().into(),
        source.identity().version().into(),
        " p1\t".into(),
    )?;
    let provenance = StorageDatasetPackageRecordProvenance::puzzle(
        identity,
        0,
        vec![StorageDatasetPackageSourceRecord::new(
            source.identity().clone(),
            " p1\t".into(),
            PackageDigest::hash_slice(b"original"),
            vec![],
        )?],
    )?;
    assert_eq!(provenance.selection_identity().bytes(), b" p1\t");
    assert_eq!(provenance.source_records()[0].record_id(), " p1\t");
    Ok(())
}

struct StorageDatasetPackageFixture;

impl StorageDatasetPackageFixture {
    const CONTENT: &[u8] = b"integrity does not establish semantic chess validity";

    fn budget() -> Result<CapacityBudget, Box<dyn std::error::Error>> {
        Ok(CapacityBudget::new(
            NonZeroUsize::new(1).ok_or("invalid fixture capacity")?,
        )?)
    }

    fn descriptor(
        path: std::path::PathBuf,
    ) -> Result<StorageDatasetPackage, Box<dyn std::error::Error>> {
        Ok(StorageDatasetPackage::new(
            StorageDatasetPackageEnumFormat::Pgn,
            PackageDigest::read(Cursor::new(Self::CONTENT))?,
            path,
        )?)
    }
}

struct StorageDatasetPackageManifestFixture;

impl StorageDatasetPackageManifestFixture {
    fn raw_fields(
        fields: &[(&str, &str)],
    ) -> Result<Vec<StorageDatasetPackageRawField>, StorageDatasetPackageError> {
        fields
            .iter()
            .map(|&(name, value)| StorageDatasetPackageRawField::new(name.into(), value.into()))
            .collect()
    }

    fn sequence_many(
        subset: StorageDatasetSelectionEnumSubset,
    ) -> Result<StorageDatasetSelectionSequence<u8>, Box<dyn std::error::Error>> {
        let mut candidates = Vec::new();
        for index in 0..10 {
            let (identity, stratum) = match subset {
                StorageDatasetSelectionEnumSubset::Master
                | StorageDatasetSelectionEnumSubset::Broad => {
                    let identity = NotationGameIdentity::new(
                        &BoardPosition::standard(),
                        &[],
                        &GameMetadata::new(
                            GameMetadataKnownTags {
                                white: Some(GameMetadataText::new(format!("White{index}"))?),
                                ..GameMetadataKnownTags::default()
                            },
                            vec![],
                        )?,
                        None,
                    )?;
                    let stratum = if subset == StorageDatasetSelectionEnumSubset::Master {
                        StorageDatasetSelectionEnumStratum::Master
                    } else {
                        StorageDatasetSelectionEnumStratum::Game(
                            StorageDatasetSelection::classify_game(None, None, None)?
                                .stratum()
                                .clone(),
                        )
                    };
                    (StorageDatasetSelectionIdentity::game(&identity)?, stratum)
                }
                StorageDatasetSelectionEnumSubset::Puzzles => (
                    StorageDatasetSelectionIdentity::puzzle(&PuzzleDefinitionIdentity::new(
                        "Puzzles".into(),
                        "frozen-fixture".into(),
                        format!("p{index}"),
                    )?),
                    StorageDatasetSelectionEnumStratum::Puzzle(
                        StorageDatasetSelection::classify_puzzle(Some("249"), Some("mate"))?
                            .stratum()
                            .clone(),
                    ),
                ),
            };
            candidates.push(StorageDatasetSelectionCandidate::new(
                index, identity, stratum,
            )?);
        }
        Ok(StorageDatasetSelectionSequence::new(subset, candidates)?)
    }
    fn reference(
        path: &str,
    ) -> Result<StorageDatasetPackageArtifactReference, Box<dyn std::error::Error>> {
        Ok(StorageDatasetPackageArtifactReference::new(
            path.to_owned(),
            PackageDigest::hash_slice(path.as_bytes()),
        )?)
    }

    fn source(
        kind: StorageDatasetPackageEnumSourceKind,
    ) -> Result<StorageDatasetPackageSource, Box<dyn std::error::Error>> {
        let name = format!("{kind:?}");
        Ok(StorageDatasetPackageSource::new(
            StorageDatasetPackageSourceIdentity::new(
                name.clone(),
                "frozen-fixture".into(),
                PackageDigest::hash_slice(b"acquisition manifest"),
            )?,
            kind,
            format!("https://fixtures.invalid/{name}"),
            PackageDigest::hash_slice(name.as_bytes()),
            StorageDatasetPackageLicense::new(
                StorageDatasetPackageEnumLicense::Cc0,
                Self::reference("licenses/cc0.txt")?,
            ),
        )?)
    }

    fn game() -> Result<NotationGameIdentity, Box<dyn std::error::Error>> {
        Ok(NotationGameIdentity::new(
            &BoardPosition::standard(),
            &[],
            &GameMetadata::new(GameMetadataKnownTags::default(), vec![])?,
            None,
        )?)
    }

    fn sequence(
        subset: StorageDatasetSelectionEnumSubset,
    ) -> Result<StorageDatasetSelectionSequence<u8>, Box<dyn std::error::Error>> {
        let (identity, stratum) = match subset {
            StorageDatasetSelectionEnumSubset::Master => (
                StorageDatasetSelectionIdentity::game(&Self::game()?)?,
                StorageDatasetSelectionEnumStratum::Master,
            ),
            StorageDatasetSelectionEnumSubset::Broad => (
                StorageDatasetSelectionIdentity::game(&Self::game()?)?,
                StorageDatasetSelectionEnumStratum::Game(
                    StorageDatasetSelection::classify_game(None, None, None)?
                        .stratum()
                        .clone(),
                ),
            ),
            StorageDatasetSelectionEnumSubset::Puzzles => (
                StorageDatasetSelectionIdentity::puzzle(&PuzzleDefinitionIdentity::new(
                    "Puzzles".into(),
                    "frozen-fixture".into(),
                    "p1".into(),
                )?),
                StorageDatasetSelectionEnumStratum::Puzzle(
                    StorageDatasetSelection::classify_puzzle(Some("249"), Some("mate"))?
                        .stratum()
                        .clone(),
                ),
            ),
        };
        Ok(StorageDatasetSelectionSequence::new(
            subset,
            vec![StorageDatasetSelectionCandidate::new(0, identity, stratum)?],
        )?)
    }

    fn artifact(
        path: &str,
        component: StorageDatasetSelectionEnumComponent,
        role: StorageDatasetPackageEnumArtifactRole,
        sources: Vec<StorageDatasetPackageSourceIdentity>,
    ) -> Result<StorageDatasetPackageArtifact, Box<dyn std::error::Error>> {
        Ok(StorageDatasetPackageArtifact::new(
            StorageDatasetPackageArtifactInput {
                reference: Self::reference(path)?,
                component,
                role,
                installed_bytes: 17,
                fixed_bytes: 0,
                sources,
            },
        )?)
    }

    fn input() -> Result<StorageDatasetPackageManifestInput, Box<dyn std::error::Error>> {
        let games = Self::source(StorageDatasetPackageEnumSourceKind::StandardGames)?;
        let puzzles = Self::source(StorageDatasetPackageEnumSourceKind::Puzzles)?;
        let openings = Self::source(StorageDatasetPackageEnumSourceKind::Openings)?;
        let mut artifacts = vec![Self::artifact(
            "licenses/cc0.txt",
            StorageDatasetSelectionEnumComponent::Labels,
            StorageDatasetPackageEnumArtifactRole::LicenseCopy,
            vec![
                games.identity().clone(),
                puzzles.identity().clone(),
                openings.identity().clone(),
            ],
        )?];
        let mut observations = Vec::new();
        for subset in StorageDatasetSelectionEnumSubset::ORDER {
            let (component, source) = match subset {
                StorageDatasetSelectionEnumSubset::Master => {
                    (StorageDatasetSelectionEnumComponent::Master, &games)
                }
                StorageDatasetSelectionEnumSubset::Broad => {
                    (StorageDatasetSelectionEnumComponent::Broad, &games)
                }
                StorageDatasetSelectionEnumSubset::Puzzles => {
                    (StorageDatasetSelectionEnumComponent::Puzzles, &puzzles)
                }
            };
            let path = format!("{subset:?}/provenance.bin");
            let provenance = Self::reference(&path)?;
            artifacts.push(Self::artifact(
                &path,
                component,
                StorageDatasetPackageEnumArtifactRole::Provenance,
                vec![source.identity().clone()],
            )?);
            artifacts.push(Self::artifact(
                &format!("{subset:?}/records.bin"),
                component,
                StorageDatasetPackageEnumArtifactRole::Dataset,
                vec![source.identity().clone()],
            )?);
            observations.push(StorageDatasetPackageObservation::from_sequence(
                &Self::sequence(subset)?,
                StorageDatasetPackageObservationInput {
                    subset,
                    sources: vec![source.identity().clone()],
                    counts: StorageDatasetPackageCounts::new(
                        1,
                        1,
                        u64::from(subset == StorageDatasetSelectionEnumSubset::Broad),
                        0,
                        0,
                    )?,
                    requested_prefix: 1,
                    provenance,
                },
            )?);
        }
        artifacts.push(Self::artifact(
            "book.bin",
            StorageDatasetSelectionEnumComponent::Book,
            StorageDatasetPackageEnumArtifactRole::Book,
            vec![games.identity().clone()],
        )?);
        artifacts.push(Self::artifact(
            "labels.bin",
            StorageDatasetSelectionEnumComponent::Labels,
            StorageDatasetPackageEnumArtifactRole::Labels,
            vec![openings.identity().clone()],
        )?);
        Ok(StorageDatasetPackageManifestInput {
            sources: vec![games, puzzles, openings],
            observations,
            artifacts,
            initial_prefixes: StorageDatasetSelectionPrefixes::new(1, 1, 1)?,
            reductions: vec![],
            unavailable: vec![],
        })
    }

    fn manifest() -> Result<StorageDatasetPackageManifest, Box<dyn std::error::Error>> {
        Ok(StorageDatasetPackageManifest::new(Self::input()?)?)
    }
}

#[test]
fn manifest_replays_ordered_book_reduction_and_reproduces_smaller_master_prefix()
-> StorageDatasetPackageTestResult {
    let mut input = StorageDatasetPackageManifestFixture::input()?;
    input.initial_prefixes = StorageDatasetSelectionPrefixes::new(10, 10, 10)?;
    let measurements = StorageDatasetPackageMeasurements::new(
        StorageDatasetSelectionEnumComponent::ORDER
            .into_iter()
            .map(|component| {
                StorageDatasetPackageMeasurement::new(
                    component,
                    if component == StorageDatasetSelectionEnumComponent::Book {
                        component.ceiling_bytes() + 1
                    } else {
                        1
                    },
                    0,
                )
            })
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    input.reductions.push(StorageDatasetPackageReduction::new(
        measurements,
        input.initial_prefixes,
    )?);
    let mut observations = Vec::new();
    for previous in &input.observations {
        let subset = previous.subset();
        observations.push(StorageDatasetPackageObservation::from_sequence(
            &StorageDatasetPackageManifestFixture::sequence_many(subset)?,
            StorageDatasetPackageObservationInput {
                subset,
                sources: previous.sources().to_vec(),
                counts: StorageDatasetPackageCounts::new(
                    10,
                    10,
                    if subset == StorageDatasetSelectionEnumSubset::Broad {
                        10
                    } else {
                        0
                    },
                    0,
                    0,
                )?,
                requested_prefix: if subset == StorageDatasetSelectionEnumSubset::Master {
                    9
                } else {
                    10
                },
                provenance: previous.provenance().clone(),
            },
        )?);
    }
    input.observations = observations;
    let manifest = StorageDatasetPackageManifest::new(input)?;
    assert_eq!(
        manifest.final_prefixes(),
        StorageDatasetSelectionPrefixes::new(9, 10, 10)?
    );
    assert_eq!(manifest.reductions().len(), 1);
    let decoded = StorageDatasetPackageManifest::from_json(&manifest.to_json()?)?;
    assert_eq!(decoded, manifest);
    for subset in StorageDatasetSelectionEnumSubset::ORDER {
        decoded.validate_reproduction(
            subset,
            &StorageDatasetPackageManifestFixture::sequence_many(subset)?,
        )?;
    }
    Ok(())
}

#[test]
fn source_identity_binds_manifest_and_version_independently_of_location()
-> StorageDatasetPackageTestResult {
    let original = StorageDatasetPackageManifestFixture::source(
        StorageDatasetPackageEnumSourceKind::StandardGames,
    )?;
    let identity = original.identity();
    assert_ne!(
        identity,
        &StorageDatasetPackageSourceIdentity::new(
            identity.source().into(),
            identity.version().into(),
            PackageDigest::hash_slice(b"changed manifest")
        )?
    );
    assert_ne!(
        identity,
        &StorageDatasetPackageSourceIdentity::new(
            identity.source().into(),
            "other-version".into(),
            identity.manifest_digest()
        )?
    );
    let encoded = serde_json::to_string(&original)?;
    let decoded: StorageDatasetPackageSource = serde_json::from_str(&encoded)?;
    assert_eq!(original, decoded);
    assert!(
        StorageDatasetPackageSourceIdentity::new(
            " ".into(),
            "v1".into(),
            identity.manifest_digest()
        )
        .is_err()
    );
    assert!(
        StorageDatasetPackageSourceIdentity::new(
            "source".into(),
            "v\0bad".into(),
            identity.manifest_digest()
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn manifest_roundtrip_reproduces_real_sequences_and_installed_accounting()
-> StorageDatasetPackageTestResult {
    let manifest = StorageDatasetPackageManifestFixture::manifest()?;
    let decoded = StorageDatasetPackageManifest::from_json(&manifest.to_json()?)?;
    assert_eq!(manifest, decoded);
    assert_eq!(
        manifest.policy_version(),
        StorageDatasetSelection::POLICY_VERSION
    );
    assert_eq!(
        manifest.final_prefixes(),
        StorageDatasetSelectionPrefixes::new(1, 1, 1)?
    );
    assert_eq!(manifest.measurements().total_bytes(), 153);
    for subset in StorageDatasetSelectionEnumSubset::ORDER {
        manifest.validate_reproduction(
            subset,
            &StorageDatasetPackageManifestFixture::sequence(subset)?,
        )?;
    }
    Ok(())
}

#[test]
fn count_constructor_refuses_inconsistency_unknown_overcount_and_overflow()
-> StorageDatasetPackageTestResult {
    assert!(StorageDatasetPackageCounts::new(9, 5, 2, 1, 2).is_err());
    assert!(StorageDatasetPackageCounts::new(3, 3, 4, 0, 0).is_err());
    assert!(StorageDatasetPackageCounts::new(u64::MAX, u64::MAX, 0, 1, 0).is_err());
    let counts = StorageDatasetPackageCounts::new(9, 5, 2, 1, 3)?;
    assert_eq!(
        (
            counts.observed(),
            counts.available(),
            counts.unknown(),
            counts.rejected(),
            counts.duplicates()
        ),
        (9, 5, 2, 1, 3)
    );
    Ok(())
}

#[test]
fn artifact_paths_refuse_cross_platform_escape_and_noncanonical_aliases()
-> StorageDatasetPackageTestResult {
    for path in [
        "",
        "/root/file",
        "../file",
        "a/../file",
        "a/./file",
        "a//file",
        "a/",
        "C:/file",
        "a\\file",
        "a\0file",
        "a/CON",
        "a/file.",
        "a/file ",
    ] {
        assert!(
            StorageDatasetPackageManifestFixture::reference(path).is_err(),
            "{path:?}"
        );
    }
    assert_eq!(
        StorageDatasetPackageManifestFixture::reference("data/provenance.bin")?.path(),
        "data/provenance.bin"
    );
    Ok(())
}

#[test]
fn manifest_boundary_refuses_bad_versions_counts_paths_and_unknown_fields()
-> StorageDatasetPackageTestResult {
    let encoded = String::from_utf8(StorageDatasetPackageManifestFixture::manifest()?.to_json()?)?;
    for (before, after) in [
        ("\"format_version\":1", "\"format_version\":2"),
        ("\"schema_version\":1", "\"schema_version\":0"),
        ("\"policy_version\":1", "\"policy_version\":999"),
        ("\"observed\":1", "\"observed\":2"),
        ("\"unknown\":1", "\"unknown\":0"),
        ("licenses/cc0.txt", "../cc0.txt"),
        ("\"total_bytes\":153", "\"total_bytes\":154"),
    ] {
        assert!(encoded.contains(before));
        assert!(
            StorageDatasetPackageManifest::from_json(encoded.replacen(before, after, 1).as_bytes())
                .is_err(),
            "{before}"
        );
    }
    assert!(
        StorageDatasetPackageManifest::from_json(
            encoded.replacen('{', "{\"unexpected\":0,", 1).as_bytes()
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn manifest_refuses_duplicate_and_missing_source_or_artifact_bindings()
-> StorageDatasetPackageTestResult {
    let mut input = StorageDatasetPackageManifestFixture::input()?;
    input.sources.push(input.sources[0].clone());
    assert!(StorageDatasetPackageManifest::new(input).is_err());
    let mut input = StorageDatasetPackageManifestFixture::input()?;
    input.artifacts.push(input.artifacts[0].clone());
    assert!(StorageDatasetPackageManifest::new(input).is_err());
    let mut input = StorageDatasetPackageManifestFixture::input()?;
    input.artifacts.remove(0);
    assert!(StorageDatasetPackageManifest::new(input).is_err());
    let mut input = StorageDatasetPackageManifestFixture::input()?;
    input.sources.remove(0);
    assert!(StorageDatasetPackageManifest::new(input).is_err());
    let mut input = StorageDatasetPackageManifestFixture::input()?;
    input.observations.swap(0, 1);
    assert!(StorageDatasetPackageManifest::new(input).is_err());
    Ok(())
}

#[test]
fn installed_component_source_kind_must_match_its_dataset_family() -> StorageDatasetPackageTestResult
{
    let mut input = StorageDatasetPackageManifestFixture::input()?;
    input
        .artifacts
        .push(StorageDatasetPackageManifestFixture::artifact(
            "book-index.bin",
            StorageDatasetSelectionEnumComponent::Book,
            StorageDatasetPackageEnumArtifactRole::Index,
            vec![input.sources[1].identity().clone()],
        )?);
    assert!(StorageDatasetPackageManifest::new(input).is_err());
    Ok(())
}

#[test]
fn manifest_refuses_unobserved_prefixes_and_installed_component_overflow()
-> StorageDatasetPackageTestResult {
    let mut input = StorageDatasetPackageManifestFixture::input()?;
    input.initial_prefixes = StorageDatasetSelectionPrefixes::new(2, 1, 1)?;
    assert!(StorageDatasetPackageManifest::new(input).is_err());
    let mut input = StorageDatasetPackageManifestFixture::input()?;
    input.artifacts.push(StorageDatasetPackageArtifact::new(
        StorageDatasetPackageArtifactInput {
            reference: StorageDatasetPackageManifestFixture::reference("labels/oversized.bin")?,
            component: StorageDatasetSelectionEnumComponent::Labels,
            role: StorageDatasetPackageEnumArtifactRole::Index,
            installed_bytes: StorageDatasetSelectionEnumComponent::Labels.ceiling_bytes(),
            fixed_bytes: 0,
            sources: vec![input.sources[2].identity().clone()],
        },
    )?);
    assert!(StorageDatasetPackageManifest::new(input).is_err());
    Ok(())
}

#[test]
fn per_record_provenance_preserves_raw_metadata_full_identity_and_derived_stratum()
-> StorageDatasetPackageTestResult {
    let source = StorageDatasetPackageManifestFixture::source(
        StorageDatasetPackageEnumSourceKind::StandardGames,
    )?;
    let identity = StorageDatasetPackageManifestFixture::game()?;
    let fields = vec![
        StorageDatasetPackageRawField::new("WhiteElo".into(), " invalid ".into())?,
        StorageDatasetPackageRawField::new("BlackElo".into(), "2000".into())?,
        StorageDatasetPackageRawField::new("TimeControl".into(), "300+0".into())?,
        StorageDatasetPackageRawField::new("Custom".into(), "first".into())?,
        StorageDatasetPackageRawField::new("Custom".into(), "second".into())?,
    ];
    let provenance = StorageDatasetPackageRecordProvenance::game(
        identity.clone(),
        0,
        vec![StorageDatasetPackageSourceRecord::new(
            source.identity().clone(),
            "record-1".into(),
            PackageDigest::hash_slice(b"original record"),
            fields.clone(),
        )?],
    )?;
    assert_eq!(provenance.game_identity(), Some(&identity));
    assert_eq!(provenance.representative().raw_fields(), fields);
    assert_eq!(
        provenance.selection_identity(),
        &StorageDatasetSelectionIdentity::game(&identity)?
    );
    assert_eq!(
        provenance.stratum(),
        &StorageDatasetSelectionEnumStratum::Game(
            StorageDatasetSelection::classify_game(Some(" invalid "), Some("2000"), Some("300+0"))?
                .stratum()
                .clone()
        )
    );
    Ok(())
}

#[test]
fn puzzle_provenance_retains_all_original_themes_and_refuses_source_mismatch()
-> StorageDatasetPackageTestResult {
    let source =
        StorageDatasetPackageManifestFixture::source(StorageDatasetPackageEnumSourceKind::Puzzles)?;
    let identity = PuzzleDefinitionIdentity::new(
        source.identity().source().into(),
        source.identity().version().into(),
        "p1".into(),
    )?;
    let fields = vec![StorageDatasetPackageRawField::new(
        "Themes".into(),
        "zeta mate zeta\tAlpha".into(),
    )?];
    let records = vec![StorageDatasetPackageSourceRecord::new(
        source.identity().clone(),
        "p1".into(),
        PackageDigest::hash_slice(b"raw row"),
        fields.clone(),
    )?];
    let provenance = StorageDatasetPackageRecordProvenance::puzzle(identity, 0, records)?;
    assert_eq!(provenance.representative().raw_fields(), fields);
    assert_eq!(
        provenance.stratum(),
        &StorageDatasetSelectionEnumStratum::Puzzle(
            StorageDatasetSelection::classify_puzzle(None, Some("zeta mate zeta\tAlpha"))?
                .stratum()
                .clone()
        )
    );
    assert!(
        StorageDatasetPackageRecordProvenance::puzzle(
            PuzzleDefinitionIdentity::new("different".into(), "version".into(), "p1".into())?,
            0,
            vec![StorageDatasetPackageSourceRecord::new(
                source.identity().clone(),
                "p1".into(),
                PackageDigest::hash_slice(b"raw row"),
                vec![],
            )?]
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn unavailable_metadata_retains_source_category_and_reason_without_observed_counts()
-> StorageDatasetPackageTestResult {
    let source =
        StorageDatasetPackageManifestFixture::source(StorageDatasetPackageEnumSourceKind::Puzzles)?;
    let finding = StorageDatasetPackageUnavailable::new(
        source.identity().clone(),
        StorageDatasetPackageEnumUnavailableCategory::Rating,
        StorageDatasetPackageEnumUnavailableReason::NotProvided,
    );
    assert_eq!(finding.source(), source.identity());
    assert_eq!(
        finding.category(),
        StorageDatasetPackageEnumUnavailableCategory::Rating
    );
    assert_eq!(
        finding.reason(),
        StorageDatasetPackageEnumUnavailableReason::NotProvided
    );
    Ok(())
}

#[test]
fn accepted_manifest_refuses_unavailable_required_source_and_repeated_findings()
-> StorageDatasetPackageTestResult {
    let mut input = StorageDatasetPackageManifestFixture::input()?;
    input
        .unavailable
        .push(StorageDatasetPackageUnavailable::new(
            input.sources[0].identity().clone(),
            StorageDatasetPackageEnumUnavailableCategory::Source,
            StorageDatasetPackageEnumUnavailableReason::ChangedDigest,
        ));
    assert!(StorageDatasetPackageManifest::new(input).is_err());
    let mut input = StorageDatasetPackageManifestFixture::input()?;
    let finding = StorageDatasetPackageUnavailable::new(
        input.sources[0].identity().clone(),
        StorageDatasetPackageEnumUnavailableCategory::Rating,
        StorageDatasetPackageEnumUnavailableReason::NotProvided,
    );
    input.unavailable = vec![finding.clone(), finding];
    assert!(StorageDatasetPackageManifest::new(input).is_err());
    Ok(())
}

#[tokio::test]
async fn enriched_descriptor_keeps_integrity_and_semantic_states_independent()
-> StorageDatasetPackageTestResult {
    let directory = tempfile::tempdir()?;
    let location = directory.path().join("fixture.pgn");
    tokio::fs::write(&location, StorageDatasetPackageFixture::CONTENT).await?;
    let source = StorageDatasetPackageManifestFixture::source(
        StorageDatasetPackageEnumSourceKind::StandardGames,
    )?;
    let descriptor = StorageDatasetPackageFixture::descriptor(location)?.with_source(source)?;
    assert_eq!(
        descriptor.semantic_validation(),
        StorageDatasetPackageEnumSemanticValidation::Unchecked
    );
    let executor = CapacityExecutor::new(StorageDatasetPackageFixture::budget()?)?;
    let verified = descriptor.verify_integrity(&executor)?.join().await??;
    assert!(verified.source().is_some());
    assert_eq!(
        verified.semantic_validation(),
        StorageDatasetPackageEnumSemanticValidation::Unchecked
    );
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn verified_bytes_retain_format_and_canonical_location_without_claiming_semantic_validation()
-> StorageDatasetPackageTestResult {
    let directory = tempfile::tempdir()?;
    let location = directory.path().join("fixture.pgn");
    tokio::fs::write(&location, StorageDatasetPackageFixture::CONTENT).await?;
    let descriptor = StorageDatasetPackageFixture::descriptor(location.clone())?;
    assert_eq!(
        descriptor.validation(),
        StorageDatasetPackageEnumValidation::Unchecked
    );
    let checksum = descriptor.checksum();
    let executor = CapacityExecutor::new(StorageDatasetPackageFixture::budget()?)?;
    let verified = descriptor.verify_integrity(&executor)?.join().await??;
    assert_eq!(verified.format(), StorageDatasetPackageEnumFormat::Pgn);
    assert_eq!(verified.checksum(), checksum);
    assert_eq!(verified.location(), location.canonicalize()?);
    assert_eq!(
        verified.validation(),
        StorageDatasetPackageEnumValidation::IntegrityVerified
    );
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn checksum_failure_never_produces_a_verified_replacement() -> StorageDatasetPackageTestResult
{
    let directory = tempfile::tempdir()?;
    let location = directory.path().join("changed.pgn");
    tokio::fs::write(&location, b"changed bytes").await?;
    let descriptor = StorageDatasetPackageFixture::descriptor(location)?;
    let executor = CapacityExecutor::new(StorageDatasetPackageFixture::budget()?)?;
    assert!(matches!(
        descriptor.verify_integrity(&executor)?.join().await?,
        Err(StorageDatasetPackageError::Checksum { .. })
    ));
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn relative_missing_and_nonregular_locations_fail_explicitly()
-> StorageDatasetPackageTestResult {
    assert!(matches!(
        StorageDatasetPackage::new(
            StorageDatasetPackageEnumFormat::PuzzleCsv,
            PackageDigest::read(Cursor::new(StorageDatasetPackageFixture::CONTENT))?,
            "relative.csv".into(),
        ),
        Err(StorageDatasetPackageError::RelativeLocation(_))
    ));
    let directory = tempfile::tempdir()?;
    let executor = CapacityExecutor::new(StorageDatasetPackageFixture::budget()?)?;
    let missing = StorageDatasetPackageFixture::descriptor(directory.path().join("missing.pgn"))?;
    assert!(matches!(
        missing.verify_integrity(&executor)?.join().await?,
        Err(StorageDatasetPackageError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound
    ));
    let nonregular = StorageDatasetPackageFixture::descriptor(directory.path().to_path_buf())?;
    assert!(matches!(
        nonregular.verify_integrity(&executor)?.join().await?,
        Err(StorageDatasetPackageError::NotRegularFile(_))
    ));
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn verification_respects_shared_capacity_before_touching_a_missing_file()
-> StorageDatasetPackageTestResult {
    let directory = tempfile::tempdir()?;
    let budget = StorageDatasetPackageFixture::budget()?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let lease = budget.try_lease(1)?;
    let descriptor =
        StorageDatasetPackageFixture::descriptor(directory.path().join("missing.pgn"))?;
    assert!(matches!(
        descriptor.verify_integrity(&executor),
        Err(StorageDatasetPackageError::Executor(
            CapacityExecutorError::Budget(CapacityBudgetError::Unavailable)
        ))
    ));
    drop(lease);
    assert_eq!(budget.available(), budget.workers());
    executor.shutdown().await?;
    Ok(())
}
