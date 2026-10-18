use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::desktop::renderer_commands::{
    DesktopRendererCommands, DesktopRendererCommandsEnumCause, DesktopRendererCommandsEnumState,
    DesktopRendererCommandsError,
};
use chess::presentation::board::recovery::PresentationBoardRecovery;
use chess::scene::quality::{
    SceneQuality, SceneQualityEnumChange, SceneQualityEnumMode, SceneQualityEnumSample,
    SceneQualityEnumTier,
};

type SceneQualityResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct SceneQualityFixture {
    quality: SceneQuality,
    now_ms: u64,
}

impl SceneQualityFixture {
    fn new() -> Self {
        Self {
            quality: SceneQuality::default(),
            now_ms: 0,
        }
    }

    fn at(&self) -> Result<ClockInstant, Box<dyn std::error::Error>> {
        Ok(ClockInstant::from_millis(self.now_ms)?)
    }

    fn window(&mut self, frame_ms: u64) -> SceneQualityResult<Option<SceneQualityEnumChange>> {
        let mut last = None;
        for _ in 0..=20 {
            let frame = SceneQualityEnumSample::Active(ClockDuration::from_millis(frame_ms)?);
            last = self.quality.record(self.at()?, frame)?;
            self.now_ms += 100;
        }
        Ok(last)
    }

    fn windows(
        &mut self,
        count: u8,
        frame_ms: u64,
    ) -> SceneQualityResult<Vec<SceneQualityEnumChange>> {
        let mut changes = Vec::new();
        for _ in 0..count {
            changes.extend(self.window(frame_ms)?);
        }
        Ok(changes)
    }
}

#[test]
fn automatic_starts_at_medium_with_full_resolution() {
    let quality = SceneQuality::default();
    assert_eq!(quality.mode(), SceneQualityEnumMode::Automatic);
    assert_eq!(quality.profile().tier(), SceneQualityEnumTier::Medium);
    assert_eq!(quality.profile().render_scale(), 1.0);
    assert_eq!(quality.profile().shadow_map_size(), Some(1024));
    assert_eq!(SceneQualityEnumTier::High.shadow_map_size(), Some(2048));
    assert_eq!(SceneQualityEnumTier::Low.shadow_map_size(), None);
    assert_eq!(SceneQualityEnumTier::Low.dpr_cap(), 1.25);
    assert_eq!(SceneQualityEnumTier::Medium.dpr_cap(), 1.5);
    assert_eq!(SceneQualityEnumTier::High.dpr_cap(), 2.0);
}

#[test]
fn p95_uses_the_nearest_rank() {
    let samples: Vec<u64> = (1..=100).collect();
    assert_eq!(
        SceneQuality::p95(&samples).map(ClockDuration::as_nanos),
        Some(95)
    );
    assert!(SceneQuality::p95(&[]).is_none());
}

#[test]
fn three_slow_windows_degrade_effects_first_then_resolution_in_tenth_steps_to_the_floor()
-> SceneQualityResult {
    let mut fixture = SceneQualityFixture::new();
    assert!(fixture.windows(2, 25)?.is_empty());
    assert_eq!(
        fixture.windows(1, 25)?,
        vec![SceneQualityEnumChange::Degraded]
    );
    assert_eq!(fixture.quality.profile().tier(), SceneQualityEnumTier::Low);
    assert_eq!(fixture.quality.profile().render_scale(), 1.0);
    fixture.now_ms += 31_000;
    fixture.windows(3, 25)?;
    assert_eq!(fixture.quality.profile().render_scale(), 0.9);
    fixture.now_ms += 31_000;
    fixture.windows(3, 25)?;
    fixture.now_ms += 31_000;
    fixture.windows(3, 25)?;
    assert_eq!(fixture.quality.profile().render_scale(), 0.75);
    fixture.now_ms += 31_000;
    assert!(fixture.windows(6, 25)?.is_empty());
    assert_eq!(fixture.quality.profile().render_scale(), 0.75);
    Ok(())
}

#[test]
fn cooldown_blocks_a_second_change_for_thirty_seconds() -> SceneQualityResult {
    let mut fixture = SceneQualityFixture::new();
    fixture.windows(3, 25)?;
    assert_eq!(fixture.quality.profile().tier(), SceneQualityEnumTier::Low);
    let changes = fixture.windows(3, 25)?;
    assert!(changes.is_empty(), "cooldown must hold the profile");
    assert_eq!(fixture.quality.profile().render_scale(), 1.0);
    Ok(())
}

