use strum::IntoEnumIterator;
use chess::desktop::preferences::{
    DesktopPreferences, DesktopPreferencesChord, DesktopPreferencesDraft,
    DesktopPreferencesEnumAudioCategory, DesktopPreferencesEnumShortcutAction,
    DesktopPreferencesRevision,
};
use chess::presentation::action::palette::PresentationActionPalette;
use chess::presentation::action::registry::{
    PresentationActionContext, PresentationActionEnumAvailability, PresentationActionEnumWorkspace,
    PresentationActionRegistry,
};
use chess::presentation::action::shortcuts::{
    PresentationShortcutEnumContext, PresentationShortcutEnumPlatform, PresentationShortcuts,
};
use chess::presentation::settings::audio::PresentationSettingsAudio;
use chess::presentation::settings::shortcuts::{
    PresentationSettingsShortcutError, PresentationSettingsShortcuts,
};

fn draft() -> DesktopPreferencesDraft {
    DesktopPreferencesDraft::new(
        DesktopPreferences::default(),
        DesktopPreferencesRevision::INITIAL,
    )
}

fn chord(command: bool, key: char) -> DesktopPreferencesChord {
    DesktopPreferencesChord::new(command, false, false, key)
        .unwrap_or_else(|_error| DesktopPreferencesEnumShortcutAction::Palette.default_chord())
}

#[test]
fn palette_derives_availability_and_confirmation_from_the_registry() {
    let hidden = PresentationActionContext {
        board_visible: false,
        settings_open: false,
    };
    let entries = PresentationActionPalette::entries("", hidden);
    assert_eq!(
        entries.len(),
        PresentationActionRegistry::workspace_actions().len()
    );
    let flip = entries
        .iter()
        .find(|entry| entry.action() == PresentationActionEnumWorkspace::FlipBoard);
    assert!(flip.is_some_and(|entry| matches!(
        entry.availability(),
        PresentationActionEnumAvailability::Unavailable(_)
    )));
    assert!(entries.iter().any(
        |entry| entry.action() == PresentationActionEnumWorkspace::Quit
            && entry.requires_confirmation()
    ));
    assert_eq!(
        PresentationActionPalette::entries("settings", hidden).len(),
        1
    );
    assert!(
        PresentationActionPalette::activate(PresentationActionEnumWorkspace::FlipBoard, hidden)
            .is_err()
    );
}

#[test]
fn every_shortcut_action_belongs_to_one_registered_action() {
    for action in DesktopPreferencesEnumShortcutAction::iter() {
        let workspace = PresentationActionRegistry::workspace_for(action);
        assert!(workspace.is_some_and(|found| found.shortcut_action() == Some(action)));
    }
}

#[test]
fn default_chords_use_command_modifier_and_plain_flip() {
    let preferences = DesktopPreferences::default();
    let mac = PresentationShortcutEnumPlatform::Mac;
    let other = PresentationShortcutEnumPlatform::Other;
    let label = |platform, action| {
        PresentationShortcuts::label(platform, PresentationShortcuts::chord(preferences, action))
    };
    assert_eq!(
        label(mac, DesktopPreferencesEnumShortcutAction::Palette),
        "Cmd+K"
    );
    assert_eq!(
        label(other, DesktopPreferencesEnumShortcutAction::NewGame),
        "Ctrl+N"
    );
    assert_eq!(
        label(other, DesktopPreferencesEnumShortcutAction::Settings),
        "Ctrl+,"
    );
    assert_eq!(
        label(mac, DesktopPreferencesEnumShortcutAction::FlipBoard),
        "F"
    );
}

#[test]
fn flip_is_ignored_in_text_inputs_but_command_shortcuts_remain() {
    let preferences = DesktopPreferences::default();
    let flip = DesktopPreferencesEnumShortcutAction::FlipBoard.default_chord();
    let palette = DesktopPreferencesEnumShortcutAction::Palette.default_chord();
    assert_eq!(
        PresentationShortcuts::resolve(preferences, PresentationShortcutEnumContext::Global, flip),
        Some(DesktopPreferencesEnumShortcutAction::FlipBoard)
    );
    assert_eq!(
        PresentationShortcuts::resolve(
            preferences,
            PresentationShortcutEnumContext::TextInput,
            flip
        ),
        None
    );
    assert_eq!(
        PresentationShortcuts::resolve(
            preferences,
            PresentationShortcutEnumContext::TextInput,
            palette
        ),
        Some(DesktopPreferencesEnumShortcutAction::Palette)
    );
}

