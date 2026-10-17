use chess::desktop::focus::{DesktopFocus, DesktopFocusEnumChange, DesktopFocusEnumWindow};
use chess::desktop::preferences::DesktopPreferencesEnumShortcutAction;
use chess::presentation::action::shortcuts::{
    PresentationShortcutEnumPlatform, PresentationShortcuts,
};

#[test]
fn reserved_keys_are_platform_specific() {
    let quit = chess::desktop::preferences::DesktopPreferencesChord::new(true, false, false, 'q');
    let quit =
        quit.unwrap_or_else(|_error| DesktopPreferencesEnumShortcutAction::Palette.default_chord());
    assert!(PresentationShortcuts::is_reserved(
        PresentationShortcutEnumPlatform::Mac,
        quit
    ));
    assert!(!PresentationShortcuts::is_reserved(
        PresentationShortcutEnumPlatform::Other,
        quit
    ));
}

#[test]
fn hiding_the_puzzle_window_keeps_the_app_focused_while_main_has_focus() {
    let mut focus = DesktopFocus::default();
    assert_eq!(
        focus.window_changed(DesktopFocusEnumWindow::Main, true),
        Some(DesktopFocusEnumChange::AppGained)
    );
    assert_eq!(
        focus.window_changed(DesktopFocusEnumWindow::Puzzle, true),
        None
    );
    assert!(!focus.puzzle_paused_by_focus());
    assert_eq!(
        focus.window_changed(DesktopFocusEnumWindow::Puzzle, false),
        None
    );
    assert!(focus.app_focused());
    assert!(focus.puzzle_paused_by_focus());
    assert_eq!(
        focus.window_changed(DesktopFocusEnumWindow::Main, false),
        Some(DesktopFocusEnumChange::AppLost)
    );
}
