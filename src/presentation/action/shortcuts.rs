use strum::IntoEnumIterator;

use crate::desktop::preferences::{
    DesktopPreferences, DesktopPreferencesChord, DesktopPreferencesEnumShortcutAction,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::Display)]
pub enum PresentationShortcutEnumPlatform {
    #[strum(serialize = "Cmd")]
    Mac,
    #[strum(serialize = "Ctrl")]
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

    fn command_name(self) -> &'static str {
        self.into()
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
        DesktopPreferencesEnumShortcutAction::iter()
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
        DesktopPreferencesEnumShortcutAction::iter()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationShortcutEnumScope {
    Global,
    Notation,
    Game,
    Puzzle,
    Editor,
    Promotion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationShortcutEnumKey {
    Char(char),
    Left,
    Right,
    Home,
    End,
    Space,
    Escape,
    Delete,
    Backspace,
    Enter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationShortcutBinding {
    scope: PresentationShortcutEnumScope,
    command: bool,
    shift: bool,
    key: PresentationShortcutEnumKey,
    action: &'static str,
}

impl PresentationShortcutBinding {
    const fn new(
        scope: PresentationShortcutEnumScope,
        command: bool,
        shift: bool,
        key: PresentationShortcutEnumKey,
        action: &'static str,
    ) -> Self {
        Self {
            scope,
            command,
            shift,
            key,
            action,
        }
    }

    pub const fn scope(self) -> PresentationShortcutEnumScope {
        self.scope
    }

    pub const fn action(self) -> &'static str {
        self.action
    }

    pub const fn key(self) -> PresentationShortcutEnumKey {
        self.key
    }

    pub const fn command(self) -> bool {
        self.command
    }

    pub const fn shift(self) -> bool {
        self.shift
    }
}

impl PresentationShortcuts {
    const CONTEXT_BINDINGS: [PresentationShortcutBinding; 24] = {
        use PresentationShortcutEnumKey as Key;
        use PresentationShortcutEnumScope as Scope;
        [
            PresentationShortcutBinding::new(Scope::Global, false, false, Key::Escape, "Cancel"),
            PresentationShortcutBinding::new(Scope::Notation, false, false, Key::Left, "Previous move"),
            PresentationShortcutBinding::new(Scope::Notation, false, false, Key::Right, "Next move"),
            PresentationShortcutBinding::new(Scope::Notation, false, false, Key::Home, "First position"),
            PresentationShortcutBinding::new(Scope::Notation, false, false, Key::End, "Live position"),
            PresentationShortcutBinding::new(Scope::Game, false, false, Key::Space, "Activate focused square"),
            PresentationShortcutBinding::new(Scope::Game, false, false, Key::Char('p'), "Pause or resume"),
            PresentationShortcutBinding::new(Scope::Puzzle, false, false, Key::Char('h'), "Puzzle hint"),
            PresentationShortcutBinding::new(Scope::Puzzle, false, true, Key::Char('n'), "Next puzzle"),
            PresentationShortcutBinding::new(Scope::Editor, false, false, Key::Char('k'), "Select king"),
            PresentationShortcutBinding::new(Scope::Editor, false, false, Key::Char('q'), "Select queen"),
            PresentationShortcutBinding::new(Scope::Editor, false, false, Key::Char('r'), "Select rook"),
            PresentationShortcutBinding::new(Scope::Editor, false, false, Key::Char('b'), "Select bishop"),
            PresentationShortcutBinding::new(Scope::Editor, false, false, Key::Char('n'), "Select knight"),
            PresentationShortcutBinding::new(Scope::Editor, false, false, Key::Char('p'), "Select pawn"),
            PresentationShortcutBinding::new(Scope::Editor, false, false, Key::Char('c'), "Toggle placement color"),
            PresentationShortcutBinding::new(Scope::Editor, false, false, Key::Delete, "Remove piece"),
            PresentationShortcutBinding::new(Scope::Editor, false, false, Key::Backspace, "Remove piece"),
            PresentationShortcutBinding::new(Scope::Editor, false, false, Key::Enter, "Place piece"),
            PresentationShortcutBinding::new(Scope::Editor, true, false, Key::Char('z'), "Undo"),
            PresentationShortcutBinding::new(Scope::Editor, true, true, Key::Char('z'), "Redo"),
            PresentationShortcutBinding::new(Scope::Promotion, false, false, Key::Char('q'), "Promote to queen"),
            PresentationShortcutBinding::new(Scope::Promotion, false, false, Key::Char('r'), "Promote to rook"),
            PresentationShortcutBinding::new(Scope::Promotion, false, false, Key::Char('n'), "Promote to knight"),
        ]
    };

    pub fn context_bindings() -> &'static [PresentationShortcutBinding] {
        &Self::CONTEXT_BINDINGS
    }

    pub fn context_conflict(chord: DesktopPreferencesChord) -> Option<PresentationShortcutBinding> {
        Self::CONTEXT_BINDINGS.into_iter().find(|binding| {
            binding.key == PresentationShortcutEnumKey::Char(chord.key())
                && binding.command == chord.command()
                && binding.shift == chord.shift()
                && !chord.alt()
        })
    }
}
