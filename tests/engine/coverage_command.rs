use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::engine::package::host::EnginePackageHost;
use chess::engine::package::manifest::{
    EnginePackageEntry, EnginePackageEnumTarget, EnginePackageManifest,
};
use chess::package::digest::PackageDigest;
use chess::workbench::enums::WorkbenchEnumOperation;
use tempfile::TempDir;

use crate::engine::coverage::{
    ToolsEngineCoverageArtifact, ToolsEngineCoverageContext, ToolsEngineCoverageEntry,
    ToolsEngineCoverageEnumArtifact, ToolsEngineCoverageEnumBinding,
    ToolsEngineCoverageEnumVerdict, ToolsEngineCoverageError, ToolsEngineCoverageId,
    ToolsEngineCoverageInventory, ToolsEngineCoverageObservation, ToolsEngineCoverageOutcome,
    ToolsEngineCoverageProvider, ToolsEngineCoverageReport, ToolsEngineCoverageRunner,
};
use crate::engine::coverage_command::{
    ToolsEngineCoverageCommand, ToolsEngineCoverageCommandConfig, ToolsEngineCoverageCommandError,
    ToolsEngineCoverageCommandIo, ToolsEngineCoverageCommandPrepared,
    ToolsEngineCoverageCommandProvider, ToolsEngineCoverageCommandPublication,
    ToolsEngineCoverageCommandResources, ToolsEngineCoverageCommandSummary,
};

struct ToolsEngineCoverageCommandTestFixture {
    directory: TempDir,
    package: PathBuf,
    context: ToolsEngineCoverageContext,
}

impl ToolsEngineCoverageCommandTestFixture {
    #[cfg(unix)]
    async fn publication(
        &self,
        output: PathBuf,
        reports: Vec<PathBuf>,
    ) -> Result<ToolsEngineCoverageCommandPublication, Box<dyn std::error::Error>> {
        Ok(ToolsEngineCoverageCommandPublication::new(
            self.prepared(output, reports).await?,
        ))
    }

    #[cfg(unix)]
    async fn prepared(
        &self,
        output: PathBuf,
        reports: Vec<PathBuf>,
    ) -> Result<ToolsEngineCoverageCommandPrepared, Box<dyn std::error::Error>> {
        let config = self.config(output, reports)?;
        let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid fixture budget")?)?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let cancellation = Arc::new(AtomicBool::new(false));
        let prepared = executor
            .submit(move |worker| {
                ToolsEngineCoverageCommandPrepared::prepare(
                    config,
                    cancellation,
                    Self::host()?,
                    worker,
                )
            })?
            .join()
            .await??;
        executor.shutdown().await?;
        Ok(prepared)
    }
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let package = directory.path().join("package");
        let payload = b"hermetic checksum fixture, not an executed engine";
        let entry = EnginePackageEntry::portable(
            EnginePackageEnumTarget::MacosArm,
            u64::try_from(payload.len())?,
            PackageDigest::read(&payload[..])?,
            "controlled fixture compiler".into(),
        )?;
        let executable = package.join(entry.executable());
        std::fs::create_dir_all(executable.parent().ok_or("missing package parent")?)?;
        std::fs::write(&executable, payload)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        }
        let context =
            ToolsEngineCoverageContext::new(entry.target(), entry.profile(), entry.sha256())?;
        std::fs::write(
            package.join("manifest.json"),
            serde_json::to_vec(&EnginePackageManifest::new(entry)?)?,
        )?;
        Ok(Self {
            directory,
            package,
            context,
        })
    }

    fn host() -> Result<EnginePackageHost, chess::engine::package::host::EnginePackageHostError> {
        EnginePackageHost::from_observation(EnginePackageEnumTarget::MacosArm, "14.0", "", true)
    }

    fn output(&self, directory: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let parent = self.directory.path().join(directory);
        std::fs::create_dir_all(&parent)?;
        Ok(parent.join("coverage.json"))
    }

    fn config(
        &self,
        output: PathBuf,
        reports: Vec<PathBuf>,
    ) -> Result<ToolsEngineCoverageCommandConfig, ToolsEngineCoverageCommandError> {
        ToolsEngineCoverageCommandConfig::new(self.package.clone(), output, reports)
    }

    async fn evidence(
        &self,
        directory: &str,
        actual: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let parent = self.directory.path().join(directory);
        std::fs::create_dir_all(parent.join("case"))?;
        let fixture_bytes = b"fixed hermetic input fixture";
        let output_bytes = b"controlled adapter failure evidence, no real engine execution";
        std::fs::write(parent.join("case/input.json"), fixture_bytes)?;
        std::fs::write(parent.join("case/output.json"), output_bytes)?;
        let inventory = ToolsEngineCoverageInventory::pinned()?;
        let selected = inventory
            .entries()
            .find(|entry| {
                entry.binding()
                    == ToolsEngineCoverageEnumBinding::Workbench(WorkbenchEnumOperation::Perft)
            })
            .ok_or("missing registered perft")?;
        let outcome = ToolsEngineCoverageOutcome::Measured {
            verdict: ToolsEngineCoverageEnumVerdict::Failed,
            expected: "Hermetic adapter evidence boundary only".into(),
            actual: actual.into(),
            artifacts: vec![
                ToolsEngineCoverageArtifact::capture(
                    ToolsEngineCoverageEnumArtifact::Fixture,
                    "case/input.json".into(),
                    fixture_bytes,
                )?,
                ToolsEngineCoverageArtifact::capture(
                    ToolsEngineCoverageEnumArtifact::WorkbenchExport,
                    "case/output.json".into(),
                    output_bytes,
                )?,
            ],
        };
        let mut provider = ToolsEngineCoverageCommandTestProvider {
            selected: selected.id(),
            outcome,
        };
        let report =
            ToolsEngineCoverageRunner::run(&inventory, &self.context, &mut provider).await?;
        let path = parent.join("evidence.json");
        std::fs::write(&path, report.encode()?)?;
        Ok(path)
    }

    async fn run(
        &self,
        config: ToolsEngineCoverageCommandConfig,
        cancellation: Arc<AtomicBool>,
    ) -> Result<ToolsEngineCoverageCommandSummary, ToolsEngineCoverageCommandError> {
        let budget = CapacityBudget::new(
            NonZeroUsize::new(1).ok_or(ToolsEngineCoverageCommandError::Configuration)?,
        )
        .map_err(chess::capacity::executor::CapacityExecutorError::from)?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let result = ToolsEngineCoverageCommand::run_with_host(
            config,
            ToolsEngineCoverageCommandResources::new(
                &executor,
                cancellation,
                tokio::runtime::Handle::current(),
            ),
            Self::host,
        )
        .await;
        assert_eq!(budget.available(), budget.workers());
        executor.shutdown().await?;
        result
    }

    fn parse(
        &self,
        output: &Path,
    ) -> Result<ToolsEngineCoverageReport, Box<dyn std::error::Error>> {
        Ok(ToolsEngineCoverageReport::parse(
            &std::fs::read(output)?,
            &ToolsEngineCoverageInventory::pinned()?,
            &self.context,
        )?)
    }
}

