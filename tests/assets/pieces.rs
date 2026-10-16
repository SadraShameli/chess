use std::error::Error;
use std::num::NonZeroUsize;

use bevy::math::{Mat3, Vec3};
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::package::digest::PackageDigest;
use gltf_json::validation::{Checked, Validate};
use gltf_json::{Index, Root};
use serde::Deserialize;
use thiserror::Error;

#[path = "../../tools/assets/pieces.rs"]
mod generator;

use generator::{ToolsAssetsPieces, ToolsAssetsPiecesError};

type AssetsPiecesResult<T = ()> = Result<T, Box<dyn Error>>;

#[derive(Debug, Error)]
enum AssetsPiecesFixtureError {
    #[error("The piece fixture contains invalid binary layout, geometry, or hierarchy")]
    Invalid,
}

struct AssetsPiecesFixture<'a> {
    root: Root,
    binary: &'a [u8],
}

impl<'a> AssetsPiecesFixture<'a> {
    const ORIGINAL: &'static [u8] = include_bytes!("../../resources/pieces/chess.glb");
    const BOUNDS_TOLERANCE: f32 = 0.000_01;
    const NORMAL_TOLERANCE: f32 = 0.000_01;
    const FLOAT_BYTES: usize = 4;

    fn read(bytes: &'a [u8]) -> AssetsPiecesResult<Self> {
        if bytes.len() < 28
            || Self::word(bytes, 0)? != 0x4654_6c67
            || Self::word(bytes, 4)? != 2
            || usize::try_from(Self::word(bytes, 8)?)? != bytes.len()
            || Self::word(bytes, 16)? != 0x4e4f_534a
        {
            return Err(AssetsPiecesFixtureError::Invalid.into());
        }
        let json_length = usize::try_from(Self::word(bytes, 12)?)?;
        let binary_header = 20_usize
            .checked_add(json_length)
            .ok_or(AssetsPiecesFixtureError::Invalid)?;
        if !json_length.is_multiple_of(4) || Self::word(bytes, binary_header + 4)? != 0x004e_4942 {
            return Err(AssetsPiecesFixtureError::Invalid.into());
        }
        let binary_length = usize::try_from(Self::word(bytes, binary_header)?)?;
        let binary_start = binary_header + 8;
        let binary_end = binary_start
            .checked_add(binary_length)
            .ok_or(AssetsPiecesFixtureError::Invalid)?;
        if binary_end != bytes.len() || !binary_length.is_multiple_of(4) {
            return Err(AssetsPiecesFixtureError::Invalid.into());
        }
        let root: Root = serde_json::from_slice(
            bytes
                .get(20..binary_header)
                .ok_or(AssetsPiecesFixtureError::Invalid)?,
        )?;
        let mut errors = Vec::new();
        root.validate(&root, gltf_json::Path::new, &mut |path, error| {
            errors.push(format!("{}: {error:?}", path()));
        });
        assert!(errors.is_empty(), "glTF validation errors: {errors:?}");
        Ok(Self {
            root,
            binary: bytes
                .get(binary_start..binary_end)
                .ok_or(AssetsPiecesFixtureError::Invalid)?,
        })
    }

    fn word(bytes: &[u8], offset: usize) -> AssetsPiecesResult<u32> {
        let end = offset
            .checked_add(Self::FLOAT_BYTES)
            .ok_or(AssetsPiecesFixtureError::Invalid)?;
        let word = bytes
            .get(offset..end)
            .ok_or(AssetsPiecesFixtureError::Invalid)?
            .try_into()?;
        Ok(u32::from_le_bytes(word))
    }

