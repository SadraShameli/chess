use std::collections::BTreeSet;
#[cfg(target_os = "linux")]
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EnginePackagePlatformEnumFeature {
    Sse2,
    Avx2,
    Bmi1,
    Bmi2,
    Fma,
    Lzcnt,
    Movbe,
    Avx512f,
    Avx512bw,
    Avx512cd,
    Avx512dq,
    Avx512vl,
    Neon,
}

impl EnginePackagePlatformEnumFeature {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sse2 => "SSE2",
            Self::Avx2 => "AVX2",
            Self::Bmi1 => "BMI1",
            Self::Bmi2 => "BMI2",
            Self::Fma => "FMA",
            Self::Lzcnt => "LZCNT",
            Self::Movbe => "MOVBE",
            Self::Avx512f => "AVX-512F",
            Self::Avx512bw => "AVX-512BW",
            Self::Avx512cd => "AVX-512CD",
            Self::Avx512dq => "AVX-512DQ",
            Self::Avx512vl => "AVX-512VL",
            Self::Neon => "NEON",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EnginePackagePlatformEnumOsState {
    YmmEnabled,
    ZmmEnabled,
}

impl EnginePackagePlatformEnumOsState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::YmmEnabled => "operating-system AVX register state",
            Self::ZmmEnabled => "operating-system AVX-512 register state",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnginePackagePlatform {
    features: BTreeSet<EnginePackagePlatformEnumFeature>,
    os_states: BTreeSet<EnginePackagePlatformEnumOsState>,
    numa_nodes: u16,
    libnuma: bool,
}

impl EnginePackagePlatform {
    pub fn from_observation(
        features: impl IntoIterator<Item = EnginePackagePlatformEnumFeature>,
        os_states: impl IntoIterator<Item = EnginePackagePlatformEnumOsState>,
        numa_nodes: u16,
        libnuma: bool,
    ) -> Self {
        Self {
            features: features.into_iter().collect(),
            os_states: os_states.into_iter().collect(),
            numa_nodes,
            libnuma,
        }
    }

    pub const fn numa_nodes(&self) -> u16 {
        self.numa_nodes
    }

    pub const fn has_libnuma(&self) -> bool {
        self.libnuma
    }

    pub fn has_feature(&self, feature: EnginePackagePlatformEnumFeature) -> bool {
        self.features.contains(&feature)
    }

    pub fn has_os_state(&self, state: EnginePackagePlatformEnumOsState) -> bool {
        self.os_states.contains(&state)
    }

    pub fn missing_features(
        &self,
        required: &[EnginePackagePlatformEnumFeature],
    ) -> Vec<EnginePackagePlatformEnumFeature> {
        required
            .iter()
            .copied()
            .filter(|feature| !self.has_feature(*feature))
            .collect()
    }

    pub fn missing_os_states(
        &self,
        required: &[EnginePackagePlatformEnumOsState],
    ) -> Vec<EnginePackagePlatformEnumOsState> {
        required
            .iter()
            .copied()
            .filter(|state| !self.has_os_state(*state))
            .collect()
    }

    pub fn detect() -> Self {
        let (features, os_states) = Self::detect_cpu();
        let (numa_nodes, libnuma) = Self::detect_topology();
        Self::from_observation(features, os_states, numa_nodes, libnuma)
    }

    fn detect_cpu() -> (
        Vec<EnginePackagePlatformEnumFeature>,
        Vec<EnginePackagePlatformEnumOsState>,
    ) {
        #[cfg(target_arch = "x86_64")]
        {
            let probes = [
                (
                    EnginePackagePlatformEnumFeature::Sse2,
                    std::arch::is_x86_feature_detected!("sse2"),
                ),
                (
                    EnginePackagePlatformEnumFeature::Avx2,
                    std::arch::is_x86_feature_detected!("avx2"),
                ),
                (
                    EnginePackagePlatformEnumFeature::Bmi1,
                    std::arch::is_x86_feature_detected!("bmi1"),
                ),
                (
                    EnginePackagePlatformEnumFeature::Bmi2,
                    std::arch::is_x86_feature_detected!("bmi2"),
                ),
                (
                    EnginePackagePlatformEnumFeature::Fma,
                    std::arch::is_x86_feature_detected!("fma"),
                ),
                (
                    EnginePackagePlatformEnumFeature::Lzcnt,
                    std::arch::is_x86_feature_detected!("lzcnt"),
                ),
                (
                    EnginePackagePlatformEnumFeature::Movbe,
                    std::arch::is_x86_feature_detected!("movbe"),
                ),
                (
                    EnginePackagePlatformEnumFeature::Avx512f,
                    std::arch::is_x86_feature_detected!("avx512f"),
                ),
                (
                    EnginePackagePlatformEnumFeature::Avx512bw,
                    std::arch::is_x86_feature_detected!("avx512bw"),
                ),
                (
                    EnginePackagePlatformEnumFeature::Avx512cd,
                    std::arch::is_x86_feature_detected!("avx512cd"),
                ),
                (
                    EnginePackagePlatformEnumFeature::Avx512dq,
                    std::arch::is_x86_feature_detected!("avx512dq"),
                ),
                (
                    EnginePackagePlatformEnumFeature::Avx512vl,
                    std::arch::is_x86_feature_detected!("avx512vl"),
                ),
            ];
            let features: Vec<_> = probes
                .into_iter()
                .filter_map(|(feature, present)| present.then_some(feature))
                .collect();
            let mut states = Vec::new();
            if std::arch::is_x86_feature_detected!("avx") {
                states.push(EnginePackagePlatformEnumOsState::YmmEnabled);
            }
            if std::arch::is_x86_feature_detected!("avx512f") {
                states.push(EnginePackagePlatformEnumOsState::ZmmEnabled);
            }
            (features, states)
        }
        #[cfg(target_arch = "aarch64")]
        {
            let features = if std::arch::is_aarch64_feature_detected!("neon") {
                vec![EnginePackagePlatformEnumFeature::Neon]
            } else {
                Vec::new()
            };
            (features, Vec::new())
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            (Vec::new(), Vec::new())
        }
    }

    fn detect_topology() -> (u16, bool) {
        #[cfg(target_os = "linux")]
        {
            let nodes = std::fs::read_dir("/sys/devices/system/node")
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|entry| {
                            entry.file_name().to_str().is_some_and(|name| {
                                name.strip_prefix("node")
                                    .is_some_and(|rest| rest.bytes().all(|byte| byte.is_ascii_digit()))
                            })
                        })
                        .count()
                })
                .unwrap_or(1);
            let libnuma = [
                "/usr/lib/x86_64-linux-gnu/libnuma.so.1",
                "/lib/x86_64-linux-gnu/libnuma.so.1",
                "/usr/lib64/libnuma.so.1",
            ]
            .iter()
            .any(|path| Path::new(path).is_file());
            (u16::try_from(nodes).unwrap_or(u16::MAX).max(1), libnuma)
        }
        #[cfg(not(target_os = "linux"))]
        {
            (1, false)
        }
    }
}
