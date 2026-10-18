use bevy::color::Color;
use bevy::gltf::GltfMaterial;
use bevy::math::Vec3;
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::StandardMaterial;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneMaterialsLight {
    pub illuminance: f32,
    pub shadows: bool,
    pub position: Vec3,
}

pub struct SceneMaterials;

impl SceneMaterials {
    pub const LIGHT_SQUARE: Color = Color::srgb_u8(238, 229, 207);
    pub const DARK_SQUARE: Color = Color::srgb_u8(75, 112, 87);
    pub const SQUARE_ROUGHNESS: f32 = 0.72;
    pub const SATIN_CLEARCOAT: f32 = 0.35;
    pub const SATIN_CLEARCOAT_ROUGHNESS: f32 = 0.45;
    pub const SATIN_REFLECTANCE: f32 = 0.4;
    pub const KEY_ILLUMINANCE: f32 = 10_000.0;
    pub const LIGHTS: [SceneMaterialsLight; 2] = [
        SceneMaterialsLight {
            illuminance: Self::KEY_ILLUMINANCE,
            shadows: true,
            position: Vec3::new(-3.0, 8.0, 4.0),
        },
        SceneMaterialsLight {
            illuminance: 2_500.0,
            shadows: false,
            position: Vec3::new(4.0, 5.0, -3.0),
        },
    ];

    pub fn square(light: bool) -> StandardMaterial {
        StandardMaterial {
            base_color: if light {
                Self::LIGHT_SQUARE
            } else {
                Self::DARK_SQUARE
            },
            perceptual_roughness: Self::SQUARE_ROUGHNESS,
            ..StandardMaterial::default()
        }
    }

    pub fn piece(source: &GltfMaterial) -> StandardMaterial {
        StandardMaterial {
            clearcoat: Self::SATIN_CLEARCOAT,
            clearcoat_perceptual_roughness: Self::SATIN_CLEARCOAT_ROUGHNESS,
            reflectance: Self::SATIN_REFLECTANCE,
            ..standard_material_from_gltf_material(source)
        }
    }
}
