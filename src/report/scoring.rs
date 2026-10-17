use thiserror::Error;

use crate::board::enums::BoardEnumColor;
use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::BoardPosition;
use crate::engine::analysis::{
    EngineAnalysisEnumBound, EngineAnalysisEnumScore, EngineAnalysisScore,
};

#[derive(Debug, Error)]
pub enum ReportScoringError {
    #[error("Expected-score loss must be finite and between zero and one")]
    InvalidLoss,
    #[error("An adjacent report score has the wrong reported board perspective")]
    ScorePerspective,
    #[error("The report sample count exceeds its supported range")]
    SampleCount,
    #[error(transparent)]
    Move(#[from] BoardMoveError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportScoringEnumPolicyVersion {
    ExpectedScoreV1,
}

impl ReportScoringEnumPolicyVersion {
    const CP_MIN: i64 = -10_000;
    const CP_MAX: i64 = 10_000;
    const CP_SCALE: f64 = 400.0;
    const INACCURACY: f64 = 0.05;
    const MISTAKE: f64 = 0.10;
    const BLUNDER: f64 = 0.20;
    const PERCENT: f64 = 100.0;

    pub const fn version(self) -> u32 {
        match self {
            Self::ExpectedScoreV1 => 1,
        }
    }

    pub fn classify_loss(self, loss: f64) -> Result<ReportScoringEnumLabel, ReportScoringError> {
        if !loss.is_finite() || !(0.0..=1.0).contains(&loss) {
            return Err(ReportScoringError::InvalidLoss);
        }
        Ok(self.label(loss))
    }

    fn label(self, loss: f64) -> ReportScoringEnumLabel {
        match self {
            Self::ExpectedScoreV1 if loss >= Self::BLUNDER => ReportScoringEnumLabel::Blunder,
            Self::ExpectedScoreV1 if loss >= Self::MISTAKE => ReportScoringEnumLabel::Mistake,
            Self::ExpectedScoreV1 if loss >= Self::INACCURACY => ReportScoringEnumLabel::Inaccuracy,
            Self::ExpectedScoreV1 => ReportScoringEnumLabel::Acceptable,
        }
    }

    fn expected(
        self,
        evaluation: ReportScoringEvaluation,
    ) -> Result<f64, ReportScoringEnumUnscored> {
        match evaluation {
            ReportScoringEvaluation::TerminalWinner(BoardEnumColor::White) => Ok(1.0),
            ReportScoringEvaluation::TerminalWinner(BoardEnumColor::Black) => Ok(0.0),
            ReportScoringEvaluation::TerminalDraw => Ok(0.5),
            ReportScoringEvaluation::Missing => Err(ReportScoringEnumUnscored::Missing),
            ReportScoringEvaluation::Search(score) => {
                match score.white_bound() {
                    EngineAnalysisEnumBound::Lower | EngineAnalysisEnumBound::Upper => {
                        return Err(ReportScoringEnumUnscored::Bound(score));
                    }
                    EngineAnalysisEnumBound::Exact => {}
                }
                match score.reported() {
                    EngineAnalysisEnumScore::Centipawns(_) => {
                        let cp = score.white_value().clamp(Self::CP_MIN, Self::CP_MAX);
                        let cp = i32::try_from(cp)
                            .map_err(|_error| ReportScoringEnumUnscored::NumericRange)?;
                        Ok(match self {
                            Self::ExpectedScoreV1 => {
                                1.0 / (1.0 + (-f64::from(cp) / Self::CP_SCALE).exp())
                            }
                        })
                    }
                    EngineAnalysisEnumScore::Mate(0) => {
                        Err(ReportScoringEnumUnscored::IndeterminateMate)
                    }
                    EngineAnalysisEnumScore::Mate(_) => {
                        Ok(if score.white_value() > 0 { 1.0 } else { 0.0 })
                    }
                }
            }
        }
    }

