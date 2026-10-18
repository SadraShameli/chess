use std::error::Error;
use std::path::Path;

#[path = "../../tools/assets/sounds.rs"]
mod generator;

use generator::ToolsAssetsSounds;

fn word(bytes: &[u8], offset: usize) -> Result<u32, Box<dyn Error>> {
    let slice = bytes.get(offset..offset + 4).ok_or("short wave")?;
    Ok(u32::from_le_bytes(slice.try_into()?))
}

#[test]
fn generated_cues_are_deterministic_riff_mono_pcm() -> Result<(), Box<dyn Error>> {
    let first = ToolsAssetsSounds::generate()?;
    let second = ToolsAssetsSounds::generate()?;
    assert_eq!(first.files(), second.files());
    let mut waves = 0;
    for (name, bytes) in first.files() {
        if !name.ends_with(".wav") {
            continue;
        }
        waves += 1;
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(usize::try_from(word(bytes, 4)?)? + 8, bytes.len());
        assert_eq!(word(bytes, 24)?, ToolsAssetsSounds::SAMPLE_RATE);
        assert!(bytes[44..].iter().any(|byte| *byte != 0));
    }
    assert_eq!(waves, 4);
    Ok(())
}

#[test]
fn committed_sound_resources_match_the_generator() -> Result<(), Box<dyn Error>> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/sounds");
    ToolsAssetsSounds::run(&directory, true)?;
    Ok(())
}
