use std::error::Error;

use chess::desktop::preferences::{
    DesktopPreferences, DesktopPreferencesChord, DesktopPreferencesDraft,
    DesktopPreferencesEnumAid, DesktopPreferencesEnumAudioCategory, DesktopPreferencesEnumMotion,
    DesktopPreferencesEnumPatch, DesktopPreferencesEnumSection,
    DesktopPreferencesEnumShortcutAction, DesktopPreferencesEnumTheme, DesktopPreferencesError,
    DesktopPreferencesPercent, DesktopPreferencesRevision,
};
use chess::desktop::preferences_import::{
    DesktopPreferencesCodec, DesktopPreferencesImportPreview,
};
use chess::desktop::preferences_reset::{
    DesktopPreferencesEnumResetScope, DesktopPreferencesResetPreview,
};

type DesktopPreferencesTestResult<T = ()> = Result<T, Box<dyn Error>>;

fn percent(value: u8) -> Result<DesktopPreferencesPercent, DesktopPreferencesError> {
    DesktopPreferencesPercent::new(value)
}

#[test]
fn defaults_use_seventy_percent_master_and_full_categories_multiplied() {
    let preferences = DesktopPreferences::default();
    assert_eq!(preferences.master_volume().value(), 70);
    for category in DesktopPreferencesEnumAudioCategory::ALL {
        assert_eq!(preferences.category(category).volume().value(), 100);
        assert!(preferences.category(category).enabled());
        assert!((preferences.effective_gain(category) - 0.7).abs() < f32::EPSILON);
    }
    let mut halved = preferences;
    halved.apply(DesktopPreferencesEnumPatch::CategoryVolume(
        DesktopPreferencesEnumAudioCategory::Move,
        DesktopPreferencesPercent::new(50).unwrap_or(DesktopPreferencesPercent::FULL),
    ));
    assert!((halved.effective_gain(DesktopPreferencesEnumAudioCategory::Move) - 0.35).abs() < 1e-6);
}

#[test]
fn volumes_outside_zero_to_one_hundred_are_rejected() {
    assert_eq!(
        DesktopPreferencesPercent::new(101),
        Err(DesktopPreferencesError::Percent(101))
    );
    assert!(DesktopPreferencesPercent::new(0).is_ok());
}

#[test]
fn immediate_pending_and_saved_state_stay_distinct() -> DesktopPreferencesTestResult {
    let mut draft = DesktopPreferencesDraft::new(
        DesktopPreferences::default(),
        DesktopPreferencesRevision::INITIAL,
    );
    assert!(!draft.status().has_pending_changes());
    assert!(draft.edit(DesktopPreferencesEnumPatch::Theme(
        DesktopPreferencesEnumTheme::Light
    )));
    assert_eq!(draft.pending().theme(), DesktopPreferencesEnumTheme::Light);
    assert_eq!(draft.saved().theme(), DesktopPreferencesEnumTheme::Dark);
    assert!(draft.status().has_pending_changes());
    draft.discard();
    assert!(!draft.status().has_pending_changes());
    draft.edit(DesktopPreferencesEnumPatch::Motion(
        DesktopPreferencesEnumMotion::Reduced,
    ));
    assert!(draft.commit(DesktopPreferencesRevision::INITIAL).is_err());
    draft.commit(DesktopPreferencesRevision::new(1))?;
    assert_eq!(
        draft.saved().motion(),
        DesktopPreferencesEnumMotion::Reduced
    );
    assert_eq!(draft.status().saved_revision().value(), 1);
    Ok(())
}

#[test]
fn export_round_trips_and_excludes_machine_paths() -> DesktopPreferencesTestResult {
    let mut preferences = DesktopPreferences::default();
    preferences.apply(DesktopPreferencesEnumPatch::Aid(
        DesktopPreferencesEnumAid::LegalMoves,
        false,
    ));
    preferences.apply(DesktopPreferencesEnumPatch::MasterVolume(percent(35)?));
    preferences.apply(DesktopPreferencesEnumPatch::Shortcut(
        DesktopPreferencesEnumShortcutAction::FlipBoard,
        Some(DesktopPreferencesChord::new(true, false, false, 'b')?),
    ));
    let text = DesktopPreferencesCodec::encode(preferences)?;
    assert!(!text.contains("path"));
    assert!(!text.contains('/') && !text.contains('\\'));
    assert_eq!(DesktopPreferencesCodec::load(text.as_bytes())?, preferences);
    Ok(())
}

