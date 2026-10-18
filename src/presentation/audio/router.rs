use crate::desktop::preferences::{DesktopPreferences, DesktopPreferencesEnumAudioCategory};
use crate::game::enums::GameId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationAudioEvent {
    game: GameId,
    category: DesktopPreferencesEnumAudioCategory,
}

impl PresentationAudioEvent {
    pub const fn new(game: GameId, category: DesktopPreferencesEnumAudioCategory) -> Self {
        Self { game, category }
    }

    pub const fn game(self) -> GameId {
        self.game
    }

    pub const fn category(self) -> DesktopPreferencesEnumAudioCategory {
        self.category
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentationAudioPlayback {
    category: DesktopPreferencesEnumAudioCategory,
    gain: f32,
}

impl PresentationAudioPlayback {
    pub const fn category(self) -> DesktopPreferencesEnumAudioCategory {
        self.category
    }

    pub const fn gain(self) -> f32 {
        self.gain
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PresentationAudioContext {
    pub selected_game: Option<GameId>,
    pub app_focused: bool,
    pub now_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct PresentationAudioPlaying {
    category: DesktopPreferencesEnumAudioCategory,
    until_ms: u64,
}

#[derive(Debug, Default)]
pub struct PresentationAudioRouter {
    last_started: [Option<u64>; 4],
    playing: Option<PresentationAudioPlaying>,
}

impl PresentationAudioRouter {
    pub const COALESCE_MS: u64 = 100;

    pub const fn priority(category: DesktopPreferencesEnumAudioCategory) -> u8 {
        match category {
            DesktopPreferencesEnumAudioCategory::Result => 3,
            DesktopPreferencesEnumAudioCategory::LowTime => 2,
            DesktopPreferencesEnumAudioCategory::Check => 1,
            DesktopPreferencesEnumAudioCategory::Move => 0,
        }
    }

    pub const fn duration_ms(category: DesktopPreferencesEnumAudioCategory) -> u64 {
        match category {
            DesktopPreferencesEnumAudioCategory::Move => 110,
            DesktopPreferencesEnumAudioCategory::Check => 260,
            DesktopPreferencesEnumAudioCategory::Result => 900,
            DesktopPreferencesEnumAudioCategory::LowTime => 340,
        }
    }

    const fn slot(category: DesktopPreferencesEnumAudioCategory) -> usize {
        match category {
            DesktopPreferencesEnumAudioCategory::Move => 0,
            DesktopPreferencesEnumAudioCategory::Check => 1,
            DesktopPreferencesEnumAudioCategory::Result => 2,
            DesktopPreferencesEnumAudioCategory::LowTime => 3,
        }
    }

    fn routed(event: PresentationAudioEvent, context: PresentationAudioContext) -> bool {
        match event.category {
            DesktopPreferencesEnumAudioCategory::Move
            | DesktopPreferencesEnumAudioCategory::Check => {
                context.selected_game == Some(event.game)
            }
            DesktopPreferencesEnumAudioCategory::Result
            | DesktopPreferencesEnumAudioCategory::LowTime => true,
        }
    }

    pub fn submit(
        &mut self,
        events: &[PresentationAudioEvent],
        preferences: DesktopPreferences,
        context: PresentationAudioContext,
    ) -> Option<PresentationAudioPlayback> {
        if !context.app_focused {
            return None;
        }
        let best = events
            .iter()
            .copied()
            .filter(|event| Self::routed(*event, context))
            .filter(|event| preferences.effective_gain(event.category) > 0.0)
            .max_by_key(|event| Self::priority(event.category))?;
        let slot = Self::slot(best.category);
        if let Some(last) = self.last_started[slot]
            && context.now_ms.saturating_sub(last) < Self::COALESCE_MS
        {
            return None;
        }
        if let Some(playing) = self.playing
            && context.now_ms < playing.until_ms
            && Self::priority(best.category) <= Self::priority(playing.category)
        {
            return None;
        }
        self.last_started[slot] = Some(context.now_ms);
        self.playing = Some(PresentationAudioPlaying {
            category: best.category,
            until_ms: context
                .now_ms
                .saturating_add(Self::duration_ms(best.category)),
        });
        Some(PresentationAudioPlayback {
            category: best.category,
            gain: preferences.effective_gain(best.category),
        })
    }
}
