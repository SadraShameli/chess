#[path = "native/compatibility.rs"]
mod native_compatibility;

#[path = "scene/task_pools.rs"]
mod scene_task_pools;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    native_compatibility::NativeCompatibilityHarness::run()?;
    Ok(())
}
