use chess::scene::materials::SceneMaterials;

#[test]
fn board_squares_are_matte_and_distinguishable() {
    let light = SceneMaterials::square(true);
    let dark = SceneMaterials::square(false);
    assert_ne!(light.base_color, dark.base_color);
    assert!(light.perceptual_roughness >= 0.5);
    assert!(dark.perceptual_roughness >= 0.5);
}

#[test]
fn exactly_one_shadowed_key_light_drives_the_scene() {
    let keys: Vec<_> = SceneMaterials::LIGHTS
        .iter()
        .filter(|light| light.shadows)
        .collect();
    assert_eq!(keys.len(), 1);
    assert!(
        keys.iter()
            .all(|light| light.illuminance >= SceneMaterials::KEY_ILLUMINANCE)
    );
    assert!(
        SceneMaterials::LIGHTS
            .iter()
            .filter(|light| !light.shadows)
            .all(|light| light.illuminance < SceneMaterials::KEY_ILLUMINANCE)
    );
}
