use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use crate::desktop::preferences::{
    DesktopPreferences, DesktopPreferencesChord, DesktopPreferencesDraft,
    DesktopPreferencesEnumAid, DesktopPreferencesEnumAudioCategory, DesktopPreferencesEnumMotion,
    DesktopPreferencesEnumNotation, DesktopPreferencesEnumPatch,
    DesktopPreferencesEnumShortcutAction, DesktopPreferencesEnumTheme, DesktopPreferencesError,
    DesktopPreferencesPercent,
};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DesktopPreferencesAppearanceWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    theme: Option<DesktopPreferencesEnumTheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    motion: Option<DesktopPreferencesEnumMotion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    notation: Option<DesktopPreferencesEnumNotation>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DesktopPreferencesAidWire {
    aid: DesktopPreferencesEnumAid,
    enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DesktopPreferencesCategoryWire {
    category: DesktopPreferencesEnumAudioCategory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    volume: Option<DesktopPreferencesPercent>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DesktopPreferencesAudioWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    master: Option<DesktopPreferencesPercent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    categories: Vec<DesktopPreferencesCategoryWire>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DesktopPreferencesShortcutWire {
    action: DesktopPreferencesEnumShortcutAction,
    chord: Option<DesktopPreferencesChord>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DesktopPreferencesEngineWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    threads: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hash_mib: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    move_overhead_ms: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DesktopPreferencesDocument {
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    appearance: Option<DesktopPreferencesAppearanceWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    board_aids: Option<Vec<DesktopPreferencesAidWire>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    audio: Option<DesktopPreferencesAudioWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shortcuts: Option<Vec<DesktopPreferencesShortcutWire>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    engine: Option<DesktopPreferencesEngineWire>,
}

#[derive(Deserialize)]
struct DesktopPreferencesVersionProbe {
    version: u32,
}

pub struct DesktopPreferencesCodec;

impl DesktopPreferencesCodec {
    pub const CURRENT_VERSION: u32 = 1;
    pub const MAX_BYTES: usize = 1024 * 1024;

    pub fn encode(preferences: DesktopPreferences) -> Result<String, DesktopPreferencesError> {
        let mut document = DesktopPreferencesDocument {
            version: Self::CURRENT_VERSION,
            appearance: Some(DesktopPreferencesAppearanceWire {
                theme: Some(preferences.theme()),
                motion: Some(preferences.motion()),
                notation: Some(preferences.notation()),
            }),
            board_aids: Some(
                DesktopPreferencesEnumAid::iter()
                    .map(|aid| DesktopPreferencesAidWire {
                        aid,
                        enabled: preferences.aid(aid),
                    })
                    .collect(),
            ),
            audio: Some(DesktopPreferencesAudioWire {
                master: Some(preferences.master_volume()),
                categories: DesktopPreferencesEnumAudioCategory::iter()
                    .map(|category| {
                        let entry = preferences.category(category);
                        DesktopPreferencesCategoryWire {
                            category,
                            enabled: Some(entry.enabled()),
                            volume: Some(entry.volume()),
                        }
                    })
                    .collect(),
            }),
            shortcuts: Some(
                DesktopPreferencesEnumShortcutAction::iter()
                    .map(|action| DesktopPreferencesShortcutWire {
                        action,
                        chord: preferences.shortcut(action),
                    })
                    .collect(),
            ),
            engine: None,
        };
        if preferences.engine_threads().is_some()
            || preferences.engine_hash_mib().is_some()
            || preferences.engine_move_overhead_ms().is_some()
        {
            document.engine = Some(DesktopPreferencesEngineWire {
                threads: preferences.engine_threads(),
                hash_mib: preferences.engine_hash_mib(),
                move_overhead_ms: preferences.engine_move_overhead_ms(),
            });
        }
        if document
            .shortcuts
            .as_ref()
            .is_some_and(|shortcuts| shortcuts.iter().all(|entry| entry.chord.is_none()))
        {
            document.shortcuts = None;
        }
        let text = serde_json::to_string(&document)
            .map_err(|error| DesktopPreferencesError::Document(error.to_string()))?;
        if text.len() > Self::MAX_BYTES {
            return Err(DesktopPreferencesError::TooLarge(Self::MAX_BYTES));
        }
        Ok(text)
    }

    pub fn decode(
        bytes: &[u8],
    ) -> Result<Vec<DesktopPreferencesEnumPatch>, DesktopPreferencesError> {
        if bytes.len() > Self::MAX_BYTES {
            return Err(DesktopPreferencesError::TooLarge(Self::MAX_BYTES));
        }
        let probe: DesktopPreferencesVersionProbe = serde_json::from_slice(bytes)
            .map_err(|error| DesktopPreferencesError::Document(error.to_string()))?;
        if probe.version != Self::CURRENT_VERSION {
            return Err(DesktopPreferencesError::Version(probe.version));
        }
        let document: DesktopPreferencesDocument = serde_json::from_slice(bytes)
            .map_err(|error| DesktopPreferencesError::Document(error.to_string()))?;
        let mut patches = Vec::new();
        if let Some(appearance) = document.appearance {
            patches.extend(appearance.theme.map(DesktopPreferencesEnumPatch::Theme));
            patches.extend(appearance.motion.map(DesktopPreferencesEnumPatch::Motion));
            patches.extend(
                appearance
                    .notation
                    .map(DesktopPreferencesEnumPatch::Notation),
            );
        }
        if let Some(aids) = document.board_aids {
            let mut seen = Vec::new();
            for entry in aids {
                if seen.contains(&entry.aid) {
                    return Err(DesktopPreferencesError::Duplicate);
                }
                seen.push(entry.aid);
                patches.push(DesktopPreferencesEnumPatch::Aid(entry.aid, entry.enabled));
            }
        }
        if let Some(audio) = document.audio {
            patches.extend(audio.master.map(DesktopPreferencesEnumPatch::MasterVolume));
            let mut seen = Vec::new();
            for entry in audio.categories {
                if seen.contains(&entry.category) {
                    return Err(DesktopPreferencesError::Duplicate);
                }
                seen.push(entry.category);
                patches.extend(entry.enabled.map(|enabled| {
                    DesktopPreferencesEnumPatch::CategoryEnabled(entry.category, enabled)
                }));
                patches.extend(entry.volume.map(|volume| {
                    DesktopPreferencesEnumPatch::CategoryVolume(entry.category, volume)
                }));
            }
        }
        if let Some(shortcuts) = document.shortcuts {
            let mut seen = Vec::new();
            for entry in shortcuts {
                if seen.contains(&entry.action) {
                    return Err(DesktopPreferencesError::Duplicate);
                }
                seen.push(entry.action);
                patches.push(DesktopPreferencesEnumPatch::Shortcut(
                    entry.action,
                    entry.chord,
                ));
            }
        }
        if let Some(engine) = document.engine {
            if engine.threads.is_some() {
                patches.push(DesktopPreferencesEnumPatch::engine_threads(engine.threads)?);
            }
            if engine.hash_mib.is_some() {
                patches.push(DesktopPreferencesEnumPatch::engine_hash(engine.hash_mib)?);
            }
            if engine.move_overhead_ms.is_some() {
                patches.push(DesktopPreferencesEnumPatch::engine_move_overhead(
                    engine.move_overhead_ms,
                )?);
            }
        }
        Ok(patches)
    }

    pub fn load(bytes: &[u8]) -> Result<DesktopPreferences, DesktopPreferencesError> {
        let mut preferences = DesktopPreferences::default();
        for patch in Self::decode(bytes)? {
            preferences.apply(patch);
        }
        Ok(preferences)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPreferencesImportPreview {
    base: DesktopPreferences,
    changes: Vec<DesktopPreferencesEnumPatch>,
}

impl DesktopPreferencesImportPreview {
    pub fn new(current: DesktopPreferences, bytes: &[u8]) -> Result<Self, DesktopPreferencesError> {
        let mut scratch = current;
        let changes = DesktopPreferencesCodec::decode(bytes)?
            .into_iter()
            .filter(|patch| scratch.apply(*patch))
            .collect();
        Ok(Self {
            base: current,
            changes,
        })
    }

    pub fn changes(&self) -> &[DesktopPreferencesEnumPatch] {
        &self.changes
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn confirm(self) -> DesktopPreferencesConfirmedImport {
        DesktopPreferencesConfirmedImport(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPreferencesConfirmedImport(DesktopPreferencesImportPreview);

impl DesktopPreferencesConfirmedImport {
    pub fn apply(
        self,
        draft: &mut DesktopPreferencesDraft,
    ) -> Result<usize, DesktopPreferencesError> {
        if draft.pending() != self.0.base {
            return Err(DesktopPreferencesError::StaleRevision);
        }
        let mut applied = 0;
        for patch in self.0.changes {
            if draft.edit(patch) {
                applied += 1;
            }
        }
        Ok(applied)
    }
}
