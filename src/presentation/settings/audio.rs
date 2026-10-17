use crate::desktop::preferences::{
    DesktopPreferences, DesktopPreferencesDraft, DesktopPreferencesEnumAudioCategory,
    DesktopPreferencesEnumPatch, DesktopPreferencesError, DesktopPreferencesPercent,
};

pub struct PresentationSettingsAudio;

impl PresentationSettingsAudio {
    pub fn set_master(
        draft: &mut DesktopPreferencesDraft,
        percent: u8,
    ) -> Result<bool, DesktopPreferencesError> {
        let volume = DesktopPreferencesPercent::new(percent)?;
        Ok(draft.edit(DesktopPreferencesEnumPatch::MasterVolume(volume)))
    }

    pub fn set_category_volume(
        draft: &mut DesktopPreferencesDraft,
        category: DesktopPreferencesEnumAudioCategory,
        percent: u8,
    ) -> Result<bool, DesktopPreferencesError> {
        let volume = DesktopPreferencesPercent::new(percent)?;
        Ok(draft.edit(DesktopPreferencesEnumPatch::CategoryVolume(
            category, volume,
        )))
    }

    pub fn set_category_enabled(
        draft: &mut DesktopPreferencesDraft,
        category: DesktopPreferencesEnumAudioCategory,
        enabled: bool,
    ) -> bool {
        draft.edit(DesktopPreferencesEnumPatch::CategoryEnabled(
            category, enabled,
        ))
    }

    pub fn gain(
        preferences: DesktopPreferences,
        category: DesktopPreferencesEnumAudioCategory,
    ) -> f32 {
        preferences.effective_gain(category)
    }
}