struct ToolsEngineCoverageCommandTestProvider {
    selected: ToolsEngineCoverageId,
    outcome: ToolsEngineCoverageOutcome,
}

impl ToolsEngineCoverageProvider for ToolsEngineCoverageCommandTestProvider {
    async fn classify(
        &mut self,
        entry: &ToolsEngineCoverageEntry,
        context: &ToolsEngineCoverageContext,
    ) -> Result<ToolsEngineCoverageObservation, ToolsEngineCoverageError> {
        let outcome = if entry.id() == self.selected {
            self.outcome.clone()
        } else {
            ToolsEngineCoverageCommandProvider::outstanding(entry, context)
        };
        ToolsEngineCoverageObservation::new(entry, context, outcome)
    }
}

#[tokio::test]
async fn one_worker_publishes_full_outstanding_inventory_and_explicit_summary()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let output = fixture.output("missing-evidence")?;
    let summary = fixture
        .run(
            fixture.config(output.clone(), Vec::new())?,
            Arc::new(AtomicBool::new(false)),
        )
        .await?;
    let report = fixture.parse(&output)?;
    assert_eq!(
        report.observations().len(),
        ToolsEngineCoverageInventory::pinned()?.entries().len()
    );
    assert!(!report.is_satisfied());
    assert!(report.observations().iter().all(|observation| matches!(
        observation.outcome(),
        ToolsEngineCoverageOutcome::Outstanding { .. }
    )));
    let message = summary.to_string();
    assert!(message.contains("outstanding"));
    assert!(message.contains("0 measured passed"));
    assert!(message.contains("T090 real execution acceptance remains open"));
    assert!(!output.with_file_name("coverage.json.artifacts").exists());
    Ok(())
}

#[tokio::test]
async fn imports_verify_and_relocate_artifact_bytes_without_turning_failure_into_success()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("input", "Controlled observed failure")
        .await?;
    let output = fixture.output("result")?;
    let summary = fixture
        .run(
            fixture.config(output.clone(), vec![input])?,
            Arc::new(AtomicBool::new(false)),
        )
        .await?;
    let report = fixture.parse(&output)?;
    let mut measured = 0;
    for observation in report.observations() {
        if let ToolsEngineCoverageOutcome::Measured {
            verdict, artifacts, ..
        } = observation.outcome()
        {
            measured += 1;
            assert_eq!(*verdict, ToolsEngineCoverageEnumVerdict::Failed);
            for artifact in artifacts {
                let path = output
                    .parent()
                    .ok_or("missing output parent")?
                    .join(artifact.path());
                assert!(path.is_file());
                let bytes = std::fs::read(path)?;
                assert_eq!(PackageDigest::read(bytes.as_slice())?, artifact.sha256());
                assert!(artifact.path().starts_with("coverage.json.artifacts/"));
            }
        }
    }
    assert_eq!(measured, 1);
    assert!(summary.to_string().contains("1 measured failed"));
    assert!(!report.is_satisfied());
    Ok(())
}

