#[path = "analysis_update_review_fixture.rs"]
mod fixture;

use std::time::Duration;

use chess::board::enums::BoardEnumColor;
use chess::engine::analysis::{EngineAnalysisEnumBound, EngineAnalysisEnumScore};
use chess::engine::search::session::EngineSearchSessionEnumCompletion;
use chess::report::job::ReportJobEnumState;
use chess::report::scoring::{
    ReportScoringEnumAssessment, ReportScoringEnumCoverage, ReportScoringEnumLabel,
    ReportScoringEnumPolicyVersion, ReportScoringEnumUnscored,
};

use crate::report_job::analysis_update_review::fixture::{
    ReportAnalysisUpdateReviewRun, ReportAnalysisUpdateReviewScript,
};
use crate::report_job::fixture::{ReportFixture, ReportFixtureResult};

impl ReportAnalysisUpdateReviewRun {
    fn assert_completed_searches(&self) -> ReportFixtureResult {
        assert!(self.shutdown_complete);
        assert_eq!(self.allocation, None);
        assert_eq!(self.available, 1);
        assert_eq!(self.job.state(), ReportJobEnumState::Completed);
        assert_eq!(self.job.progress().completed(), 2);
        assert_eq!(self.job.progress().total(), 2);
        assert_eq!(self.job.active_search(), None);
        assert_eq!(
            self.job.configuration().policy(),
            ReportScoringEnumPolicyVersion::ExpectedScoreV1
        );
        assert_eq!(
            self.commands,
            [
                "uci",
                "isready",
                "setoption name Threads value 1",
                "setoption name Hash value 16",
                "setoption name MoveOverhead value 0",
                "isready",
                "position startpos",
                "go movetime 3000",
                "position startpos moves e2e4",
                "go movetime 3000",
                "shutdown",
            ]
        );
        let finding = self.job.finding(0)?;
        assert_eq!(finding.game(), self.job.source().game());
        assert_eq!(finding.line_digest(), self.job.source().digest());
        assert_eq!(finding.before_node(), ReportFixture::node(1)?);
        assert_eq!(finding.after_node(), ReportFixture::node(2)?);
        assert_eq!(finding.before().position().turn(), BoardEnumColor::White);
        assert_eq!(finding.after().position().turn(), BoardEnumColor::Black);
        assert!(!finding.is_forced());
        for (index, terminal) in self.terminals.iter().enumerate() {
            let metadata = terminal.snapshot().metadata();
            assert_eq!(metadata.limit(), self.job.configuration().budget().limit());
            assert_eq!(metadata.position().history().len(), index);
            assert_eq!(
                metadata.position().current().fen(),
                self.job.source().positions()[index].position().fen()
            );
            let result = self.job.results()[index]
                .as_ref()
                .ok_or("Missing completed review result")?;
            assert_eq!(result.search_tag(), Some(metadata.tag()));
            let movement = metadata.position().current().parse_uci(if index == 0 {
                "e2e4"
            } else {
                "e7e5"
            })?;
            assert!(matches!(
                terminal.completion(),
                EngineSearchSessionEnumCompletion::BestMove(actual) if *actual == movement
            ));
        }
        assert_ne!(
            self.terminals[0].snapshot().metadata().tag(),
            self.terminals[1].snapshot().metadata().tag()
        );
        let after = finding.after_result().ok_or("Missing after result")?;
        let score = after.score().ok_or("Missing Black search score")?;
        assert_eq!(score.perspective(), BoardEnumColor::Black);
        assert_eq!(score.reported(), EngineAnalysisEnumScore::Centipawns(0));
        assert_eq!(score.bound(), EngineAnalysisEnumBound::Exact);
        Ok(())
    }

    fn assert_exact_loss(&self) -> ReportFixtureResult {
        let finding = self.job.finding(0)?;
        let before = finding.before_result().ok_or("Missing before result")?;
        let score = before.score().ok_or("Missing White search score")?;
        assert_eq!(score.perspective(), BoardEnumColor::White);
        assert_eq!(score.reported(), EngineAnalysisEnumScore::Centipawns(400));
        assert_eq!(score.white_value(), 400);
        assert_eq!(score.bound(), EngineAnalysisEnumBound::Exact);
        match finding.assessment() {
            ReportScoringEnumAssessment::Scored(scored) => {
                assert!((scored.loss() - 0.231_058_578_630_004_9).abs() < 1e-12);
                assert_eq!(scored.label(), ReportScoringEnumLabel::Blunder);
            }
            assessment @ ReportScoringEnumAssessment::Unscored { .. } => {
                return Err(
                    format!("Exact adjacent searches were unscored: {assessment:?}").into(),
                );
            }
        }
        let summary = self.job.summary()?;
        assert_eq!(summary.coverage(), ReportScoringEnumCoverage::Complete);
        let white = summary.player(BoardEnumColor::White);
        assert_eq!(white.scored_moves(), 1);
        assert_eq!(white.unscored_moves(), 0);
        assert_eq!(white.display_accuracy().as_deref(), Some("76.9"));
        assert_eq!(summary.player(BoardEnumColor::Black).accuracy(), None);
        Ok(())
    }

