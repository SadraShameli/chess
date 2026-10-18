use std::collections::BTreeMap;
use strum::IntoEnumIterator;

use chess::engine::capabilities::EngineCapabilitiesVersion;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::manifest::{EnginePackageEnumTarget, EnginePackageManifest};
use chess::package::digest::PackageDigest;
use chess::workbench::enums::WorkbenchEnumOperation;
use serde::{Deserialize, Serialize};

#[path = "../../tools/engine/coverage.rs"]
mod tools_engine_coverage;

use tools_engine_coverage::{
    ToolsEngineCoverageArtifact, ToolsEngineCoverageContext, ToolsEngineCoverageEntry,
    ToolsEngineCoverageEnumApplicability, ToolsEngineCoverageEnumArtifact,
    ToolsEngineCoverageEnumBinding, ToolsEngineCoverageEnumCategory,
    ToolsEngineCoverageEnumPrerequisite, ToolsEngineCoverageEnumVerdict, ToolsEngineCoverageError,
    ToolsEngineCoverageId, ToolsEngineCoverageInventory, ToolsEngineCoverageObservation,
    ToolsEngineCoverageOutcome, ToolsEngineCoverageProvider, ToolsEngineCoverageReport,
    ToolsEngineCoverageRunner,
};

#[derive(Serialize, Deserialize)]
struct ToolsEngineCoverageTestDocument {
    schema_version: u32,
    registry_version: u32,
    inventory_sha256: PackageDigest,
    context: ToolsEngineCoverageContext,
    rows: Vec<ToolsEngineCoverageTestRow>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ToolsEngineCoverageTestRow {
    id: ToolsEngineCoverageId,
    context_sha256: PackageDigest,
    applicability: ToolsEngineCoverageEnumApplicability,
    outcome: ToolsEngineCoverageOutcome,
}

impl ToolsEngineCoverageTestDocument {
    fn fixture() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(serde_json::from_slice(include_bytes!(
            "../fixtures/engine-capabilities.json"
        ))?)
    }

    fn expected() -> Result<ToolsEngineCoverageContext, Box<dyn std::error::Error>> {
        let manifest =
            EnginePackageManifest::parse(include_bytes!("../../resources/engine/manifest.json"))?;
        let entry = manifest
            .entries()
            .first()
            .ok_or("missing packaged engine fixture")?;
        Ok(ToolsEngineCoverageContext::new(
            entry.target(),
            entry.profile(),
            entry.sha256(),
        )?)
    }

    fn parse(&self) -> Result<ToolsEngineCoverageReport, Box<dyn std::error::Error>> {
        Ok(ToolsEngineCoverageReport::parse(
            &serde_json::to_vec(self)?,
            &ToolsEngineCoverageInventory::pinned()?,
            &Self::expected()?,
        )?)
    }
}

struct ToolsEngineCoverageTestProvider {
    outcomes: BTreeMap<ToolsEngineCoverageId, ToolsEngineCoverageOutcome>,
    visited: Vec<ToolsEngineCoverageId>,
}

impl ToolsEngineCoverageTestProvider {
    fn fixture() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            outcomes: ToolsEngineCoverageTestDocument::fixture()?
                .rows
                .into_iter()
                .map(|row| (row.id, row.outcome))
                .collect(),
            visited: Vec::new(),
        })
    }
}

impl ToolsEngineCoverageProvider for ToolsEngineCoverageTestProvider {
    async fn classify(
        &mut self,
        entry: &ToolsEngineCoverageEntry,
        context: &ToolsEngineCoverageContext,
    ) -> Result<ToolsEngineCoverageObservation, ToolsEngineCoverageError> {
        self.visited.push(entry.id());
        let outcome = self
            .outcomes
            .remove(&entry.id())
            .ok_or(ToolsEngineCoverageError::Missing(entry.id()))?;
        ToolsEngineCoverageObservation::new(entry, context, outcome)
    }
}

struct ToolsEngineCoverageTestForeignProvider {
    observation: Option<ToolsEngineCoverageObservation>,
}

impl ToolsEngineCoverageProvider for ToolsEngineCoverageTestForeignProvider {
    async fn classify(
        &mut self,
        entry: &ToolsEngineCoverageEntry,
        _context: &ToolsEngineCoverageContext,
    ) -> Result<ToolsEngineCoverageObservation, ToolsEngineCoverageError> {
        self.observation
            .take()
            .ok_or(ToolsEngineCoverageError::Missing(entry.id()))
    }
}