#[test]
fn ten_fast_windows_recover_resolution_before_effects() -> SceneQualityResult {
    let mut fixture = SceneQualityFixture::new();
    fixture.windows(3, 25)?;
    fixture.now_ms += 31_000;
    fixture.windows(3, 25)?;
    assert_eq!(fixture.quality.profile().render_scale(), 0.9);
    fixture.now_ms += 31_000;
    assert!(fixture.windows(9, 8)?.is_empty());
    assert_eq!(
        fixture.windows(1, 8)?,
        vec![SceneQualityEnumChange::Upgraded]
    );
    assert_eq!(fixture.quality.profile().render_scale(), 1.0);
    assert_eq!(fixture.quality.profile().tier(), SceneQualityEnumTier::Low);
    fixture.now_ms += 31_000;
    fixture.windows(10, 8)?;
    assert_eq!(
        fixture.quality.profile().tier(),
        SceneQualityEnumTier::Medium
    );
    Ok(())
}

#[test]
fn idle_background_and_loading_samples_never_count_and_break_streaks() -> SceneQualityResult {
    let mut fixture = SceneQualityFixture::new();
    fixture.windows(2, 25)?;
    for sample in [
        SceneQualityEnumSample::Idle,
        SceneQualityEnumSample::Background,
        SceneQualityEnumSample::Loading,
    ] {
        let at = fixture.at()?;
        fixture.quality.record(at, sample)?;
        fixture.now_ms += 2_100;
        let at = fixture.at()?;
        fixture.quality.record(
            at,
            SceneQualityEnumSample::Active(ClockDuration::from_millis(40)?),
        )?;
    }
    fixture.windows(1, 25)?;
    assert_eq!(
        fixture.quality.profile().tier(),
        SceneQualityEnumTier::Medium
    );
    Ok(())
}

#[test]
fn manual_tiers_disable_adaptation_and_switch_immediately() -> SceneQualityResult {
    let mut fixture = SceneQualityFixture::new();
    assert_eq!(
        fixture
            .quality
            .set_mode(SceneQualityEnumMode::Manual(SceneQualityEnumTier::High)),
        Some(SceneQualityEnumChange::Manual)
    );
    assert!(fixture.windows(6, 40)?.is_empty());
    assert_eq!(fixture.quality.profile().tier(), SceneQualityEnumTier::High);
    assert_eq!(
        fixture.quality.profile().effective_pixels_per_point(3.0),
        2.0
    );
    Ok(())
}

#[test]
fn graphics_recovery_pauses_games_retries_within_ten_seconds_and_never_resumes_automatically()
-> SceneQualityResult {
    let mut commands = DesktopRendererCommands::new();
    assert!(commands.is_board_available() && !commands.games_paused());
    assert!(PresentationBoardRecovery::headline(&commands).is_none());
    commands.interrupt(DesktopRendererCommandsEnumCause::ContextLoss);
    assert!(commands.games_paused() && !commands.is_board_available());
    assert!(PresentationBoardRecovery::headline(&commands).is_some());
    assert!(matches!(
        commands.recovered(ClockInstant::from_millis(0)?),
        Err(DesktopRendererCommandsError::NotRetrying)
    ));
    let deadline = commands.retry(ClockInstant::from_millis(1_000)?)?;
    assert_eq!(deadline, ClockInstant::from_millis(11_000)?);
    assert!(matches!(
        commands.retry(deadline),
        Err(DesktopRendererCommandsError::RetryUnavailable)
    ));
    commands.recovered(ClockInstant::from_millis(5_000)?)?;
    assert!(commands.is_board_available());
    assert!(commands.games_paused(), "recovery must not resume games");
    commands.resume_games()?;
    assert!(!commands.games_paused());
    Ok(())
}

#[test]
fn an_expired_retry_becomes_unrecoverable_and_allows_another_explicit_retry() -> SceneQualityResult
{
    let mut commands = DesktopRendererCommands::new();
    commands.interrupt(DesktopRendererCommandsEnumCause::Shader);
    commands.retry(ClockInstant::from_millis(0)?)?;
    assert!(!commands.observe(ClockInstant::from_millis(9_999)?));
    assert!(commands.observe(ClockInstant::from_millis(10_000)?));
    assert_eq!(
        commands.state(),
        DesktopRendererCommandsEnumState::Unrecoverable(DesktopRendererCommandsEnumCause::Shader)
    );
    assert!(matches!(
        commands.recovered(ClockInstant::from_millis(10_001)?),
        Err(DesktopRendererCommandsError::NotRetrying)
    ));
    assert!(matches!(
        commands.resume_games(),
        Err(DesktopRendererCommandsError::ResumeUnavailable)
    ));
    commands.retry(ClockInstant::from_millis(20_000)?)?;
    Ok(())
}
