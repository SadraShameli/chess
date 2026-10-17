use crate::desktop::preferences::{
    DesktopPreferences, DesktopPreferencesChord, DesktopPreferencesEnumShortcutAction,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationShortcutEnumPlatform {
    Mac,
    Other,
}

impl PresentationShortcutEnumPlatform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Other
        }
    }

    const fn command_name(self) -> &'static str {
        match self {
            Self::Mac => "Cmd",
            Self::Other => "Ctrl",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationShortcutEnumContext {
    Global,
    TextInput,
}

pub struct PresentationShortcuts;

impl PresentationShortcuts {
    pub const fn default_chord(
        action: DesktopPreferencesEnumShortcutAction,
    ) -> DesktopPreferencesChord {
        action.default_chord()
    }

    pub fn chord(
        preferences: DesktopPreferences,
        action: DesktopPreferencesEnumShortcutAction,
    ) -> DesktopPreferencesChord {
        preferences
            .shortcut(action)
            .unwrap_or_else(|| Self::default_chord(action))
    }

    pub fn is_reserved(
        platform: PresentationShortcutEnumPlatform,
        chord: DesktopPreferencesChord,
    ) -> bool {
        let command_only = chord.command() && !chord.shift() && !chord.alt();
        match platform {
            PresentationShortcutEnumPlatform::Mac => {
                command_only && matches!(chord.key(), 'q' | 'h' | 'm' | ' ' | 'w')
            }
            PresentationShortcutEnumPlatform::Other => {
                chord.command() && chord.alt() && matches!(chord.key(), 'd' | 't')
            }
        }
    }

    pub fn conflict(
        preferences: DesktopPreferences,
        action: DesktopPreferencesEnumShortcutAction,
        chord: DesktopPreferencesChord,
    ) -> Option<DesktopPreferencesEnumShortcutAction> {
        DesktopPreferencesEnumShortcutAction::ALL
            .into_iter()
            .filter(|other| *other != action)
            .find(|other| Self::chord(preferences, *other) == chord)
    }

    pub fn resolve(
        preferences: DesktopPreferences,
        context: PresentationShortcutEnumContext,
        pressed: DesktopPreferencesChord,
    ) -> Option<DesktopPreferencesEnumShortcutAction> {
        if context == PresentationShortcutEnumContext::TextInput && !pressed.command() {
            return None;
        }
        DesktopPreferencesEnumShortcutAction::ALL
            .into_iter()
            .find(|action| Self::chord(preferences, *action) == pressed)
    }

    pub fn label(
        platform: PresentationShortcutEnumPlatform,
        chord: DesktopPreferencesChord,
    ) -> String {
        let mut text = String::new();
        if chord.command() {
            text.push_str(platform.command_name());
            text.push('+');
        }
        if chord.alt() {
            text.push_str("Alt+");
        }
        if chord.shift() {
            text.push_str("Shift+");
        }
        text.push(chord.key().to_ascii_uppercase());
        text
    }
}
