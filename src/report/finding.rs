use std::mem::size_of;
use std::sync::Arc;

use crate::board::r#move::BoardMove;
use crate::engine::analysis::{EngineAnalysisLine, EngineAnalysisScore};
use crate::engine::search::identity::EngineSearchTag;
use crate::game::enums::{GameEnumResult, GameId, GameMoveId, GameRevision};
use crate::report::configuration::ReportJobConfiguration;
use crate::report::job::{ReportJobBinding, ReportJobError};
use crate::report::scoring::{
    ReportScoringEnumAssessment, ReportScoringEnumPolicyVersion, ReportScoringSample,
};
use crate::report::source::{ReportJobLineDigest, ReportJobSourceError, ReportJobSourcePosition};

#[derive(Debug)]
enum ReportFindingEnumResult {
    Search {
        tag: EngineSearchTag,
        lines: Box<[Arc<EngineAnalysisLine>]>,
        latest: Option<Arc<EngineAnalysisLine>>,
        primary: Option<Arc<EngineAnalysisLine>>,
    },
    Terminal(GameEnumResult),
    Unavailable,
}

#[derive(Debug)]
pub struct ReportFindingPositionResult {
    result: ReportFindingEnumResult,
}

impl ReportFindingPositionResult {
    pub(crate) fn search(
        tag: EngineSearchTag,
        lines: Box<[Arc<EngineAnalysisLine>]>,
        latest: Option<Arc<EngineAnalysisLine>>,
        primary: Option<Arc<EngineAnalysisLine>>,
    ) -> Self {
        Self {
            result: ReportFindingEnumResult::Search {
                tag,
                lines,
                latest,
                primary,
            },
        }
    }

    pub(crate) fn search_charge(
        line_count: usize,
        has_latest: bool,
        has_primary: bool,
    ) -> Result<usize, ReportJobSourceError> {
        let count = line_count
            .checked_add(usize::from(has_latest))
            .and_then(|count| count.checked_add(usize::from(has_primary)))
            .ok_or(ReportJobSourceError::MemoryLimit)?;
        let moves = EngineAnalysisLine::MAX_VARIATION_PLIES
            .checked_next_power_of_two()
            .and_then(|count| count.checked_mul(size_of::<BoardMove>()))
            .ok_or(ReportJobSourceError::MemoryLimit)?;
        let bytes = size_of::<EngineAnalysisLine>()
            .checked_add(moves)
            .and_then(|bytes| {
                bytes.checked_add(size_of::<Arc<EngineAnalysisLine>>() + 2 * size_of::<usize>())
            })
            .ok_or(ReportJobSourceError::MemoryLimit)?;
        count
            .checked_mul(bytes)
            .ok_or(ReportJobSourceError::MemoryLimit)
    }

    pub(crate) const fn terminal(result: GameEnumResult) -> Self {
        Self {
            result: ReportFindingEnumResult::Terminal(result),
        }
    }

    pub(crate) const fn unavailable() -> Self {
        Self {
            result: ReportFindingEnumResult::Unavailable,
        }
    }

    pub fn search_tag(&self) -> Option<EngineSearchTag> {
        match self.result {
            ReportFindingEnumResult::Search { tag, .. } => Some(tag),
            ReportFindingEnumResult::Terminal(_) | ReportFindingEnumResult::Unavailable => None,
        }
    }

    pub fn is_unavailable(&self) -> bool {
        matches!(self.result, ReportFindingEnumResult::Unavailable)
    }

    pub fn terminal_result(&self) -> Option<GameEnumResult> {
        match self.result {
            ReportFindingEnumResult::Terminal(result) => Some(result),
            ReportFindingEnumResult::Search { .. } | ReportFindingEnumResult::Unavailable => None,
        }
    }

    pub fn candidates(&self) -> &[Arc<EngineAnalysisLine>] {
        match &self.result {
            ReportFindingEnumResult::Search { lines, .. } => lines,
            ReportFindingEnumResult::Terminal(_) | ReportFindingEnumResult::Unavailable => &[],
        }
    }

    pub fn primary(&self) -> Option<&EngineAnalysisLine> {
        match &self.result {
            ReportFindingEnumResult::Search { primary, .. } => primary.as_deref(),
            ReportFindingEnumResult::Terminal(_) | ReportFindingEnumResult::Unavailable => None,
        }
    }