#[test]
fn canonical_inventory_binds_every_real_option_and_workbench_registry_member_once()
-> Result<(), Box<dyn std::error::Error>> {
    let inventory = ToolsEngineCoverageInventory::pinned()?;
    assert!(inventory.entries().len() > 0);
    for option in EngineEnumOption::iter() {
        assert_eq!(
            inventory
                .entries()
                .filter(|entry| {
                    entry.binding() == ToolsEngineCoverageEnumBinding::Option(option)
                })
                .count(),
            1
        );
    }
    for operation in WorkbenchEnumOperation::ALL {
        assert_eq!(
            inventory
                .entries()
                .filter(|entry| {
                    entry.binding() == ToolsEngineCoverageEnumBinding::Workbench(operation)
                })
                .count(),
            1
        );
    }
    for entry in inventory.entries() {
        assert!(!entry.facility().is_empty());
        assert!(!entry.requirement().is_empty());
        match entry.binding() {
            ToolsEngineCoverageEnumBinding::Option(_) => {
                assert_eq!(
                    entry.id().category(),
                    ToolsEngineCoverageEnumCategory::Option
                );
            }
            ToolsEngineCoverageEnumBinding::Workbench(_) => {
                assert_eq!(
                    entry.id().category(),
                    ToolsEngineCoverageEnumCategory::Developer
                );
            }
            ToolsEngineCoverageEnumBinding::Inventory => {}
        }
        assert_eq!(
            entry.id().to_string().parse::<ToolsEngineCoverageId>()?,
            entry.id()
        );
    }
    Ok(())
}

#[test]
fn inventory_rejects_empty_duplicate_foreign_and_registry_drift()
-> Result<(), Box<dyn std::error::Error>> {
    let source = ToolsEngineCoverageInventory::SOURCE;
    assert!(ToolsEngineCoverageInventory::parse("").is_err());
    let first = source
        .lines()
        .find(|line| line.starts_with("| OPT-"))
        .ok_or("missing option row")?;
    assert!(ToolsEngineCoverageInventory::parse(&format!("{source}\n{first}\n")).is_err());
    assert!(ToolsEngineCoverageInventory::parse(&source.replace(first, "")).is_err());
    assert!(
        ToolsEngineCoverageInventory::parse(&source.replacen("`Hash`", "`ForeignOption`", 1))
            .is_err()
    );
    let workbench = source
        .lines()
        .find(|line| line.starts_with("| DEV-") && line.contains("`perft depth`"))
        .ok_or("missing perft row")?;
    assert!(ToolsEngineCoverageInventory::parse(&source.replace(workbench, "")).is_err());
    assert!(
        ToolsEngineCoverageInventory::parse(&format!("{source}\n| FUT-01 | foreign | invalid |\n"))
            .is_err()
    );
    assert!("OPT-1".parse::<ToolsEngineCoverageId>().is_err());
    assert!("OPT-00".parse::<ToolsEngineCoverageId>().is_err());
    Ok(())
}

