use bevy_egui::egui::accesskit::{Action, ActionRequest, TreeId};

use chess::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use chess::clock::duration::ClockInstant;
use chess::desktop::layout::{
    DesktopLayout, DesktopLayoutDisplay, DesktopLayoutEnumTab, DesktopLayoutError,
};
use chess::desktop::native::accessibility::{
    DesktopNativeAccessibility, DesktopNativeAccessibilityEnumAction,
};
use chess::presentation::board::figurines::{
    PresentationBoardFigurines, PresentationBoardFigurinesEnumNotation,
};
use chess::presentation::theme::layout::PresentationThemeLayout;
use chess::presentation::workbench::status::{
    PresentationWorkbenchStatusAnnouncer, PresentationWorkbenchStatusEnumState,
};
use chess::presentation::workbench::workspace::{
    PresentationWorkbenchWorkspace, PresentationWorkbenchWorkspaceEnumPanel,
    PresentationWorkbenchWorkspaceError,
};
use chess::scene::camera::SceneCamera;

type PresentationBoardLayoutResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn the_default_window_docks_a_360_panel_beside_an_expanded_navigation()
-> PresentationBoardLayoutResult {
    let layout = PresentationWorkbenchWorkspace::layout(1440.0, 900.0, false, 360.0)?;
    assert_eq!(layout.navigation_width(), 208.0);
    assert!(!layout.is_navigation_collapsed());
    assert_eq!(
        layout.panel(),
        PresentationWorkbenchWorkspaceEnumPanel::Docked
    );
    assert_eq!(layout.panel_width(), 360.0);
    assert_eq!(layout.board_workspace_width(), 1440.0 - 208.0 - 360.0);
    assert!(layout.board_workspace_width() >= PresentationWorkbenchWorkspace::BOARD_WORKSPACE_MIN);
    Ok(())
}

#[test]
fn panel_width_is_bounded_and_a_remembered_navigation_collapse_is_kept()
-> PresentationBoardLayoutResult {
    let wide = PresentationWorkbenchWorkspace::layout(2000.0, 1000.0, false, 9_000.0)?;
    assert_eq!(wide.panel_width(), 560.0);
    let narrow = PresentationWorkbenchWorkspace::layout(2000.0, 1000.0, false, 10.0)?;
    assert_eq!(narrow.panel_width(), 300.0);
    let tight = PresentationWorkbenchWorkspace::layout(1360.0, 800.0, false, 560.0)?;
    assert!(!tight.is_navigation_collapsed());
    assert_eq!(
        tight.panel(),
        PresentationWorkbenchWorkspaceEnumPanel::Docked
    );
    assert_eq!(tight.board_workspace_width(), 1360.0 - 208.0 - 560.0);
    let remembered = PresentationWorkbenchWorkspace::layout(1360.0, 800.0, true, 560.0)?;
    assert!(remembered.is_navigation_collapsed());
    assert_eq!(remembered.navigation_width(), 64.0);
    Ok(())
}

#[test]
fn below_1360_the_panel_is_a_drawer_and_below_the_minimum_the_window_is_rejected()
-> PresentationBoardLayoutResult {
    let drawer = PresentationWorkbenchWorkspace::layout(1280.0, 800.0, false, 360.0)?;
    assert_eq!(
        drawer.panel(),
        PresentationWorkbenchWorkspaceEnumPanel::Drawer
    );
    assert!(drawer.is_navigation_collapsed());
    assert_eq!(drawer.board_workspace_width(), 1280.0 - 64.0);
    assert!(matches!(
        PresentationWorkbenchWorkspace::layout(1279.0, 800.0, false, 360.0),
        Err(PresentationWorkbenchWorkspaceError::BelowMinimum { .. })
    ));
    assert!(matches!(
        PresentationWorkbenchWorkspace::layout(1440.0, 799.0, false, 360.0),
        Err(PresentationWorkbenchWorkspaceError::BelowMinimum { .. })
    ));
    Ok(())
}

