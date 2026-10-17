use chess::clock::duration::ClockDuration;
use chess::desktop::messages::DesktopEnumCommand;
use chess::desktop::native::game_resources::DesktopNativeGameResourcesPolicy;
use chess::desktop::update_commands::DesktopUpdateOperation;
use chess::engine::configuration::EngineConfigurationOverrides;

type DesktopEngineOverridesTestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn overrides_are_bounded_by_the_automatic_policy_and_none_keeps_it() -> DesktopEngineOverridesTestResult {
    let base = DesktopNativeGameResourcesPolicy::new(4, 256)?;
    let kept = base.with_overrides(EngineConfigurationOverrides {
        threads: None,
        hash_mib: None,
        move_overhead_ms: None,
    })?;
    assert_eq!(kept.workers(), 4);
    assert_eq!(kept.hash_mib(), 256);
    assert_eq!(kept.move_overhead(), base.move_overhead());
    let bounded = base.with_overrides(EngineConfigurationOverrides {
        threads: Some(64),
        hash_mib: Some(100_000),
        move_overhead_ms: Some(250),
    })?;
    assert_eq!(bounded.workers(), 4);
    assert_eq!(bounded.hash_mib(), 256);
    assert_eq!(bounded.move_overhead(), ClockDuration::from_millis(250)?);
    let smaller = base.with_overrides(EngineConfigurationOverrides {
        threads: Some(2),
        hash_mib: Some(64),
        move_overhead_ms: None,
    })?;
    assert_eq!((smaller.workers(), smaller.hash_mib()), (2, 64));
    assert_eq!(smaller.startup_hash_mib(), base.startup_hash_mib());
    Ok(())
}

#[test]
fn update_operations_are_ordinary_nonmutating_commands() {
    let command = DesktopEnumCommand::Update(DesktopUpdateOperation::Snapshot);
    assert!(!command.is_control());
    assert!(command.mutation_entity().is_none());
    assert!(!command.is_available_while_closing());
}