#[test]
fn remapping_refuses_reserved_and_conflicting_chords_and_resets() {
    let mut draft = draft();
    let mac = PresentationShortcutEnumPlatform::Mac;
    assert_eq!(
        PresentationSettingsShortcuts::assign(
            &mut draft,
            mac,
            DesktopPreferencesEnumShortcutAction::NewGame,
            chord(true, 'q'),
        ),
        Err(PresentationSettingsShortcutError::Reserved)
    );
    assert_eq!(
        PresentationSettingsShortcuts::assign(
            &mut draft,
            mac,
            DesktopPreferencesEnumShortcutAction::NewGame,
            chord(true, 'k'),
        ),
        Err(PresentationSettingsShortcutError::Conflict(
            DesktopPreferencesEnumShortcutAction::Palette
        ))
    );
    assert_eq!(
        PresentationSettingsShortcuts::assign(
            &mut draft,
            mac,
            DesktopPreferencesEnumShortcutAction::NewGame,
            chord(true, 'j'),
        ),
        Ok(true)
    );
    assert!(PresentationSettingsShortcuts::reset(
        &mut draft,
        DesktopPreferencesEnumShortcutAction::NewGame
    ));
    assert_eq!(draft.pending(), DesktopPreferences::default());
}

#[test]
fn audio_controls_validate_and_combine_multiplicatively() {
    let mut draft = draft();
    assert!(PresentationSettingsAudio::set_master(&mut draft, 101).is_err());
    assert_eq!(
        PresentationSettingsAudio::set_master(&mut draft, 50),
        Ok(true)
    );
    assert_eq!(
        PresentationSettingsAudio::set_category_volume(
            &mut draft,
            DesktopPreferencesEnumAudioCategory::Check,
            50
        ),
        Ok(true)
    );
    let gain = PresentationSettingsAudio::gain(
        draft.pending(),
        DesktopPreferencesEnumAudioCategory::Check,
    );
    assert!((gain - 0.25).abs() < 1e-6);
    assert!(PresentationSettingsAudio::set_category_enabled(
        &mut draft,
        DesktopPreferencesEnumAudioCategory::Check,
        false
    ));
    assert_eq!(
        PresentationSettingsAudio::gain(
            draft.pending(),
            DesktopPreferencesEnumAudioCategory::Check
        ),
        0.0
    );
}

#[test]
fn the_controller_guards_leaving_with_pending_changes_and_previews_reset() {
    use chess::desktop::preferences::{DesktopPreferencesEnumSection, DesktopPreferencesEnumTheme};
    use chess::desktop::preferences_reset::DesktopPreferencesEnumResetScope;
    use chess::presentation::settings::controller::{
        PresentationSettingsController, PresentationSettingsEnumPrompt,
    };

    let mut controller = PresentationSettingsController::new(None);
    assert!(controller.request_close());
    let outcome = controller.edit(
        chess::desktop::preferences::DesktopPreferencesEnumPatch::Theme(
            DesktopPreferencesEnumTheme::Light,
        ),
    );
    assert!(outcome.appearance_changed);
    assert_eq!(
        controller.effective().theme(),
        DesktopPreferencesEnumTheme::Light
    );
    assert!(!controller.request_close());
    assert_eq!(controller.prompt(), &PresentationSettingsEnumPrompt::Leave);
    controller.stay();
    assert!(!controller.can_save());
    let outcome = controller.leave_discarding();
    assert!(outcome.close && outcome.appearance_changed);
    assert_eq!(
        controller.effective().theme(),
        DesktopPreferencesEnumTheme::Dark
    );
    controller.begin_reset(DesktopPreferencesEnumResetScope::Section(
        DesktopPreferencesEnumSection::Audio,
    ));
    assert_eq!(controller.prompt(), &PresentationSettingsEnumPrompt::None);
}

