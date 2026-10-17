use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DesktopPreferencesError {
    #[error("A volume of {0} percent is outside 0 to 100")]
    Percent(u8),
    #[error("The engine {name} value {value} must be between {minimum} and {maximum}")]
    EngineValue {
        name: &'static str,
        value: u32,
        minimum: u32,
        maximum: u32,
    },
    #[error("The shortcut key {0:?} is not a printable ASCII character")]
    ShortcutKey(char),
    #[error("The settings document version {0} is not supported")]
    Version(u32),
    #[error("The settings document exceeds {0} bytes")]
    TooLarge(usize),
    #[error("The settings document is not valid: {0}")]
    Document(String),
    #[error("The settings document repeats an entry")]
    Duplicate,
    #[error("The saved settings revision is stale")]
    StaleRevision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::EnumIter, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
pub enum DesktopPreferencesEnumTheme {
    #[strum(serialize = "Dark")]
    Dark,
    #[strum(serialize = "Light")]
    Light,
}

impl DesktopPreferencesEnumTheme {
    pub fn label(self) -> &'static str {
        self.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, strum::EnumIter, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
pub enum DesktopPreferencesEnumNotation {
    #[default]
    #[strum(serialize = "Letters")]
    Letters,
    #[strum(serialize = "Figurines")]
    Figurines,
}

impl DesktopPreferencesEnumNotation {
    pub fn label(self) -> &'static str {
        self.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::EnumIter, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
pub enum DesktopPreferencesEnumMotion {
    #[strum(serialize = "Full motion")]
    Full,
    #[strum(serialize = "Reduced motion")]
    Reduced,
}

impl DesktopPreferencesEnumMotion {
    pub fn label(self) -> &'static str {
        self.into()
    }

    pub const fn is_reduced(self) -> bool {
        matches!(self, Self::Reduced)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::EnumIter, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
pub enum DesktopPreferencesEnumAid {
    #[strum(serialize = "Board coordinates")]
    Coordinates,
    #[strum(serialize = "Legal move hints")]
    LegalMoves,
    #[strum(serialize = "Last move highlight")]
    LastMove,
    #[strum(serialize = "Check highlight")]
    CheckHighlight,
}

impl DesktopPreferencesEnumAid {
    pub fn label(self) -> &'static str {
        self.into()
    }

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, strum::EnumIter, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
pub enum DesktopPreferencesEnumAudioCategory {
    #[strum(serialize = "Moves")]
    Move,
    #[strum(serialize = "Checks")]
    Check,
    #[strum(serialize = "Results")]
    Result,
    #[strum(serialize = "Low time")]
    LowTime,
}

impl DesktopPreferencesEnumAudioCategory {
    pub fn label(self) -> &'static str {
        self.into()
    }

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, strum::EnumIter, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
pub enum DesktopPreferencesEnumShortcutAction {
    #[strum(serialize = "Command palette")]
    Palette,
    #[strum(serialize = "New game")]
    NewGame,
    #[strum(serialize = "Settings")]
    Settings,
    #[strum(serialize = "Flip board")]
    FlipBoard,
}

impl DesktopPreferencesEnumShortcutAction {
    pub fn label(self) -> &'static str {
        self.into()
    }

    pub const fn default_chord(self) -> DesktopPreferencesChord {
        let (command, key) = match self {
            Self::Palette => (true, 'k'),
            Self::NewGame => (true, 'n'),
            Self::Settings => (true, ','),
            Self::FlipBoard => (false, 'f'),
        };
        DesktopPreferencesChord {
            command,
            shift: false,
            alt: false,
            key,
        }
    }

    pub const fn slot(self) -> usize {
        self.index()
    }

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, strum::EnumIter, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
pub enum DesktopPreferencesEnumSection {
    #[strum(serialize = "Appearance")]
    Appearance,
    #[strum(serialize = "Board aids")]
    BoardAids,
    #[strum(serialize = "Audio")]
    Audio,
    #[strum(serialize = "Shortcuts")]
    Shortcuts,
    #[strum(serialize = "Engine")]
    Engine,
}

impl DesktopPreferencesEnumSection {
    pub fn label(self) -> &'static str {
        self.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct DesktopPreferencesPercent(u8);

impl DesktopPreferencesPercent {
    pub const MAX: u8 = 100;
    pub const FULL: Self = Self(Self::MAX);
    pub const DEFAULT_MASTER: Self = Self(70);

    pub const fn new(value: u8) -> Result<Self, DesktopPreferencesError> {
        if value > Self::MAX {
            return Err(DesktopPreferencesError::Percent(value));
        }
        Ok(Self(value))
    }

    pub const fn value(self) -> u8 {
        self.0
    }

    pub fn fraction(self) -> f32 {
        f32::from(self.0) / f32::from(Self::MAX)
    }
}

impl TryFrom<u8> for DesktopPreferencesPercent {
    type Error = DesktopPreferencesError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<DesktopPreferencesPercent> for u8 {
    fn from(value: DesktopPreferencesPercent) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    try_from = "DesktopPreferencesChordWire",
    into = "DesktopPreferencesChordWire"
)]
pub struct DesktopPreferencesChord {
    command: bool,
    shift: bool,
    alt: bool,
    key: char,
}

#[derive(Serialize, Deserialize)]
struct DesktopPreferencesChordWire {
    command: bool,
    shift: bool,
    alt: bool,
    key: char,
}

impl DesktopPreferencesChord {
    pub fn new(
        command: bool,
        shift: bool,
        alt: bool,
        key: char,
    ) -> Result<Self, DesktopPreferencesError> {
        if !key.is_ascii_graphic() {
            return Err(DesktopPreferencesError::ShortcutKey(key));
        }
        Ok(Self {
            command,
            shift,
            alt,
            key: key.to_ascii_lowercase(),
        })
    }

    pub const fn command(self) -> bool {
        self.command
    }

    pub const fn shift(self) -> bool {
        self.shift
    }

    pub const fn alt(self) -> bool {
        self.alt
    }

    pub const fn key(self) -> char {
        self.key
    }
}

impl TryFrom<DesktopPreferencesChordWire> for DesktopPreferencesChord {
    type Error = DesktopPreferencesError;

    fn try_from(value: DesktopPreferencesChordWire) -> Result<Self, Self::Error> {
        Self::new(value.command, value.shift, value.alt, value.key)
    }
}

impl From<DesktopPreferencesChord> for DesktopPreferencesChordWire {
    fn from(value: DesktopPreferencesChord) -> Self {
        Self {
            command: value.command,
            shift: value.shift,
            alt: value.alt,
            key: value.key,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopPreferencesAudioCategory {
    enabled: bool,
    volume: DesktopPreferencesPercent,
}

impl DesktopPreferencesAudioCategory {
    pub const DEFAULT: Self = Self {
        enabled: true,
        volume: DesktopPreferencesPercent::FULL,
    };

    pub const fn enabled(self) -> bool {
        self.enabled
    }

    pub const fn volume(self) -> DesktopPreferencesPercent {
        self.volume
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopPreferencesEnumPatch {
    Theme(DesktopPreferencesEnumTheme),
    Motion(DesktopPreferencesEnumMotion),
    Notation(DesktopPreferencesEnumNotation),
    Aid(DesktopPreferencesEnumAid, bool),
    MasterVolume(DesktopPreferencesPercent),
    CategoryEnabled(DesktopPreferencesEnumAudioCategory, bool),
    CategoryVolume(
        DesktopPreferencesEnumAudioCategory,
        DesktopPreferencesPercent,
    ),
    Shortcut(
        DesktopPreferencesEnumShortcutAction,
        Option<DesktopPreferencesChord>,
    ),
    EngineThreads(Option<u32>),
    EngineHash(Option<u32>),
    EngineMoveOverhead(Option<u32>),
}

impl DesktopPreferencesEnumPatch {
    pub const MAX_ENGINE_THREADS: u32 = 512;
    pub const MAX_ENGINE_HASH_MIB: u32 = 262_144;
    pub const MAX_ENGINE_OVERHEAD_MS: u32 = 2_000;

    fn bounded(
        name: &'static str,
        value: Option<u32>,
        minimum: u32,
        maximum: u32,
    ) -> Result<Option<u32>, DesktopPreferencesError> {
        match value {
            Some(found) if found < minimum || found > maximum => {
                Err(DesktopPreferencesError::EngineValue {
                    name,
                    value: found,
                    minimum,
                    maximum,
                })
            }
            other => Ok(other),
        }
    }

    pub fn engine_threads(value: Option<u32>) -> Result<Self, DesktopPreferencesError> {
        Self::bounded("threads", value, 1, Self::MAX_ENGINE_THREADS).map(Self::EngineThreads)
    }

    pub fn engine_hash(value: Option<u32>) -> Result<Self, DesktopPreferencesError> {
        Self::bounded("hash", value, 1, Self::MAX_ENGINE_HASH_MIB).map(Self::EngineHash)
    }

    pub fn engine_move_overhead(value: Option<u32>) -> Result<Self, DesktopPreferencesError> {
        Self::bounded("move overhead", value, 0, Self::MAX_ENGINE_OVERHEAD_MS)
            .map(Self::EngineMoveOverhead)
    }

    pub const fn section(self) -> DesktopPreferencesEnumSection {
        match self {
            Self::Theme(_) | Self::Motion(_) | Self::Notation(_) => {
                DesktopPreferencesEnumSection::Appearance
            }
            Self::Aid(..) => DesktopPreferencesEnumSection::BoardAids,
            Self::MasterVolume(_) | Self::CategoryEnabled(..) | Self::CategoryVolume(..) => {
                DesktopPreferencesEnumSection::Audio
            }
            Self::Shortcut(..) => DesktopPreferencesEnumSection::Shortcuts,
            Self::EngineThreads(_) | Self::EngineHash(_) | Self::EngineMoveOverhead(_) => {
                DesktopPreferencesEnumSection::Engine
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopPreferences {
    theme: DesktopPreferencesEnumTheme,
    motion: DesktopPreferencesEnumMotion,
    notation: DesktopPreferencesEnumNotation,
    aids: [bool; 4],
    master: DesktopPreferencesPercent,
    categories: [DesktopPreferencesAudioCategory; 4],
    shortcuts: [Option<DesktopPreferencesChord>; 4],
    engine: [Option<u32>; 3],
}

impl Default for DesktopPreferences {
    fn default() -> Self {
        Self::new_default()
    }
}

impl DesktopPreferences {
    pub const fn new_default() -> Self {
        Self {
            theme: DesktopPreferencesEnumTheme::Dark,
            motion: DesktopPreferencesEnumMotion::Full,
            notation: DesktopPreferencesEnumNotation::Letters,
            aids: [true; 4],
            master: DesktopPreferencesPercent::DEFAULT_MASTER,
            categories: [DesktopPreferencesAudioCategory::DEFAULT; 4],
            shortcuts: [None; 4],
            engine: [None; 3],
        }
    }
}

impl DesktopPreferences {
    pub const fn motion(self) -> DesktopPreferencesEnumMotion {
        self.motion
    }

    pub const fn notation(self) -> DesktopPreferencesEnumNotation {
        self.notation
    }

    pub fn set_motion(&mut self, motion: DesktopPreferencesEnumMotion) -> bool {
        self.apply(DesktopPreferencesEnumPatch::Motion(motion))
    }

    pub const fn theme(self) -> DesktopPreferencesEnumTheme {
        self.theme
    }

    pub fn set_theme(&mut self, theme: DesktopPreferencesEnumTheme) -> bool {
        self.apply(DesktopPreferencesEnumPatch::Theme(theme))
    }

    pub const fn aid(self, aid: DesktopPreferencesEnumAid) -> bool {
        self.aids[aid.index()]
    }

    pub const fn master_volume(self) -> DesktopPreferencesPercent {
        self.master
    }

    pub const fn category(
        self,
        category: DesktopPreferencesEnumAudioCategory,
    ) -> DesktopPreferencesAudioCategory {
        self.categories[category.index()]
    }

    pub fn effective_gain(self, category: DesktopPreferencesEnumAudioCategory) -> f32 {
        let entry = self.category(category);
        if entry.enabled {
            self.master.fraction() * entry.volume.fraction()
        } else {
            0.0
        }
    }

    pub const fn shortcut(
        self,
        action: DesktopPreferencesEnumShortcutAction,
    ) -> Option<DesktopPreferencesChord> {
        self.shortcuts[action.index()]
    }

    pub const fn engine_threads(self) -> Option<u32> {
        self.engine[0]
    }

    pub const fn engine_hash_mib(self) -> Option<u32> {
        self.engine[1]
    }

    pub const fn engine_move_overhead_ms(self) -> Option<u32> {
        self.engine[2]
    }

    pub fn apply(&mut self, patch: DesktopPreferencesEnumPatch) -> bool {
        let before = *self;
        match patch {
            DesktopPreferencesEnumPatch::Theme(theme) => self.theme = theme,
            DesktopPreferencesEnumPatch::Motion(motion) => self.motion = motion,
            DesktopPreferencesEnumPatch::Notation(notation) => self.notation = notation,
            DesktopPreferencesEnumPatch::Aid(aid, enabled) => self.aids[aid.index()] = enabled,
            DesktopPreferencesEnumPatch::MasterVolume(volume) => self.master = volume,
            DesktopPreferencesEnumPatch::CategoryEnabled(category, enabled) => {
                self.categories[category.index()].enabled = enabled;
            }
            DesktopPreferencesEnumPatch::CategoryVolume(category, volume) => {
                self.categories[category.index()].volume = volume;
            }
            DesktopPreferencesEnumPatch::Shortcut(action, chord) => {
                self.shortcuts[action.index()] = chord;
            }
            DesktopPreferencesEnumPatch::EngineThreads(value) => self.engine[0] = value,
            DesktopPreferencesEnumPatch::EngineHash(value) => self.engine[1] = value,
            DesktopPreferencesEnumPatch::EngineMoveOverhead(value) => self.engine[2] = value,
        }
        *self != before
    }

    pub fn patches(self) -> Vec<DesktopPreferencesEnumPatch> {
        let mut patches = vec![
            DesktopPreferencesEnumPatch::Theme(self.theme),
            DesktopPreferencesEnumPatch::Motion(self.motion),
            DesktopPreferencesEnumPatch::Notation(self.notation),
        ];
        patches.extend(
            DesktopPreferencesEnumAid::iter()
                .map(|aid| DesktopPreferencesEnumPatch::Aid(aid, self.aid(aid))),
        );
        patches.push(DesktopPreferencesEnumPatch::MasterVolume(self.master));
        for category in DesktopPreferencesEnumAudioCategory::iter() {
            let entry = self.category(category);
            patches.push(DesktopPreferencesEnumPatch::CategoryEnabled(
                category,
                entry.enabled,
            ));
            patches.push(DesktopPreferencesEnumPatch::CategoryVolume(
                category,
                entry.volume,
            ));
        }
        patches.extend(
            DesktopPreferencesEnumShortcutAction::iter()
                .map(|action| DesktopPreferencesEnumPatch::Shortcut(action, self.shortcut(action))),
        );
        patches.push(DesktopPreferencesEnumPatch::EngineThreads(self.engine[0]));
        patches.push(DesktopPreferencesEnumPatch::EngineHash(self.engine[1]));
        patches.push(DesktopPreferencesEnumPatch::EngineMoveOverhead(self.engine[2]));
        patches
    }

    pub fn section_patches(
        self,
        section: DesktopPreferencesEnumSection,
    ) -> Vec<DesktopPreferencesEnumPatch> {
        self.patches()
            .into_iter()
            .filter(|patch| patch.section() == section)
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DesktopPreferencesRevision(u64);

impl DesktopPreferencesRevision {
    pub const INITIAL: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Result<Self, DesktopPreferencesError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(DesktopPreferencesError::StaleRevision)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopPreferencesStatus {
    saved_revision: DesktopPreferencesRevision,
    pending: bool,
}

impl DesktopPreferencesStatus {
    pub const fn saved_revision(self) -> DesktopPreferencesRevision {
        self.saved_revision
    }

    pub const fn has_pending_changes(self) -> bool {
        self.pending
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopPreferencesDraft {
    saved: DesktopPreferences,
    revision: DesktopPreferencesRevision,
    pending: DesktopPreferences,
}

impl DesktopPreferencesDraft {
    pub const fn new(saved: DesktopPreferences, revision: DesktopPreferencesRevision) -> Self {
        Self {
            saved,
            revision,
            pending: saved,
        }
    }

    pub const fn saved(self) -> DesktopPreferences {
        self.saved
    }

    pub const fn pending(self) -> DesktopPreferences {
        self.pending
    }

    pub fn status(self) -> DesktopPreferencesStatus {
        DesktopPreferencesStatus {
            saved_revision: self.revision,
            pending: self.pending != self.saved,
        }
    }

    pub fn edit(&mut self, patch: DesktopPreferencesEnumPatch) -> bool {
        self.pending.apply(patch)
    }

    pub fn discard(&mut self) {
        self.pending = self.saved;
    }

    pub fn commit(
        &mut self,
        saved_revision: DesktopPreferencesRevision,
    ) -> Result<(), DesktopPreferencesError> {
        if saved_revision <= self.revision {
            return Err(DesktopPreferencesError::StaleRevision);
        }
        self.saved = self.pending;
        self.revision = saved_revision;
        Ok(())
    }
}