#[test]
fn fixture_classifies_every_canonical_member_with_concrete_outstanding_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageTestDocument::fixture()?;
    let inventory = ToolsEngineCoverageInventory::pinned()?;
    assert_eq!(fixture.rows.len(), inventory.entries().len());
    let report = fixture.parse()?;
    assert!(!report.is_satisfied());
    assert_eq!(report.observations().len(), fixture.rows.len());
    for observation in report.observations() {
        assert!(fixture.rows.iter().any(|row| row.id == observation.id()));
        assert!(matches!(
            observation.outcome(),
            ToolsEngineCoverageOutcome::Outstanding { .. }
        ));
    }
    for entry in inventory.entries() {
        let row = fixture
            .rows
            .iter()
            .find(|row| row.id == entry.id())
            .ok_or("unclassified inventory member")?;
        assert_eq!(
            row.applicability,
            entry.applicability(fixture.context.target())
        );
        let ToolsEngineCoverageOutcome::Outstanding {
            detail,
            next_action,
            ..
        } = &row.outcome
        else {
            return Err("the schema fixture must not claim an engine run".into());
        };
        assert!(!detail.trim().is_empty());
        assert!(!next_action.trim().is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn runner_visits_the_canonical_registry_once_and_preserves_outstanding_results()
-> Result<(), Box<dyn std::error::Error>> {
    let inventory = ToolsEngineCoverageInventory::pinned()?;
    let context = ToolsEngineCoverageTestDocument::expected()?;
    let mut provider = ToolsEngineCoverageTestProvider::fixture()?;
    let report = ToolsEngineCoverageRunner::run(&inventory, &context, &mut provider).await?;
    assert!(!report.is_satisfied());
    assert!(provider.outcomes.is_empty());
    assert_eq!(
        provider.visited,
        inventory
            .entries()
            .map(ToolsEngineCoverageEntry::id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        report.encode()?,
        ToolsEngineCoverageTestDocument::fixture()?
            .parse()?
            .encode()?
    );
    Ok(())
}

#[tokio::test]
async fn runner_propagates_a_missing_workflow_instead_of_an_empty_or_successful_report()
-> Result<(), Box<dyn std::error::Error>> {
    let inventory = ToolsEngineCoverageInventory::pinned()?;
    let context = ToolsEngineCoverageTestDocument::expected()?;
    let mut provider = ToolsEngineCoverageTestProvider::fixture()?;
    provider.outcomes.clear();
    assert!(matches!(
        ToolsEngineCoverageRunner::run(&inventory, &context, &mut provider).await,
        Err(ToolsEngineCoverageError::Missing(_))
    ));
    assert_eq!(provider.visited.len(), 1);
    Ok(())
}

#[tokio::test]
async fn runner_rejects_a_provider_result_for_another_member_or_package()
-> Result<(), Box<dyn std::error::Error>> {
    let inventory = ToolsEngineCoverageInventory::pinned()?;
    let context = ToolsEngineCoverageTestDocument::expected()?;
    let mut fixture = ToolsEngineCoverageTestProvider::fixture()?;
    let foreign = inventory
        .entries()
        .nth(1)
        .ok_or("missing second inventory member")?;
    let outcome = fixture
        .outcomes
        .remove(&foreign.id())
        .ok_or("missing classified member")?;
    let mut provider = ToolsEngineCoverageTestForeignProvider {
        observation: Some(ToolsEngineCoverageObservation::new(
            foreign, &context, outcome,
        )?),
    };
    assert!(matches!(
        ToolsEngineCoverageRunner::run(&inventory, &context, &mut provider).await,
        Err(ToolsEngineCoverageError::Foreign(_))
    ));
    let first = inventory.entries().next().ok_or("empty inventory")?;
    let other_package = ToolsEngineCoverageContext::new(
        context.target(),
        chess::engine::package::manifest::EnginePackageEnumProfile::Portable,
        PackageDigest::read(&b"another measured package"[..])?,
    )?;
    let outcome = fixture
        .outcomes
        .remove(&first.id())
        .ok_or("missing first member")?;
    let mut provider = ToolsEngineCoverageTestForeignProvider {
        observation: Some(ToolsEngineCoverageObservation::new(
            first,
            &other_package,
            outcome,
        )?),
    };
    assert!(matches!(
        ToolsEngineCoverageRunner::run(&inventory, &context, &mut provider).await,
        Err(ToolsEngineCoverageError::Context)
    ));
    Ok(())
}

#[test]
fn reports_reject_empty_duplicate_missing_and_foreign_classifications()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = ToolsEngineCoverageTestDocument::fixture()?;
    let row = fixture
        .rows
        .first()
        .ok_or("missing coverage fixture")?
        .clone();
    fixture.rows.push(row);
    assert!(fixture.parse().is_err());
    let mut fixture = ToolsEngineCoverageTestDocument::fixture()?;
    fixture.rows.pop().ok_or("missing coverage fixture")?;
    assert!(fixture.parse().is_err());
    let mut fixture = ToolsEngineCoverageTestDocument::fixture()?;
    fixture
        .rows
        .first_mut()
        .ok_or("missing coverage fixture")?
        .id = "OPT-99".parse()?;
    assert!(fixture.parse().is_err());
    fixture.rows.clear();
    assert!(fixture.parse().is_err());
    Ok(())
}

#[test]
fn report_versions_and_inventory_digests_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = ToolsEngineCoverageTestDocument::fixture()?;
    fixture.schema_version = ToolsEngineCoverageReport::SCHEMA_VERSION + 1;
    assert!(fixture.parse().is_err());
    fixture.schema_version = ToolsEngineCoverageReport::SCHEMA_VERSION;
    fixture.registry_version = EngineCapabilitiesVersion::CURRENT.get() + 1;
    assert!(fixture.parse().is_err());
    fixture.registry_version = EngineCapabilitiesVersion::CURRENT.get();
    fixture.inventory_sha256 = PackageDigest::read(&b"another inventory"[..])?;
    assert!(fixture.parse().is_err());
    Ok(())
}

#[test]
fn reports_reject_wrong_package_target_model_source_and_row_context()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageTestDocument::fixture()?;
    let context = ToolsEngineCoverageTestDocument::expected()?;
    let other_package = ToolsEngineCoverageContext::new(
        context.target(),
        chess::engine::package::manifest::EnginePackageEnumProfile::Portable,
        PackageDigest::read(&b"different executable"[..])?,
    )?;
    assert!(
        ToolsEngineCoverageReport::parse(
            &serde_json::to_vec(&fixture)?,
            &ToolsEngineCoverageInventory::pinned()?,
            &other_package
        )
        .is_err()
    );
    let mut wrong_target = ToolsEngineCoverageTestDocument::fixture()?;
    wrong_target.context = ToolsEngineCoverageContext::new(
        EnginePackageEnumTarget::WindowsX64,
        chess::engine::package::manifest::EnginePackageEnumProfile::Portable,
        PackageDigest::read(&b"different target"[..])?,
    )?;
    assert!(wrong_target.parse().is_err());
    let bytes = String::from_utf8(serde_json::to_vec(&fixture)?)?;
    for invalid in [
        bytes.replace(EnginePackageManifest::NETWORK_SHA256, &"0".repeat(64)),
        bytes.replace(EnginePackageManifest::COMMIT, &"0".repeat(40)),
        bytes.replace("\"portable\"", "\"unbuilt_avx_profile\""),
    ] {
        assert!(
            ToolsEngineCoverageReport::parse(
                invalid.as_bytes(),
                &ToolsEngineCoverageInventory::pinned()?,
                &context
            )
            .is_err()
        );
    }
    let mut fixture = ToolsEngineCoverageTestDocument::fixture()?;
    fixture
        .rows
        .first_mut()
        .ok_or("missing coverage fixture")?
        .context_sha256 = PackageDigest::read(&b"other context"[..])?;
    assert!(fixture.parse().is_err());
    Ok(())
}

