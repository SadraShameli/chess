use std::ffi::OsStr;
use thiserror::Error;

#[cfg(feature = "asset-tools")]
mod assets;
mod engine;

#[derive(Debug, Error)]
enum ToolsCommandError {
    #[error(
        "Usage: chess-tools engine-build | engine-check | assets-generate <directory> [--check] (requires asset-tools)"
    )]
    Usage,
}

enum ToolsEnumCommand {
    EngineBuild,
    EngineCheck,
    #[cfg(feature = "asset-tools")]
    AssetsGenerate {
        destination: std::path::PathBuf,
        check: bool,
    },
}

impl ToolsEnumCommand {
    fn parse() -> Result<Self, ToolsCommandError> {
        let mut args = std::env::args_os().skip(1);
        #[cfg(feature = "asset-tools")]
        {
            let supplied: Vec<_> = std::env::args_os().skip(1).collect();
            if supplied
                .first()
                .is_some_and(|command| command == "assets-generate")
            {
                return match supplied.as_slice() {
                    [_, destination] => Ok(Self::AssetsGenerate {
                        destination: destination.into(),
                        check: false,
                    }),
                    [_, destination, check] if check == "--check" => Ok(Self::AssetsGenerate {
                        destination: destination.into(),
                        check: true,
                    }),
                    _ => Err(ToolsCommandError::Usage),
                };
            }
        }
        match (args.next().as_deref(), args.next()) {
            (Some(command), None) if command == OsStr::new("engine-build") => Ok(Self::EngineBuild),
            (Some(command), None) if command == OsStr::new("engine-check") => Ok(Self::EngineCheck),
            _ => Err(ToolsCommandError::Usage),
        }
    }

    fn execute(self) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::EngineBuild => {
                let temporary = tempfile::tempdir()?;
                let budget = chess::capacity::budget::CapacityBudget::discover()?;
                let build = engine::ToolsEngineBuild::new(
                    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                    temporary.path().join("Reckless"),
                    budget,
                )?;
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(build.run())?;
                temporary.close()?;
            }
            Self::EngineCheck => {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(engine::ToolsEngineCheck::run())?;
            }
            #[cfg(feature = "asset-tools")]
            Self::AssetsGenerate { destination, check } => {
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
