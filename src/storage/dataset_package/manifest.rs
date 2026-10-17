use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::storage::dataset_package::StorageDatasetPackageEnumUnavailableCategory;

use crate::storage::dataset_package::measurement::StorageDatasetPackagePrefixesRecord;
use crate::storage::dataset_package::{
    StorageDatasetPackageArtifact, StorageDatasetPackageArtifactReference,
    StorageDatasetPackageEnumArtifactRole, StorageDatasetPackageEnumLicense,
    StorageDatasetPackageEnumSourceKind, StorageDatasetPackageError,
    StorageDatasetPackageMeasurements, StorageDatasetPackageObservation,
    StorageDatasetPackageReduction, StorageDatasetPackageSource,
    StorageDatasetPackageSourceIdentity, StorageDatasetPackageUnavailable,
};
use crate::storage::dataset_selection::{
    StorageDatasetSelection, StorageDatasetSelectionEnumComponent,
    StorageDatasetSelectionEnumSubset, StorageDatasetSelectionEnumTransition,
    StorageDatasetSelectionPrefixes, StorageDatasetSelectionSequence,
};

#[derive(Debug)]
pub struct StorageDatasetPackageManifestInput {
    pub sources: Vec<StorageDatasetPackageSource>,
    pub observations: Vec<StorageDatasetPackageObservation>,
    pub artifacts: Vec<StorageDatasetPackageArtifact>,
    pub initial_prefixes: StorageDatasetSelectionPrefixes,
    pub reductions: Vec<StorageDatasetPackageReduction>,
    pub unavailable: Vec<StorageDatasetPackageUnavailable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StorageDatasetPackageEnumManifestFormat {
    StarterPreparation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StorageDatasetPackageManifestRecord")]
pub struct StorageDatasetPackageManifest {
    format: StorageDatasetPackageEnumManifestFormat,
    format_version: u32,
    schema_version: u32,
    policy_version: u32,
    sources: Vec<StorageDatasetPackageSource>,
    observations: Vec<StorageDatasetPackageObservation>,
    artifacts: Vec<StorageDatasetPackageArtifact>,
    initial_prefixes: StorageDatasetSelectionPrefixes,
    final_prefixes: StorageDatasetSelectionPrefixes,
    reductions: Vec<StorageDatasetPackageReduction>,
    final_measurements: StorageDatasetPackageMeasurements,
    unavailable: Vec<StorageDatasetPackageUnavailable>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageDatasetPackageManifestRecord {
    format: StorageDatasetPackageEnumManifestFormat,
    format_version: u32,
    schema_version: u32,
    policy_version: u32,
    sources: Vec<StorageDatasetPackageSource>,
    observations: Vec<StorageDatasetPackageObservation>,
    artifacts: Vec<StorageDatasetPackageArtifact>,
    initial_prefixes: StorageDatasetPackagePrefixesRecord,
    final_prefixes: StorageDatasetPackagePrefixesRecord,
    reductions: Vec<StorageDatasetPackageReduction>,
    final_measurements: StorageDatasetPackageMeasurements,
    unavailable: Vec<StorageDatasetPackageUnavailable>,
}

impl TryFrom<StorageDatasetPackageManifestRecord> for StorageDatasetPackageManifest {
    type Error = StorageDatasetPackageError;
    fn try_from(record: StorageDatasetPackageManifestRecord) -> Result<Self, Self::Error> {
        if record.format != StorageDatasetPackageEnumManifestFormat::StarterPreparation
            || record.format_version != Self::FORMAT_VERSION
            || record.schema_version != Self::SCHEMA_VERSION
            || record.policy_version != StorageDatasetSelection::POLICY_VERSION
        {
            return Err(StorageDatasetPackageError::Version);
        }
        let manifest = Self::new(StorageDatasetPackageManifestInput {
            sources: record.sources,
            observations: record.observations,
            artifacts: record.artifacts,
            initial_prefixes: record.initial_prefixes.validate()?,
            reductions: record.reductions,
            unavailable: record.unavailable,
        })?;
        if manifest.final_prefixes != record.final_prefixes.validate()?
            || manifest.final_measurements != record.final_measurements
        {
            return Err(StorageDatasetPackageError::Measurements);
        }
        Ok(manifest)
    }
}

impl StorageDatasetPackageManifest {
    pub const FORMAT_VERSION: u32 = 1;
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn new(
        input: StorageDatasetPackageManifestInput,
    ) -> Result<Self, StorageDatasetPackageError> {
        Self::validate_sources(&input.sources)?;
        if !input
            .observations
            .iter()
            .map(StorageDatasetPackageObservation::subset)
            .eq(StorageDatasetSelectionEnumSubset::ORDER)
        {
            return Err(StorageDatasetPackageError::ObservationOrder);
        }
        let mut initial = Vec::new();
        let mut selected = Vec::new();
        for observation in &input.observations {
            let target = u64::try_from(observation.subset().upper_target())
                .map_err(|_error| StorageDatasetPackageError::Counts)?;
            initial.push(observation.counts().available().min(target));
            selected.push(observation.selected_prefix());
        }
        let initial = Self::prefixes(&initial)?;
        let final_prefixes = Self::prefixes(&selected)?;
        if initial != input.initial_prefixes {
            return Err(StorageDatasetPackageError::Prefix);
        }
        let mut current = initial;
        for step in &input.reductions {
            if step.decision().before() != current {
                return Err(StorageDatasetPackageError::Reduction);
            }
            current = step.decision().after();
        }
        if current != final_prefixes {
            return Err(StorageDatasetPackageError::Prefix);
        }
        Self::validate_references(&input)?;
        let final_measurements =
            StorageDatasetPackageMeasurements::from_artifacts(&input.artifacts)?;
        match final_measurements.footprint().transition(final_prefixes)? {
            StorageDatasetSelectionEnumTransition::Accept => {}
            StorageDatasetSelectionEnumTransition::Reduce(_) => {
                return Err(StorageDatasetPackageError::Reduction);
            }
        }
        Ok(Self {
            format: StorageDatasetPackageEnumManifestFormat::StarterPreparation,
            format_version: Self::FORMAT_VERSION,
            schema_version: Self::SCHEMA_VERSION,
            policy_version: StorageDatasetSelection::POLICY_VERSION,
            sources: input.sources,
            observations: input.observations,
            artifacts: input.artifacts,
            initial_prefixes: initial,
            final_prefixes,
            reductions: input.reductions,
            final_measurements,
            unavailable: input.unavailable,
        })
    }

    fn prefixes(
        counts: &[u64],
    ) -> Result<StorageDatasetSelectionPrefixes, StorageDatasetPackageError> {
        let [master, broad, puzzles] = counts else {
            return Err(StorageDatasetPackageError::ObservationOrder);
        };
        Ok(StorageDatasetSelectionPrefixes::new(
            *master, *broad, *puzzles,
        )?)
    }

    fn validate_sources(
        sources: &[StorageDatasetPackageSource],
    ) -> Result<(), StorageDatasetPackageError> {
        StorageDatasetPackageSourceIdentity::validate_bindings(
            &sources
                .iter()
                .map(|source| source.identity().clone())
                .collect::<Vec<_>>(),
        )?;
        let mut kinds = BTreeSet::new();
        for source in sources {
            if source.license().terms() != &StorageDatasetPackageEnumLicense::Cc0
                || !kinds.insert(source.kind())
            {
                return Err(StorageDatasetPackageError::SourceKinds);
            }
        }
        if kinds
            != BTreeSet::from([
                StorageDatasetPackageEnumSourceKind::StandardGames,
                StorageDatasetPackageEnumSourceKind::Puzzles,
                StorageDatasetPackageEnumSourceKind::Openings,
            ])
        {
            return Err(StorageDatasetPackageError::SourceKinds);
        }
        Ok(())
    }

    fn source<'a>(
        sources: &'a [StorageDatasetPackageSource],
        identity: &StorageDatasetPackageSourceIdentity,
    ) -> Result<&'a StorageDatasetPackageSource, StorageDatasetPackageError> {
        sources
            .iter()
            .find(|source| source.identity() == identity)
            .ok_or(StorageDatasetPackageError::SourceBinding)
    }

    fn referenced<'a>(
        artifacts: &BTreeMap<&str, &'a StorageDatasetPackageArtifact>,
        reference: &StorageDatasetPackageArtifactReference,
    ) -> Result<&'a StorageDatasetPackageArtifact, StorageDatasetPackageError> {
        let artifact = artifacts
            .get(reference.path())
            .copied()
            .ok_or(StorageDatasetPackageError::ArtifactReference)?;
        if artifact.reference() != reference {
            return Err(StorageDatasetPackageError::ArtifactReference);
        }
        Ok(artifact)
    }

    fn component(
        subset: StorageDatasetSelectionEnumSubset,
    ) -> StorageDatasetSelectionEnumComponent {
        match subset {
            StorageDatasetSelectionEnumSubset::Master => {
                StorageDatasetSelectionEnumComponent::Master
            }
            StorageDatasetSelectionEnumSubset::Broad => StorageDatasetSelectionEnumComponent::Broad,
            StorageDatasetSelectionEnumSubset::Puzzles => {
                StorageDatasetSelectionEnumComponent::Puzzles
            }
        }
    }

    fn validate_references(
        input: &StorageDatasetPackageManifestInput,
    ) -> Result<(), StorageDatasetPackageError> {
        let mut artifacts = BTreeMap::new();
        let mut paths = BTreeSet::new();
        for artifact in &input.artifacts {
            if !paths.insert(artifact.reference().collision_key()) {
                return Err(StorageDatasetPackageError::ArtifactReference);
            }
            artifacts.insert(artifact.reference().path(), artifact);
            for identity in artifact.sources() {
                let source = Self::source(&input.sources, identity)?;
                let expected = match artifact.component() {
                    StorageDatasetSelectionEnumComponent::Master
                    | StorageDatasetSelectionEnumComponent::Broad
                    | StorageDatasetSelectionEnumComponent::Book => {
                        Some(StorageDatasetPackageEnumSourceKind::StandardGames)
                    }
                    StorageDatasetSelectionEnumComponent::Puzzles => {
                        Some(StorageDatasetPackageEnumSourceKind::Puzzles)
                    }
                    StorageDatasetSelectionEnumComponent::Labels => {
                        if artifact.role() == StorageDatasetPackageEnumArtifactRole::Labels {
                            Some(StorageDatasetPackageEnumSourceKind::Openings)
                        } else {
                            None
                        }
                    }
                };
                if expected.is_some_and(|kind| source.kind() != kind) {
                    return Err(StorageDatasetPackageError::ArtifactBinding);
                }
            }
        }
        for path in &paths {
            for (offset, _) in path.match_indices('/') {
                if paths.contains(&path[..offset]) {
                    return Err(StorageDatasetPackageError::ArtifactReference);
                }
            }
        }
        for source in &input.sources {
            let license = Self::referenced(&artifacts, source.license().copy())?;
            if license.role() != StorageDatasetPackageEnumArtifactRole::LicenseCopy
                || !license.sources().contains(source.identity())
            {
                return Err(StorageDatasetPackageError::ArtifactBinding);
            }
        }
        for observation in &input.observations {
            let expected_kind = match observation.subset() {
                StorageDatasetSelectionEnumSubset::Master
                | StorageDatasetSelectionEnumSubset::Broad => {
                    StorageDatasetPackageEnumSourceKind::StandardGames
                }
                StorageDatasetSelectionEnumSubset::Puzzles => {
                    StorageDatasetPackageEnumSourceKind::Puzzles
                }
            };
            for identity in observation.sources() {
                if Self::source(&input.sources, identity)?.kind() != expected_kind {
                    return Err(StorageDatasetPackageError::SourceBinding);
                }
            }
            let provenance = Self::referenced(&artifacts, observation.provenance())?;
            if provenance.role() != StorageDatasetPackageEnumArtifactRole::Provenance
                || provenance.component() != Self::component(observation.subset())
                || provenance.sources() != observation.sources()
            {
                return Err(StorageDatasetPackageError::ArtifactBinding);
            }
            if !input.artifacts.iter().any(|artifact| {
                artifact.role() == StorageDatasetPackageEnumArtifactRole::Dataset
                    && artifact.component() == provenance.component()
                    && artifact.sources() == observation.sources()
            }) {
                return Err(StorageDatasetPackageError::ArtifactBinding);
            }
        }
        for (role, kind) in [
            (
                StorageDatasetPackageEnumArtifactRole::Book,
                StorageDatasetPackageEnumSourceKind::StandardGames,
            ),
            (
                StorageDatasetPackageEnumArtifactRole::Labels,
                StorageDatasetPackageEnumSourceKind::Openings,
            ),
        ] {
            if !input.artifacts.iter().any(|artifact| {
                artifact.role() == role
                    && artifact.sources().iter().any(|identity| {
                        input
                            .sources
                            .iter()
                            .any(|source| source.kind() == kind && source.identity() == identity)
                    })
            }) {
                return Err(StorageDatasetPackageError::ArtifactBinding);
            }
        }
        let mut unavailable = BTreeSet::new();
        for finding in &input.unavailable {
            Self::source(&input.sources, finding.source())?;
            if finding.category() == StorageDatasetPackageEnumUnavailableCategory::Source {
                return Err(StorageDatasetPackageError::RequiredSourceUnavailable);
            }
            if !unavailable.insert(finding) {
                return Err(StorageDatasetPackageError::SourceBinding);
            }
        }
        Ok(())
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, StorageDatasetPackageError> {
        Ok(serde_json::from_slice(bytes)?)
    }
    pub fn to_json(&self) -> Result<Vec<u8>, StorageDatasetPackageError> {
        Ok(serde_json::to_vec(self)?)
    }
    pub fn validate_reproduction<H>(
        &self,
        subset: StorageDatasetSelectionEnumSubset,
        sequence: &StorageDatasetSelectionSequence<H>,
    ) -> Result<(), StorageDatasetPackageError> {
        self.observations
            .iter()
            .find(|observation| observation.subset() == subset)
            .ok_or(StorageDatasetPackageError::ObservationOrder)?
            .validate_reproduction(sequence)
    }
    pub const fn format_version(&self) -> u32 {
        self.format_version
    }
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
    pub const fn policy_version(&self) -> u32 {
        self.policy_version
    }
    pub fn sources(&self) -> &[StorageDatasetPackageSource] {
        &self.sources
    }
    pub fn observations(&self) -> &[StorageDatasetPackageObservation] {
        &self.observations
    }
    pub fn artifacts(&self) -> &[StorageDatasetPackageArtifact] {
        &self.artifacts
    }
    pub const fn initial_prefixes(&self) -> StorageDatasetSelectionPrefixes {
        self.initial_prefixes
    }
    pub const fn final_prefixes(&self) -> StorageDatasetSelectionPrefixes {
        self.final_prefixes
    }
    pub fn reductions(&self) -> &[StorageDatasetPackageReduction] {
        &self.reductions
    }
    pub const fn measurements(&self) -> &StorageDatasetPackageMeasurements {
        &self.final_measurements
    }
    pub fn unavailable(&self) -> &[StorageDatasetPackageUnavailable] {
        &self.unavailable
    }
}