#[test]
fn encoding_is_deterministic_after_row_permutation() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = ToolsEngineCoverageTestDocument::fixture()?;
    let expected = fixture.parse()?.encode()?;
    fixture.rows.reverse();
    assert_eq!(fixture.parse()?.encode()?, expected);
    let inventory = ToolsEngineCoverageInventory::pinned()?;
    let context = ToolsEngineCoverageTestDocument::expected()?;
    assert_eq!(
        ToolsEngineCoverageReport::parse(&expected, &inventory, &context)?.encode()?,
        expected
    );
    Ok(())
}

#[test]
fn upstream_unavailability_requires_source_and_build_evidence_and_cannot_hide_application_work()
-> Result<(), Box<dyn std::error::Error>> {
    let inventory = ToolsEngineCoverageInventory::pinned()?;
    let context = ToolsEngineCoverageTestDocument::expected()?;
    let spsa = inventory
        .entries()
        .find(|entry| entry.facility().contains("`spsa`"))
        .ok_or("missing SPSA inventory member")?;
    let artifacts = vec![
        ToolsEngineCoverageArtifact::capture(
            ToolsEngineCoverageEnumArtifact::Source,
            "fixtures/spsa-source.txt".into(),
            b"hermetic source evidence for schema validation",
        )?,
        ToolsEngineCoverageArtifact::capture(
            ToolsEngineCoverageEnumArtifact::ControlledBuildLog,
            "fixtures/spsa-build.txt".into(),
            b"hermetic build evidence for schema validation",
        )?,
    ];
    let unavailable = ToolsEngineCoverageOutcome::UpstreamUnavailable {
        reason: "Hermetic schema case, no actual compile result claimed".into(),
        artifacts,
    };
    assert!(ToolsEngineCoverageObservation::new(spsa, &context, unavailable.clone()).is_ok());
    for entry in inventory.entries().filter(|entry| entry.id() != spsa.id()) {
        assert!(ToolsEngineCoverageObservation::new(entry, &context, unavailable.clone()).is_err());
    }
    let missing_build = ToolsEngineCoverageOutcome::UpstreamUnavailable {
        reason: "Source suspicion alone is insufficient".into(),
        artifacts: vec![ToolsEngineCoverageArtifact::capture(
            ToolsEngineCoverageEnumArtifact::Source,
            "fixtures/source.txt".into(),
            b"source only",
        )?],
    };
    assert!(ToolsEngineCoverageObservation::new(spsa, &context, missing_build).is_err());
    Ok(())
}

