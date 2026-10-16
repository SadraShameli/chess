use bevy::math::Vec3;

use super::{ToolsAssetsPiecesError, ToolsAssetsPiecesGeometry, ToolsAssetsPiecesVertex};

#[test]
fn canonical_geometry_retains_oriented_surface_across_vertex_and_triangle_permutations()
-> Result<(), ToolsAssetsPiecesError> {
    let points = [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y];
    let first = ToolsAssetsPiecesGeometry {
        vertices: points
            .map(|position| ToolsAssetsPiecesVertex {
                position,
                normal: Vec3::Z,
            })
            .to_vec(),
        indices: vec![0, 1, 2, 0, 2, 3],
    }
    .indexed()?;
    let second = ToolsAssetsPiecesGeometry {
        vertices: [points[2], points[0], points[3], points[1]]
            .map(|position| ToolsAssetsPiecesVertex {
                position,
                normal: Vec3::Z,
            })
            .to_vec(),
        indices: vec![0, 2, 1, 3, 0, 1],
    }
    .indexed()?;
    first.validate()?;
    second.validate()?;
    assert_eq!(first.indices, second.indices);
    assert_eq!(
        first
            .vertices
            .iter()
            .map(|vertex| vertex.key())
            .collect::<Vec<_>>(),
        second
            .vertices
            .iter()
            .map(|vertex| vertex.key())
            .collect::<Vec<_>>(),
    );
    for triangle in first.indices.as_chunks::<3>().0 {
        let a = first.vertex_at(triangle[0])?.position;
        let b = first.vertex_at(triangle[1])?.position;
        let c = first.vertex_at(triangle[2])?.position;
        assert!((b - a).cross(c - a).dot(Vec3::Z) > 0.0);
    }
    Ok(())
}

#[test]
fn canonical_geometry_rejects_nonfinite_coordinates() {
    let geometry = ToolsAssetsPiecesGeometry {
        vertices: vec![ToolsAssetsPiecesVertex {
            position: Vec3::new(f32::NAN, 0.0, 0.0),
            normal: Vec3::Z,
        }],
        indices: vec![0, 0, 0],
    };
    assert!(matches!(
        geometry.indexed(),
        Err(ToolsAssetsPiecesError::Geometry)
    ));
}