#[tokio::test]
async fn equivalent_overlaps_deduplicate_and_input_order_does_not_change_encoding()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let first = fixture
        .evidence("first", "Same controlled observation")
        .await?;
    let second = fixture
        .evidence("second", "Same controlled observation")
        .await?;
    let output_a = fixture.output("result-a")?;
    let output_b = fixture.output("result-b")?;
    fixture
        .run(
            fixture.config(output_a.clone(), vec![first.clone(), second.clone()])?,
            Arc::new(AtomicBool::new(false)),
        )
        .await?;
    fixture
        .run(
            fixture.config(output_b.clone(), vec![second, first])?,
            Arc::new(AtomicBool::new(false)),
        )
        .await?;
    assert_eq!(std::fs::read(&output_a)?, std::fs::read(&output_b)?);
    assert_eq!(
        fixture
            .parse(&output_a)?
            .observations()
            .iter()
            .filter(|observation| matches!(
                observation.outcome(),
                ToolsEngineCoverageOutcome::Measured { .. }
            ))
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn identical_artifact_aliases_preserve_the_valid_observation_deterministically()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("aliases", "Controlled observed failure")
        .await?;
    let expected_output = fixture.output("without-alias")?;
    fixture
        .run(
            fixture.config(expected_output.clone(), vec![input.clone()])?,
            Arc::new(AtomicBool::new(false)),
        )
        .await?;
    let inventory = ToolsEngineCoverageInventory::pinned()?;
    let report =
        ToolsEngineCoverageReport::parse(&std::fs::read(&input)?, &inventory, &fixture.context)?;
    let observation = report
        .observations()
        .iter()
        .find(|observation| {
            matches!(
                observation.outcome(),
                ToolsEngineCoverageOutcome::Measured { .. }
            )
        })
        .ok_or("missing measured observation")?;
    let mut outcome = observation.outcome().clone();
    let ToolsEngineCoverageOutcome::Measured { artifacts, .. } = &mut outcome else {
        return Err("missing measured artifact collection".into());
    };
    let original = artifacts
        .iter()
        .find(|artifact| artifact.kind() == ToolsEngineCoverageEnumArtifact::Fixture)
        .ok_or("missing original fixture artifact")?;
    let report_root = input.parent().ok_or("missing selected report root")?;
    let bytes = std::fs::read(report_root.join(original.path()))?;
    let alias = "case/input-alias.json";
    std::fs::write(report_root.join(alias), &bytes)?;
    artifacts.push(ToolsEngineCoverageArtifact::capture(
        original.kind(),
        alias.into(),
        &bytes,
    )?);
    let mut provider = ToolsEngineCoverageCommandTestProvider {
        selected: observation.id(),
        outcome,
    };
    let aliased =
        ToolsEngineCoverageRunner::run(&inventory, &fixture.context, &mut provider).await?;
    let encoded = aliased.encode()?;
    assert!(ToolsEngineCoverageReport::parse(&encoded, &inventory, &fixture.context).is_ok());
    std::fs::write(&input, encoded)?;
    let actual_output = fixture.output("with-alias")?;
    fixture
        .run(
            fixture.config(actual_output.clone(), vec![input])?,
            Arc::new(AtomicBool::new(false)),
        )
        .await?;
    assert_eq!(
        std::fs::read(expected_output)?,
        std::fs::read(&actual_output)?
    );
    let actual = fixture.parse(&actual_output)?;
    let measured = actual
        .observations()
        .iter()
        .find(|observation| observation.id() == provider.selected)
        .ok_or("missing preserved observation")?;
    let ToolsEngineCoverageOutcome::Measured {
        verdict, artifacts, ..
    } = measured.outcome()
    else {
        return Err("the valid measured observation was lost".into());
    };
    assert_eq!(*verdict, ToolsEngineCoverageEnumVerdict::Failed);
    assert_eq!(artifacts.len(), 2);
    Ok(())
}

#[tokio::test]
async fn conflicting_overlaps_fail_without_publishing_a_report_or_bundle()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let first = fixture
        .evidence("first", "First controlled observation")
        .await?;
    let second = fixture
        .evidence("second", "Conflicting controlled observation")
        .await?;
    let output = fixture.output("conflict")?;
    assert!(matches!(
        fixture
            .run(
                fixture.config(output.clone(), vec![first, second])?,
                Arc::new(AtomicBool::new(false))
            )
            .await,
        Err(ToolsEngineCoverageCommandError::Conflict(_))
    ));
    assert!(!output.exists());
    assert!(!output.with_file_name("coverage.json.artifacts").exists());
    assert_eq!(
        std::fs::read_dir(output.parent().ok_or("missing output parent")?)?.count(),
        0
    );
    Ok(())
}

#[tokio::test]
async fn artifact_tampering_and_missing_files_are_errors_not_outstanding_fallbacks()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("input", "Controlled original observation")
        .await?;
    let artifact = input
        .parent()
        .ok_or("missing evidence parent")?
        .join("case/output.json");
    std::fs::write(&artifact, b"tampered")?;
    let output = fixture.output("tampered")?;
    assert!(matches!(
        fixture
            .run(
                fixture.config(output.clone(), vec![input.clone()])?,
                Arc::new(AtomicBool::new(false))
            )
            .await,
        Err(ToolsEngineCoverageCommandError::ArtifactHash(_))
    ));
    assert!(!output.exists());
    std::fs::remove_file(artifact)?;
    let missing_output = fixture.output("missing")?;
    assert!(
        fixture
            .run(
                fixture.config(missing_output.clone(), vec![input])?,
                Arc::new(AtomicBool::new(false))
            )
            .await
            .is_err()
    );
    assert!(!missing_output.exists());
    Ok(())
}

#[tokio::test]
async fn report_metadata_and_artifact_path_tampering_are_rejected_before_use()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("input", "Controlled original observation")
        .await?;
    let original = std::fs::read_to_string(&input)?;
    let wrong_source = original.replace(EnginePackageManifest::COMMIT, &"0".repeat(40));
    std::fs::write(&input, wrong_source)?;
    let output = fixture.output("metadata")?;
    assert!(
        fixture
            .run(
                fixture.config(output.clone(), vec![input.clone()])?,
                Arc::new(AtomicBool::new(false))
            )
            .await
            .is_err()
    );
    assert!(!output.exists());
    std::fs::write(
        &input,
        original.replace("case/output.json", "../outside.json"),
    )?;
    let output = fixture.output("traversal")?;
    assert!(
        fixture
            .run(
                fixture.config(output.clone(), vec![input])?,
                Arc::new(AtomicBool::new(false))
            )
            .await
            .is_err()
    );
    assert!(!output.exists());
    Ok(())
}