#[test]
fn the_remembered_layout_resolves_through_the_theme_and_resets_to_defaults()
-> PresentationBoardLayoutResult {
    let mut layout = DesktopLayout::default();
    assert_eq!(layout.window_size(), (1440.0, 900.0));
    layout.set_navigation_collapsed(true);
    layout.set_panel_width(9_000.0);
    layout.set_tab(DesktopLayoutEnumTab::Analysis);
    assert_eq!(layout.panel_width(), 560.0);
    let resolved = PresentationThemeLayout::resolve(&layout, 1600.0, 900.0)?;
    assert!(resolved.is_navigation_collapsed());
    layout.reset();
    assert_eq!(layout, DesktopLayout::default());
    assert_eq!(layout.tab(), DesktopLayoutEnumTab::Game);
    Ok(())
}

#[test]
fn the_remembered_camera_pose_and_layout_round_trip_through_json() -> PresentationBoardLayoutResult
{
    let mut layout = DesktopLayout::default();
    let camera = SceneCamera::initial(BoardEnumColor::White)
        .rotate_right()
        .tilt_up()
        .zoom_in();
    layout.remember_camera(camera);
    layout.set_window(10.0, 20.0, 1500.0, 950.0)?;
    let json = serde_json::to_string(&layout)?;
    let restored: DesktopLayout = serde_json::from_str(&json)?;
    assert_eq!(restored, layout);
    assert_eq!(
        restored.camera(BoardEnumColor::Black)?.pose(),
        camera.pose()
    );
    assert_eq!(
        restored.camera(BoardEnumColor::Black)?.orientation(),
        BoardEnumColor::Black
    );
    Ok(())
}

#[test]
fn changed_displays_pull_the_window_inside_or_report_a_display_that_cannot_fit()
-> PresentationBoardLayoutResult {
    let mut layout = DesktopLayout::default();
    layout.set_window(2000.0, 1200.0, 1800.0, 1100.0)?;
    layout.fit_to_display(DesktopLayoutDisplay {
        x: 0.0,
        y: 0.0,
        width: 1600.0,
        height: 1000.0,
    })?;
    assert_eq!(layout.window_size(), (1600.0, 1000.0));
    assert_eq!(layout.window_position(), (0.0, 0.0));
    let outcome = layout.fit_to_display(DesktopLayoutDisplay {
        x: 0.0,
        y: 0.0,
        width: 1024.0,
        height: 768.0,
    });
    assert!(matches!(
        outcome,
        Err(DesktopLayoutError::DisplayTooSmall { .. })
    ));
    assert!(layout.set_window(f32::NAN, 0.0, 1400.0, 900.0).is_err());
    Ok(())
}

#[test]
fn status_states_are_prefixed_with_text_and_error_announcements_bypass_throttling()
-> PresentationBoardLayoutResult {
    let loading = PresentationWorkbenchStatusEnumState::Loading("Analyzing".to_owned());
    let result = PresentationWorkbenchStatusEnumState::Result("Depth 20".to_owned());
    let error = PresentationWorkbenchStatusEnumState::Error("Engine stopped".to_owned());
    assert_eq!(loading.announcement(), "Loading: Analyzing");
    assert_eq!(
        PresentationWorkbenchStatusEnumState::Empty("No lines".to_owned()).announcement(),
        "Empty: No lines"
    );
    let mut announcer = PresentationWorkbenchStatusAnnouncer::new(
        PresentationWorkbenchStatusAnnouncer::ANALYSIS_INTERVAL,
    );
    let at = |millis| ClockInstant::from_millis(millis);
    assert!(announcer.offer(at(0)?, &loading)?.is_some());
    assert!(announcer.offer(at(100)?, &loading)?.is_none());
    assert!(announcer.offer(at(200)?, &result)?.is_none(), "throttled");
    assert!(
        announcer.offer(at(300)?, &error)?.is_some(),
        "errors are urgent"
    );
    assert!(announcer.offer(at(1_400)?, &result)?.is_some());
    Ok(())
}

