use std::fmt::Write;
use std::num::NonZeroU32;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::board::enums::BoardEnumColor;
use crate::board::position::BoardPosition;
use crate::engine::analysis::{
    EngineAnalysis, EngineAnalysisEnumBound, EngineAnalysisEnumScore, EngineAnalysisError,
    EngineAnalysisHashOccupancy, EngineAnalysisLine,
};
use crate::engine::capabilities::persistence::{
    EnginePersistenceError, EngineRecordJson, EngineRecordList, EngineRecordText,
};
use crate::engine::process_provider::EngineProcessLimits;

#[derive(Serialize, Deserialize)]
#[serde(
    remote = "EngineAnalysisEnumScore",
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum EngineAnalysisRecordEnumScore {
    Centipawns(i32),
    Mate(i32),
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "EngineAnalysisEnumBound", rename_all = "snake_case")]
enum EngineAnalysisRecordEnumBound {
    Exact,
    Lower,
    Upper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineAnalysisRecordScore {
    #[serde(with = "EngineAnalysisRecordEnumScore")]
    reported: EngineAnalysisEnumScore,
    #[serde(with = "EngineAnalysisRecordEnumBound")]
    bound: EngineAnalysisEnumBound,
    perspective: BoardEnumColor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineAnalysisLineRecordData {
    depth: Option<u32>,
    selective_depth: Option<u32>,
    rank: Option<NonZeroU32>,
    score: Option<EngineAnalysisRecordScore>,
    nodes: Option<u64>,
    elapsed_millis: Option<u64>,
    nodes_per_second: Option<u64>,
    hash_occupancy: Option<u16>,
    tablebase_hits: Option<u64>,
    variation:
        Option<EngineRecordList<EngineRecordText<5>, { EngineAnalysisLine::MAX_VARIATION_PLIES }>>,
}

impl EngineAnalysisLineRecordData {
    fn validate(&self) -> Result<(), EnginePersistenceError> {
        if let Some(value) = self.hash_occupancy {
            EngineAnalysis::parse(
                &format!("info hashfull {value}"),
                &BoardPosition::standard(),
            )?;
        }
        if self
            .variation
            .as_ref()
            .is_some_and(|variation| variation.as_slice().is_empty())
        {
            return Err(EngineAnalysisError::Field("pv").into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineAnalysisLineRecord {
    data: EngineAnalysisLineRecordData,
}

impl EngineAnalysisLineRecord {
    pub const MAX_BYTES: usize = 16 * 1024;

    pub fn capture(
        line: &EngineAnalysisLine,
        position: &BoardPosition,
    ) -> Result<Self, EnginePersistenceError> {
        let elapsed_millis = line
            .elapsed()
            .map(|elapsed| {
                let millis = u64::try_from(elapsed.as_millis())?;
                if elapsed != Duration::from_millis(millis) {
                    return Err(EnginePersistenceError::FractionalMilliseconds);
                }
                Ok(millis)
            })
            .transpose()?;
        let variation = line
            .variation()
            .map(|moves| {
                if moves.len() > EngineAnalysisLine::MAX_VARIATION_PLIES {
                    return Err(EnginePersistenceError::Bounds);
                }
                let mut variation = EngineRecordList::new();
                for movement in moves {
                    variation.push(EngineRecordText::new(&movement.uci(position.variant()))?)?;
                }
                Ok(variation)
            })
            .transpose()?;
        let record = Self {
            data: EngineAnalysisLineRecordData {
                depth: line.depth(),
                selective_depth: line.selective_depth(),
                rank: line.rank(),
                score: line.score().map(|score| EngineAnalysisRecordScore {
                    reported: score.reported(),
                    bound: score.bound(),
                    perspective: score.perspective(),
                }),
                nodes: line.nodes(),
                elapsed_millis,
                nodes_per_second: line.nodes_per_second(),
                hash_occupancy: line
                    .hash_occupancy()
                    .map(EngineAnalysisHashOccupancy::per_mille),
                tablebase_hits: line.tablebase_hits(),
                variation,
            },
        };
        if record.restore(position)? != *line {
            return Err(EnginePersistenceError::Context);
        }
        Ok(record)
    }

    fn statistic(
        text: &mut EngineRecordText<{ EngineProcessLimits::LINE_BYTES }>,
        name: &str,
        value: Option<impl std::fmt::Display>,
    ) -> Result<(), EnginePersistenceError> {
        if let Some(value) = value {
            write!(text, " {name} {value}").map_err(|_error| EnginePersistenceError::Bounds)?;
        }
        Ok(())
    }

    pub fn restore(
        &self,
        position: &BoardPosition,
    ) -> Result<EngineAnalysisLine, EnginePersistenceError> {
        if self
            .data
            .score
            .is_some_and(|score| score.perspective != position.turn())
        {
            return Err(EnginePersistenceError::Perspective);
        }
        let mut text = EngineRecordText::new("info")?;
        Self::statistic(&mut text, "depth", self.data.depth)?;
        Self::statistic(&mut text, "seldepth", self.data.selective_depth)?;
        Self::statistic(&mut text, "multipv", self.data.rank)?;
        if let Some(score) = self.data.score {
            match score.reported {
                EngineAnalysisEnumScore::Centipawns(value) => write!(text, " score cp {value}"),
                EngineAnalysisEnumScore::Mate(value) => write!(text, " score mate {value}"),
            }
            .map_err(|_error| EnginePersistenceError::Bounds)?;
            text.write_str(match score.bound {
                EngineAnalysisEnumBound::Exact => "",
                EngineAnalysisEnumBound::Lower => " lowerbound",
                EngineAnalysisEnumBound::Upper => " upperbound",
            })
            .map_err(|_error| EnginePersistenceError::Bounds)?;
        }
        Self::statistic(&mut text, "nodes", self.data.nodes)?;
        Self::statistic(&mut text, "time", self.data.elapsed_millis)?;
        Self::statistic(&mut text, "nps", self.data.nodes_per_second)?;
        Self::statistic(&mut text, "hashfull", self.data.hash_occupancy)?;
        Self::statistic(&mut text, "tbhits", self.data.tablebase_hits)?;
        if let Some(variation) = &self.data.variation {
            text.write_str(" pv")
                .map_err(|_error| EnginePersistenceError::Bounds)?;
            for movement in variation.as_slice() {
                write!(text, " {}", movement.as_str())
                    .map_err(|_error| EnginePersistenceError::Bounds)?;
            }
        }
        if text.as_str() == "info" {
            return Ok(EngineAnalysisLine::default());
        }
        EngineAnalysis::parse(text.as_str(), position)?
            .into_line()
            .ok_or_else(|| EngineAnalysisError::Line.into())
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, EnginePersistenceError> {
        EngineRecordJson::parse::<Self, { Self::MAX_BYTES }>(bytes)
    }
}

impl Serialize for EngineAnalysisLineRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EngineRecordJson::serialize(&self.data, serializer)
    }
}

impl<'de> Deserialize<'de> for EngineAnalysisLineRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let record = Self {
            data: EngineRecordJson::deserialize::<
                D,
                EngineAnalysisLineRecordData,
                { Self::MAX_BYTES },
            >(deserializer)?,
        };
        record.data.validate().map_err(serde::de::Error::custom)?;
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::board::position::BoardPosition;
    use crate::engine::analysis::EngineAnalysisLine;
    use crate::engine::analysis::persistence::EngineAnalysisLineRecord;
    use crate::engine::capabilities::persistence::EnginePersistenceError;

    #[test]
    fn capture_refuses_fractional_milliseconds_without_rounding() {
        let line = EngineAnalysisLine {
            elapsed: Some(Duration::from_nanos(1_000_001)),
            ..EngineAnalysisLine::default()
        };
        assert!(matches!(
            EngineAnalysisLineRecord::capture(&line, &BoardPosition::standard()),
            Err(EnginePersistenceError::FractionalMilliseconds)
        ));
    }
}