#[tokio::test]
async fn actual_package_checksum_and_imported_context_must_both_match()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("input", "Controlled original observation")
        .await?;
    let manifest =
        EnginePackageManifest::parse(&std::fs::read(fixture.package.join("manifest.json"))?)?;
    let entry = manifest.entries().first().ok_or("missing package entry")?;
    let executable = fixture.package.join(entry.executable());
    let changed = b"different authenticated hermetic package bytes";
    std::fs::write(&executable, changed)?;
    let output = fixture.output("wrong-checksum")?;
    assert!(matches!(
        fixture
            .run(
                fixture.config(output.clone(), vec![input.clone()])?,
                Arc::new(AtomicBool::new(false))
            )
            .await,
        Err(ToolsEngineCoverageCommandError::Package(_))
    ));
    assert!(!output.exists());
    let replacement = EnginePackageEntry::portable(
        entry.target(),
        u64::try_from(changed.len())?,
        PackageDigest::read(&changed[..])?,
        "different controlled fixture compiler".into(),
    )?;
    std::fs::write(
        fixture.package.join("manifest.json"),
        serde_json::to_vec(&EnginePackageManifest::new(replacement)?)?,
    )?;
    let output = fixture.output("wrong-import-context")?;
    assert!(matches!(
        fixture
            .run(
                fixture.config(output.clone(), vec![input])?,
                Arc::new(AtomicBool::new(false))
            )
            .await,
        Err(ToolsEngineCoverageCommandError::Coverage(_))
    ));
    assert!(!output.exists());
    Ok(())
}

#[tokio::test]
async fn oversized_artifacts_and_report_sets_fail_at_bounded_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("input", "Controlled original observation")
        .await?;
    let artifact = input
        .parent()
        .ok_or("missing evidence parent")?
        .join("case/output.json");
    std::fs::File::create(&artifact)?
        .set_len(ToolsEngineCoverageCommandIo::MAXIMUM_ARTIFACT_BYTES + 1)?;
    let output = fixture.output("oversized")?;
    assert!(matches!(
        fixture
            .run(
                fixture.config(output.clone(), vec![input.clone()])?,
                Arc::new(AtomicBool::new(false))
            )
            .await,
        Err(ToolsEngineCoverageCommandError::Bounds { .. })
    ));
    assert!(!output.exists());
    assert!(
        fixture
            .config(
                output,
                vec![input; ToolsEngineCoverageCommandConfig::MAXIMUM_REPORTS + 1]
            )
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn existing_reports_and_artifact_bundles_are_never_overwritten()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let output = fixture.output("existing")?;
    std::fs::write(&output, b"existing owner report")?;
    assert!(matches!(
        fixture
            .run(
                fixture.config(output.clone(), Vec::new())?,
                Arc::new(AtomicBool::new(false))
            )
            .await,
        Err(ToolsEngineCoverageCommandError::ExistingDestination(_))
    ));
    assert_eq!(std::fs::read(&output)?, b"existing owner report");
    let input = fixture.evidence("input", "Controlled observation").await?;
    let output = fixture.output("existing-bundle")?;
    let bundle = output.with_file_name("coverage.json.artifacts");
    std::fs::create_dir(&bundle)?;
    std::fs::write(bundle.join("owner-data"), b"preserved")?;
    assert!(
        fixture
            .run(
                fixture.config(output.clone(), vec![input])?,
                Arc::new(AtomicBool::new(false))
            )
            .await
            .is_err()
    );
    assert!(!output.exists());
    assert_eq!(std::fs::read(bundle.join("owner-data"))?, b"preserved");
    Ok(())
}

#[tokio::test]
async fn one_worker_cancelled_commands_publish_nothing_and_return_the_shared_capacity()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let output = fixture.output("cancelled")?;
    let cancellation = Arc::new(AtomicBool::new(true));
    assert!(matches!(
        fixture
            .run(fixture.config(output.clone(), Vec::new())?, cancellation)
            .await,
        Err(ToolsEngineCoverageCommandError::Cancelled)
    ));
    assert!(!output.exists());
    assert_eq!(
        std::fs::read_dir(output.parent().ok_or("missing output parent")?)?.count(),
        0
    );
    Ok(())
}

