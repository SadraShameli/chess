use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use thiserror::Error;

#[cfg(feature = "asset-tools")]
mod assets;
mod data;
mod engine;
mod packaging;

#[derive(Debug, Error)]
enum ToolsCommandError {
    #[error(
        "Usage: chess-tools data-verify <sources.json> <cache-directory> | engine-build [--profile portable|x86-64-v3|x86-64-v4|numa] | engine-check | package-plan|package-run <windows|macos|linux> | package-verify <windows|macos|linux> <directory> | notices-check | engine-pgo <source> <model> <output> <llvm-profdata> [--counts-only] | engine-spsa-check <source> <model> <output> | engine-queued-check <output> | engine-coverage <package-directory> <new-output-report> [evidence-report...] | assets-generate <directory> [--check] | sounds-generate <directory> [--check] (requires asset-tools)"
    )]
    Usage,
    #[error(transparent)]
    Coverage(#[from] engine::ToolsEngineCoverageCommandError),
    #[error(transparent)]
    Packaging(#[from] packaging::ToolsPackagingError),
    #[error("The operation failed ({operation}); cleanup also failed ({cleanup})")]
    Execution {
        #[source]
        operation: Box<dyn std::error::Error>,
        cleanup: Box<dyn std::error::Error>,
    },
}

type ToolsCommandResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct ToolsCommandCancellation {
    developer: engine::ToolsEnginePgoCancellation,
    coverage: Arc<AtomicBool>,
}

impl ToolsCommandCancellation {
    fn new() -> Self {
        Self {
            developer: engine::ToolsEnginePgoCancellation::new(),
            coverage: Arc::new(AtomicBool::new(false)),
        }
    }

    fn cancel(&self) {
        self.developer.cancel();
        self.coverage.store(true, Ordering::Release);
    }
}

enum ToolsEnumEngineWorkflow {
    Pgo {
        inputs: engine::ToolsEnginePgoInputs,
        profdata: PathBuf,
    },
    Spsa(engine::ToolsEnginePgoInputs),
    Queued(PathBuf),
    Coverage(engine::ToolsEngineCoverageCommandConfig),
}

impl ToolsEnumEngineWorkflow {
    fn execute(self) -> ToolsCommandResult {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let budget = CapacityBudget::discover()?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let cancellation = ToolsCommandCancellation::new();
        let handle = runtime.handle().clone();
        runtime.block_on(async {
            let outcome = {
                let operation = self.run(&budget, &executor, &cancellation, handle);
                tokio::pin!(operation);
                tokio::select! {
                    result = &mut operation => result,
                    signal = tokio::signal::ctrl_c() => {
                        cancellation.cancel();
                        let outcome = operation.await;
                        Self::complete(outcome, signal.map_err(Into::into))
                    }
                }
            };
            let cleanup = executor.shutdown().await.map_err(Into::into);
            let message = Self::complete(outcome, cleanup)?;
            println!("{message}");
            Ok(())
        })
    }

    async fn run(
        self,
        budget: &CapacityBudget,
        executor: &CapacityExecutor,
        cancellation: &ToolsCommandCancellation,
        handle: tokio::runtime::Handle,
    ) -> ToolsCommandResult<String> {
        let resources =
            engine::ToolsEnginePgoResources::new(budget, executor, &cancellation.developer);
        let result = match self {
            Self::Pgo { inputs, profdata } => {
                engine::ToolsEnginePgo::run(inputs, profdata, resources).await
            }
            Self::Spsa(inputs) => engine::ToolsEngineSpsaCheck::run(inputs, resources).await,
            Self::Queued(output) => {
                engine::ToolsEnginePgo::check_queued(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                    output,
                    resources,
                )
                .await
            }
            Self::Coverage(config) => {
                return engine::ToolsEngineCoverageCommand::run(
                    config,
                    engine::ToolsEngineCoverageCommandResources::new(
                        executor,
                        Arc::clone(&cancellation.coverage),
                        handle,
                    ),
                )
                .await
                .map(|summary| summary.to_string())
                .map_err(Into::into);
            }
        };
        result
            .map(|path| format!("Report: {}", path.display()))
            .map_err(Into::into)
    }

    fn complete<T>(
        outcome: ToolsCommandResult<T>,
        cleanup: ToolsCommandResult,
    ) -> ToolsCommandResult<T> {
        match (outcome, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
            (Err(operation), Err(cleanup)) => {
                Err(ToolsCommandError::Execution { operation, cleanup }.into())
            }
        }
    }
}

enum ToolsEnumCommand {
    VerifyData { sources: PathBuf, cache: PathBuf },
    BuildEngine(chess::engine::package::manifest::EnginePackageEnumProfile),
    Packaging(packaging::ToolsPackagingEnumCommand),
    CheckEngine,
    RunEngineWorkflow(ToolsEnumEngineWorkflow),
    #[cfg(feature = "asset-tools")]
    GenerateAssets {
        destination: std::path::PathBuf,
        check: bool,
    },
    #[cfg(feature = "asset-tools")]
    GenerateSounds {
        destination: std::path::PathBuf,
        check: bool,
    },
}

impl ToolsEnumCommand {
    fn parse() -> Result<Self, ToolsCommandError> {
        let supplied: Vec<OsString> = std::env::args_os().skip(1).collect();
        match supplied.as_slice() {
            [command, sources, cache] if command == "data-verify" => {
                return Ok(Self::VerifyData {
                    sources: sources.into(),
                    cache: cache.into(),
                });
            }
            [command, package, output, reports @ ..] if command == "engine-coverage" => {
                return Ok(Self::RunEngineWorkflow(ToolsEnumEngineWorkflow::Coverage(
                    engine::ToolsEngineCoverageCommandConfig::new(
                        package.into(),
                        output.into(),
                        reports.iter().map(PathBuf::from).collect(),
                    )?,
                )));
            }
            [command, source, model, output, profdata, options @ ..] if command == "engine-pgo" => {
                let instrumentation = match options {
                    [] => engine::ToolsEnginePgoEnumInstrumentation::Full,
                    [option] if option == "--counts-only" => {
                        engine::ToolsEnginePgoEnumInstrumentation::CountsOnly
                    }
                    _ => return Err(ToolsCommandError::Usage),
                };
                return Ok(Self::RunEngineWorkflow(ToolsEnumEngineWorkflow::Pgo {
                    inputs: engine::ToolsEnginePgoInputs::new(
                        source.into(),
                        model.into(),
                        output.into(),
                    )
                    .with_instrumentation(instrumentation),
                    profdata: profdata.into(),
                }));
            }
            [command, source, model, output] if command == "engine-spsa-check" => {
                return Ok(Self::RunEngineWorkflow(ToolsEnumEngineWorkflow::Spsa(
                    engine::ToolsEnginePgoInputs::new(source.into(), model.into(), output.into()),
                )));
            }
            [command, flag, name] if command == "engine-build" && flag == "--profile" => {
                let profile = chess::engine::package::manifest::EnginePackageEnumProfile::ALL
                    .into_iter()
                    .find(|profile| {
                        profile.file_suffix().unwrap_or("portable") == name.to_str().unwrap_or("")
                    })
                    .ok_or(ToolsCommandError::Usage)?;
                return Ok(Self::BuildEngine(profile));
            }
            [command, output] if command == "engine-queued-check" => {
                return Ok(Self::RunEngineWorkflow(ToolsEnumEngineWorkflow::Queued(
                    output.into(),
                )));
            }
            _ => {}
        }
        if let Some(packaging) = packaging::ToolsPackagingEnumCommand::parse(&supplied) {
            return Ok(Self::Packaging(packaging?));
        }
        #[cfg(feature = "asset-tools")]
        {
            if supplied
                .first()
                .is_some_and(|command| command == "sounds-generate")
            {
                return match supplied.as_slice() {
                    [_, destination] => Ok(Self::GenerateSounds {
                        destination: destination.into(),
                        check: false,
                    }),
                    [_, destination, check] if check == "--check" => Ok(Self::GenerateSounds {
                        destination: destination.into(),
                        check: true,
                    }),
                    _ => Err(ToolsCommandError::Usage),
                };
            }
            if supplied
                .first()
                .is_some_and(|command| command == "assets-generate")
            {
                return match supplied.as_slice() {
                    [_, destination] => Ok(Self::GenerateAssets {
                        destination: destination.into(),
                        check: false,
                    }),
                    [_, destination, check] if check == "--check" => Ok(Self::GenerateAssets {
                        destination: destination.into(),
                        check: true,
                    }),
                    _ => Err(ToolsCommandError::Usage),
                };
            }
        }
        let mut args = supplied.into_iter();
        match (args.next().as_deref(), args.next()) {
            (Some(command), None) if command == OsStr::new("engine-build") => {
                Ok(Self::BuildEngine(chess::engine::package::manifest::EnginePackageEnumProfile::Portable))
            }
            (Some(command), None) if command == OsStr::new("engine-check") => Ok(Self::CheckEngine),
            _ => Err(ToolsCommandError::Usage),
        }
    }

    fn execute(self) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::VerifyData { sources, cache } => {
                let verified = data::ToolsDataAcquire::verify(&sources, &cache)?;
                println!("Verified {verified} frozen dataset sources");
            }
            Self::RunEngineWorkflow(workflow) => workflow.execute()?,
            Self::BuildEngine(profile) => {
                let temporary = tempfile::tempdir()?;
                let budget = chess::capacity::budget::CapacityBudget::discover()?;
                let build = engine::ToolsEngineBuild::new(
                    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                    temporary.path().join("Reckless"),
                    budget,
                )?
                .with_profile(profile)?;
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(build.run())?;
                temporary.close()?;
            }
            Self::Packaging(command) => {
                let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                println!("{}", command.execute(&root)?);
            }
            Self::CheckEngine => {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(engine::ToolsEngineCheck::run())?;
            }
            #[cfg(feature = "asset-tools")]
            Self::GenerateSounds { destination, check } => {
                assets::ToolsAssetsSounds::run(&destination, check)?;
            }
            #[cfg(feature = "asset-tools")]
            Self::GenerateAssets { destination, check } => {
                let budget = chess::capacity::budget::CapacityBudget::discover()?;
                let executor = chess::capacity::executor::CapacityExecutor::new(budget.clone())?;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(async {
                    let generation =
                        assets::ToolsAssetsPieces::run(&destination, check, &executor, &budget)
                            .await;
                    let shutdown = executor.shutdown().await;
                    generation?;
                    shutdown?;
                    Ok::<(), Box<dyn std::error::Error>>(())
                })?;
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    ToolsEnumCommand::parse()?.execute()
}
