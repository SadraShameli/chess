#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EngineEnumOption {
    Hash,
    Threads,
    MoveOverhead,
    Minimal,
    ClearHash,
    Chess960,
    MultiPv,
    SyzygyPath,
}

impl EngineEnumOption {
    pub const ALL: [Self; 8] = [
        Self::Hash,
        Self::Threads,
        Self::MoveOverhead,
        Self::Minimal,
        Self::ClearHash,
        Self::Chess960,
        Self::MultiPv,
        Self::SyzygyPath,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Hash => "Hash",
            Self::Threads => "Threads",
            Self::MoveOverhead => "MoveOverhead",
            Self::Minimal => "Minimal",
            Self::ClearHash => "Clear Hash",
            Self::Chess960 => "UCI_Chess960",
            Self::MultiPv => "MultiPV",
            Self::SyzygyPath => "SyzygyPath",
        }
    }

    pub const fn descriptor(self) -> &'static str {
        match self {
            Self::Hash => "spin default 16 min 1 max 262144",
            Self::Threads => "spin default 1 min 1 max 512",
            Self::MoveOverhead => "spin default 100 min 0 max 2000",
            Self::Minimal | Self::Chess960 => "check default false",
            Self::ClearHash => "button",
            Self::MultiPv => "spin default 1 min 1 max 256",
            Self::SyzygyPath => "string default",
        }
    }
}