#[tokio::test]
async fn publication_races_and_stage_failure_preserve_existing_data_and_clean_owned_artifacts()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("input", "Controlled original observation")
        .await?;
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid fixture budget")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let output = fixture.output("race")?;
    let config = fixture.config(output.clone(), vec![input.clone()])?;
    let cancellation = Arc::new(AtomicBool::new(false));
    let token = Arc::clone(&cancellation);
    let prepared = executor
        .submit(move |worker| {
            ToolsEngineCoverageCommandPrepared::prepare(
                config,
                token,
                ToolsEngineCoverageCommandTestFixture::host()?,
                worker,
            )
        })?
        .join()
        .await??;
    std::fs::write(&output, b"concurrent owner report")?;
    let token = Arc::clone(&cancellation);
    assert!(
        executor
            .submit(move |worker| prepared.publish(b"unused unpublished bytes", token, worker))?
            .join()
            .await?
            .is_err()
    );
    assert_eq!(std::fs::read(&output)?, b"concurrent owner report");
    let output = fixture.output("stage-failure")?;
    let config = fixture.config(output.clone(), vec![input])?;
    let token = Arc::clone(&cancellation);
    let prepared = executor
        .submit(move |worker| {
            ToolsEngineCoverageCommandPrepared::prepare(
                config,
                token,
                ToolsEngineCoverageCommandTestFixture::host()?,
                worker,
            )
        })?
        .join()
        .await??;
    let staged = prepared
        .artifacts
        .staged
        .values()
        .next()
        .ok_or("missing staged fixture")?;
    std::fs::remove_file(staged)?;
    assert!(
        executor
            .submit(move |worker| prepared.publish(
                b"unused unpublished bytes",
                cancellation,
                worker
            ))?
            .join()
            .await?
            .is_err()
    );
    assert!(!output.exists());
    assert!(!output.with_file_name("coverage.json.artifacts").exists());
    assert_eq!(budget.available(), budget.workers());
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn cancellation_after_preparation_prevents_publication()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("input", "Controlled original observation")
        .await?;
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid fixture budget")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let output = fixture.output("cancel-before-commit")?;
    let config = fixture.config(output.clone(), vec![input])?;
    let cancellation = Arc::new(AtomicBool::new(false));
    let token = Arc::clone(&cancellation);
    let prepared = executor
        .submit(move |worker| {
            ToolsEngineCoverageCommandPrepared::prepare(
                config,
                token,
                ToolsEngineCoverageCommandTestFixture::host()?,
                worker,
            )
        })?
        .join()
        .await??;
    cancellation.store(true, Ordering::SeqCst);
    assert!(matches!(
        executor
            .submit(move |worker| prepared.publish(
                b"unused unpublished bytes",
                cancellation,
                worker
            ))?
            .join()
            .await?,
        Err(ToolsEngineCoverageCommandError::Cancelled)
    ));
    assert!(!output.exists());
    assert!(!output.with_file_name("coverage.json.artifacts").exists());
    executor.shutdown().await?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_artifacts_cannot_escape_an_explicitly_selected_report_location()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("input", "Controlled original observation")
        .await?;
    let artifact = input
        .parent()
        .ok_or("missing evidence parent")?
        .join("case/output.json");
    std::fs::remove_file(&artifact)?;
    let outside = fixture.directory.path().join("outside");
    std::fs::write(&outside, b"preserved outside bytes")?;
    std::os::unix::fs::symlink(&outside, artifact)?;
    let output = fixture.output("symlink")?;
    assert!(matches!(
        fixture
            .run(
                fixture.config(output.clone(), vec![input])?,
                Arc::new(AtomicBool::new(false))
            )
            .await,
        Err(ToolsEngineCoverageCommandError::UnsafePath(_))
    ));
    assert!(!output.exists());
    assert_eq!(std::fs::read(outside)?, b"preserved outside bytes");
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn intermediate_directory_replacement_after_path_check_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let selected = fixture.directory.path().join("selected");
    let intermediate = selected.join("intermediate");
    let outside = fixture.directory.path().join("outside");
    std::fs::create_dir_all(&intermediate)?;
    std::fs::create_dir_all(&outside)?;
    std::fs::write(intermediate.join("artifact.json"), b"selected bytes")?;
    std::fs::write(
        outside.join("artifact.json"),
        b"outside bytes must not be accepted",
    )?;
    let selected = selected.canonicalize()?;
    let checked = ToolsEngineCoverageCommandIo::regular_below(
        &selected,
        Path::new("intermediate/artifact.json"),
    )?;
    std::fs::rename(&intermediate, selected.join("retained-intermediate"))?;
    std::os::unix::fs::symlink(&outside, &intermediate)?;
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid fixture budget")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let cancellation = Arc::new(AtomicBool::new(false));
    let result = executor
        .submit(move |worker| {
            ToolsEngineCoverageCommandIo::new(cancellation).read(
                &checked,
                ToolsEngineCoverageCommandIo::MAXIMUM_ARTIFACT_BYTES,
                worker,
            )
        })?
        .join()
        .await?;
    executor.shutdown().await?;
    assert!(
        result.is_err(),
        "replacing a checked ancestor must not permit outside evidence bytes"
    );
    assert_eq!(
        std::fs::read(outside.join("artifact.json"))?,
        b"outside bytes must not be accepted"
    );
    Ok(())
}

#[cfg(unix)]
struct ToolsEngineCoverageCommandTestReplacingWriter {
    intermediate: PathBuf,
    retained: PathBuf,
    outside: PathBuf,
}

#[cfg(unix)]
impl ToolsEngineCoverageCommandTestReplacingWriter {
    const FAILURE: &str = "controlled copy output failure";
}

#[cfg(unix)]
impl std::io::Write for ToolsEngineCoverageCommandTestReplacingWriter {
    fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
        std::fs::rename(&self.intermediate, &self.retained)?;
        std::os::unix::fs::symlink(&self.outside, &self.intermediate)?;
        Err(std::io::Error::other(Self::FAILURE))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(unix)]
