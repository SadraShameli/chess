use std::collections::{BTreeMap, btree_map::Entry};
use std::fmt::{Display, Formatter};
use std::path::{Component, Path};
use std::str::FromStr;

use chess::engine::capabilities::EngineCapabilitiesVersion;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::manifest::{
    EnginePackageEnumProfile, EnginePackageEnumTarget, EnginePackageManifest,
};
use chess::package::digest::{PackageDigest, PackageDigestError};
use chess::workbench::enums::WorkbenchEnumOperation;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ToolsEngineCoverageError {
    #[error("The canonical capability inventory is empty or malformed")]
    Inventory,
    #[error("Capability identity {0} is not canonical")]
    Identity(String),
    #[error("Capability {0} appears more than once")]
    Duplicate(ToolsEngineCoverageId),
    #[error("Capability {0} is missing")]
    Missing(ToolsEngineCoverageId),
    #[error("Capability {0} is outside the pinned inventory")]
    Foreign(ToolsEngineCoverageId),
    #[error("The option or workbench registry differs from the pinned inventory")]
    Registry,
    #[error("Coverage schema version {0} is unsupported")]
    Version(u32),
    #[error("The coverage inventory, registry, package, target, profile, source or model differs")]
    Context,
    #[error("Capability {0} lacks valid evidence or a concrete outstanding prerequisite")]
    Evidence(ToolsEngineCoverageId),
    #[error("Cannot decode or encode capability evidence: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Digest(#[from] PackageDigestError),
    #[error("Capability workflow failed: {0}")]
    Workflow(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ToolsEngineCoverageEnumCategory {
    Option,
    Protocol,
    Search,
    Output,
    Chess,
    Evaluation,
    Cpu,
    Tablebase,
    Numa,
    Developer,
}

impl ToolsEngineCoverageEnumCategory {
    fn parse(value: &str) -> Result<Self, ToolsEngineCoverageError> {
        match value {
            "OPT" => Ok(Self::Option),
            "PRO" => Ok(Self::Protocol),
            "SRC" => Ok(Self::Search),
            "OUT" => Ok(Self::Output),
            "CHS" => Ok(Self::Chess),
            "EVA" => Ok(Self::Evaluation),
            "CPU" => Ok(Self::Cpu),
            "TB" => Ok(Self::Tablebase),
            "NUM" => Ok(Self::Numa),
            "DEV" => Ok(Self::Developer),
            _ => Err(ToolsEngineCoverageError::Identity(value.to_owned())),
        }
    }

    const fn prefix(self) -> &'static str {
        match self {
            Self::Option => "OPT",
            Self::Protocol => "PRO",
            Self::Search => "SRC",
            Self::Output => "OUT",
            Self::Chess => "CHS",
            Self::Evaluation => "EVA",
            Self::Cpu => "CPU",
            Self::Tablebase => "TB",
            Self::Numa => "NUM",
            Self::Developer => "DEV",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct ToolsEngineCoverageId {
    category: ToolsEngineCoverageEnumCategory,
    ordinal: u8,
}

impl ToolsEngineCoverageId {
    pub(crate) const fn category(self) -> ToolsEngineCoverageEnumCategory {
        self.category
    }
}

impl FromStr for ToolsEngineCoverageId {
    type Err = ToolsEngineCoverageError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (prefix, ordinal) = value
            .split_once('-')
            .ok_or_else(|| ToolsEngineCoverageError::Identity(value.to_owned()))?;
        let category = ToolsEngineCoverageEnumCategory::parse(prefix)?;
        let ordinal: u8 = ordinal
            .parse()
            .map_err(|_error| ToolsEngineCoverageError::Identity(value.to_owned()))?;
        let identity = Self { category, ordinal };
        if ordinal == 0 || identity.to_string() != value {
            return Err(ToolsEngineCoverageError::Identity(value.to_owned()));
        }
        Ok(identity)
    }
}

impl TryFrom<String> for ToolsEngineCoverageId {
    type Error = ToolsEngineCoverageError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<ToolsEngineCoverageId> for String {
    fn from(value: ToolsEngineCoverageId) -> Self {
        value.to_string()
    }
}

impl Display for ToolsEngineCoverageId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}-{:02}", self.category.prefix(), self.ordinal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolsEngineCoverageEnumBinding {
    Option(EngineEnumOption),
    Workbench(WorkbenchEnumOperation),
    Inventory,
}

impl ToolsEngineCoverageEnumBinding {
    fn parse(id: ToolsEngineCoverageId, facility: &str) -> Result<Self, ToolsEngineCoverageError> {
        if id.category() == ToolsEngineCoverageEnumCategory::Option {
            let name = facility.trim_matches('`');
            return EngineEnumOption::ALL
                .into_iter()
                .find(|option| option.name() == name)
                .map(Self::Option)
                .ok_or(ToolsEngineCoverageError::Registry);
        }
        if id.category() == ToolsEngineCoverageEnumCategory::Developer {
            let command = facility
                .strip_prefix('`')
                .and_then(|value| value.split([' ', '`']).next());
            if let Some(operation) = WorkbenchEnumOperation::ALL
                .into_iter()
                .find(|operation| Some(Self::command(*operation)) == command)
            {
                return Ok(Self::Workbench(operation));
            }
        }
        Ok(Self::Inventory)
    }

    const fn command(operation: WorkbenchEnumOperation) -> &'static str {
        match operation {
            WorkbenchEnumOperation::Inspector => "d",
            WorkbenchEnumOperation::RawEvaluation => "eval",
            WorkbenchEnumOperation::Compiler => "compiler",
            WorkbenchEnumOperation::Perft => "perft",
            WorkbenchEnumOperation::Bench => "bench",
        }
    }
}

#[derive(Debug)]
pub(crate) struct ToolsEngineCoverageEntry {
    id: ToolsEngineCoverageId,
    facility: String,
    requirement: String,
    binding: ToolsEngineCoverageEnumBinding,
}

impl ToolsEngineCoverageEntry {
    pub(crate) const fn id(&self) -> ToolsEngineCoverageId {
        self.id
    }

    pub(crate) fn facility(&self) -> &str {
        &self.facility
    }

    pub(crate) fn requirement(&self) -> &str {
        &self.requirement
    }

    pub(crate) const fn binding(&self) -> ToolsEngineCoverageEnumBinding {
        self.binding
    }

    pub(crate) fn applicability(
        &self,
        target: EnginePackageEnumTarget,
    ) -> ToolsEngineCoverageEnumApplicability {
        match (self.id.category(), self.facility()) {
            (
                ToolsEngineCoverageEnumCategory::Cpu,
                "Scalar/x86 baseline" | "AVX2 path" | "AVX512 path",
            ) if target == EnginePackageEnumTarget::MacosArm => {
                ToolsEngineCoverageEnumApplicability::OtherTarget
            }
            (ToolsEngineCoverageEnumCategory::Cpu, "AArch64 NEON path")
                if target != EnginePackageEnumTarget::MacosArm =>
            {
                ToolsEngineCoverageEnumApplicability::OtherTarget
            }
            (ToolsEngineCoverageEnumCategory::Numa, _)
                if target != EnginePackageEnumTarget::LinuxX64 =>
            {
                ToolsEngineCoverageEnumApplicability::OtherTarget
            }
            (
                ToolsEngineCoverageEnumCategory::Option
                | ToolsEngineCoverageEnumCategory::Protocol
                | ToolsEngineCoverageEnumCategory::Search
                | ToolsEngineCoverageEnumCategory::Output
                | ToolsEngineCoverageEnumCategory::Chess
                | ToolsEngineCoverageEnumCategory::Evaluation
                | ToolsEngineCoverageEnumCategory::Cpu
                | ToolsEngineCoverageEnumCategory::Tablebase
                | ToolsEngineCoverageEnumCategory::Numa
                | ToolsEngineCoverageEnumCategory::Developer,
                _,
            ) => ToolsEngineCoverageEnumApplicability::CurrentTarget,
        }
    }

    fn permits_upstream_unavailability(&self) -> bool {
        self.id.category() == ToolsEngineCoverageEnumCategory::Developer
            && self.facility() == "Optional `spsa` feature"
    }

    pub(crate) fn prerequisite(
        &self,
        target: EnginePackageEnumTarget,
    ) -> ToolsEngineCoverageEnumPrerequisite {
        if self.applicability(target) == ToolsEngineCoverageEnumApplicability::OtherTarget {
            return ToolsEngineCoverageEnumPrerequisite::NativeTarget;
        }
        match (self.id.category(), self.facility()) {
            (ToolsEngineCoverageEnumCategory::Tablebase, _)
            | (ToolsEngineCoverageEnumCategory::Option, "`SyzygyPath`")
            | (ToolsEngineCoverageEnumCategory::Output, "`tbhits`") => {
                ToolsEngineCoverageEnumPrerequisite::AuthenticTablebases
            }
            (ToolsEngineCoverageEnumCategory::Numa, _) => {
                ToolsEngineCoverageEnumPrerequisite::NumaHost
            }
            (ToolsEngineCoverageEnumCategory::Cpu, "Parallel workers") => {
                ToolsEngineCoverageEnumPrerequisite::ParallelScaling
            }
            (ToolsEngineCoverageEnumCategory::Cpu, _) => {
                ToolsEngineCoverageEnumPrerequisite::CpuProfile
            }
            (ToolsEngineCoverageEnumCategory::Developer, "PGO release workflow") => {
                ToolsEngineCoverageEnumPrerequisite::PgoExecution
            }
            (ToolsEngineCoverageEnumCategory::Developer, "Optional `spsa` feature") => {
                ToolsEngineCoverageEnumPrerequisite::SpsaCompileCheck
            }
            (
                ToolsEngineCoverageEnumCategory::Option
                | ToolsEngineCoverageEnumCategory::Protocol
                | ToolsEngineCoverageEnumCategory::Search
                | ToolsEngineCoverageEnumCategory::Output
                | ToolsEngineCoverageEnumCategory::Chess
                | ToolsEngineCoverageEnumCategory::Evaluation
                | ToolsEngineCoverageEnumCategory::Developer,
                _,
            ) => ToolsEngineCoverageEnumPrerequisite::ApplicationWorkflow,
        }
    }
}

pub(crate) struct ToolsEngineCoverageInventory {
    sha256: PackageDigest,
    entries: BTreeMap<ToolsEngineCoverageId, ToolsEngineCoverageEntry>,
}

impl ToolsEngineCoverageInventory {
    pub(crate) const SOURCE: &str =
        include_str!("../../specs/001-reckless-desktop-chess/contracts/reckless-capabilities.md");

    pub(crate) fn pinned() -> Result<Self, ToolsEngineCoverageError> {
        Self::parse(Self::SOURCE)
    }

    pub(crate) fn parse(source: &str) -> Result<Self, ToolsEngineCoverageError> {
        if !source.contains(EnginePackageManifest::COMMIT)
            || !source.contains(EnginePackageManifest::TAG)
        {
            return Err(ToolsEngineCoverageError::Context);
        }
        let mut entries = BTreeMap::new();
        for line in source.lines().filter(|line| line.starts_with("| ")) {
            let cells: Vec<_> = line.split('|').map(str::trim).collect();
            let Some(identity) = cells.get(1) else {
                return Err(ToolsEngineCoverageError::Inventory);
            };
            if *identity == "ID" || identity.starts_with('-') {
                continue;
            }
            let id: ToolsEngineCoverageId = identity.parse()?;
            let Some(facility) = cells.get(2).filter(|value| !value.is_empty()) else {
                return Err(ToolsEngineCoverageError::Inventory);
            };
            let requirements = cells
                .get(3..cells.len().saturating_sub(1))
                .filter(|values| !values.is_empty() && values.iter().all(|value| !value.is_empty()))
                .ok_or(ToolsEngineCoverageError::Inventory)?;
            let entry = ToolsEngineCoverageEntry {
                id,
                facility: (*facility).to_owned(),
                requirement: requirements.join(" | "),
                binding: ToolsEngineCoverageEnumBinding::parse(id, facility)?,
            };
            match entries.entry(id) {
                Entry::Vacant(slot) => {
                    slot.insert(entry);
                }
                Entry::Occupied(_) => return Err(ToolsEngineCoverageError::Duplicate(id)),
            }
        }
        if entries.is_empty() {
            return Err(ToolsEngineCoverageError::Inventory);
        }
        for option in EngineEnumOption::ALL {
            Self::require_binding(&entries, ToolsEngineCoverageEnumBinding::Option(option))?;
        }
        for operation in WorkbenchEnumOperation::ALL {
            Self::require_binding(
                &entries,
                ToolsEngineCoverageEnumBinding::Workbench(operation),
            )?;
        }
        Ok(Self {
            sha256: PackageDigest::read(source.as_bytes())?,
            entries,
        })
    }

    fn require_binding(
        entries: &BTreeMap<ToolsEngineCoverageId, ToolsEngineCoverageEntry>,
        binding: ToolsEngineCoverageEnumBinding,
    ) -> Result<(), ToolsEngineCoverageError> {
        if entries
            .values()
            .filter(|entry| entry.binding() == binding)
            .count()
            != 1
        {
            return Err(ToolsEngineCoverageError::Registry);
        }
        Ok(())
    }

    pub(crate) fn entries(&self) -> impl ExactSizeIterator<Item = &ToolsEngineCoverageEntry> {
        self.entries.values()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(remote = "Self")]
pub(crate) struct ToolsEngineCoverageContext {
    target: EnginePackageEnumTarget,
    profile: EnginePackageEnumProfile,
    package_sha256: PackageDigest,
    source_commit: String,
    model_sha256: PackageDigest,
}

impl ToolsEngineCoverageContext {
    pub(crate) fn new(
        target: EnginePackageEnumTarget,
        profile: EnginePackageEnumProfile,
        package_sha256: PackageDigest,
    ) -> Result<Self, ToolsEngineCoverageError> {
        Ok(Self {
            target,
            profile,
            package_sha256,
            source_commit: EnginePackageManifest::COMMIT.to_owned(),
            model_sha256: EnginePackageManifest::NETWORK_SHA256.parse()?,
        })
    }

    pub(crate) fn fingerprint(&self) -> Result<PackageDigest, ToolsEngineCoverageError> {
        Ok(PackageDigest::read(serde_json::to_vec(self)?.as_slice())?)
    }

    pub(crate) const fn target(&self) -> EnginePackageEnumTarget {
        self.target
    }

    fn validate(&self) -> Result<(), ToolsEngineCoverageError> {
        if self.source_commit != EnginePackageManifest::COMMIT
            || self.model_sha256.to_string() != EnginePackageManifest::NETWORK_SHA256
        {
            return Err(ToolsEngineCoverageError::Context);
        }
        Ok(())
    }
}

impl Serialize for ToolsEngineCoverageContext {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Self::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for ToolsEngineCoverageContext {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let context = Self::deserialize(deserializer)?;
        context.validate().map_err(serde::de::Error::custom)?;
        Ok(context)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolsEngineCoverageEnumApplicability {
    CurrentTarget,
    OtherTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolsEngineCoverageEnumArtifact {
    Fixture,
    EngineTranscript,
    WorkbenchExport,
    NativeMeasurement,
    Source,
    ControlledBuildLog,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolsEngineCoverageArtifact {
    kind: ToolsEngineCoverageEnumArtifact,
    path: String,
    sha256: PackageDigest,
}

impl ToolsEngineCoverageArtifact {
    pub(crate) const fn kind(&self) -> ToolsEngineCoverageEnumArtifact {
        self.kind
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) const fn sha256(&self) -> PackageDigest {
        self.sha256
    }

    pub(crate) fn capture(
        kind: ToolsEngineCoverageEnumArtifact,
        path: String,
        bytes: &[u8],
    ) -> Result<Self, ToolsEngineCoverageError> {
        let artifact = Self {
            kind,
            path,
            sha256: PackageDigest::read(bytes)?,
        };
        if bytes.is_empty() || !artifact.is_valid() {
            return Err(ToolsEngineCoverageError::Inventory);
        }
        Ok(artifact)
    }

    fn is_valid(&self) -> bool {
        ToolsEngineCoverageObservation::has_text(&self.path)
            && !self.path.contains('\\')
            && Path::new(&self.path)
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolsEngineCoverageEnumVerdict {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolsEngineCoverageEnumPrerequisite {
    ApplicationWorkflow,
    NativeTarget,
    CpuProfile,
    ParallelScaling,
    AuthenticTablebases,
    NumaHost,
    PgoExecution,
    SpsaCompileCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ToolsEngineCoverageOutcome {
    Measured {
        verdict: ToolsEngineCoverageEnumVerdict,
        expected: String,
        actual: String,
        artifacts: Vec<ToolsEngineCoverageArtifact>,
    },
    Outstanding {
        prerequisite: ToolsEngineCoverageEnumPrerequisite,
        detail: String,
        next_action: String,
    },
    UpstreamUnavailable {
        reason: String,
        artifacts: Vec<ToolsEngineCoverageArtifact>,
    },
}

impl ToolsEngineCoverageOutcome {
    fn canonicalize(&mut self) {
        match self {
            Self::Measured { artifacts, .. } | Self::UpstreamUnavailable { artifacts, .. } => {
                artifacts.sort_by(|left, right| left.path.cmp(&right.path));
            }
            Self::Outstanding { .. } => {}
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolsEngineCoverageObservation {
    id: ToolsEngineCoverageId,
    context_sha256: PackageDigest,
    applicability: ToolsEngineCoverageEnumApplicability,
    outcome: ToolsEngineCoverageOutcome,
}

impl ToolsEngineCoverageObservation {
    const MAXIMUM_TEXT_BYTES: usize = 16 * 1024;
    const MAXIMUM_ARTIFACTS: usize = 32;

    pub(crate) const fn id(&self) -> ToolsEngineCoverageId {
        self.id
    }

    pub(crate) const fn outcome(&self) -> &ToolsEngineCoverageOutcome {
        &self.outcome
    }

    pub(crate) fn new(
        entry: &ToolsEngineCoverageEntry,
        context: &ToolsEngineCoverageContext,
        outcome: ToolsEngineCoverageOutcome,
    ) -> Result<Self, ToolsEngineCoverageError> {
        let mut observation = Self {
            id: entry.id(),
            context_sha256: context.fingerprint()?,
            applicability: entry.applicability(context.target()),
            outcome,
        };
        observation.validate(entry, context)?;
        observation.outcome.canonicalize();
        Ok(observation)
    }

    fn has_text(value: &str) -> bool {
        !value.trim().is_empty()
            && value.len() <= Self::MAXIMUM_TEXT_BYTES
            && !value.chars().any(char::is_control)
    }

    fn has_artifacts(artifacts: &[ToolsEngineCoverageArtifact]) -> bool {
        !artifacts.is_empty()
            && artifacts.len() <= Self::MAXIMUM_ARTIFACTS
            && artifacts.iter().enumerate().all(|(index, artifact)| {
                artifact.is_valid()
                    && !artifacts[..index]
                        .iter()
                        .any(|previous| previous.path == artifact.path)
            })
    }

    fn validate(
        &self,
        entry: &ToolsEngineCoverageEntry,
        context: &ToolsEngineCoverageContext,
    ) -> Result<(), ToolsEngineCoverageError> {
        context.validate()?;
        if self.id != entry.id() {
            return Err(ToolsEngineCoverageError::Foreign(self.id));
        }
        if self.context_sha256 != context.fingerprint()?
            || self.applicability != entry.applicability(context.target())
        {
            return Err(ToolsEngineCoverageError::Context);
        }
        let valid = match &self.outcome {
            ToolsEngineCoverageOutcome::Measured {
                expected,
                actual,
                artifacts,
                ..
            } => {
                self.applicability == ToolsEngineCoverageEnumApplicability::CurrentTarget
                    && Self::has_text(expected)
                    && Self::has_text(actual)
                    && Self::has_artifacts(artifacts)
                    && artifacts
                        .iter()
                        .any(|artifact| artifact.kind == ToolsEngineCoverageEnumArtifact::Fixture)
                    && artifacts.iter().any(|artifact| {
                        matches!(
                            artifact.kind,
                            ToolsEngineCoverageEnumArtifact::EngineTranscript
                                | ToolsEngineCoverageEnumArtifact::WorkbenchExport
                                | ToolsEngineCoverageEnumArtifact::NativeMeasurement
                                | ToolsEngineCoverageEnumArtifact::ControlledBuildLog
                        )
                    })
            }
            ToolsEngineCoverageOutcome::Outstanding {
                prerequisite,
                detail,
                next_action,
            } => {
                Self::has_text(detail)
                    && Self::has_text(next_action)
                    && *prerequisite == entry.prerequisite(context.target())
            }
            ToolsEngineCoverageOutcome::UpstreamUnavailable { reason, artifacts } => {
                entry.permits_upstream_unavailability()
                    && Self::has_text(reason)
                    && Self::has_artifacts(artifacts)
                    && artifacts
                        .iter()
                        .any(|artifact| artifact.kind == ToolsEngineCoverageEnumArtifact::Source)
                    && artifacts.iter().any(|artifact| {
                        artifact.kind == ToolsEngineCoverageEnumArtifact::ControlledBuildLog
                    })
            }
        };
        if !valid {
            return Err(ToolsEngineCoverageError::Evidence(self.id));
        }
        Ok(())
    }
}

pub(crate) trait ToolsEngineCoverageProvider {
    async fn classify(
        &mut self,
        entry: &ToolsEngineCoverageEntry,
        context: &ToolsEngineCoverageContext,
    ) -> Result<ToolsEngineCoverageObservation, ToolsEngineCoverageError>;
}

#[derive(Debug)]
pub(crate) struct ToolsEngineCoverageReport {
    document: ToolsEngineCoverageDocument,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolsEngineCoverageDocument {
    schema_version: u32,
    registry_version: u32,
    inventory_sha256: PackageDigest,
    context: ToolsEngineCoverageContext,
    rows: Vec<ToolsEngineCoverageObservation>,
}

impl ToolsEngineCoverageReport {
    pub(crate) const SCHEMA_VERSION: u32 = 1;
    pub(crate) const MAXIMUM_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

    pub(crate) fn observations(&self) -> &[ToolsEngineCoverageObservation] {
        &self.document.rows
    }

    pub(crate) fn parse(
        bytes: &[u8],
        inventory: &ToolsEngineCoverageInventory,
        expected: &ToolsEngineCoverageContext,
    ) -> Result<Self, ToolsEngineCoverageError> {
        if bytes.len() > Self::MAXIMUM_DOCUMENT_BYTES {
            return Err(ToolsEngineCoverageError::Inventory);
        }
        let mut report = Self {
            document: serde_json::from_slice(bytes)?,
        };
        report.validate(inventory, expected)?;
        for row in &mut report.document.rows {
            row.outcome.canonicalize();
        }
        report.document.rows.sort_by_key(|row| row.id);
        Ok(report)
    }

    fn validate(
        &self,
        inventory: &ToolsEngineCoverageInventory,
        expected: &ToolsEngineCoverageContext,
    ) -> Result<(), ToolsEngineCoverageError> {
        let document = &self.document;
        if document.schema_version != Self::SCHEMA_VERSION {
            return Err(ToolsEngineCoverageError::Version(document.schema_version));
        }
        if document.registry_version != EngineCapabilitiesVersion::CURRENT.get()
            || document.inventory_sha256 != inventory.sha256
            || document.context != *expected
        {
            return Err(ToolsEngineCoverageError::Context);
        }
        document.context.validate()?;
        let mut observed = BTreeMap::new();
        for row in &document.rows {
            let entry = inventory
                .entries
                .get(&row.id)
                .ok_or(ToolsEngineCoverageError::Foreign(row.id))?;
            row.validate(entry, &document.context)?;
            if observed.insert(row.id, row).is_some() {
                return Err(ToolsEngineCoverageError::Duplicate(row.id));
            }
        }
        for entry in inventory.entries() {
            if !observed.contains_key(&entry.id()) {
                return Err(ToolsEngineCoverageError::Missing(entry.id()));
            }
        }
        Ok(())
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, ToolsEngineCoverageError> {
        let bytes = serde_json::to_vec_pretty(&self.document)?;
        if bytes.len() > Self::MAXIMUM_DOCUMENT_BYTES {
            return Err(ToolsEngineCoverageError::Inventory);
        }
        Ok(bytes)
    }

    pub(crate) fn is_satisfied(&self) -> bool {
        !self.document.rows.is_empty()
            && self.document.rows.iter().all(|row| {
                matches!(
                    row.outcome,
                    ToolsEngineCoverageOutcome::Measured {
                        verdict: ToolsEngineCoverageEnumVerdict::Passed,
                        ..
                    } | ToolsEngineCoverageOutcome::UpstreamUnavailable { .. }
                )
            })
    }
}

pub(crate) struct ToolsEngineCoverageRunner;

impl ToolsEngineCoverageRunner {
    pub(crate) async fn run<P: ToolsEngineCoverageProvider>(
        inventory: &ToolsEngineCoverageInventory,
        context: &ToolsEngineCoverageContext,
        provider: &mut P,
    ) -> Result<ToolsEngineCoverageReport, ToolsEngineCoverageError> {
        let mut rows = Vec::with_capacity(inventory.entries.len());
        for entry in inventory.entries() {
            let row = provider.classify(entry, context).await?;
            row.validate(entry, context)?;
            rows.push(row);
        }
        let report = ToolsEngineCoverageReport {
            document: ToolsEngineCoverageDocument {
                schema_version: ToolsEngineCoverageReport::SCHEMA_VERSION,
                registry_version: EngineCapabilitiesVersion::CURRENT.get(),
                inventory_sha256: inventory.sha256,
                context: context.clone(),
                rows,
            },
        };
        report.validate(inventory, context)?;
        Ok(report)
    }
}