    pub fn latest_update(&self) -> Option<&EngineAnalysisLine> {
        match &self.result {
            ReportFindingEnumResult::Search { latest, .. } => latest.as_deref(),
            ReportFindingEnumResult::Terminal(_) | ReportFindingEnumResult::Unavailable => None,
        }
    }

    pub fn score(&self) -> Option<EngineAnalysisScore> {
        self.primary().and_then(EngineAnalysisLine::score)
    }
}

#[derive(Debug)]
pub struct ReportMoveFinding<'a> {
    binding: &'a ReportJobBinding,
    before: &'a ReportJobSourcePosition,
    after: &'a ReportJobSourcePosition,
    before_result: Option<&'a ReportFindingPositionResult>,
    after_result: Option<&'a ReportFindingPositionResult>,
    played_move: BoardMove,
    sample: ReportScoringSample,
}

impl<'a> ReportMoveFinding<'a> {
    pub(crate) fn new(
        binding: &'a ReportJobBinding,
        index: usize,
        before_result: Option<&'a ReportFindingPositionResult>,
        after_result: Option<&'a ReportFindingPositionResult>,
    ) -> Result<Self, ReportJobError> {
        let before = binding
            .source
            .positions()
            .get(index)
            .ok_or(ReportJobError::PositionIndex)?;
        let after = binding
            .source
            .positions()
            .get(index.checked_add(1).ok_or(ReportJobError::PositionIndex)?)
            .ok_or(ReportJobError::PositionIndex)?;
        let played_move = after.movement().ok_or(ReportJobError::PositionIndex)?;
        let mut sample = ReportScoringSample::new(
            before.position(),
            played_move,
            before_result.and_then(ReportFindingPositionResult::score),
            after_result.and_then(ReportFindingPositionResult::score),
        )?;
        if after_result.and_then(ReportFindingPositionResult::terminal_result)
            == Some(GameEnumResult::Draw)
        {
            sample = sample.with_terminal_draw();
        }
        Ok(Self {
            binding,
            before,
            after,
            before_result,
            after_result,
            played_move,
            sample,
        })
    }

    pub fn game(&self) -> GameId {
        self.binding.source.game()
    }
    pub fn source_revision(&self) -> GameRevision {
        self.binding.source.revision()
    }
    pub fn line_digest(&self) -> ReportJobLineDigest {
        self.binding.source.digest()
    }
    pub fn configuration(&self) -> &ReportJobConfiguration {
        &self.binding.configuration
    }
    pub fn policy(&self) -> ReportScoringEnumPolicyVersion {
        self.configuration().policy()
    }
    pub const fn before_node(&self) -> GameMoveId {
        self.before.node()
    }
    pub const fn after_node(&self) -> GameMoveId {
        self.after.node()
    }
    pub const fn before(&self) -> &ReportJobSourcePosition {
        self.before
    }
    pub const fn after(&self) -> &ReportJobSourcePosition {
        self.after
    }
    pub const fn played_move(&self) -> BoardMove {
        self.played_move
    }
    pub const fn before_result(&self) -> Option<&ReportFindingPositionResult> {
        self.before_result
    }
    pub const fn after_result(&self) -> Option<&ReportFindingPositionResult> {
        self.after_result
    }
    pub const fn sample(&self) -> &ReportScoringSample {
        &self.sample
    }
    pub fn assessment(&self) -> ReportScoringEnumAssessment {
        self.sample.assess(self.policy())
    }
    pub fn is_forced(&self) -> bool {
        self.sample.is_forced()
    }

    pub fn is_best(&self) -> bool {
        self.before_result
            .and_then(|result| {
                result
                    .candidates()
                    .iter()
                    .find(|line| line.rank().is_none_or(|rank| rank.get() == 1))
            })
            .and_then(|line| line.variation())
            .and_then(|line| line.first())
            .is_some_and(|movement| *movement == self.played_move)
    }

    pub fn alternatives(&self) -> &[Arc<EngineAnalysisLine>] {
        match self.before_result {
            Some(result) => result.candidates(),
            None => &[],
        }
    }
}
