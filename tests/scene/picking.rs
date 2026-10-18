use bevy::math::{UVec2, Vec2, Vec3};
use bevy_egui::egui::{Pos2, Rect};
use chess::board::enums::BoardEnumColor;
use chess::board::square::BoardSquare;
use chess::scene::camera::SceneCamera;
use chess::scene::picking::{ScenePicking, ScenePickingViewport};
use chess::scene::renderer::SceneRendererGeneration;

#[test]
fn viewport_maps_logical_image_points_to_physical_pixels_and_rejects_clipping()
-> Result<(), Box<dyn std::error::Error>> {
    let image = Rect::from_min_max(Pos2::new(40.0, 70.0), Pos2::new(840.0, 670.0));
    let clip = Rect::from_min_max(Pos2::new(80.0, 90.0), Pos2::new(800.0, 630.0));
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let viewport = ScenePickingViewport::new(image, clip, scale, 1.0)?;
        let expected = (Vec2::new(800.0, 600.0) * scale).as_uvec2();
        assert_eq!(viewport.physical_size(), expected);
        assert_eq!(
            viewport.to_physical(Pos2::new(440.0, 370.0)),
            Some(expected.as_vec2() / 2.0)
        );
        assert!(viewport.to_physical(Pos2::new(60.0, 370.0)).is_none());
        assert!(viewport.to_physical(Pos2::new(800.0, 370.0)).is_none());
        assert!(viewport.to_physical(Pos2::new(f32::NAN, 370.0)).is_none());
    }
    assert!(ScenePickingViewport::new(image, clip, 0.0, 1.0).is_err());
    assert!(ScenePickingViewport::new(image, clip, f32::INFINITY, 1.0).is_err());
    assert!(ScenePickingViewport::new(Rect::NOTHING, clip, 1.0, 1.0).is_err());
    Ok(())
}

#[test]
fn every_square_round_trips_through_allowed_camera_views_and_display_scales()
-> Result<(), Box<dyn std::error::Error>> {
    for orientation in [BoardEnumColor::White, BoardEnumColor::Black] {
        for yaw in [0.0, 15.0, 90.0, 180.0, 270.0, 359.0] {
            for tilt in [0.0, 20.0, 45.0] {
                for zoom in [1.0, 1.1, 1.6] {
                    let camera = SceneCamera::new(yaw, tilt, zoom, orientation)?;
                    for size in [UVec2::new(560, 720), UVec2::new(1400, 900)] {
                        let frame = camera.fit(size)?;
                        for index in 0..64 {
                            let square = BoardSquare::try_from(index)?;
                            let center = ScenePicking::square_center(square);
                            let pixel = frame.project(center).ok_or("square outside camera")?;
                            let ray = frame.ray(pixel)?;
                            assert_eq!(ScenePicking::board_square(ray), Some(square));
                        }
                        for corner in SceneCamera::framing_corners() {
                            let pixel = frame.project(corner).ok_or("corner outside camera")?;
                            assert!(pixel.x >= 0.0 && pixel.x < size.as_vec2().x);
                            assert!(pixel.y >= 0.0 && pixel.y < size.as_vec2().y);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn camera_controls_retain_orientation_and_enforce_finite_bounds()
-> Result<(), Box<dyn std::error::Error>> {
    let camera = SceneCamera::new(350.0, 45.0, 1.6, BoardEnumColor::Black)?;
    let rotated = camera.rotate_right();
    assert_eq!(rotated.yaw_degrees(), 5.0);
    assert_eq!(camera.tilt_up().tilt_degrees(), 45.0);
    assert_eq!(camera.zoom_out().zoom(), 1.6);
    let reset = camera.reset();
    assert_eq!(reset.orientation(), BoardEnumColor::Black);
    assert_eq!(reset.yaw_degrees(), 0.0);
    assert_eq!(reset.tilt_degrees(), 20.0);
    assert_eq!(reset.zoom(), 1.1);
    assert!(SceneCamera::new(f32::NAN, 20.0, 1.1, BoardEnumColor::White).is_err());
    assert!(SceneCamera::new(0.0, 45.1, 1.1, BoardEnumColor::White).is_err());
    assert!(SceneCamera::new(0.0, 20.0, 0.99, BoardEnumColor::White).is_err());
    assert!(camera.fit(UVec2::ZERO).is_err());
    let point = ScenePicking::square_center("a1".parse()?);
    assert_eq!(point, Vec3::new(-3.5, 0.0, 3.5));
    Ok(())
}

#[test]
fn geometry_change_consumes_release_and_repeated_release_cannot_duplicate_an_action()
-> Result<(), Box<dyn std::error::Error>> {
    let original = SceneRendererGeneration::initial();
    let changed = original.next()?;
    assert_ne!(SceneRendererGeneration::initial(), original);
    let from = "e2".parse()?;
    let to = "e4".parse()?;
    let mut picking = ScenePicking::new(original);
    picking.press(original, from);
    picking.invalidate(changed);
    assert_eq!(picking.release(changed, Some(to)), None);
    assert_eq!(picking.release(changed, Some(to)), None);
    picking.press(changed, from);
    assert_eq!(picking.release(changed, Some(to)), Some((from, to)));
    assert_eq!(picking.release(changed, Some(to)), None);
    picking.press(changed, from);
    assert_eq!(picking.release(changed, None), None);
    Ok(())
}