    fn mover_expected(self, white: f64, mover: BoardEnumColor) -> f64 {
        match (self, mover) {
            (Self::ExpectedScoreV1, BoardEnumColor::White) => white,
            (Self::ExpectedScoreV1, BoardEnumColor::Black) => 1.0 - white,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportScoringEnumLabel {
    Acceptable,
    Inaccuracy,
    Mistake,
    Blunder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportScoringEnumUnscored {
    Missing,
    Bound(EngineAnalysisScore),
    IndeterminateMate,
    NumericRange,
}

#[derive(Debug, Clone, Copy)]
enum ReportScoringEvaluation {
    Search(EngineAnalysisScore),
    TerminalWinner(BoardEnumColor),
    TerminalDraw,
    Missing,
}

impl ReportScoringEvaluation {
    fn at(position: &BoardPosition, score: Option<EngineAnalysisScore>) -> Self {
        if position.is_checkmate() {
            Self::TerminalWinner(position.turn().opposite())
        } else if position.is_stalemate()
            || (position.has_insufficient_mating_material(BoardEnumColor::White)
                && position.has_insufficient_mating_material(BoardEnumColor::Black))
        {
            Self::TerminalDraw
        } else {
            score.map_or(Self::Missing, Self::Search)
        }
    }

    fn white_mate(self) -> Option<i64> {
        match self {
            Self::Search(score) => match score.reported() {
                EngineAnalysisEnumScore::Mate(_)
                    if score.white_bound() == EngineAnalysisEnumBound::Exact =>
                {
                    Some(score.white_value())
                }
                EngineAnalysisEnumScore::Mate(_) | EngineAnalysisEnumScore::Centipawns(_) => None,
            },
            Self::TerminalWinner(_) | Self::TerminalDraw | Self::Missing => None,
        }
    }

    fn mate_winner(self) -> Option<BoardEnumColor> {
        match self {
            Self::TerminalWinner(winner) => Some(winner),
            Self::Search(_) => self.white_mate().and_then(|distance| {
                if distance > 0 {
                    Some(BoardEnumColor::White)
                } else if distance < 0 {
                    Some(BoardEnumColor::Black)
                } else {
                    None
                }
            }),
            Self::TerminalDraw | Self::Missing => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReportScoringSample {
    mover: BoardEnumColor,
    forced: bool,
    before: ReportScoringEvaluation,
    after: ReportScoringEvaluation,
}

impl ReportScoringSample {
    pub fn new(
        position: &BoardPosition,
        movement: BoardMove,
        before: Option<EngineAnalysisScore>,
        after: Option<EngineAnalysisScore>,
    ) -> Result<Self, ReportScoringError> {
        let next = position.play(movement)?;
        if before.is_some_and(|score| score.perspective() != position.turn())
            || after.is_some_and(|score| score.perspective() != next.turn())
        {
            return Err(ReportScoringError::ScorePerspective);
        }
        Ok(Self {
            mover: position.turn(),
            forced: position.legal_moves().len() == 1,
            before: ReportScoringEvaluation::at(position, before),
            after: ReportScoringEvaluation::at(&next, after),
        })
    }

    pub const fn mover(self) -> BoardEnumColor {
        self.mover
    }
    pub const fn is_forced(self) -> bool {
        self.forced
    }

    pub fn assess(self, policy: ReportScoringEnumPolicyVersion) -> ReportScoringEnumAssessment {
        let before = policy.expected(self.before);
        let after = policy.expected(self.after);
        match (before, after) {
            (Ok(before), Ok(after)) => {
                let loss = (policy.mover_expected(before, self.mover)
                    - policy.mover_expected(after, self.mover))
                .max(0.0);
                ReportScoringEnumAssessment::Scored(ReportScoringMove {
                    loss,
                    label: policy.label(loss),
                    missed_mate: self.before.mate_winner() == Some(self.mover)
                        && self.after.mate_winner() != Some(self.mover),
                    before_white_mate: self.before.white_mate(),
                    after_white_mate: self.after.white_mate(),
                })
            }
            (before, after) => ReportScoringEnumAssessment::Unscored {
                before: before.err(),
                after: after.err(),
            },
        }
    }

    pub(crate) fn with_terminal_draw(mut self) -> Self {
        self.after = ReportScoringEvaluation::TerminalDraw;
        self
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ReportScoringEnumAssessment {
    Scored(ReportScoringMove),
    Unscored {
        before: Option<ReportScoringEnumUnscored>,
        after: Option<ReportScoringEnumUnscored>,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct ReportScoringMove {
    loss: f64,
    label: ReportScoringEnumLabel,
    missed_mate: bool,
    before_white_mate: Option<i64>,
    after_white_mate: Option<i64>,
}

impl ReportScoringMove {
    pub const fn loss(self) -> f64 {
        self.loss
    }
    pub const fn label(self) -> ReportScoringEnumLabel {
        self.label
    }
    pub const fn is_missed_mate(self) -> bool {
        self.missed_mate
    }
    pub const fn before_white_mate(self) -> Option<i64> {
        self.before_white_mate
    }
    pub const fn after_white_mate(self) -> Option<i64> {
        self.after_white_mate
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportScoringEnumCoverage {
    Partial,
    Complete,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ReportScoringPlayerSummary {
    scored: u32,
    forced: u32,
    unscored: u32,
    loss_sum: f64,
}

impl ReportScoringPlayerSummary {
    fn record(
        &mut self,
        sample: ReportScoringSample,
        policy: ReportScoringEnumPolicyVersion,
    ) -> Result<(), ReportScoringError> {
        let count = if sample.is_forced() {
            &mut self.forced
        } else {
            match sample.assess(policy) {
                ReportScoringEnumAssessment::Scored(scored) => {
                    self.loss_sum += scored.loss();
                    &mut self.scored
                }
                ReportScoringEnumAssessment::Unscored { .. } => &mut self.unscored,
            }
        };
        *count = count
            .checked_add(1)
            .ok_or(ReportScoringError::SampleCount)?;
        Ok(())
    }

    pub const fn scored_moves(self) -> u32 {
        self.scored
    }
    pub const fn forced_moves(self) -> u32 {
        self.forced
    }
    pub const fn unscored_moves(self) -> u32 {
        self.unscored
    }

    pub fn accuracy(self) -> Option<f64> {
        (self.scored > 0).then(|| {
            (ReportScoringEnumPolicyVersion::PERCENT
                * (1.0 - self.loss_sum / f64::from(self.scored)))
            .clamp(0.0, ReportScoringEnumPolicyVersion::PERCENT)
        })
    }

    pub fn display_accuracy(self) -> Option<String> {
        self.accuracy().map(|accuracy| format!("{accuracy:.1}"))
    }
}

#[derive(Debug)]
pub struct ReportScoringSummary {
    policy: ReportScoringEnumPolicyVersion,
    coverage: ReportScoringEnumCoverage,
    white: ReportScoringPlayerSummary,
    black: ReportScoringPlayerSummary,
}

impl ReportScoringSummary {
    pub fn new<'a>(
        policy: ReportScoringEnumPolicyVersion,
        coverage: ReportScoringEnumCoverage,
        samples: impl IntoIterator<Item = &'a ReportScoringSample>,
    ) -> Result<Self, ReportScoringError> {
        let mut summary = Self {
            policy,
            coverage,
            white: ReportScoringPlayerSummary::default(),
            black: ReportScoringPlayerSummary::default(),
        };
        for sample in samples {
            summary.record(*sample)?;
        }
        Ok(summary)
    }

    pub(crate) fn record(&mut self, sample: ReportScoringSample) -> Result<(), ReportScoringError> {
        let player = match sample.mover() {
            BoardEnumColor::White => &mut self.white,
            BoardEnumColor::Black => &mut self.black,
        };
        player.record(sample, self.policy)?;
        if player.unscored_moves() > 0 {
            self.coverage = ReportScoringEnumCoverage::Partial;
        }
        Ok(())
    }

    pub const fn policy(&self) -> ReportScoringEnumPolicyVersion {
        self.policy
    }
    pub const fn coverage(&self) -> ReportScoringEnumCoverage {
        self.coverage
    }
    pub const fn player(&self, color: BoardEnumColor) -> &ReportScoringPlayerSummary {
        match color {
            BoardEnumColor::White => &self.white,
            BoardEnumColor::Black => &self.black,
        }
    }
}
