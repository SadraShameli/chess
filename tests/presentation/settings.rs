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
    for action in DesktopPreferencesEnumShortcutAction::ALL {
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
