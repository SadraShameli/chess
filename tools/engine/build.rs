use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use chess::capacity::budget::{CapacityBudget, CapacityBudgetError};
use chess::capacity::executor::{CapacityExecutor, CapacityExecutorError};
use chess::engine::package::digest::{EnginePackageDigest, EnginePackageDigestError};
use chess::engine::package::manifest::{
    EnginePackageEntry, EnginePackageEnumTarget, EnginePackageManifest, EnginePackageManifestError,
};
use chess::storage::atomic_file::{
    StorageAtomicFileError, StorageAtomicFileStage, StorageAtomicFileTarget,
};
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

#[derive(Debug, Error)]
pub(crate) enum ToolsEngineBuildError {
    #[error(transparent)]
    Capacity(#[from] CapacityBudgetError),
    #[error(transparent)]
    Executor(#[from] CapacityExecutorError),
    #[error(transparent)]
    Manifest(#[from] EnginePackageManifestError),
    #[error(transparent)]
    Digest(#[from] EnginePackageDigestError),
    #[error(transparent)]
    File(#[from] StorageAtomicFileError),
    #[error("Engine build I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Engine build command {stage:?} failed with {status}")]
    Process {
        stage: ToolsEngineBuildEnumStage,
        status: ExitStatus,
    },
    #[error("Engine build command {0:?} exceeded its deadline")]
    Timeout(ToolsEngineBuildEnumStage),
    #[error("Engine build command output exceeded its bound or was not UTF-8")]
    Output,
    #[error("The checked out engine source differs from the approved commit")]
    SourceMismatch,
    #[error("The network size or SHA-256 differs from the approved model")]
    NetworkMismatch,
    #[error("Cannot encode the engine manifest: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use crate::engine::build::ToolsEngineBuild;
    use chess::engine::package::manifest::{EnginePackageEnumTarget, EnginePackageManifest};

    #[test]
    fn published_engine_bytes_match_the_verified_source_and_manifest()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let built = directory.path().join("built");
        std::fs::write(&built, b"verified engine fixture")?;
        let output = directory.path().join("package");
        let target = EnginePackageEnumTarget::current()?;
        ToolsEngineBuild::publish(&built, &output, target, "test compiler".to_owned())?;
        assert_eq!(
            std::fs::read(output.join(target.relative_path()))?,
            std::fs::read(built)?
        );
        let manifest = EnginePackageManifest::parse(&std::fs::read(output.join("manifest.json"))?)?;
        assert_eq!(manifest.entries().len(), 1);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ToolsEngineBuildEnumStage {
    Clone,
    Inspect,
    Download,
    Compile,
}

impl ToolsEngineBuildEnumStage {
    fn deadline(self) -> Duration {
        match self {
            Self::Clone => Duration::from_secs(120),
            Self::Inspect => Duration::from_secs(30),
            Self::Download | Self::Compile => Duration::from_secs(1800),
        }
    }
}

pub(crate) struct ToolsEngineBuild {
    root: PathBuf,
    checkout: PathBuf,
    target: EnginePackageEnumTarget,
    budget: CapacityBudget,
    executor: CapacityExecutor,
}

impl ToolsEngineBuild {
    pub(crate) fn new(
        root: PathBuf,
        checkout: PathBuf,
        budget: CapacityBudget,
    ) -> Result<Self, ToolsEngineBuildError> {
        let executor = CapacityExecutor::new(budget.clone())?;
        Ok(Self {
            root,
            checkout,
            target: EnginePackageEnumTarget::current()?,
            budget,
            executor,
        })
    }

    pub(crate) async fn run(self) -> Result<(), ToolsEngineBuildError> {
        let outcome = self.build().await;
        self.executor.shutdown().await?;
        outcome
    }

    async fn build(&self) -> Result<(), ToolsEngineBuildError> {
        let model = if self.budget.workers() > 1 {
            let (source, model) = tokio::join!(self.acquire_source(), self.acquire_network());
            source?;
            model?
        } else {
            self.acquire_source().await?;
            self.acquire_network().await?
        };
        let compiler = self
            .execute(
                Command::new("rustc").arg("--version"),
                ToolsEngineBuildEnumStage::Inspect,
                1,
            )
            .await?;
        let build_target = self.checkout.join("target");
        let mut command = Command::new("cargo");
        command
            .current_dir(&self.checkout)
            .args([
                "build",
                "--locked",
                "--release",
                "--features",
                "syzygy",
                "--target",
                self.target.triple(),
            ])
            .env("EVALFILE", model)
            .env("CARGO_TARGET_DIR", &build_target)
            .env_remove("RUSTFLAGS")
            .env("CARGO_ENCODED_RUSTFLAGS", self.target.portable_flags())
            .env("CFLAGS", "-O3");
        if self.target == EnginePackageEnumTarget::MacosArm {
            command.env("MACOSX_DEPLOYMENT_TARGET", "14.0");
        }
        self.execute(
            &mut command,
            ToolsEngineBuildEnumStage::Compile,
            self.budget.workers(),
        )
        .await?;
        let executable = if self.target == EnginePackageEnumTarget::WindowsX64 {
            "reckless.exe"
        } else {
            "reckless"
        };
        let built = build_target
            .join(self.target.triple())
            .join("release")
            .join(executable);
        let destination = self.root.join("resources/engine");
        let target = self.target;
        self.executor
            .submit(move |_cancellation| {
                Self::publish(&built, &destination, target, compiler.trim().to_owned())
            })?
            .join()
            .await??;
        Ok(())
    }

    async fn acquire_source(&self) -> Result<(), ToolsEngineBuildError> {
        let mut command = Command::new("git");
        command
            .args([
                "-c",
                "pack.threads=1",
                "-c",
                "index.threads=1",
                "clone",
                "--depth",
                "1",
                "--single-branch",
                "--branch",
                EnginePackageManifest::TAG,
                EnginePackageManifest::REPOSITORY,
            ])
            .arg(&self.checkout);
        self.execute(&mut command, ToolsEngineBuildEnumStage::Clone, 1)
            .await?;
        let commit = self
            .execute(
                Command::new("git")
                    .arg("-C")
                    .arg(&self.checkout)
                    .args(["rev-parse", "HEAD"]),
                ToolsEngineBuildEnumStage::Inspect,
                1,
            )
            .await?;
        if commit.trim() != EnginePackageManifest::COMMIT {
            return Err(ToolsEngineBuildError::SourceMismatch);
        }
        Ok(())
    }

    async fn acquire_network(&self) -> Result<PathBuf, ToolsEngineBuildError> {
        let directory = self.root.join("resources/.cache/networks");
        let staged = self
            .executor
            .submit(move |_cancellation| {
                std::fs::create_dir_all(&directory)?;
                let path = directory.join(EnginePackageManifest::NETWORK_NAME);
                if path.try_exists()? {
                    Self::verify_network(&path)?;
                    Ok::<_, ToolsEngineBuildError>((path, None))
                } else {
                    let temporary = tempfile::NamedTempFile::new_in(directory)?;
                    Ok((path, Some(temporary)))
                }
            })?
            .join()
            .await??;
        let (path, temporary) = staged;
        if let Some(temporary) = temporary {
            let mut command = Command::new("curl");
            command
                .args([
                    "--fail",
                    "--location",
                    "--silent",
                    "--show-error",
                    "--proto",
                    "=https",
                    "--connect-timeout",
                    "30",
                    "--max-time",
                    "1800",
                    "--max-filesize",
                ])
                .arg(EnginePackageManifest::NETWORK_BYTES.to_string())
                .arg("--output")
                .arg(temporary.path())
                .arg(EnginePackageManifest::network_url());
            self.execute(&mut command, ToolsEngineBuildEnumStage::Download, 1)
                .await?;
            let destination = path.clone();
            self.executor
                .submit(move |_cancellation| {
                    Self::verify_network(temporary.path())?;
                    temporary.as_file().sync_all()?;
                    temporary
                        .persist_noclobber(destination)
                        .map_err(|error| error.error)?;
                    Ok::<_, ToolsEngineBuildError>(())
                })?
                .join()
                .await??;
        }
        Ok(path)
    }

    fn verify_network(path: &Path) -> Result<(), ToolsEngineBuildError> {
        let file = std::fs::File::open(path)?;
        if file.metadata()?.len() != EnginePackageManifest::NETWORK_BYTES
            || EnginePackageDigest::read(file)?.to_string() != EnginePackageManifest::NETWORK_SHA256
        {
            return Err(ToolsEngineBuildError::NetworkMismatch);
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: &mut Command,
        stage: ToolsEngineBuildEnumStage,
        workers: usize,
    ) -> Result<String, ToolsEngineBuildError> {
        let _lease = self.budget.try_lease(workers)?;
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = child.stdout.take().ok_or(ToolsEngineBuildError::Output)?;
        let operation = async {
            const MAX_OUTPUT: u64 = 64 * 1024;
            let mut bytes = Vec::new();
            stdout.take(MAX_OUTPUT + 1).read_to_end(&mut bytes).await?;
            if u64::try_from(bytes.len()).map_err(|_error| ToolsEngineBuildError::Output)?
                > MAX_OUTPUT
            {
                return Err(ToolsEngineBuildError::Output);
            }
            let status = child.wait().await?;
            if !status.success() {
                return Err(ToolsEngineBuildError::Process { stage, status });
            }
            String::from_utf8(bytes).map_err(|_error| ToolsEngineBuildError::Output)
        };
        match tokio::time::timeout(stage.deadline(), operation).await {
            Ok(Ok(output)) => Ok(output),
            outcome => {
                if child.try_wait()?.is_none() {
                    child.kill().await?;
                }
                child.wait().await?;
                match outcome {
                    Ok(Err(error)) => Err(error),
                    Err(_elapsed) => Err(ToolsEngineBuildError::Timeout(stage)),
                    Ok(Ok(_output)) => Err(ToolsEngineBuildError::Output),
                }
            }
        }
    }

    fn publish(
        built: &Path,
        directory: &Path,
        target: EnginePackageEnumTarget,
        compiler: String,
    ) -> Result<(), ToolsEngineBuildError> {
        std::fs::create_dir_all(directory.join("bin"))?;
        let mut file = std::fs::File::open(built)?;
        let bytes = file.metadata()?.len();
        let digest = EnginePackageDigest::read(file.try_clone()?)?;
        file.rewind()?;
        let entry = EnginePackageEntry::portable(target, bytes, digest, compiler)?;
        let manifest_path = directory.join("manifest.json");
        let manifest = if manifest_path.try_exists()? {
            let mut manifest = EnginePackageManifest::parse(&std::fs::read(&manifest_path)?)?;
            manifest.replace(entry);
            manifest
        } else {
            EnginePackageManifest::new(entry)?
        };
        manifest.validate()?;
        let mut json = serde_json::to_vec_pretty(&manifest)?;
        json.push(b'\n');
        let mut stage = StorageAtomicFileStage::new(StorageAtomicFileTarget::capture(
            directory.join(target.relative_path()),
        )?)?;
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let count = Read::read(&mut file, &mut buffer)?;
            if count == 0 {
                break;
            }
            stage.write(&buffer[..count])?;
        }
        stage.commit()?;
        let path = directory.join(target.relative_path());
        let permissions = std::fs::metadata(built)?.permissions();
        std::fs::set_permissions(path, permissions)?;
        let mut stage =
            StorageAtomicFileStage::new(StorageAtomicFileTarget::capture(manifest_path)?)?;
        stage.write(&json)?;
        stage.commit()?;
        Ok(())
    }
}