#[test]
fn measured_outcomes_require_both_fixture_and_actual_output_artifacts()
-> Result<(), Box<dyn std::error::Error>> {
    let inventory = ToolsEngineCoverageInventory::pinned()?;
    let context = ToolsEngineCoverageTestDocument::expected()?;
    let entry = inventory
        .entries()
        .find(|entry| {
            matches!(
                entry.binding(),
                ToolsEngineCoverageEnumBinding::Workbench(WorkbenchEnumOperation::Perft)
            )
        })
        .ok_or("missing registered perft")?;
    let fixture = ToolsEngineCoverageArtifact::capture(
        ToolsEngineCoverageEnumArtifact::Fixture,
        "fixtures/perft.json".into(),
        b"hermetic invalid perft fixture",
    )?;
    let output = ToolsEngineCoverageArtifact::capture(
        ToolsEngineCoverageEnumArtifact::WorkbenchExport,
        "fixtures/perft-export.json".into(),
        b"hermetic schema output, no native run",
    )?;
    assert_eq!(fixture.kind(), ToolsEngineCoverageEnumArtifact::Fixture);
    assert_eq!(fixture.path(), "fixtures/perft.json");
    assert_ne!(fixture.sha256(), output.sha256());
    let outcome = ToolsEngineCoverageOutcome::Measured {
        verdict: ToolsEngineCoverageEnumVerdict::Failed,
        expected: "This case validates the evidence boundary only".into(),
        actual: "Native execution was not performed by this hermetic case".into(),
        artifacts: vec![fixture.clone(), output.clone()],
    };
    assert!(ToolsEngineCoverageObservation::new(entry, &context, outcome.clone()).is_ok());
    let mut document = ToolsEngineCoverageTestDocument::fixture()?;
    document
        .rows
        .iter_mut()
        .find(|row| row.id == entry.id())
        .ok_or("missing perft coverage fixture")?
        .outcome = outcome;
    let expected_encoding = document.parse()?.encode()?;
    let row = document
        .rows
        .iter_mut()
        .find(|row| row.id == entry.id())
        .ok_or("missing measured perft fixture")?;
    let ToolsEngineCoverageOutcome::Measured { artifacts, .. } = &mut row.outcome else {
        return Err("missing measured fixture artifacts".into());
    };
    artifacts.reverse();
    assert_eq!(document.parse()?.encode()?, expected_encoding);
    for artifacts in [
        vec![],
        vec![fixture.clone()],
        vec![output.clone()],
        vec![fixture.clone(), output.clone(), output],
        vec![fixture.clone(), fixture],
    ] {
        let outcome = ToolsEngineCoverageOutcome::Measured {
            verdict: ToolsEngineCoverageEnumVerdict::Passed,
            expected: "Expected perft count".into(),
            actual: "No observed output".into(),
            artifacts,
        };
        assert!(ToolsEngineCoverageObservation::new(entry, &context, outcome).is_err());
    }
    assert!(
        ToolsEngineCoverageArtifact::capture(
            ToolsEngineCoverageEnumArtifact::Fixture,
            "../outside.json".into(),
            b"outside"
        )
        .is_err()
    );
    assert!(
        ToolsEngineCoverageArtifact::capture(
            ToolsEngineCoverageEnumArtifact::Fixture,
            "fixtures/empty.json".into(),
            b""
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn outstanding_prerequisites_and_platform_applicability_are_required()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = ToolsEngineCoverageTestDocument::fixture()?;
    let row = fixture.rows.first_mut().ok_or("missing coverage fixture")?;
    row.outcome = ToolsEngineCoverageOutcome::Outstanding {
        prerequisite: ToolsEngineCoverageEnumPrerequisite::ApplicationWorkflow,
        detail: String::new(),
        next_action: "Run the owning workflow".into(),
    };
    assert!(fixture.parse().is_err());
    let mut fixture = ToolsEngineCoverageTestDocument::fixture()?;
    let row = fixture
        .rows
        .iter_mut()
        .find(|row| row.applicability == ToolsEngineCoverageEnumApplicability::OtherTarget)
        .ok_or("missing native-target prerequisite")?;
    row.outcome = ToolsEngineCoverageOutcome::Outstanding {
        prerequisite: ToolsEngineCoverageEnumPrerequisite::ApplicationWorkflow,
        detail: "Unavailable local host".into(),
        next_action: "Run on the required native hardware".into(),
    };
    assert!(fixture.parse().is_err());
    let mut fixture = ToolsEngineCoverageTestDocument::fixture()?;
    fixture
        .rows
        .first_mut()
        .ok_or("missing coverage fixture")?
        .applicability = ToolsEngineCoverageEnumApplicability::OtherTarget;
    assert!(fixture.parse().is_err());
    let error = ToolsEngineCoverageError::Workflow(Box::new(std::io::Error::other(
        "controlled adapter failure",
    )));
    assert!(std::error::Error::source(&error).is_some());
    Ok(())
}
