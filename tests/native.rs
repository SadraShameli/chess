#[path = "native/compatibility.rs"]
mod native_compatibility;

#[path = "presentation/font.rs"]
mod presentation_font;

#[path = "scene/task_pools.rs"]
mod scene_task_pools;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    native_compatibility::NativeCompatibilityHarness::run()?;
    Ok(())
}
