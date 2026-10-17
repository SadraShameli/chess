use strum::IntoEnumIterator;
use std::path::{Path, PathBuf};

use chess::package::digest::PackageDigest;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ToolsAssetsSoundsError {
    #[error("Sound generation I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Sound manifest serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Generated sound differs from {0}; the existing file was preserved")]
    Drift(PathBuf),
}

#[derive(Clone, Copy)]
struct ToolsAssetsSoundsTone {
    frequency: f64,
    start_ms: u32,
    length_ms: u32,
    amplitude: f64,
}

#[derive(Clone, Copy, strum::IntoStaticStr, strum::Display, strum::EnumIter)]
pub(crate) enum ToolsAssetsSoundsEnumCue {
    #[strum(serialize = "move.wav")]
    Move,
    #[strum(serialize = "check.wav")]
    Check,
    #[strum(serialize = "result.wav")]
    Result,
    #[strum(serialize = "low-time.wav")]
    LowTime,
}

impl ToolsAssetsSoundsEnumCue {
    fn file_name(self) -> &'static str {
        self.into()
    }

    const fn category(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Check => "check",
            Self::Result => "result",
            Self::LowTime => "low_time",
        }
    }

    fn tones(self) -> &'static [ToolsAssetsSoundsTone] {
        match self {
            Self::Move => &[
                ToolsAssetsSoundsTone {
                    frequency: 700.0,
                    start_ms: 0,
                    length_ms: 110,
                    amplitude: 0.5,
                },
                ToolsAssetsSoundsTone {
                    frequency: 350.0,
                    start_ms: 0,
                    length_ms: 110,
                    amplitude: 0.35,
                },
            ],
            Self::Check => &[
                ToolsAssetsSoundsTone {
                    frequency: 880.0,
                    start_ms: 0,
                    length_ms: 130,
                    amplitude: 0.5,
                },
                ToolsAssetsSoundsTone {
                    frequency: 1100.0,
                    start_ms: 130,
                    length_ms: 130,
                    amplitude: 0.5,
                },
            ],
            Self::Result => &[
                ToolsAssetsSoundsTone {
                    frequency: 523.0,
                    start_ms: 0,
                    length_ms: 250,
                    amplitude: 0.45,
                },
                ToolsAssetsSoundsTone {
                    frequency: 659.0,
                    start_ms: 250,
                    length_ms: 250,
                    amplitude: 0.45,
                },
                ToolsAssetsSoundsTone {
                    frequency: 784.0,
                    start_ms: 500,
                    length_ms: 400,
                    amplitude: 0.45,
                },
            ],
            Self::LowTime => &[
                ToolsAssetsSoundsTone {
                    frequency: 1000.0,
                    start_ms: 0,
                    length_ms: 120,
                    amplitude: 0.5,
                },
                ToolsAssetsSoundsTone {
                    frequency: 1000.0,
                    start_ms: 220,
                    length_ms: 120,
                    amplitude: 0.5,
                },
            ],
        }
    }

    fn duration_ms(self) -> u32 {
        self.tones()
            .iter()
            .map(|tone| tone.start_ms + tone.length_ms)
            .max()
            .unwrap_or(0)
    }
}

#[derive(Serialize)]
struct ToolsAssetsSoundsManifestEntry {
    file: &'static str,
    category: &'static str,
    duration_ms: u32,
    bytes: usize,
    sha256: String,
}

#[derive(Serialize)]
struct ToolsAssetsSoundsManifest {
    purpose: &'static str,
    generator: &'static str,
    license: &'static str,
    format: &'static str,
    sounds: Vec<ToolsAssetsSoundsManifestEntry>,
}

pub(crate) struct ToolsAssetsSoundsOutput {
    files: Vec<(&'static str, Vec<u8>)>,
}

impl ToolsAssetsSoundsOutput {
    pub(crate) fn files(&self) -> &[(&'static str, Vec<u8>)] {
        &self.files
    }
}

pub(crate) struct ToolsAssetsSounds;

impl ToolsAssetsSounds {
    pub(crate) const SAMPLE_RATE: u32 = 44_100;
    pub(crate) const MANIFEST_NAME: &'static str = "manifest.json";

