use chess::clock::duration::ClockDuration;
use chess::engine::configuration::{
    EngineConfiguration, EngineConfigurationEnumBoundary, EngineConfigurationEnumStatus,
};
use chess::engine::resource_config::EngineResourceConfig;

fn config(threads: usize) -> Result<EngineResourceConfig, Box<dyn std::error::Error>> {
    Ok(EngineResourceConfig::new(
        threads,
        64,
        ClockDuration::from_millis(30)?,
    )?)
}

#[test]
fn an_idle_engine_applies_immediately() -> Result<(), Box<dyn std::error::Error>> {
    let mut configuration = EngineConfiguration::new(config(1)?);
    let status = configuration.request(config(4)?, EngineConfigurationEnumBoundary::Idle);
    assert_eq!(status, EngineConfigurationEnumStatus::InSync);
    assert_eq!(configuration.effective(), config(4)?);
    Ok(())
}

#[test]
fn an_active_search_defers_until_the_safe_boundary_and_snapshots_stay()
-> Result<(), Box<dyn std::error::Error>> {
    let mut configuration = EngineConfiguration::new(config(1)?);
    let running = configuration.snapshot();
    let status = configuration.request(config(8)?, EngineConfigurationEnumBoundary::SearchActive);
    assert_eq!(
        status,
        EngineConfigurationEnumStatus::PendingNextBoundary(config(8)?)
    );
    assert_eq!(configuration.effective(), config(1)?);
    assert_eq!(configuration.reach_boundary(), Some(config(8)?));
    assert_eq!(configuration.effective(), config(8)?);
    assert_eq!(running, config(1)?);
    assert_eq!(configuration.reach_boundary(), None);
    Ok(())
}

#[test]
fn requesting_the_effective_value_clears_a_pending_change() -> Result<(), Box<dyn std::error::Error>>
{
    let mut configuration = EngineConfiguration::new(config(1)?);
    configuration.request(config(2)?, EngineConfigurationEnumBoundary::SearchActive);
    let status = configuration.request(config(1)?, EngineConfigurationEnumBoundary::SearchActive);
    assert_eq!(status, EngineConfigurationEnumStatus::InSync);
    assert_eq!(configuration.pending(), None);
    Ok(())
}