    fn assert_before_bound_is_unscored(&self) -> ReportFixtureResult {
        let finding = self.job.finding(0)?;
        match finding.assessment() {
            ReportScoringEnumAssessment::Unscored {
                before: Some(ReportScoringEnumUnscored::Bound(score)),
                after: None,
            } => {
                assert_eq!(score.perspective(), BoardEnumColor::White);
                assert_eq!(score.reported(), EngineAnalysisEnumScore::Centipawns(400));
                assert_eq!(score.bound(), EngineAnalysisEnumBound::Lower);
                assert_eq!(score.white_bound(), EngineAnalysisEnumBound::Lower);
            }
            assessment @ ReportScoringEnumAssessment::Scored(_)
            | assessment @ ReportScoringEnumAssessment::Unscored { .. } => {
                return Err(format!(
                    "The latest Before bound must remain unscored: {assessment:?}"
                )
                .into());
            }
        }
        let before = finding.before_result().ok_or("Missing before result")?;
        let score = before.score().ok_or("Missing retained Before bound")?;
        assert_eq!(score.reported(), EngineAnalysisEnumScore::Centipawns(400));
        assert_eq!(score.bound(), EngineAnalysisEnumBound::Lower);
        let summary = self.job.summary()?;
        assert_eq!(summary.coverage(), ReportScoringEnumCoverage::Partial);
        let white = summary.player(BoardEnumColor::White);
        assert_eq!(white.scored_moves(), 0);
        assert_eq!(white.unscored_moves(), 1);
        assert_eq!(white.accuracy(), None);
        assert_eq!(white.display_accuracy(), None);
        assert_eq!(summary.player(BoardEnumColor::Black).accuracy(), None);
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn newest_rank_one_score_without_pv_preserves_evaluation_and_retained_candidate_best_flag()
-> ReportFixtureResult {
    let updated = ReportAnalysisUpdateReviewRun::execute(ReportAnalysisUpdateReviewScript {
        before: &[
            "info multipv 1 score cp 0 pv e2e4 e7e5",
            "info multipv 1 score cp 400",
            "bestmove e2e4",
        ],
        after: &["info score cp 0 pv e7e5 g1f3", "bestmove e7e5"],
    })
    .await?;
    let control = ReportAnalysisUpdateReviewRun::execute(ReportAnalysisUpdateReviewScript {
        before: &["info multipv 1 score cp 400 pv e2e4 e7e5", "bestmove e2e4"],
        after: &["info score cp 0 pv e7e5 g1f3", "bestmove e7e5"],
    })
    .await?;
    updated.assert_completed_searches()?;
    control.assert_completed_searches()?;
    control.assert_exact_loss()?;
    assert!(control.job.finding(0)?.is_best());
    updated.assert_exact_loss()?;
    let finding = updated.job.finding(0)?;
    let latest = finding
        .before_result()
        .and_then(|result| result.latest_update())
        .ok_or("Missing score-only update")?;
    assert!(latest.variation().is_none());
    assert_eq!(latest.rank().map(std::num::NonZeroU32::get), Some(1));
    assert_eq!(finding.alternatives().len(), 1);
    let candidate = &finding.alternatives()[0];
    assert_eq!(
        candidate.variation().and_then(|line| line.first()),
        Some(&finding.played_move())
    );
    assert!(finding.is_best());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn score_only_bound_followed_by_statistics_cannot_score_an_older_exact_candidate()
-> ReportFixtureResult {
    let updated = ReportAnalysisUpdateReviewRun::execute(ReportAnalysisUpdateReviewScript {
        before: &[
            "info score cp 0 pv e2e4 e7e5",
            "info score cp 400 lowerbound",
            "info nodes 100 time 1",
            "bestmove e2e4",
        ],
        after: &["info score cp 0 pv e7e5 g1f3", "bestmove e7e5"],
    })
    .await?;
    let bound_control = ReportAnalysisUpdateReviewRun::execute(ReportAnalysisUpdateReviewScript {
        before: &[
            "info score cp 0 pv e2e4 e7e5",
            "info score cp 400 lowerbound pv e2e4 e7e5",
            "info nodes 100 time 1",
            "bestmove e2e4",
        ],
        after: &["info score cp 0 pv e7e5 g1f3", "bestmove e7e5"],
    })
    .await?;
    let exact_control = ReportAnalysisUpdateReviewRun::execute(ReportAnalysisUpdateReviewScript {
        before: &[
            "info score cp 0 pv e2e4 e7e5",
            "info score cp 400 pv e2e4 e7e5",
            "info nodes 100 time 1",
            "bestmove e2e4",
        ],
        after: &["info score cp 0 pv e7e5 g1f3", "bestmove e7e5"],
    })
    .await?;
    updated.assert_completed_searches()?;
    bound_control.assert_completed_searches()?;
    exact_control.assert_completed_searches()?;
    bound_control.assert_before_bound_is_unscored()?;
    exact_control.assert_exact_loss()?;
    let latest = updated.terminals[0]
        .snapshot()
        .latest_update()
        .ok_or("Missing final statistics update")?;
    assert_eq!(latest.nodes(), Some(100));
    assert_eq!(latest.elapsed(), Some(Duration::from_millis(1)));
    assert!(latest.score().is_none());
    assert!(latest.variation().is_none());
    let finding = updated.job.finding(0)?;
    assert_eq!(finding.alternatives().len(), 1);
    assert_eq!(
        finding.alternatives()[0]
            .variation()
            .and_then(|line| line.first()),
        Some(&finding.played_move())
    );
    updated.assert_before_bound_is_unscored()?;
    Ok(())
}
