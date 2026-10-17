#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopPreferencesEnumTheme {
    Dark,
    Light,
}

impl DesktopPreferencesEnumTheme {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopPreferencesEnumMotion {
    Full,
    Reduced,
}

impl DesktopPreferencesEnumMotion {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Full => "Full motion",
            Self::Reduced => "Reduced motion",
        }
    }

    pub const fn is_reduced(self) -> bool {
        matches!(self, Self::Reduced)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopPreferencesEnumSection {
    Appearance,
}

impl DesktopPreferencesEnumSection {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopPreferences {
    theme: DesktopPreferencesEnumTheme,
    motion: DesktopPreferencesEnumMotion,
}

impl Default for DesktopPreferences {
    fn default() -> Self {
        Self {
            theme: DesktopPreferencesEnumTheme::Dark,
            motion: DesktopPreferencesEnumMotion::Full,
        }
    }
}

impl DesktopPreferences {
    const SECTIONS: [DesktopPreferencesEnumSection; 1] =
        [DesktopPreferencesEnumSection::Appearance];
    const THEMES: [DesktopPreferencesEnumTheme; 2] = [
        DesktopPreferencesEnumTheme::Dark,
        DesktopPreferencesEnumTheme::Light,
    ];
    const MOTIONS: [DesktopPreferencesEnumMotion; 2] = [
        DesktopPreferencesEnumMotion::Full,
        DesktopPreferencesEnumMotion::Reduced,
    ];

    pub fn sections() -> &'static [DesktopPreferencesEnumSection] {
        &Self::SECTIONS
    }

    pub fn themes() -> &'static [DesktopPreferencesEnumTheme] {
        &Self::THEMES
    }

    pub fn motions() -> &'static [DesktopPreferencesEnumMotion] {
        &Self::MOTIONS
    }

    pub const fn motion(self) -> DesktopPreferencesEnumMotion {
        self.motion
    }

    pub fn set_motion(&mut self, motion: DesktopPreferencesEnumMotion) -> bool {
        if self.motion == motion {
            return false;
        }
        self.motion = motion;
        true
    }

    pub const fn theme(self) -> DesktopPreferencesEnumTheme {
        self.theme
    }

    pub fn set_theme(&mut self, theme: DesktopPreferencesEnumTheme) -> bool {
        if self.theme == theme {
            return false;
        }
        self.theme = theme;
        true
    }
}
