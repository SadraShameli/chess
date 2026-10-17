use bevy_egui::egui::{Event, Key, accesskit};

use chess::board::enums::BoardEnumColor;
use chess::board::square::BoardSquare;
use chess::presentation::board::enums::PresentationBoardEnumDirection;
use chess::presentation::board::scene::PresentationBoardScene;
use chess::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use chess::scene::camera::SceneCamera;

use crate::presentation_board::{PresentationBoardFixture, PresentationBoardTestResult};

impl PresentationBoardFixture {
    fn with_free_camera(
        orientation: BoardEnumColor,
        yaw: f32,
    ) -> PresentationBoardTestResult<Self> {
        let mut fixture = Self::new(orientation)?;
        let viewport = fixture.scene.viewport();
        fixture.scene = PresentationBoardScene::new(
            fixture.scene.texture(),
            fixture.scene.generation(),
            fixture.scene.lifecycle(),
            viewport,
            SceneCamera::new(yaw, 20.0, 1.1, orientation)?.fit(viewport.physical_size())?,
        );
        Ok(fixture)
    }
}

#[test]
fn mounted_keyboard_directions_keep_side_orientation_independent_of_free_camera_yaw()
-> PresentationBoardTestResult {
    let origin: BoardSquare = "d4".parse()?;
    for (orientation, destinations) in [
        (BoardEnumColor::White, ["d5", "d3", "c4", "e4"]),
        (BoardEnumColor::Black, ["d3", "d5", "e4", "c4"]),
    ] {
        for yaw in [0.0, 90.0, 180.0, 270.0] {
            for (key, destination) in [
                Key::ArrowUp,
                Key::ArrowDown,
                Key::ArrowLeft,
                Key::ArrowRight,
            ]
            .into_iter()
            .zip(destinations)
            {
                let mut fixture = PresentationBoardFixture::with_free_camera(orientation, yaw)?;
                let original = fixture.snapshot.fen().to_owned();
                fixture.frame(Vec::new(), false)?;
                fixture.frame(
                    vec![Event::AccessKitActionRequest(
                        PresentationBoardFixture::request(origin, accesskit::Action::Focus),
                    )],
                    false,
                )?;
                fixture.frame(vec![PresentationBoardFixture::key(key)], true)?;
                let focused = fixture.context.memory(|memory| memory.focused());
                let expected: BoardSquare = destination.parse()?;
                let unchanged = fixture.snapshot.fen() == original;
                let submitted = fixture.actions.len();
                drop(fixture);
                assert_eq!(
                    focused,
                    Some(PresentationBoardSemanticGrid::square_id(expected)),
                    "{orientation:?}, yaw {yaw}, {key:?} must focus {expected}"
                );
                assert!(unchanged);
                assert_eq!(submitted, 0);
            }
        }
    }
    Ok(())
}

#[test]
fn keyboard_grid_edges_do_not_wrap_after_camera_rotation() -> PresentationBoardTestResult {
    for (orientation, cases) in [
        (
            BoardEnumColor::White,
            [
                ("a1", PresentationBoardEnumDirection::Left),
                ("a1", PresentationBoardEnumDirection::Down),
                ("h8", PresentationBoardEnumDirection::Right),
                ("h8", PresentationBoardEnumDirection::Up),
            ],
        ),
        (
            BoardEnumColor::Black,
            [
                ("h8", PresentationBoardEnumDirection::Left),
                ("h8", PresentationBoardEnumDirection::Down),
                ("a1", PresentationBoardEnumDirection::Right),
                ("a1", PresentationBoardEnumDirection::Up),
            ],
        ),
    ] {
        for (square, direction) in cases {
            let fixture = PresentationBoardFixture::with_free_camera(orientation, 90.0)?;
            let square = square.parse()?;
            let neighbor =
                PresentationBoardSemanticGrid::neighbor(square, direction, &fixture.scene);
            drop(fixture);
            assert_eq!(neighbor, None, "{orientation:?}, {square}, {direction:?}");
        }
    }
    Ok(())
}