#[test]
fn the_controller_previews_an_import_before_changing_pending_values() {
    use chess::presentation::settings::controller::{
        PresentationSettingsController, PresentationSettingsEnumPrompt,
    };

    let mut controller = PresentationSettingsController::new(None);
    controller
        .exchange_mut()
        .push_str(r#"{"version":1,"audio":{"master":20}}"#);
    controller.begin_import();
    assert!(matches!(
        controller.prompt(),
        PresentationSettingsEnumPrompt::Import(_)
    ));
    assert_eq!(controller.draft().pending().master_volume().value(), 70);
    controller.confirm_prompt();
    assert_eq!(controller.draft().pending().master_volume().value(), 20);
    assert!(controller.draft().status().has_pending_changes());
}

#[test]
fn every_settings_section_is_registered_once_with_its_own_label() {
    use chess::desktop::preferences::DesktopPreferencesEnumSection;
    use strum::IntoEnumIterator;
    let sections: Vec<_> = DesktopPreferencesEnumSection::iter().collect();
    assert_eq!(
        sections,
        [
            DesktopPreferencesEnumSection::Appearance,
            DesktopPreferencesEnumSection::BoardAids,
            DesktopPreferencesEnumSection::Audio,
            DesktopPreferencesEnumSection::Shortcuts,
            DesktopPreferencesEnumSection::Engine,
        ]
    );
    let mut labels: Vec<_> = sections.iter().map(|section| section.label()).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), sections.len());
    assert!(labels.iter().all(|label| !label.is_empty()));
}

#[test]
fn saved_pending_and_effective_options_stay_distinct_until_a_newer_revision_is_committed()
-> Result<(), Box<dyn std::error::Error>> {
    use chess::desktop::preferences::{DesktopPreferencesEnumPatch, DesktopPreferencesEnumTheme};
    use chess::presentation::settings::controller::PresentationSettingsController;

    let mut draft = draft();
    let saved_theme = draft.saved().theme();
    assert_ne!(saved_theme, DesktopPreferencesEnumTheme::Light);
    assert!(draft.edit(DesktopPreferencesEnumPatch::Theme(
        DesktopPreferencesEnumTheme::Light
    )));
    assert_eq!(draft.saved().theme(), saved_theme);
    assert_eq!(draft.pending().theme(), DesktopPreferencesEnumTheme::Light);
    assert!(draft.status().has_pending_changes());
    assert_eq!(
        draft.status().saved_revision(),
        DesktopPreferencesRevision::INITIAL
    );
    assert!(draft.commit(DesktopPreferencesRevision::INITIAL).is_err());
    draft.commit(DesktopPreferencesRevision::INITIAL.next()?)?;
    assert_eq!(draft.saved().theme(), DesktopPreferencesEnumTheme::Light);
    assert!(!draft.status().has_pending_changes());

    let mut controller = PresentationSettingsController::new(None);
    let outcome = controller.edit(DesktopPreferencesEnumPatch::Theme(
        DesktopPreferencesEnumTheme::Light,
    ));
    assert!(outcome.appearance_changed);
    assert_eq!(
        controller.effective().theme(),
        DesktopPreferencesEnumTheme::Light
    );
    assert_eq!(controller.draft().saved().theme(), saved_theme);
    assert!(controller.can_save());
    Ok(())
}

#[test]
fn saving_reports_an_unavailable_store_and_keeps_the_pending_changes() {
    use chess::desktop::preferences::{DesktopPreferencesEnumPatch, DesktopPreferencesEnumTheme};
    use chess::presentation::settings::controller::PresentationSettingsController;

    let mut controller = PresentationSettingsController::new(None);
    controller.edit(DesktopPreferencesEnumPatch::Theme(
        DesktopPreferencesEnumTheme::Light,
    ));
    controller.save();
    assert_eq!(controller.notice(), Some("Saving settings is unavailable"));
    assert!(!controller.is_saving());
    assert!(controller.draft().status().has_pending_changes());
}

#[test]
fn update_credentials_stay_out_of_exported_preferences() {
    use chess::presentation::settings::controller::PresentationSettingsController;

    let mut controller = PresentationSettingsController::new(None);
    controller
        .update_credential_mut()
        .push_str("secret-update-credential");
    controller.export();
    let exported = controller.exchange_mut().clone();
    assert!(!exported.is_empty());
    assert!(!exported.contains("secret-update-credential"));
    assert!(!exported.to_lowercase().contains("credential"));
}