    fn sine(phase: f64) -> f64 {
        let turn = phase - (phase / std::f64::consts::TAU).floor() * std::f64::consts::TAU;
        let x = if turn > std::f64::consts::PI {
            turn - std::f64::consts::TAU
        } else {
            turn
        };
        let y = 1.273_239_54 * x - 0.405_284_735 * x * x.abs();
        0.225 * (y * y.abs() - y) + y
    }

    fn sample_count(milliseconds: u32) -> usize {
        usize::try_from(u64::from(Self::SAMPLE_RATE) * u64::from(milliseconds) / 1000).unwrap_or(0)
    }

    fn render(cue: ToolsAssetsSoundsEnumCue) -> Vec<u8> {
        let total = Self::sample_count(cue.duration_ms());
        let mut samples = vec![0.0_f64; total];
        for tone in cue.tones() {
            let start = Self::sample_count(tone.start_ms);
            let length = Self::sample_count(tone.length_ms);
            let step = std::f64::consts::TAU * tone.frequency / f64::from(Self::SAMPLE_RATE);
            for index in 0..length {
                let Some(target) = samples.get_mut(start + index) else {
                    continue;
                };
                let position = index as f64 / length as f64;
                let attack = (index as f64 / 64.0).min(1.0);
                let release = (1.0 - position).powi(3);
                *target += tone.amplitude * attack * release * Self::sine(step * index as f64);
            }
        }
        let data_bytes = u32::try_from(total * 2).unwrap_or(0);
        let mut bytes = Vec::with_capacity(44 + total * 2);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&Self::SAMPLE_RATE.to_le_bytes());
        bytes.extend_from_slice(&(Self::SAMPLE_RATE * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_bytes.to_le_bytes());
        for sample in samples {
            let scaled = (sample.clamp(-1.0, 1.0) * 32_000.0).round();
            let value = scaled as i16;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    pub(crate) fn generate() -> Result<ToolsAssetsSoundsOutput, ToolsAssetsSoundsError> {
        let mut files = Vec::new();
        let mut entries = Vec::new();
        for cue in ToolsAssetsSoundsEnumCue::iter() {
            let wave = Self::render(cue);
            entries.push(ToolsAssetsSoundsManifestEntry {
                file: cue.file_name(),
                category: cue.category(),
                duration_ms: cue.duration_ms(),
                bytes: wave.len(),
                sha256: PackageDigest::hash_slice(&wave).to_string(),
            });
            files.push((cue.file_name(), wave));
        }
        let mut manifest = serde_json::to_vec_pretty(&ToolsAssetsSoundsManifest {
            purpose: "Original offline move, check, result and low-time cues",
            generator: "chess-tools sounds-generate: additive sine tones with a polynomial sine and cubic release, no sampled or third-party audio",
            license: "Original work generated by this repository; no external license applies",
            format: "RIFF WAVE, 16-bit signed PCM, mono, 44100 Hz",
            sounds: entries,
        })?;
        manifest.push(b'\n');
        files.push((Self::MANIFEST_NAME, manifest));
        Ok(ToolsAssetsSoundsOutput { files })
    }

    pub(crate) fn run(destination: &Path, check: bool) -> Result<(), ToolsAssetsSoundsError> {
        let generated = Self::generate()?;
        if check {
            for (name, bytes) in generated.files() {
                let path = destination.join(name);
                if std::fs::read(&path)? != *bytes {
                    return Err(ToolsAssetsSoundsError::Drift(path));
                }
            }
        } else {
            std::fs::create_dir_all(destination)?;
            for (name, bytes) in generated.files() {
                std::fs::write(destination.join(name), bytes)?;
            }
        }
        Ok(())
    }
}
