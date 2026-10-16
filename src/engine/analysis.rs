use std::num::NonZeroU32;
use std::str::SplitAsciiWhitespace;
use std::time::Duration;

use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::BoardPosition;
use crate::engine::process_provider::EngineProcessLimits;

#[derive(Debug, Error)]
pub enum EngineAnalysisError {
    #[error("The engine analysis line is empty, oversized or not a single info response")]
    Line,
    #[error("The engine analysis contains an invalid, missing or duplicate {0} field")]
    Field(&'static str),
    #[error("The engine analysis contains an unknown field after structured statistics")]
    UnknownField,
    #[error("The engine analysis variation is illegal: {0}")]
    Variation(#[from] BoardMoveError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineAnalysisEnumBound {
    Exact,
    Lower,
    Upper,
}

impl EngineAnalysisEnumBound {
    const fn opposite(self) -> Self {
        match self {
            Self::Exact => Self::Exact,
            Self::Lower => Self::Upper,
            Self::Upper => Self::Lower,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineAnalysisEnumScore {
    Centipawns(i32),
    Mate(i32),
}

impl EngineAnalysisEnumScore {
    const fn value(self) -> i32 {
        match self {
            Self::Centipawns(value) | Self::Mate(value) => value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineAnalysisScore {
    reported: EngineAnalysisEnumScore,
    bound: EngineAnalysisEnumBound,
    perspective: BoardEnumColor,
}

impl EngineAnalysisScore {
    pub const fn reported(self) -> EngineAnalysisEnumScore {
        self.reported
    }

    pub const fn bound(self) -> EngineAnalysisEnumBound {
        self.bound
    }

    pub const fn perspective(self) -> BoardEnumColor {
        self.perspective
    }

    pub fn white_value(self) -> i64 {
        let value = i64::from(self.reported.value());
        match self.perspective {
            BoardEnumColor::White => value,
            BoardEnumColor::Black => -value,
        }
    }

    pub const fn white_bound(self) -> EngineAnalysisEnumBound {
        match self.perspective {
            BoardEnumColor::White => self.bound,
            BoardEnumColor::Black => self.bound.opposite(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineAnalysisStaticEvaluation(i32);

impl EngineAnalysisStaticEvaluation {
    pub fn parse(text: &str) -> Result<Self, EngineAnalysisError> {
        if text.len() > EngineProcessLimits::LINE_BYTES || text.contains(['\r', '\n']) {
            return Err(EngineAnalysisError::Line);
        }
        text.trim()
            .parse()
            .map(Self)
            .map_err(|_error| EngineAnalysisError::Field("static evaluation"))
    }

    pub const fn white_value(self) -> i32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineAnalysisHashOccupancy(u16);

impl EngineAnalysisHashOccupancy {
    pub const FULL: u16 = 1_000;

    pub const fn per_mille(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineAnalysisLine {
    depth: Option<u32>,
    selective_depth: Option<u32>,
    rank: Option<NonZeroU32>,
    score: Option<EngineAnalysisScore>,
    nodes: Option<u64>,
    elapsed: Option<Duration>,
    nodes_per_second: Option<u64>,
    hash_occupancy: Option<EngineAnalysisHashOccupancy>,
    tablebase_hits: Option<u64>,
    variation: Option<Vec<BoardMove>>,
}

impl EngineAnalysisLine {
    pub const MAX_VARIATION_PLIES: usize = 130;

    pub const fn depth(&self) -> Option<u32> {
        self.depth
    }

    pub const fn selective_depth(&self) -> Option<u32> {
        self.selective_depth
    }

    pub const fn rank(&self) -> Option<NonZeroU32> {
        self.rank
    }

    pub const fn score(&self) -> Option<EngineAnalysisScore> {
        self.score
    }

    pub const fn nodes(&self) -> Option<u64> {
        self.nodes
    }

    pub const fn elapsed(&self) -> Option<Duration> {
        self.elapsed
    }

    pub const fn nodes_per_second(&self) -> Option<u64> {
        self.nodes_per_second
    }

    pub const fn hash_occupancy(&self) -> Option<EngineAnalysisHashOccupancy> {
        self.hash_occupancy
    }

    pub const fn tablebase_hits(&self) -> Option<u64> {
        self.tablebase_hits
    }

    pub fn variation(&self) -> Option<&[BoardMove]> {
        self.variation.as_deref()
    }

    fn assign<T>(
        destination: &mut Option<T>,
        value: T,
        name: &'static str,
    ) -> Result<(), EngineAnalysisError> {
        if destination.is_some() {
            return Err(EngineAnalysisError::Field(name));
        }
        *destination = Some(value);
        Ok(())
    }

    fn number<T: std::str::FromStr>(
        tokens: &mut SplitAsciiWhitespace<'_>,
        name: &'static str,
    ) -> Result<T, EngineAnalysisError> {
        tokens
            .next()
            .and_then(|text| text.parse().ok())
            .ok_or(EngineAnalysisError::Field(name))
    }

    fn parse(
        mut tokens: SplitAsciiWhitespace<'_>,
        position: &BoardPosition,
    ) -> Result<Self, EngineAnalysisError> {
        let mut line = Self::default();
        while let Some(field) = tokens.next() {
            match field {
                "depth" => Self::assign(
                    &mut line.depth,
                    Self::number(&mut tokens, "depth")?,
                    "depth",
                )?,
                "seldepth" => Self::assign(
                    &mut line.selective_depth,
                    Self::number(&mut tokens, "seldepth")?,
                    "seldepth",
                )?,
                "multipv" => {
                    let rank: NonZeroU32 = Self::number(&mut tokens, "multipv")?;
                    let legal_count = position.legal_moves().len();
                    if usize::try_from(rank.get()).map_or(true, |value| value > legal_count) {
                        return Err(EngineAnalysisError::Field("multipv"));
                    }
                    Self::assign(&mut line.rank, rank, "multipv")?;
                }
                "score" => {
                    let kind = tokens.next().ok_or(EngineAnalysisError::Field("score"))?;
                    let value = Self::number(&mut tokens, "score")?;
                    let reported = match kind {
                        "cp" => EngineAnalysisEnumScore::Centipawns(value),
                        "mate" => EngineAnalysisEnumScore::Mate(value),
                        _ => return Err(EngineAnalysisError::Field("score")),
                    };
                    Self::assign(
                        &mut line.score,
                        EngineAnalysisScore {
                            reported,
                            bound: EngineAnalysisEnumBound::Exact,
                            perspective: position.turn(),
                        },
                        "score",
                    )?;
                }
                "lowerbound" | "upperbound" => {
                    let score = line
                        .score
                        .as_mut()
                        .ok_or(EngineAnalysisError::Field("bound"))?;
                    if score.bound != EngineAnalysisEnumBound::Exact {
                        return Err(EngineAnalysisError::Field("bound"));
                    }
                    score.bound = if field == "lowerbound" {
                        EngineAnalysisEnumBound::Lower
                    } else {
                        EngineAnalysisEnumBound::Upper
                    };
                }
                "nodes" => Self::assign(
                    &mut line.nodes,
                    Self::number(&mut tokens, "nodes")?,
                    "nodes",
                )?,
                "time" => Self::assign(
                    &mut line.elapsed,
                    Duration::from_millis(Self::number(&mut tokens, "time")?),
                    "time",
                )?,
                "nps" => Self::assign(
                    &mut line.nodes_per_second,
                    Self::number(&mut tokens, "nps")?,
                    "nps",
                )?,
                "hashfull" => {
                    let value = Self::number(&mut tokens, "hashfull")?;
                    if value > EngineAnalysisHashOccupancy::FULL {
                        return Err(EngineAnalysisError::Field("hashfull"));
                    }
                    Self::assign(
                        &mut line.hash_occupancy,
                        EngineAnalysisHashOccupancy(value),
                        "hashfull",
                    )?;
                }
                "tbhits" => Self::assign(
                    &mut line.tablebase_hits,
                    Self::number(&mut tokens, "tbhits")?,
                    "tbhits",
                )?,
                "pv" => {
                    let mut current = position.clone();
                    let mut variation = Vec::new();
                    for text in tokens.by_ref() {
                        if variation.len() >= Self::MAX_VARIATION_PLIES {
                            return Err(EngineAnalysisError::Field("pv"));
                        }
                        let movement = current.parse_uci(text)?;
                        current = current.play(movement)?;
                        variation.push(movement);
                    }
                    if variation.is_empty() {
                        return Err(EngineAnalysisError::Field("pv"));
                    }
                    line.variation = Some(variation);
                }
                _ => return Err(EngineAnalysisError::UnknownField),
            }
        }
        Ok(line)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineAnalysis {
    text: String,
    line: Option<EngineAnalysisLine>,
}

impl EngineAnalysis {
    pub fn parse(text: &str, position: &BoardPosition) -> Result<Self, EngineAnalysisError> {
        if text.len() > EngineProcessLimits::LINE_BYTES || text.contains(['\r', '\n']) {
            return Err(EngineAnalysisError::Line);
        }
        let mut tokens = text.split_ascii_whitespace();
        if tokens.next() != Some("info") {
            return Err(EngineAnalysisError::Line);
        }
        let first = tokens.clone().next().ok_or(EngineAnalysisError::Line)?;
        let line = match first {
            "depth" | "seldepth" | "multipv" | "score" | "lowerbound" | "upperbound" | "nodes"
            | "time" | "nps" | "hashfull" | "tbhits" | "pv" => {
                Some(EngineAnalysisLine::parse(tokens, position)?)
            }
            _ => None,
        };
        Ok(Self {
            text: text.to_owned(),
            line,
        })
    }

    pub fn line(&self) -> Option<&EngineAnalysisLine> {
        self.line.as_ref()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn into_text(self) -> String {
        self.text
    }
}
