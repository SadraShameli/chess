#[path = "native/compatibility.rs"]
mod native_compatibility;

#[path = "native/installation.rs"]
mod native_installation;

#[path = "native/host_lifecycle.rs"]
mod native_host_lifecycle;

#[path = "native/engine_live_game.rs"]
mod native_engine_live_game;

#[path = "native/engine_analysis_session.rs"]
mod native_engine_analysis_session;

#[path = "native/engine_options.rs"]
mod native_engine_options;

#[path = "native/engine_composition.rs"]
mod native_engine_composition;
#[path = "native/workbench.rs"]
mod native_workbench;

#[path = "native/game_composition.rs"]
mod native_game_composition;

#[path = "scene/task_pools.rs"]
mod scene_task_pools;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => native_compatibility::NativeCompatibilityHarness::run()?,
        [operation, directory] if operation == "--installation" => {
            native_installation::NativeInstallationHarness::run(std::path::Path::new(directory))?;
        }
        [operation] if operation == "--host-lifecycle" => {
            native_host_lifecycle::NativeHostLifecycleHarness::run()?;
        }
        [operation] if operation == "--game-composition" => {
            native_game_composition::NativeGameCompositionHarness::run()?;
        }
        [operation] if operation == "--engine-composition" => {
            native_engine_composition::NativeEngineCompositionHarness::run()?;
        }
        [operation, directory] if operation == "--engine-analysis-session" => {
            native_engine_analysis_session::NativeEngineAnalysisSessionHarness::run(
                std::path::Path::new(directory),
            )?;
        }
        [operation, directory] if operation == "--engine-options" => {
            native_engine_options::NativeEngineOptionsHarness::run(std::path::Path::new(
                directory,
            ))?;
        }
        [operation, directory] if operation == "--workbench" => {
            native_workbench::NativeWorkbenchHarness::run(std::path::Path::new(directory))?;
        }
        [operation, directory] if operation == "--engine-live-game" => {
            native_engine_live_game::NativeEngineLiveGameHarness::run(std::path::Path::new(
                directory,
            ))?;
        }
        _ => {
            return Err(
                "Usage: native [--host-lifecycle | --game-composition | --engine-composition | --engine-live-game ENGINE_DIRECTORY | --engine-options ENGINE_DIRECTORY | --engine-analysis-session ENGINE_DIRECTORY | --workbench ENGINE_DIRECTORY]"
                    .into(),
            );
        }
    }
    Ok(())
}