#[test]
fn letters_are_the_default_notation_and_figurines_replace_only_piece_letters()
-> PresentationBoardLayoutResult {
    assert_eq!(
        PresentationBoardFigurinesEnumNotation::default(),
        PresentationBoardFigurinesEnumNotation::Letters
    );
    let white = BoardEnumColor::White;
    let letters = PresentationBoardFigurinesEnumNotation::Letters;
    let figurines = PresentationBoardFigurinesEnumNotation::Figurines;
    assert_eq!(
        PresentationBoardFigurines::display_san("Nf3", letters, white),
        "Nf3"
    );
    assert_eq!(
        PresentationBoardFigurines::display_san("Nf3", figurines, white),
        "\u{2658}f3"
    );
    assert_eq!(
        PresentationBoardFigurines::display_san("exd8=Q+", figurines, white),
        "exd8=\u{2655}+"
    );
    assert_eq!(
        PresentationBoardFigurines::display_san("O-O", figurines, white),
        "O-O"
    );
    assert_eq!(
        PresentationBoardFigurines::glyph(BoardEnumColor::Black, BoardEnumPieceKind::Pawn),
        '\u{265f}'
    );
    assert_eq!(PresentationBoardFigurines::spoken_san("Nxe5"), "knight xe5");
    Ok(())
}

#[test]
fn only_click_and_focus_requests_are_accepted_from_assistive_technology() {
    let request = |action| ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: accesskit_node(),
        data: None,
    };
    assert_eq!(
        DesktopNativeAccessibility::classify(&request(Action::Click)),
        Ok(DesktopNativeAccessibilityEnumAction::Click)
    );
    assert_eq!(
        DesktopNativeAccessibility::classify(&request(Action::Focus)),
        Ok(DesktopNativeAccessibilityEnumAction::Focus)
    );
    assert!(DesktopNativeAccessibility::classify(&request(Action::Increment)).is_err());
    assert!(DesktopNativeAccessibility::is_supported(Action::Click));
    assert!(!DesktopNativeAccessibility::is_supported(Action::Collapse));
}

fn accesskit_node() -> bevy_egui::egui::accesskit::NodeId {
    bevy_egui::egui::Id::new("chess.test.node").accesskit_id()
}

#[test]
fn the_layout_merges_into_existing_preferences_without_dropping_sibling_keys()
-> PresentationBoardLayoutResult {
    use chess::desktop::native::layout_store::DesktopNativeLayoutStore;
    let mut layout = DesktopLayout::default();
    layout.set_tab(DesktopLayoutEnumTab::Report);
    let fresh = DesktopNativeLayoutStore::merge(None, &layout)?;
    assert_eq!(DesktopNativeLayoutStore::extract(&fresh)?, Some(layout));
    let merged = DesktopNativeLayoutStore::merge(Some(r#"{"theme":"dark"}"#), &layout)?;
    assert!(merged.contains("\"theme\":\"dark\""));
    assert_eq!(DesktopNativeLayoutStore::extract(&merged)?, Some(layout));
    assert_eq!(
        DesktopNativeLayoutStore::extract(r#"{"theme":"dark"}"#)?,
        None
    );
    assert!(DesktopNativeLayoutStore::merge(Some("[]"), &layout).is_err());
    assert!(DesktopNativeLayoutStore::extract("not json").is_err());
    Ok(())
}

#[test]
fn board_squares_accept_only_click_and_focus_action_requests() -> PresentationBoardLayoutResult {
    use chess::presentation::accessibility::{
        PresentationAccessibilityError, PresentationAccessibilityInput,
    };
    use chess::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
    use chess::scene::renderer::SceneRendererGeneration;
    let square = "e4".parse()?;
    let request = |action| ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: PresentationBoardSemanticGrid::square_id(square).accesskit_id(),
        data: None,
    };
    let generation = Some(SceneRendererGeneration::initial());
    let mut input = PresentationAccessibilityInput::new();
    input.try_forward(request(Action::Click), generation)?;
    input.try_forward(request(Action::Focus), generation)?;
    assert!(matches!(
        input.try_forward(request(Action::Increment), generation),
        Err(PresentationAccessibilityError::UnsupportedSquareAction(Action::Increment))
    ));
    Ok(())
}