    fn primitive(&self, name: &str) -> AssetsPiecesResult<&gltf_json::mesh::Primitive> {
        let parent = self
            .root
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some(name))
            .ok_or(AssetsPiecesFixtureError::Invalid)?;
        assert!(parent.mesh.is_none());
        let children = parent
            .children
            .as_ref()
            .ok_or(AssetsPiecesFixtureError::Invalid)?;
        assert_eq!(children.len(), 1);
        let child = self
            .root
            .nodes
            .get(children[0].value())
            .ok_or(AssetsPiecesFixtureError::Invalid)?;
        assert_eq!(
            child.name.as_deref(),
            Some(format!("{name}_sculpture").as_str())
        );
        let mesh = self
            .root
            .meshes
            .get(child.mesh.ok_or(AssetsPiecesFixtureError::Invalid)?.value())
            .ok_or(AssetsPiecesFixtureError::Invalid)?;
        assert_eq!(mesh.name.as_deref(), Some(format!("{name}_mesh").as_str()));
        assert_eq!(mesh.primitives.len(), 1);
        Ok(&mesh.primitives[0])
    }

    fn bytes(&self, index: Index<gltf_json::Accessor>) -> AssetsPiecesResult<&[u8]> {
        let accessor = self
            .root
            .accessors
            .get(index.value())
            .ok_or(AssetsPiecesFixtureError::Invalid)?;
        let view = self
            .root
            .buffer_views
            .get(
                accessor
                    .buffer_view
                    .ok_or(AssetsPiecesFixtureError::Invalid)?
                    .value(),
            )
            .ok_or(AssetsPiecesFixtureError::Invalid)?;
        assert!(view.byte_stride.is_none());
        let view_offset = view.byte_offset.map_or(0, |offset| offset.0);
        let offset = usize::try_from(
            view_offset
                .checked_add(accessor.byte_offset.map_or(0, |offset| offset.0))
                .ok_or(AssetsPiecesFixtureError::Invalid)?,
        )?;
        let end = usize::try_from(
            view_offset
                .checked_add(view.byte_length.0)
                .ok_or(AssetsPiecesFixtureError::Invalid)?,
        )?;
        self.binary
            .get(offset..end)
            .ok_or_else(|| AssetsPiecesFixtureError::Invalid.into())
    }

    fn vectors(&self, index: Index<gltf_json::Accessor>) -> AssetsPiecesResult<Vec<Vec3>> {
        let accessor = &self.root.accessors[index.value()];
        assert_eq!(
            accessor.type_,
            Checked::Valid(gltf_json::accessor::Type::Vec3)
        );
        assert!(matches!(
            accessor.component_type,
            Checked::Valid(gltf_json::accessor::GenericComponentType(
                gltf_json::accessor::ComponentType::F32
            ))
        ));
        let bytes = self.bytes(index)?;
        let count = usize::try_from(accessor.count.0)?;
        let required = count
            .checked_mul(3 * Self::FLOAT_BYTES)
            .ok_or(AssetsPiecesFixtureError::Invalid)?;
        let bytes = bytes
            .get(..required)
            .ok_or(AssetsPiecesFixtureError::Invalid)?;
        bytes
            .as_chunks::<12>()
            .0
            .iter()
            .map(|vector| {
                Ok(Vec3::new(
                    f32::from_bits(Self::word(vector, 0)?),
                    f32::from_bits(Self::word(vector, 4)?),
                    f32::from_bits(Self::word(vector, 8)?),
                ))
            })
            .collect()
    }

    fn indices(&self, index: Index<gltf_json::Accessor>) -> AssetsPiecesResult<Vec<usize>> {
        let accessor = &self.root.accessors[index.value()];
        assert_eq!(
            accessor.type_,
            Checked::Valid(gltf_json::accessor::Type::Scalar)
        );
        assert!(matches!(
            accessor.component_type,
            Checked::Valid(gltf_json::accessor::GenericComponentType(
                gltf_json::accessor::ComponentType::U32
            ))
        ));
        let bytes = self.bytes(index)?;
        let count = usize::try_from(accessor.count.0)?;
        assert!(count.is_multiple_of(3));
        let mut indices = Vec::with_capacity(count);
        for word in bytes
            .get(
                ..count
                    .checked_mul(4)
                    .ok_or(AssetsPiecesFixtureError::Invalid)?,
            )
            .ok_or(AssetsPiecesFixtureError::Invalid)?
            .as_chunks::<4>()
            .0
        {
            indices.push(usize::try_from(u32::from_le_bytes(*word))?);
        }
        Ok(indices)
    }

    fn attribute(
        primitive: &gltf_json::mesh::Primitive,
        semantic: gltf_json::mesh::Semantic,
    ) -> AssetsPiecesResult<Index<gltf_json::Accessor>> {
        primitive
            .attributes
            .get(&Checked::Valid(semantic))
            .copied()
            .ok_or_else(|| AssetsPiecesFixtureError::Invalid.into())
    }

    fn bounds(
        &self,
        primitive: &gltf_json::mesh::Primitive,
    ) -> AssetsPiecesResult<([f32; 3], [f32; 3])> {
        let index = Self::attribute(primitive, gltf_json::mesh::Semantic::Positions)?;
        let accessor = &self.root.accessors[index.value()];
        let min = serde_json::from_value(
            accessor
                .min
                .clone()
                .ok_or(AssetsPiecesFixtureError::Invalid)?,
        )?;
        let max = serde_json::from_value(
            accessor
                .max
                .clone()
                .ok_or(AssetsPiecesFixtureError::Invalid)?,
        )?;
        Ok((min, max))
    }

    fn geometry(&self, name: &str) -> AssetsPiecesResult {
        let primitive = self.primitive(name)?;
        let positions = self.vectors(Self::attribute(
            primitive,
            gltf_json::mesh::Semantic::Positions,
        )?)?;
        let normals = self.vectors(Self::attribute(
            primitive,
            gltf_json::mesh::Semantic::Normals,
        )?)?;
        let indices = self.indices(primitive.indices.ok_or(AssetsPiecesFixtureError::Invalid)?)?;
        assert_eq!(positions.len(), normals.len());
        assert!(!positions.is_empty());
        assert!(!indices.is_empty());
        let (min, max) = self.bounds(primitive)?;
        let min = Vec3::from_array(min);
        let max = Vec3::from_array(max);
        for (point, normal) in positions.iter().zip(normals) {
            assert!(point.is_finite());
            assert!(normal.is_finite());
            assert!((normal.length_squared() - 1.0).abs() <= Self::NORMAL_TOLERANCE);
            assert!(point.cmpge(min - Vec3::splat(Self::BOUNDS_TOLERANCE)).all());
            assert!(point.cmple(max + Vec3::splat(Self::BOUNDS_TOLERANCE)).all());
        }
        assert!(indices.iter().all(|index| *index < positions.len()));
        Ok(())
    }

    fn bishop_cut(&self) -> AssetsPiecesResult {
        let primitive = self.primitive("white_bishop")?;
        let positions = self.vectors(Self::attribute(
            primitive,
            gltf_json::mesh::Semantic::Positions,
        )?)?;
        let normals = self.vectors(Self::attribute(
            primitive,
            gltf_json::mesh::Semantic::Normals,
        )?)?;
        let indices = self.indices(primitive.indices.ok_or(AssetsPiecesFixtureError::Invalid)?)?;
        let inverse = Mat3::from_rotation_z(std::f32::consts::PI / 5.0);
        let origin = Vec3::new(0.09, 0.91, 0.0);
        let cutter_half = Vec3::new(0.0225, 0.17, 0.21);
        let mut cut_faces = 0;
        for triangle in indices.as_chunks::<3>().0 {
            let points = triangle.map(|index| positions[index]);
            let local = points.map(|point| inverse * (point - origin));
            for point in local {
                assert!(
                    !point
                        .abs()
                        .cmplt(cutter_half - Vec3::splat(Self::BOUNDS_TOLERANCE))
                        .all()
                );
            }
            for sign in [-1.0_f32, 1.0] {
                if local
                    .iter()
                    .all(|point| (point.x - sign * cutter_half.x).abs() < Self::BOUNDS_TOLERANCE)
                {
                    let expected = inverse.transpose() * Vec3::new(-sign, 0.0, 0.0);
                    for index in triangle {
                        assert!(normals[*index].dot(expected) > 0.999);
                    }
                    let winding = (points[1] - points[0]).cross(points[2] - points[0]);
                    assert!(winding.dot(expected) > 0.0);
                    cut_faces += 1;
                }
            }
        }
        assert!(
            cut_faces > 0,
            "The bishop must contain actual inward-facing slit geometry"
        );
        Ok(())
    }
}