#[test]
fn unsupported_older_newer_oversized_and_malformed_documents_are_rejected() {
    assert_eq!(
        DesktopPreferencesCodec::load(br#"{"version":0}"#),
        Err(DesktopPreferencesError::Version(0))
    );
    assert_eq!(
        DesktopPreferencesCodec::load(br#"{"version":2,"future":true}"#),
        Err(DesktopPreferencesError::Version(2))
    );
    let oversized = vec![b' '; DesktopPreferencesCodec::MAX_BYTES + 1];
    assert!(matches!(
        DesktopPreferencesCodec::load(&oversized),
        Err(DesktopPreferencesError::TooLarge(_))
    ));
    assert!(DesktopPreferencesCodec::load(br#"{"version":1,"audio":{"master":101}}"#).is_err());
    assert!(DesktopPreferencesCodec::load(br#"{"version":1,"unknown":1}"#).is_err());
    assert!(
        DesktopPreferencesCodec::load(
            br#"{"version":1,"board_aids":[{"aid":"coordinates","enabled":true},{"aid":"coordinates","enabled":false}]}"#
        )
        .is_err()
    );
}

#[test]
fn import_merges_only_included_fields_after_confirmation() -> DesktopPreferencesTestResult {
    let mut draft = DesktopPreferencesDraft::new(
        DesktopPreferences::default(),
        DesktopPreferencesRevision::INITIAL,
    );
    draft.edit(DesktopPreferencesEnumPatch::Theme(
        DesktopPreferencesEnumTheme::Light,
    ));
    let preview = DesktopPreferencesImportPreview::new(
        draft.pending(),
        br#"{"version":1,"audio":{"master":30}}"#,
    )?;
    assert_eq!(preview.changes().len(), 1);
    let applied = preview.confirm().apply(&mut draft)?;
    assert_eq!(applied, 1);
    assert_eq!(draft.pending().master_volume().value(), 30);
    assert_eq!(draft.pending().theme(), DesktopPreferencesEnumTheme::Light);
    Ok(())
}

#[test]
fn a_stale_import_preview_changes_nothing() -> DesktopPreferencesTestResult {
    let mut draft = DesktopPreferencesDraft::new(
        DesktopPreferences::default(),
        DesktopPreferencesRevision::INITIAL,
    );
    let preview = DesktopPreferencesImportPreview::new(
        draft.pending(),
        br#"{"version":1,"appearance":{"theme":"light"}}"#,
    )?;
    draft.edit(DesktopPreferencesEnumPatch::Motion(
        DesktopPreferencesEnumMotion::Reduced,
    ));
    let before = draft.pending();
    assert_eq!(
        preview.confirm().apply(&mut draft),
        Err(DesktopPreferencesError::StaleRevision)
    );
    assert_eq!(draft.pending(), before);
    Ok(())
}

#[test]
fn section_and_all_reset_restore_defaults_without_touching_other_sections() {
    let mut draft = DesktopPreferences::default();
    draft.apply(DesktopPreferencesEnumPatch::Theme(
        DesktopPreferencesEnumTheme::Light,
    ));
    draft.apply(DesktopPreferencesEnumPatch::Aid(
        DesktopPreferencesEnumAid::Coordinates,
        false,
    ));
    let mut state = DesktopPreferencesDraft::new(draft, DesktopPreferencesRevision::new(4));
    state.edit(DesktopPreferencesEnumPatch::Aid(
        DesktopPreferencesEnumAid::Coordinates,
        false,
    ));
    let preview = DesktopPreferencesResetPreview::new(
        state.pending(),
        DesktopPreferencesEnumResetScope::Section(DesktopPreferencesEnumSection::BoardAids),
    );
    assert_eq!(preview.changes().len(), 1);
    let status = preview.confirm().apply(&mut state);
    assert!(status.is_some_and(|status| status.has_pending_changes()));
    assert!(state.pending().aid(DesktopPreferencesEnumAid::Coordinates));
    assert_eq!(state.pending().theme(), DesktopPreferencesEnumTheme::Light);
    let all =
        DesktopPreferencesResetPreview::new(state.pending(), DesktopPreferencesEnumResetScope::All);
    all.confirm().apply(&mut state);
    assert_eq!(state.pending(), DesktopPreferences::default());
    assert_eq!(state.saved().theme(), DesktopPreferencesEnumTheme::Light);
}