#[tokio::test]
async fn failed_copy_revalidates_ancestors_and_preserves_the_operation_error()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let selected = fixture.directory.path().join("selected-copy");
    let intermediate = selected.join("intermediate");
    let outside = fixture.directory.path().join("outside-copy");
    std::fs::create_dir_all(&intermediate)?;
    std::fs::create_dir_all(&outside)?;
    std::fs::write(intermediate.join("artifact.json"), b"selected copy bytes")?;
    std::fs::write(outside.join("artifact.json"), b"outside copy bytes")?;
    let selected = selected.canonicalize()?;
    let checked = ToolsEngineCoverageCommandIo::regular_below(
        &selected,
        Path::new("intermediate/artifact.json"),
    )?;
    let mut writer = ToolsEngineCoverageCommandTestReplacingWriter {
        intermediate,
        retained: selected.join("retained-intermediate"),
        outside: outside.clone(),
    };
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid fixture budget")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let cancellation = Arc::new(AtomicBool::new(false));
    let result = executor
        .submit(move |worker| {
            ToolsEngineCoverageCommandIo::new(cancellation).copy(
                &checked,
                ToolsEngineCoverageCommandIo::MAXIMUM_ARTIFACT_BYTES,
                &mut writer,
                worker,
            )
        })?
        .join()
        .await?;
    executor.shutdown().await?;
    let Err(ToolsEngineCoverageCommandError::PathRevalidation {
        operation,
        validation,
    }) = result
    else {
        return Err("copy failure must retain its ancestor revalidation failure".into());
    };
    let ToolsEngineCoverageCommandError::Io { source, .. } = *operation else {
        return Err("the original output failure was lost".into());
    };
    assert_eq!(
        source.to_string(),
        ToolsEngineCoverageCommandTestReplacingWriter::FAILURE
    );
    assert!(matches!(
        *validation,
        ToolsEngineCoverageCommandError::UnsafePath(_)
    ));
    assert_eq!(
        std::fs::read(outside.join("artifact.json"))?,
        b"outside copy bytes"
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn publication_rejects_a_kind_directory_symlink_inside_its_unchanged_bundle()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("publication-input", "Controlled publication observation")
        .await?;
    let output = fixture.output("publication-symlink")?;
    let config = fixture.config(output.clone(), vec![input])?;
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid fixture budget")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let cancellation = Arc::new(AtomicBool::new(false));
    let prepared = executor
        .submit(move |worker| {
            ToolsEngineCoverageCommandPrepared::prepare(
                config,
                cancellation,
                ToolsEngineCoverageCommandTestFixture::host()?,
                worker,
            )
        })?
        .join()
        .await??;
    executor.shutdown().await?;
    let kind = prepared
        .artifacts
        .staged
        .keys()
        .next()
        .and_then(|relative| Path::new(relative).parent())
        .ok_or("missing canonical artifact kind directory")?;
    let injected = prepared.destination.parent.join(kind);
    let mut publication = ToolsEngineCoverageCommandPublication::new(prepared);
    publication.create_bundle()?;
    assert!(publication.validate_bundle().is_ok());
    let outside = fixture.directory.path().join("outside-publication");
    std::fs::create_dir(&outside)?;
    let sentinel = outside.join("owner-data");
    std::fs::write(&sentinel, b"outside owner bytes")?;
    std::os::unix::fs::symlink(&outside, &injected)?;
    let result = publication.validate_bundle();
    std::fs::remove_file(injected)?;
    publication.cleanup()?;
    publication.cleanup_staging()?;
    assert_eq!(std::fs::read(sentinel)?, b"outside owner bytes");
    assert_eq!(std::fs::read_dir(&outside)?.count(), 1);
    assert!(!output.exists());
    assert!(
        result.is_err(),
        "a kind-directory symlink must be rejected even when the bundle root identity is unchanged"
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn retained_kind_descriptor_cannot_link_artifacts_into_a_replacement_symlink()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("kind-link-input", "Controlled kind-link observation")
        .await?;
    let output = fixture.output("kind-link-result")?;
    let mut publication = fixture.publication(output.clone(), vec![input]).await?;
    publication.create_bundle()?;
    let relative = publication
        .prepared
        .artifacts
        .staged
        .keys()
        .next()
        .ok_or("missing staged artifact")?
        .clone();
    let parts = publication.parts(&relative)?;
    publication.ensure_kind(&parts.kind)?;
    let kind = publication
        .kinds
        .get(parts.kind.as_os_str())
        .ok_or("missing retained kind")?
        .duplicate()?;
    let source =
        crate::engine::coverage_command::ToolsEngineCoverageCommandIoPath::capture_relative(
            vec![publication.prepared.artifacts.identity.duplicate()?],
            Path::new(&relative),
        )?;
    let retained = publication
        .prepared
        .destination
        .bundle_path()
        .join("retained-kind");
    let outside = fixture.directory.path().join("outside-kind-link");
    std::fs::create_dir(&outside)?;
    std::fs::write(outside.join("owner-data"), b"outside kind owner bytes")?;
    std::fs::rename(&kind.path, &retained)?;
    std::os::unix::fs::symlink(&outside, &kind.path)?;
    ToolsEngineCoverageCommandPublication::link_bound(&source, &kind, &parts.name)?;
    assert!(retained.join(&parts.name).is_file());
    assert!(!outside.join(&parts.name).exists());
    assert!(publication.validate_bundle().is_err());
    assert!(publication.cleanup().is_err());
    assert!(!output.exists());
    assert_eq!(
        std::fs::read(outside.join("owner-data"))?,
        b"outside kind owner bytes"
    );
    assert_eq!(std::fs::read_dir(&outside)?.count(), 1);
    std::fs::remove_file(&kind.path)?;
    publication.cleanup_staging()?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn retained_parent_descriptor_keeps_report_links_and_cleanup_out_of_replacement_paths()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let output = fixture.output("report-link-result")?;
    let mut publication = fixture.publication(output.clone(), Vec::new()).await?;
    let bytes = b"complete private report fixture";
    let name = ToolsEngineCoverageCommandPublication::REPORT_STAGE_NAME;
    std::fs::write(
        publication.prepared.artifacts.directory.path().join(name),
        bytes,
    )?;
    let source =
        crate::engine::coverage_command::ToolsEngineCoverageCommandIoPath::capture_relative(
            vec![publication.prepared.artifacts.identity.duplicate()?],
            Path::new(name),
        )?;
    let identity = same_file::Handle::from_file(source.open()?)?;
    let parent = publication
        .prepared
        .destination
        .parent_identity
        .duplicate()?;
    let report_name = output.file_name().ok_or("missing report filename")?;
    let retained = fixture.directory.path().join("retained-report-parent");
    let outside = fixture.directory.path().join("outside-report-parent");
    std::fs::create_dir(&outside)?;
    std::fs::write(outside.join("owner-data"), b"outside report owner bytes")?;
    std::fs::rename(&parent.path, &retained)?;
    std::os::unix::fs::symlink(&outside, &parent.path)?;
    ToolsEngineCoverageCommandPublication::link_bound(&source, &parent, report_name)?;
    publication.report_identity = Some(identity);
    publication.report_digest = Some(PackageDigest::read(&bytes[..])?);
    assert_eq!(std::fs::read(retained.join(report_name))?, bytes);
    assert!(!outside.join(report_name).exists());
    assert!(publication.verify_report().is_err());
    publication.cleanup()?;
    publication.cleanup_staging()?;
    assert_eq!(
        std::fs::read(outside.join("owner-data"))?,
        b"outside report owner bytes"
    );
    assert_eq!(std::fs::read_dir(&outside)?.count(), 1);
    assert_eq!(std::fs::read_dir(&retained)?.count(), 0);
    std::fs::remove_file(&parent.path)?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn parent_replacement_before_commit_preserves_outside_staging_lookalikes()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("parent-change-input", "Controlled parent replacement")
        .await?;
    let output = fixture.output("parent-change-result")?;
    let publication = fixture.publication(output.clone(), vec![input]).await?;
    let prepared = publication.prepared;
    let stage_name = prepared
        .artifacts
        .directory
        .path()
        .file_name()
        .ok_or("missing private staging name")?
        .to_owned();
    let parent = output.parent().ok_or("missing selected report parent")?;
    let retained = fixture.directory.path().join("retained-before-commit");
    let outside = fixture.directory.path().join("outside-before-commit");
    std::fs::create_dir(&outside)?;
    let lookalike = outside.join(stage_name);
    std::fs::create_dir(&lookalike)?;
    std::fs::write(
        lookalike.join("owner-data"),
        b"preserved outside staging lookalike",
    )?;
    std::fs::rename(parent, &retained)?;
    std::os::unix::fs::symlink(&outside, parent)?;
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid fixture budget")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let cancellation = Arc::new(AtomicBool::new(false));
    let result = executor
        .submit(move |worker| prepared.publish(b"unpublished report bytes", cancellation, worker))?
        .join()
        .await?;
    executor.shutdown().await?;
    assert!(result.is_err());
    assert!(!output.exists());
    assert_eq!(
        std::fs::read(lookalike.join("owner-data"))?,
        b"preserved outside staging lookalike"
    );
    assert_eq!(std::fs::read_dir(&lookalike)?.count(), 1);
    assert_eq!(std::fs::read_dir(&retained)?.count(), 0);
    std::fs::remove_file(parent)?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn dropping_prepublication_staging_preserves_outside_same_name_lookalikes()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let output = fixture.output("stage-drop-result")?;
    let prepared = fixture.prepared(output.clone(), Vec::new()).await?;
    let stage = prepared.artifacts.directory.path().to_owned();
    let stage_name = stage.file_name().ok_or("missing staging directory name")?;
    let parent = output.parent().ok_or("missing selected parent")?;
    let retained = fixture.directory.path().join("retained-stage-drop-parent");
    let outside = fixture.directory.path().join("outside-stage-drop-parent");
    let lookalike = outside.join(stage_name);
    std::fs::create_dir_all(&lookalike)?;
    let sentinel = lookalike.join("owner-data");
    std::fs::write(&sentinel, b"outside prepublication owner bytes")?;
    std::fs::rename(parent, &retained)?;
    std::os::unix::fs::symlink(&outside, parent)?;
    drop(prepared);
    let preserved = std::fs::read(&sentinel);
    let original_cleaned = !retained.join(stage_name).exists();
    std::fs::remove_file(parent)?;
    std::fs::rename(&retained, parent)?;
    match stage.symlink_metadata() {
        Ok(_metadata) => std::fs::remove_dir_all(&stage)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    assert!(
        preserved.is_ok(),
        "dropping staging before publication must not delete an outside lookalike"
    );
    assert_eq!(preserved?, b"outside prepublication owner bytes");
    assert!(
        original_cleaned,
        "fallback cleanup must remove the original owned staging directory"
    );
    assert!(!output.exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn prepublication_import_rejects_parent_replacement_without_outside_staging_writes()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("stage-import-input", "Controlled staging import")
        .await?
        .canonicalize()?;
    let output = fixture.output("stage-import-result")?;
    let mut prepared = fixture.prepared(output.clone(), Vec::new()).await?;
    let report = ToolsEngineCoverageReport::parse(
        &std::fs::read(&input)?,
        &prepared.inventory,
        &prepared.context,
    )?;
    let artifact = report
        .observations()
        .iter()
        .find_map(|observation| match observation.outcome() {
            ToolsEngineCoverageOutcome::Measured { artifacts, .. } => artifacts.first().cloned(),
            ToolsEngineCoverageOutcome::Outstanding { .. }
            | ToolsEngineCoverageOutcome::UpstreamUnavailable { .. } => None,
        })
        .ok_or("missing actual fixture artifact")?;
    let selected = ToolsEngineCoverageCommandIo::regular_below(
        input.parent().ok_or("missing selected evidence parent")?,
        Path::new(input.file_name().ok_or("missing selected report name")?),
    )?;
    let stage_name = prepared
        .artifacts
        .directory
        .path()
        .file_name()
        .ok_or("missing staging directory name")?
        .to_owned();
    let relative = prepared.destination.artifact_path(&artifact);
    let parent = output.parent().ok_or("missing selected output parent")?;
    let retained = fixture
        .directory
        .path()
        .join("retained-stage-import-parent");
    let outside = fixture.directory.path().join("outside-stage-import-parent");
    let lookalike = outside.join(stage_name);
    let outside_artifact = lookalike.join(relative);
    std::fs::create_dir_all(&lookalike)?;
    let sentinel = lookalike.join("owner-data");
    std::fs::write(&sentinel, b"outside staging import owner bytes")?;
    std::fs::rename(parent, &retained)?;
    std::os::unix::fs::symlink(&outside, parent)?;
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid fixture budget")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let cancellation = Arc::new(AtomicBool::new(false));
    let (result, prepared) = executor
        .submit(move |worker| {
            let mut io = ToolsEngineCoverageCommandIo::new(cancellation);
            let result = prepared.artifacts.import(
                crate::engine::coverage_command::ToolsEngineCoverageCommandArtifactsImport {
                    artifact: &artifact,
                    report: &selected,
                    destination: &prepared.destination,
                },
                &mut io,
                worker,
            );
            (result, prepared)
        })?
        .join()
        .await?;
    executor.shutdown().await?;
    let wrote_outside = outside_artifact.exists();
    let preserved = std::fs::read(&sentinel);
    std::fs::remove_file(parent)?;
    std::fs::rename(&retained, parent)?;
    drop(prepared);
    assert!(
        !wrote_outside,
        "artifact staging must not write into a replacement-parent lookalike"
    );
    assert!(
        result.is_err(),
        "staging import must reject the changed selected parent"
    );
    assert_eq!(preserved?, b"outside staging import owner bytes");
    assert!(!output.exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn runner_cancellation_finalizes_prepublication_staging_on_one_worker()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("runner-cancel-input", "Controlled cancellation evidence")
        .await?;
    let output = fixture.output("runner-cancel-result")?;
    let prepared = fixture.prepared(output.clone(), vec![input]).await?;
    let stage = prepared.artifacts.directory.path().to_owned();
    let cancellation = Arc::clone(&prepared.provider.cancellation);
    cancellation.store(true, Ordering::SeqCst);
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("invalid fixture budget")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let runtime = tokio::runtime::Handle::current();
    let result = executor
        .submit(move |worker| prepared.complete(&runtime, cancellation, worker))?
        .join()
        .await?;
    executor.shutdown().await?;
    assert!(matches!(
        result,
        Err(ToolsEngineCoverageCommandError::Coverage(
            ToolsEngineCoverageError::Workflow(_)
        ))
    ));
    assert!(!stage.exists());
    assert!(!output.exists());
    assert_eq!(
        std::fs::read_dir(output.parent().ok_or("missing output parent")?)?.count(),
        0
    );
    assert_eq!(budget.available(), budget.workers());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn early_failure_preserves_cleanup_cause_and_cleans_original_stage_without_following_replacement()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ToolsEngineCoverageCommandTestFixture::new()?;
    let input = fixture
        .evidence("cleanup-cause-input", "Controlled cleanup evidence")
        .await?;
    let output = fixture.output("cleanup-cause-result")?;
    let mut prepared = fixture.prepared(output.clone(), vec![input]).await?;
    let stage = prepared.artifacts.directory.path().to_owned();
    let retained = fixture.directory.path().join("retained-original-stage");
    let outside = fixture.directory.path().join("outside-cleanup-cause");
    std::fs::create_dir(&outside)?;
    let sentinel = outside.join("owner-data");
    std::fs::write(&sentinel, b"outside cleanup cause owner bytes")?;
    std::fs::rename(&stage, &retained)?;
    std::os::unix::fs::symlink(&outside, &stage)?;
    let result: Result<(), ToolsEngineCoverageCommandError> =
        prepared.fail(ToolsEngineCoverageCommandError::Cancelled);
    let remaining = std::fs::read_dir(&retained)?.count();
    let preserved = std::fs::read(&sentinel)?;
    std::fs::remove_file(&stage)?;
    std::fs::rename(&retained, &stage)?;
    prepared.artifacts.directory.cleanup()?;
    assert!(
        matches!(result, Err(ToolsEngineCoverageCommandError::Cleanup { operation, cleanup })
        if matches!(*operation, ToolsEngineCoverageCommandError::Cancelled)
            && matches!(*cleanup, ToolsEngineCoverageCommandError::Io { .. } | ToolsEngineCoverageCommandError::UnsafePath(_)))
    );
    assert_eq!(remaining, 0);
    assert_eq!(preserved, b"outside cleanup cause owner bytes");
    assert_eq!(std::fs::read_dir(&outside)?.count(), 1);
    assert!(!stage.exists());
    assert!(!output.exists());
    Ok(())
}
