use thiserror::Error;

use crate::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};

#[derive(Debug, Error)]
pub enum SceneQualityError {
    #[error("The quality clock failed: {0}")]
    Clock(#[from] ClockDurationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SceneQualityEnumTier {
    Low,
    Medium,
    High,
}

impl SceneQualityEnumTier {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }

    pub const fn shadow_map_size(self) -> Option<u32> {
        match self {
            Self::Low => None,
            Self::Medium => Some(1024),
            Self::High => Some(2048),
        }
    }

    pub const fn has_antialiasing(self) -> bool {
        matches!(self, Self::High)
    }

    pub const fn dpr_cap(self) -> f32 {
        match self {
            Self::Low => 1.25,
            Self::Medium => 1.5,
            Self::High => 2.0,
        }
    }

    const fn lower(self) -> Option<Self> {
        match self {
            Self::High => Some(Self::Medium),
            Self::Medium => Some(Self::Low),
            Self::Low => None,
        }
    }

    const fn higher(self) -> Option<Self> {
        match self {
            Self::Low => Some(Self::Medium),
            Self::Medium => Some(Self::High),
            Self::High => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SceneQualityEnumMode {
    #[default]
    Automatic,
    Manual(SceneQualityEnumTier),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneQualityEnumSample {
    Active(ClockDuration),
    Idle,
    Background,
    Loading,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneQualityProfile {
    tier: SceneQualityEnumTier,
    render_scale: f32,
}

impl SceneQualityProfile {
    pub const fn tier(self) -> SceneQualityEnumTier {
        self.tier
    }

    pub const fn render_scale(self) -> f32 {
        self.render_scale
    }

    pub const fn shadow_map_size(self) -> Option<u32> {
        self.tier.shadow_map_size()
    }

    pub fn effective_pixels_per_point(self, pixels_per_point: f32) -> f32 {
        pixels_per_point.min(self.tier.dpr_cap())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneQualityEnumChange {
    Degraded,
    Upgraded,
    Manual,
}

pub struct SceneQuality {
    mode: SceneQualityEnumMode,
    tier: SceneQualityEnumTier,
    scale_step: u8,
    window_started: Option<ClockInstant>,
    samples: Vec<u64>,
    window_ignored: bool,
    high_streak: u8,
    low_streak: u8,
    cooldown_until: Option<ClockInstant>,
}

impl Default for SceneQuality {
    fn default() -> Self {
        Self::new(SceneQualityEnumMode::Automatic)
    }
}

impl SceneQuality {
    pub const WINDOW: ClockDuration = ClockDuration::from_nanos(2_000_000_000);
    pub const COOLDOWN: ClockDuration = ClockDuration::from_nanos(30_000_000_000);
    pub const DEGRADE_THRESHOLD: ClockDuration = ClockDuration::from_nanos(20_000_000);
    pub const UPGRADE_THRESHOLD: ClockDuration = ClockDuration::from_nanos(13_000_000);
    pub const DEGRADE_WINDOWS: u8 = 3;
    pub const UPGRADE_WINDOWS: u8 = 10;
    pub const MAX_SCALE_STEP: u8 = 3;
    const SCALES: [f32; 4] = [1.0, 0.9, 0.8, 0.75];

    pub fn new(mode: SceneQualityEnumMode) -> Self {
        Self {
            mode,
            tier: match mode {
                SceneQualityEnumMode::Automatic => SceneQualityEnumTier::Medium,
                SceneQualityEnumMode::Manual(tier) => tier,
            },
            scale_step: 0,
            window_started: None,
            samples: Vec::new(),
            window_ignored: false,
            high_streak: 0,
            low_streak: 0,
            cooldown_until: None,
        }
    }

    pub const fn mode(&self) -> SceneQualityEnumMode {
        self.mode
    }

    pub fn profile(&self) -> SceneQualityProfile {
        SceneQualityProfile {
            tier: self.tier,
            render_scale: Self::SCALES[usize::from(self.scale_step)],
        }
    }

    pub fn set_mode(&mut self, mode: SceneQualityEnumMode) -> Option<SceneQualityEnumChange> {
        let before = self.profile();
        self.mode = mode;
        if let SceneQualityEnumMode::Manual(tier) = mode {
            self.tier = tier;
            self.scale_step = 0;
        }
        self.reset_window();
        self.high_streak = 0;
        self.low_streak = 0;
        (self.profile() != before).then_some(SceneQualityEnumChange::Manual)
    }

    pub fn record(
        &mut self,
        now: ClockInstant,
        sample: SceneQualityEnumSample,
    ) -> Result<Option<SceneQualityEnumChange>, SceneQualityError> {
        if self.mode != SceneQualityEnumMode::Automatic {
            return Ok(None);
        }
        let started = *self.window_started.get_or_insert(now);
        match sample {
            SceneQualityEnumSample::Active(frame) => self.samples.push(frame.as_nanos()),
            SceneQualityEnumSample::Idle
            | SceneQualityEnumSample::Background
            | SceneQualityEnumSample::Loading => self.window_ignored = true,
        }
        if now.elapsed_since(started)?.as_nanos() < Self::WINDOW.as_nanos() {
            return Ok(None);
        }
        let outcome = self.close_window(now)?;
        self.reset_window();
        Ok(outcome)
    }

    pub fn p95(samples: &[u64]) -> Option<ClockDuration> {
        if samples.is_empty() {
            return None;
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let rank = (sorted.len() * 95).div_ceil(100).max(1);
        sorted.get(rank - 1).copied().map(ClockDuration::from_nanos)
    }

    fn reset_window(&mut self) {
        self.window_started = None;
        self.samples.clear();
        self.window_ignored = false;
    }

    fn close_window(
        &mut self,
        now: ClockInstant,
    ) -> Result<Option<SceneQualityEnumChange>, SceneQualityError> {
        let p95 = if self.window_ignored {
            None
        } else {
            Self::p95(&self.samples)
        };
        let Some(p95) = p95 else {
            self.high_streak = 0;
            self.low_streak = 0;
            return Ok(None);
        };
        if p95.as_nanos() > Self::DEGRADE_THRESHOLD.as_nanos() {
            self.high_streak = self.high_streak.saturating_add(1);
            self.low_streak = 0;
        } else if p95.as_nanos() < Self::UPGRADE_THRESHOLD.as_nanos() {
            self.low_streak = self.low_streak.saturating_add(1);
            self.high_streak = 0;
        } else {
            self.high_streak = 0;
            self.low_streak = 0;
        }
        let cooled = match self.cooldown_until {
            Some(until) => now.has_reached(until),
            None => true,
        };
        if !cooled {
            return Ok(None);
        }
        let change = if self.high_streak >= Self::DEGRADE_WINDOWS {
            self.degrade().then_some(SceneQualityEnumChange::Degraded)
        } else if self.low_streak >= Self::UPGRADE_WINDOWS {
            self.upgrade().then_some(SceneQualityEnumChange::Upgraded)
        } else {
            None
        };
        if change.is_some() {
            self.high_streak = 0;
            self.low_streak = 0;
            self.cooldown_until = Some(now.checked_add(Self::COOLDOWN)?);
        }
        Ok(change)
    }

    fn degrade(&mut self) -> bool {
        if let Some(lower) = self.tier.lower() {
            self.tier = lower;
            true
        } else if self.scale_step < Self::MAX_SCALE_STEP {
            self.scale_step += 1;
            true
        } else {
            false
        }
    }

    fn upgrade(&mut self) -> bool {
        if self.scale_step > 0 {
            self.scale_step -= 1;
            true
        } else if let Some(higher) = self.tier.higher() {
            self.tier = higher;
            true
        } else {
            false
        }
    }
}
