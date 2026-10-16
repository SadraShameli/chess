use std::sync::Arc;

use crate::board::enums::BoardEnumColor;
use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::BoardPosition;
use crate::engine::analysis::{
    EngineAnalysisEnumBound, EngineAnalysisEnumScore, EngineAnalysisLine, EngineAnalysisScore,
};
use crate::engine::search::session::EngineSearchSessionSnapshot;

pub(super) struct PresentationAnalysisLine {
    source: Arc<EngineAnalysisLine>,
    variation: String,
}

impl PresentationAnalysisLine {
    pub(super) fn from_snapshot(
        snapshot: &EngineSearchSessionSnapshot,
        previous: &[Self],
    ) -> Result<Vec<Self>, BoardMoveError> {
        let mut lines = snapshot
            .lines()
            .iter()
            .map(|line| {
                Ok(Self {
                    source: Arc::clone(line),
                    variation: match previous
                        .iter()
                        .find(|previous| Arc::ptr_eq(&previous.source, line))
                    {
                        Some(previous) => previous.variation.clone(),
                        None => Self::variation(
                            snapshot.metadata().position().current(),
                            line.variation(),
                        )?,
                    },
                })
            })
            .collect::<Result<Vec<_>, BoardMoveError>>()?;
        lines.sort_by_key(Self::rank);
        Ok(lines)
    }

    pub(super) fn rank(&self) -> u32 {
        self.source.rank().map_or(1, std::num::NonZeroU32::get)
    }
    pub(super) fn source(&self) -> &EngineAnalysisLine {
        &self.source
    }
    pub(super) fn variation_text(&self) -> &str {
        &self.variation
    }

    fn variation(
        position: &BoardPosition,
        moves: Option<&[BoardMove]>,
    ) -> Result<String, BoardMoveError> {
        let Some(moves) = moves else {
            return Ok("Variation unavailable".to_owned());
        };
        let mut position = position.clone();
        let mut parts = Vec::with_capacity(moves.len());
        for (index, movement) in moves.iter().enumerate() {
            let san = position.san(&movement.uci(position.variant()))?;
            let text = match position.turn() {
                BoardEnumColor::White => format!("{}. {san}", position.fullmoves()),
                BoardEnumColor::Black if index == 0 => format!("{}... {san}", position.fullmoves()),
                BoardEnumColor::Black => san,
            };
            parts.push(text);
            position = position.play(*movement)?;
        }
        Ok(parts.join(" "))
    }
}

pub(super) struct PresentationAnalysisLineScore(EngineAnalysisScore);

impl PresentationAnalysisLineScore {
    pub(super) const fn new(score: EngineAnalysisScore) -> Self {
        Self(score)
    }

    pub(super) fn white_label(&self) -> String {
        format!(
            "White: {}{} ({})",
            Self::inequality(self.0.white_bound()),
            self.value(self.0.white_value()),
            Self::bound(self.0.white_bound())
        )
    }

    pub(super) fn reported_label(&self) -> String {
        let side = match self.0.perspective() {
            BoardEnumColor::White => "White",
            BoardEnumColor::Black => "Black",
        };
        let value = match self.0.reported() {
            EngineAnalysisEnumScore::Centipawns(value) => format!("{value:+} cp"),
            EngineAnalysisEnumScore::Mate(value) => format!("mate {value:+}"),
        };
        format!(
            "Reported {side}: {}{value} ({})",
            Self::inequality(self.0.bound()),
            Self::bound(self.0.bound())
        )
    }

    pub(super) fn fraction(&self) -> Result<f32, std::num::TryFromIntError> {
        Ok(match self.0.reported() {
            EngineAnalysisEnumScore::Centipawns(_) => {
                let cp = self.0.white_value().clamp(-10_000, 10_000);
                let cp = f32::from(i16::try_from(cp)?);
                1.0 / (1.0 + (-cp / 400.0).exp())
            }
            EngineAnalysisEnumScore::Mate(_) => match self.0.white_value().cmp(&0) {
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Less => 0.0,
                std::cmp::Ordering::Equal => match self.0.perspective() {
                    BoardEnumColor::White => 0.0,
                    BoardEnumColor::Black => 1.0,
                },
            },
        })
    }

    pub(super) fn is_bound(&self) -> bool {
        self.0.white_bound() != EngineAnalysisEnumBound::Exact
    }

    fn value(&self, value: i64) -> String {
        match self.0.reported() {
            EngineAnalysisEnumScore::Centipawns(_) => {
                let sign = if value < 0 { "-" } else { "+" };
                let magnitude = value.unsigned_abs();
                format!("{sign}{}.{:02} pawns", magnitude / 100, magnitude % 100)
            }
            EngineAnalysisEnumScore::Mate(_) => format!("mate {value:+}"),
        }
    }

    const fn inequality(bound: EngineAnalysisEnumBound) -> &'static str {
        match bound {
            EngineAnalysisEnumBound::Exact => "",
            EngineAnalysisEnumBound::Lower => "≥ ",
            EngineAnalysisEnumBound::Upper => "≤ ",
        }
    }

    const fn bound(bound: EngineAnalysisEnumBound) -> &'static str {
        match bound {
            EngineAnalysisEnumBound::Exact => "exact",
            EngineAnalysisEnumBound::Lower => "lower bound",
            EngineAnalysisEnumBound::Upper => "upper bound",
        }
    }
}
