use bevy::math::{UVec2, Vec2};
use chess::board::enums::BoardEnumColor;
use chess::scene::camera::{SceneCamera, SceneCameraError};

const ASPECT_SIZES: [UVec2; 5] = [
    UVec2::new(400, 1200),
    UVec2::new(560, 720),
    UVec2::new(900, 900),
    UVec2::new(1400, 900),
    UVec2::new(3000, 600),
];

#[test]
fn initial_pose_matches_the_contract() {
    for orientation in [BoardEnumColor::White, BoardEnumColor::Black] {
        let camera = SceneCamera::initial(orientation);
        assert_eq!(camera.yaw_degrees(), 0.0);
        assert_eq!(camera.tilt_degrees(), 20.0);
        assert_eq!(camera.zoom(), 1.1);
        assert_eq!(camera.orientation(), orientation);
    }
    assert_eq!(SceneCamera::FIELD_OF_VIEW_DEGREES, 35.0);
    assert_eq!(SceneCamera::FRAMING_MARGIN, 1.08);
}

#[test]
fn constructor_rejects_out_of_bounds_poses_and_wraps_yaw() -> Result<(), SceneCameraError> {
    let white = BoardEnumColor::White;
    assert!(SceneCamera::new(0.0, -0.1, 1.0, white).is_err());
    assert!(SceneCamera::new(0.0, 45.1, 1.0, white).is_err());
    assert!(SceneCamera::new(0.0, 20.0, 0.99, white).is_err());
    assert!(SceneCamera::new(0.0, 20.0, 1.61, white).is_err());
    assert!(SceneCamera::new(f32::NAN, 20.0, 1.0, white).is_err());
    assert!(SceneCamera::new(f32::INFINITY, 20.0, 1.0, white).is_err());
    assert_eq!(
        SceneCamera::new(375.0, 0.0, 1.0, white)?.yaw_degrees(),
        15.0
    );
    assert_eq!(
        SceneCamera::new(-15.0, 45.0, 1.6, white)?.yaw_degrees(),
        345.0
    );
    Ok(())
}

#[test]
fn yaw_steps_are_fifteen_degrees_and_cover_the_full_turn() {
    let mut camera = SceneCamera::initial(BoardEnumColor::White);
    for _ in 0..24 {
        camera = camera.rotate_right();
    }
    assert_eq!(camera.yaw_degrees(), 0.0);
    assert_eq!(camera.rotate_right().yaw_degrees(), 15.0);
    assert_eq!(camera.rotate_left().yaw_degrees(), 345.0);
}

#[test]
fn tilt_steps_are_five_degrees_and_clamp_to_zero_through_forty_five() {
    let mut camera = SceneCamera::initial(BoardEnumColor::White);
    assert_eq!(camera.tilt_up().tilt_degrees(), 25.0);
    assert_eq!(camera.tilt_down().tilt_degrees(), 15.0);
    for _ in 0..10 {
        camera = camera.tilt_up();
    }
    assert_eq!(camera.tilt_degrees(), 45.0);
    for _ in 0..20 {
        camera = camera.tilt_down();
    }
    assert_eq!(camera.tilt_degrees(), 0.0);
}

#[test]
fn zoom_steps_are_ten_percent_and_clamp_to_the_multiplier_range() {
    let mut camera = SceneCamera::initial(BoardEnumColor::White);
    assert!((camera.zoom_out().zoom() - 1.21).abs() < 1e-6);
    assert!((camera.zoom_in().zoom() - 1.0).abs() < 1e-6);
    for _ in 0..10 {
        camera = camera.zoom_out();
    }
    assert_eq!(camera.zoom(), SceneCamera::MAX_ZOOM);
    for _ in 0..20 {
        camera = camera.zoom_in();
    }
    assert_eq!(camera.zoom(), SceneCamera::MIN_ZOOM);
}

#[test]
fn reset_restores_the_default_pose_and_retains_orientation() {
    for orientation in [BoardEnumColor::White, BoardEnumColor::Black] {
        let moved = SceneCamera::initial(orientation)
            .rotate_right()
            .tilt_up()
            .zoom_out()
            .zoom_out();
        assert_ne!(moved, SceneCamera::initial(orientation));
        let reset = moved.reset();
        assert_eq!(reset, SceneCamera::initial(orientation));
        assert_eq!(reset.orientation(), orientation);
    }
}

