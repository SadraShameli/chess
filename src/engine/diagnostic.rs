use crate::engine::search::position::EngineSearchPosition;

pub(crate) enum EngineDiagnosticEnumCommand<'a> {
    Inspector(&'a EngineSearchPosition),
    RawEvaluation(&'a EngineSearchPosition),
    Compiler,
    Perft {
        position: &'a EngineSearchPosition,
        depth: u32,
    },
    Bench {
        hash_mib: u32,
        threads: u32,
        depth: u32,
    },
}

impl EngineDiagnosticEnumCommand<'_> {
    pub(crate) fn position(&self) -> Option<&EngineSearchPosition> {
        match self {
            Self::Inspector(position)
            | Self::RawEvaluation(position)
            | Self::Perft { position, .. } => Some(position),
            Self::Compiler | Self::Bench { .. } => None,
        }
    }

    pub(crate) fn command(&self) -> String {
        match self {
            Self::Inspector(_) => "d".to_owned(),
            Self::RawEvaluation(_) => "eval".to_owned(),
            Self::Compiler => "compiler".to_owned(),
            Self::Perft { depth, .. } => format!("perft {depth}"),
            Self::Bench {
                hash_mib,
                threads,
                depth,
            } => format!("bench {hash_mib} {threads} {depth}"),
        }
    }
}
