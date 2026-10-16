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
}

impl Default for DesktopPreferences {
    fn default() -> Self {
        Self {
            theme: DesktopPreferencesEnumTheme::Dark,
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

    pub fn sections() -> &'static [DesktopPreferencesEnumSection] {
        &Self::SECTIONS
    }

    pub fn themes() -> &'static [DesktopPreferencesEnumTheme] {
        &Self::THEMES
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