#[test]
fn orientation_change_keeps_the_pose() {
    let camera = SceneCamera::initial(BoardEnumColor::White)
        .rotate_right()
        .tilt_up();
    let flipped = camera.with_orientation(BoardEnumColor::Black);
    assert_eq!(flipped.pose(), camera.pose());
    assert_eq!(flipped.orientation(), BoardEnumColor::Black);
}

#[test]
fn pose_round_trips_through_the_typed_value() -> Result<(), SceneCameraError> {
    let camera = SceneCamera::new(135.0, 30.0, 1.4, BoardEnumColor::White)?;
    let pose = camera.pose();
    assert_eq!(pose.yaw_degrees(), 135.0);
    assert_eq!(pose.tilt_degrees(), 30.0);
    assert_eq!(pose.zoom(), 1.4);
    let restored = SceneCamera::from_pose(pose, BoardEnumColor::Black)?;
    assert_eq!(restored.pose(), pose);
    assert_eq!(restored.orientation(), BoardEnumColor::Black);
    Ok(())
}

#[test]
fn empty_viewport_is_rejected() {
    let camera = SceneCamera::initial(BoardEnumColor::White);
    assert!(camera.fit(UVec2::new(0, 600)).is_err());
    assert!(camera.fit(UVec2::new(800, 0)).is_err());
}

#[test]
fn every_framing_corner_stays_inside_the_frustum_with_margin() -> Result<(), SceneCameraError> {
    for orientation in [BoardEnumColor::White, BoardEnumColor::Black] {
        for yaw in (0_u16..360).step_by(15) {
            for tilt in [0.0, 5.0, 20.0, 45.0] {
                for zoom in [1.0, 1.1, 1.6] {
                    let camera = SceneCamera::new(f32::from(yaw), tilt, zoom, orientation)?;
                    for size in ASPECT_SIZES {
                        let frame = camera.fit(size)?;
                        let size_float = size.as_vec2();
                        let center = size_float / 2.0;
                        let inverse_margin = 1.0 / SceneCamera::FRAMING_MARGIN;
                        for corner in SceneCamera::framing_corners() {
                            let pixel = frame.project(corner).ok_or(SceneCameraError::Ray)?;
                            let ndc = (pixel - center).abs() / center;
                            assert!(
                                ndc.x <= inverse_margin + 1e-3 && ndc.y <= inverse_margin + 1e-3,
                                "{orientation:?} yaw {yaw} tilt {tilt} zoom {zoom} {size}: {ndc}"
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn fit_distance_grows_with_zoom_and_projection_matches_the_viewport() -> Result<(), SceneCameraError>
{
    let size = UVec2::new(1000, 500);
    let near = SceneCamera::new(0.0, 20.0, 1.0, BoardEnumColor::White)?.fit(size)?;
    let far = SceneCamera::new(0.0, 20.0, 1.6, BoardEnumColor::White)?.fit(size)?;
    assert!(far.transform().translation.length() > near.transform().translation.length());
    assert!((near.projection().aspect_ratio - 2.0).abs() < 1e-6);
    assert!((near.projection().fov.to_degrees() - 35.0).abs() < 1e-4);
    let center = near
        .project(bevy::math::Vec3::ZERO)
        .ok_or(SceneCameraError::Ray)?;
    assert!((center - Vec2::new(500.0, 250.0)).length() < 1e-2);
    Ok(())
}

#[test]
fn yaw_offset_wraps_without_touching_tilt_zoom_or_orientation() {
    let camera = SceneCamera::initial(BoardEnumColor::Black).rotate_right();
    let offset = camera.with_yaw_offset(-180.0);
    assert_eq!(offset.yaw_degrees(), 195.0);
    assert_eq!(offset.tilt_degrees(), camera.tilt_degrees());
    assert_eq!(offset.zoom(), camera.zoom());
    assert_eq!(offset.orientation(), BoardEnumColor::Black);
    assert_eq!(camera.with_yaw_offset(f32::NAN), camera);
    assert_eq!(camera.with_yaw_offset(0.0), camera);
}
