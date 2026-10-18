use thiserror::Error;

use crate::desktop::preferences::{
    DesktopPreferencesChord, DesktopPreferencesDraft, DesktopPreferencesEnumPatch,
    DesktopPreferencesEnumShortcutAction,
};
use crate::presentation::action::shortcuts::{
    PresentationShortcutEnumPlatform, PresentationShortcuts,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PresentationSettingsShortcutError {
    #[error("The shortcut is reserved by the operating system")]
    Reserved,
    #[error("The shortcut is already used by {0:?}")]
    Conflict(DesktopPreferencesEnumShortcutAction),
    #[error("The shortcut is already used for {0} in a focused context")]
    ContextConflict(&'static str),
}

pub struct PresentationSettingsShortcuts;

impl PresentationSettingsShortcuts {
    pub fn assign(
        draft: &mut DesktopPreferencesDraft,
        platform: PresentationShortcutEnumPlatform,
        action: DesktopPreferencesEnumShortcutAction,
        chord: DesktopPreferencesChord,
    ) -> Result<bool, PresentationSettingsShortcutError> {
        if PresentationShortcuts::is_reserved(platform, chord) {
            return Err(PresentationSettingsShortcutError::Reserved);
        }
        if let Some(other) = PresentationShortcuts::conflict(draft.pending(), action, chord) {
            return Err(PresentationSettingsShortcutError::Conflict(other));
        }
        if let Some(binding) = PresentationShortcuts::context_conflict(chord) {
            return Err(PresentationSettingsShortcutError::ContextConflict(
                binding.action(),
            ));
        }
        let stored = (chord != action.default_chord()).then_some(chord);
        Ok(draft.edit(DesktopPreferencesEnumPatch::Shortcut(action, stored)))
    }

    pub fn reset(
        draft: &mut DesktopPreferencesDraft,
        action: DesktopPreferencesEnumShortcutAction,
    ) -> bool {
        draft.edit(DesktopPreferencesEnumPatch::Shortcut(action, None))
    }
}
