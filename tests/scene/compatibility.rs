use bevy::app::App;
use bevy::ecs::hierarchy::ChildOf;
use bevy::math::Vec3;
use bevy::prelude::{Component, GlobalTransform, Transform, TransformPlugin, Window};
use bevy::render::renderer::RenderDevice;
use bevy::tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool};
use chess::board::piece::BoardPiece;
use chess::board::position::BoardPosition;
use chess::board::square::BoardSquare;

use crate::scene_task_pools::SceneTaskPoolFixture;

#[derive(Component)]
struct SceneCompatibilityPiece {
    square: BoardSquare,
    piece: BoardPiece,
}

impl SceneCompatibilityPiece {
    const FILES: u8 = 8;
    const TOLERANCE: f32 = 0.000_001;

    fn translation(square: BoardSquare) -> Vec3 {
        let index = u8::from(square);
        Vec3::new(
            f32::from(index % Self::FILES),
            0.0,
            f32::from(index / Self::FILES),
        )
    }
}

#[test]
fn headless_bevy_propagates_domain_piece_transforms_without_window_or_gpu()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();
    app.add_plugins((SceneTaskPoolFixture::plugin(), TransformPlugin));
    let position = BoardPosition::standard();
    let initial_identity = position.identity();
    let mut parent_transform = Transform::from_xyz(8.0, 1.0, -4.0);
    let parent = app.world_mut().spawn(parent_transform).id();
    for (square, piece) in position.pieces() {
        app.world_mut().spawn((
            SceneCompatibilityPiece { square, piece },
            Transform::from_translation(SceneCompatibilityPiece::translation(square)),
            ChildOf(parent),
        ));
    }

    for offset in [Vec3::ZERO, Vec3::new(-2.0, 3.0, 4.0)] {
        parent_transform.translation += offset;
        app.world_mut().entity_mut(parent).insert(parent_transform);
        app.update();

        let world = app.world_mut();
        let mut pieces = world.query::<(&SceneCompatibilityPiece, &GlobalTransform)>();
        assert_eq!(pieces.iter(world).count(), position.pieces().count());
        for (piece, transform) in pieces.iter(world) {
            let expected =
                parent_transform.translation + SceneCompatibilityPiece::translation(piece.square);
            assert!(
                transform
                    .translation()
                    .abs_diff_eq(expected, SceneCompatibilityPiece::TOLERANCE)
            );
            assert!(
                position
                    .pieces()
                    .any(|entry| entry == (piece.square, piece.piece))
            );
        }
        assert!(!world.contains_resource::<RenderDevice>());
        assert_eq!(world.query::<&Window>().iter(world).count(), 0);
        assert_eq!(position.identity(), initial_identity);
    }

    assert_eq!(
        IoTaskPool::get().thread_num(),
        SceneTaskPoolFixture::THREADS_PER_POOL
    );
    assert_eq!(
        AsyncComputeTaskPool::get().thread_num(),
        SceneTaskPoolFixture::THREADS_PER_POOL
    );
    assert_eq!(
        ComputeTaskPool::get().thread_num(),
        SceneTaskPoolFixture::THREADS_PER_POOL
    );
    Ok(())
}
