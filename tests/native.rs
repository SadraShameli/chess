#[path = "native/compatibility.rs"]
mod native_compatibility;

#[path = "native/host_lifecycle.rs"]
mod native_host_lifecycle;

#[path = "scene/task_pools.rs"]
mod scene_task_pools;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => native_compatibility::NativeCompatibilityHarness::run()?,
        [operation] if operation == "--host-lifecycle" => {
            native_host_lifecycle::NativeHostLifecycleHarness::run()?;
        }
        [operation, candidate, output] if operation == "--compare-assets" => {
            native_compatibility::NativeCompatibilityHarness::compare_assets(
                std::path::Path::new(candidate),
                std::path::Path::new(output),
            )?;
        }
        _ => {
            return Err(
                "Usage: native [--host-lifecycle | --compare-assets CANDIDATE_GLB OUTPUT_DIRECTORY]"
                    .into(),
            );
        }
    }
    Ok(())
}
