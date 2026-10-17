use std::path::{Path, PathBuf};

use chess::engine::package::manifest::EnginePackageManifest;
use chess::package::digest::PackageDigest;

use crate::packaging::plan::{ToolsPackagingEnumPlatform, ToolsPackagingError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolsPackagingEnumArchitecture {
    ElfX64,
    ElfArm64,
    ElfOther,
    PeX64,
    PeArm64,
    PeOther,
    MachoArm64,
    MachoX64,
    MachoFat,
    MachoOther,
}

impl ToolsPackagingEnumArchitecture {
    const fn is_wanted(self, platform: ToolsPackagingEnumPlatform) -> bool {
        matches!(
            (platform, self),
            (ToolsPackagingEnumPlatform::Windows, Self::PeX64)
                | (ToolsPackagingEnumPlatform::Linux, Self::ElfX64)
                | (ToolsPackagingEnumPlatform::Macos, Self::MachoArm64)
        )
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ToolsPackagingVerifyReport {
    pub(crate) failures: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

pub(crate) struct ToolsPackagingVerify;

impl ToolsPackagingVerify {
    pub(crate) const FORBIDDEN_MARKERS: [&'static str; 6] = [
        "http://localhost",
        "http://127.0.0.1",
        "ws://localhost",
        "devUrl",
        "CHESS_TEST_LISTENER",
        "chess-test-permissions",
    ];
    pub(crate) const REQUIRED: [&'static str; 2] = ["pieces/chess.glb", "engine/manifest.json"];
    pub(crate) const OPTIONAL: [&'static str; 3] = ["sounds", "licenses", "notices"];
    pub(crate) const MAX_GLIBC: (u16, u16) = (2, 35);
    const SCAN_LIMIT: u64 = 16 * 1024 * 1024;

    pub(crate) fn architecture(bytes: &[u8]) -> Option<ToolsPackagingEnumArchitecture> {
        if bytes.len() >= 20 && bytes.starts_with(b"\x7fELF") {
            let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
            return Some(match machine {
                0x3e => ToolsPackagingEnumArchitecture::ElfX64,
                0xb7 => ToolsPackagingEnumArchitecture::ElfArm64,
                _ => ToolsPackagingEnumArchitecture::ElfOther,
            });
        }
        if bytes.len() >= 0x40 && bytes.starts_with(b"MZ") {
            let offset = u32::from_le_bytes([bytes[0x3c], bytes[0x3d], bytes[0x3e], bytes[0x3f]]);
            let start = usize::try_from(offset).ok()?;
            let header = bytes.get(start..start.checked_add(6)?)?;
            if !header.starts_with(b"PE\0\0") {
                return None;
            }
            let machine = u16::from_le_bytes([header[4], header[5]]);
            return Some(match machine {
                0x8664 => ToolsPackagingEnumArchitecture::PeX64,
                0xaa64 => ToolsPackagingEnumArchitecture::PeArm64,
                _ => ToolsPackagingEnumArchitecture::PeOther,
            });
        }
        if bytes.len() >= 8 {
            let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if magic == 0xfeed_facf {
                let cpu = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                return Some(match cpu {
                    0x0100_000c => ToolsPackagingEnumArchitecture::MachoArm64,
                    0x0100_0007 => ToolsPackagingEnumArchitecture::MachoX64,
                    _ => ToolsPackagingEnumArchitecture::MachoOther,
                });
            }
            if bytes.starts_with(&[0xca, 0xfe, 0xba, 0xbe]) {
                return Some(ToolsPackagingEnumArchitecture::MachoFat);
            }
        }
        None
    }

    pub(crate) fn max_glibc(bytes: &[u8]) -> Option<(u16, u16)> {
        let marker = b"GLIBC_";
        let mut best: Option<(u16, u16)> = None;
        let mut index = 0;
        while let Some(position) = bytes[index..]
            .windows(marker.len())
            .position(|window| window == marker)
        {
            let start = index + position + marker.len();
            let text: String = bytes[start..]
                .iter()
                .take(12)
                .take_while(|byte| byte.is_ascii_digit() || **byte == b'.')
                .map(|byte| char::from(*byte))
                .collect();
            let mut parts = text.split('.');
            if let (Some(major), Some(minor)) = (parts.next(), parts.next())
                && let (Ok(major), Ok(minor)) = (major.parse::<u16>(), minor.parse::<u16>())
                && best.is_none_or(|current| (major, minor) > current)
            {
                best = Some((major, minor));
            }
            index = start;
        }
        best
    }

    pub(crate) fn contains_forbidden(bytes: &[u8]) -> Vec<&'static str> {
        Self::FORBIDDEN_MARKERS
            .into_iter()
            .filter(|marker| {
                let needle = marker.as_bytes();
                bytes.windows(needle.len()).any(|window| window == needle)
            })
            .collect()
    }

    pub(crate) fn verify(
        platform: ToolsPackagingEnumPlatform,
        directory: &Path,
    ) -> Result<ToolsPackagingVerifyReport, ToolsPackagingError> {
        let mut report = ToolsPackagingVerifyReport::default();
        let mut files = Vec::new();
        Self::walk(directory, &mut files)?;
        for relative in Self::REQUIRED {
            if !directory.join(relative).is_file() {
                report
                    .failures
                    .push(format!("required resource {relative} is missing"));
            }
        }
        for relative in Self::OPTIONAL {
            if !directory.join(relative).exists() {
                report
                    .warnings
                    .push(format!("optional resource {relative} is absent"));
            }
        }
        for file in &files {
            Self::inspect(platform, directory, file, &mut report)?;
        }
        Self::verify_engine(platform, directory, &mut report)?;
        if report.failures.is_empty() {
            Ok(report)
        } else {
            Err(ToolsPackagingError::Verification(report.failures.join("\n")))
        }
    }

    fn inspect(
        platform: ToolsPackagingEnumPlatform,
        root: &Path,
        file: &Path,
        report: &mut ToolsPackagingVerifyReport,
    ) -> Result<(), ToolsPackagingError> {
        let name = file.strip_prefix(root).unwrap_or(file).display().to_string();
        let size = file.metadata()?.len();
        let bytes = std::fs::read(file)?;
        if let Some(architecture) = Self::architecture(&bytes) {
            if !architecture.is_wanted(platform) {
                report
                    .failures
                    .push(format!("{name} targets an unwanted architecture {architecture:?}"));
            }
            if platform == ToolsPackagingEnumPlatform::Linux
                && let Some(glibc) = Self::max_glibc(&bytes)
                && glibc > Self::MAX_GLIBC
            {
                report.failures.push(format!(
                    "{name} requires glibc {}.{}, newer than {}.{}",
                    glibc.0,
                    glibc.1,
                    Self::MAX_GLIBC.0,
                    Self::MAX_GLIBC.1
                ));
            }
        }
        if !name.contains("engine") && size <= Self::SCAN_LIMIT {
            for marker in Self::contains_forbidden(&bytes) {
                report
                    .failures
                    .push(format!("{name} contains the forbidden marker '{marker}'"));
            }
        }
        Ok(())
    }

    fn verify_engine(
        platform: ToolsPackagingEnumPlatform,
        root: &Path,
        report: &mut ToolsPackagingVerifyReport,
    ) -> Result<(), ToolsPackagingError> {
        let manifest_path = root.join("engine/manifest.json");
        if !manifest_path.is_file() {
            return Ok(());
        }
        let manifest = EnginePackageManifest::parse(&std::fs::read(&manifest_path)?)?;
        let mut matched = false;
        for entry in manifest.entries() {
            let path = root.join("engine").join(entry.executable());
            if entry.target().triple() != platform.engine_triple() {
                if path.exists() {
                    report.failures.push(format!(
                        "engine binary for {} must not ship in the {} package",
                        entry.target().triple(),
                        platform.name()
                    ));
                }
                continue;
            }
            matched = true;
            let bytes = std::fs::read(&path).map_err(|_error| {
                ToolsPackagingError::Verification(format!(
                    "engine binary {} is missing",
                    path.display()
                ))
            })?;
            if u64::try_from(bytes.len()).ok() != Some(entry.bytes())
                || PackageDigest::hash_slice(&bytes) != entry.sha256()
            {
                report.failures.push(format!(
                    "engine binary {} does not match its manifest hash",
                    path.display()
                ));
            }
        }
        if !matched {
            report.failures.push(format!(
                "the engine manifest has no package for {}",
                platform.engine_triple()
            ));
        }
        Ok(())
    }

    fn walk(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), ToolsPackagingError> {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_dir() {
                Self::walk(&path, files)?;
            } else if path.is_file() {
                files.push(path);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ToolsPackagingEnumArchitecture, ToolsPackagingVerify};

    #[test]
    fn executable_headers_identify_the_architecture() {
        let mut elf = vec![0_u8; 32];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[18] = 0x3e;
        assert_eq!(
            ToolsPackagingVerify::architecture(&elf),
            Some(ToolsPackagingEnumArchitecture::ElfX64)
        );
        elf[18] = 0xb7;
        assert_eq!(
            ToolsPackagingVerify::architecture(&elf),
            Some(ToolsPackagingEnumArchitecture::ElfArm64)
        );
        let mut macho = vec![0_u8; 16];
        macho[..4].copy_from_slice(&0xfeed_facf_u32.to_le_bytes());
        macho[4..8].copy_from_slice(&0x0100_000c_u32.to_le_bytes());
        assert_eq!(
            ToolsPackagingVerify::architecture(&macho),
            Some(ToolsPackagingEnumArchitecture::MachoArm64)
        );
        assert_eq!(ToolsPackagingVerify::architecture(b"glTF"), None);
    }

    #[test]
    fn the_newest_glibc_symbol_version_is_found() {
        let bytes = b"xx GLIBC_2.17\0 GLIBC_2.34\0 GLIBC_2.5\0 GLIBC_PRIVATE";
        assert_eq!(ToolsPackagingVerify::max_glibc(bytes), Some((2, 34)));
        assert_eq!(ToolsPackagingVerify::max_glibc(b"no symbols"), None);
    }

    #[test]
    fn development_urls_and_test_listeners_are_flagged() {
        let found = ToolsPackagingVerify::contains_forbidden(b"connect http://localhost:1420 now");
        assert_eq!(found, vec!["http://localhost"]);
        assert!(ToolsPackagingVerify::contains_forbidden(b"clean bytes").is_empty());
    }
}