#[derive(Deserialize)]
struct AssetsPiecesProvenance {
    generator: AssetsPiecesGeneratorIdentity,
    asset: AssetsPiecesAssetIdentity,
}

#[derive(Deserialize)]
struct AssetsPiecesGeneratorIdentity {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
struct AssetsPiecesAssetIdentity {
    sha256: String,
    bytes: usize,
}

#[tokio::test(flavor = "current_thread")]
async fn generation_preserves_original_hierarchy_materials_bounds_and_bishop_slit()
-> AssetsPiecesResult {
    let executor = CapacityExecutor::new(CapacityBudget::new(NonZeroUsize::MIN)?)?;
    let generated = ToolsAssetsPieces::generate(&executor).await;
    executor.shutdown().await?;
    let generated = generated?;
    let original = AssetsPiecesFixture::read(AssetsPiecesFixture::ORIGINAL)?;
    let fixture = AssetsPiecesFixture::read(generated.binary())?;
    assert_eq!(fixture.root.nodes.len(), original.root.nodes.len());
    assert_eq!(fixture.root.meshes.len(), original.root.meshes.len());
    assert_eq!(fixture.root.accessors.len(), 18);
    assert_eq!(fixture.root.scenes.len(), 1);
    assert_eq!(fixture.root.scenes[0].nodes.len(), 12);
    assert!(fixture.root.images.is_empty());
    assert!(fixture.root.textures.is_empty());
    assert!(fixture.root.extensions_required.is_empty());
    assert_eq!(fixture.root.buffers.len(), 1);
    assert!(fixture.root.buffers[0].uri.is_none());
    assert_eq!(fixture.root.materials.len(), original.root.materials.len());
    for (material, expected) in fixture.root.materials.iter().zip(&original.root.materials) {
        assert_eq!(material.name, expected.name);
        assert_eq!(material.pbr_metallic_roughness.metallic_factor.0, 0.0);
        assert_eq!(
            material.pbr_metallic_roughness.roughness_factor.0,
            expected.pbr_metallic_roughness.roughness_factor.0
        );
        for (actual, expected) in material
            .pbr_metallic_roughness
            .base_color_factor
            .0
            .into_iter()
            .zip(expected.pbr_metallic_roughness.base_color_factor.0)
        {
            assert!((actual - expected).abs() < AssetsPiecesFixture::BOUNDS_TOLERANCE);
        }
    }
    for parent in &original.root.scenes[0].nodes {
        let name = original.root.nodes[parent.value()]
            .name
            .as_deref()
            .ok_or(AssetsPiecesFixtureError::Invalid)?;
        let original_bounds = original.bounds(original.primitive(name)?)?;
        let generated_bounds = fixture.bounds(fixture.primitive(name)?)?;
        for (actual, expected) in generated_bounds
            .0
            .into_iter()
            .chain(generated_bounds.1)
            .zip(original_bounds.0.into_iter().chain(original_bounds.1))
        {
            assert!(
                (actual - expected).abs() < AssetsPiecesFixture::BOUNDS_TOLERANCE,
                "{name}: generated bound {actual}, original {expected}"
            );
        }
        fixture.geometry(name)?;
        if let Some(kind) = name.strip_prefix("white_") {
            let white = fixture.primitive(name)?;
            let black = fixture.primitive(&format!("black_{kind}"))?;
            assert_eq!(white.attributes, black.attributes);
            assert_eq!(white.indices, black.indices);
            assert_ne!(white.material, black.material);
        }
    }
    fixture.bishop_cut()?;
    let provenance: AssetsPiecesProvenance = serde_json::from_slice(generated.provenance())?;
    assert_eq!(provenance.generator.path, "tools/assets/pieces.rs");
    assert_eq!(
        provenance.generator.sha256,
        PackageDigest::read(include_bytes!("../../tools/assets/pieces.rs").as_slice())?.to_string()
    );
    assert_eq!(provenance.asset.bytes, generated.binary().len());
    assert_eq!(
        provenance.asset.sha256,
        PackageDigest::read(generated.binary())?.to_string()
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn generation_is_identical_with_one_worker_and_detected_maximum_capacity()
-> AssetsPiecesResult {
    let executor = CapacityExecutor::new(CapacityBudget::new(NonZeroUsize::MIN)?)?;
    let single = ToolsAssetsPieces::generate(&executor).await;
    executor.shutdown().await?;
    let single = single?;
    let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
    let maximum = ToolsAssetsPieces::generate(&executor).await;
    executor.shutdown().await?;
    let maximum = maximum?;
    assert_eq!(single.binary(), maximum.binary());
    assert_eq!(single.provenance(), maximum.provenance());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn checking_detects_drift_without_overwriting_either_output() -> AssetsPiecesResult {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    ToolsAssetsPieces::run(directory.path(), false, &executor, &budget).await?;
    ToolsAssetsPieces::run(directory.path(), true, &executor, &budget).await?;
    let binary_path = directory.path().join("chess.glb");
    let provenance_path = directory.path().join("provenance.json");
    let provenance = tokio::fs::read(&provenance_path).await?;
    let changed = b"Preserve this deliberately changed fixture";
    tokio::fs::write(&binary_path, changed).await?;
    let check = ToolsAssetsPieces::run(directory.path(), true, &executor, &budget).await;
    executor.shutdown().await?;
    assert!(matches!(check, Err(ToolsAssetsPiecesError::Drift(path)) if path == binary_path));
    assert_eq!(tokio::fs::read(&binary_path).await?, changed);
    assert_eq!(tokio::fs::read(&provenance_path).await?, provenance);
    assert_eq!(
        PackageDigest::read(AssetsPiecesFixture::ORIGINAL)?.to_string(),
        "ee0c9bf8cd98f1239046b0566748030c4c320f049fd6ab3232f1f7e9691a020e"
    );
    directory.close()?;
    Ok(())
}
