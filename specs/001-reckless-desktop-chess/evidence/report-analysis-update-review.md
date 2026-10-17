# Report analysis update review regressions

This is an incremental tests-only handoff against base
`0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68` plus the absolute dependency
`/tmp/claude-report-model-scoring-20261017/revision-2/report-model-scoring.patch`,
whose verified SHA-256 is
`579e8f540df69d484ab57387811bffe06fb49536fb9a2f98c5c641a0f52f0a21`.
No production repair, original test, registration root, task checkbox, commit or
push belongs to this handoff.

## Contract basis

[US7 acceptance 1 and FR-018](../spec.md#user-story-7---review-full-games-and-explore-openings-priority-p1)
require findings to retain their source position, evaluation and available
alternatives. FR-018 forbids exact classifications from bounds or missing scores.
SC-011 requires each completed entry to match its recorded configuration and
underlying search result. The [study-tools report contract](../contracts/study-tools.md#full-game-analysis-reports)
also requires per-position search identities and the original engine perspective.

The [selected report policy](../decisions.md#report-scoring-and-job-scheduling)
states: "Mark a matching top candidate as best separately" and "Bound scores are
not exact: show the bound and leave that move unclassified until exact search
results are available." It also specifies adjacent searches at the same budget,
the expected-score formula, scored sample counts, and unavailable accuracy when
there are no scored moves. These are the reasons for the assertions below.
There is no additional requirement that a score update repeat its PV, or that
statistics erase the latest evaluation.

## Actual session and job paths

The parent includes `tests/report/analysis_update_review.rs` as the private
`report_job::analysis_update_review` child. That file includes the new optional
`analysis_update_review_fixture.rs`. The dependency actually supplies
`tests/report/fixture.rs` as `report_job::fixture`; it contains no `job_fixture.rs`,
`Scripted` or `RawLine` API and can finish only one info line per search. The
new adapter therefore reuses the existing `ReportFixture` for the validated
package, immutable game line and captured job configuration, and supplies the
multi-line transcript through the existing `EngineProcess` boundary.

Every case starts a real `EngineSession` and `EngineSearchSession`, uses an actual
`ReportJob` attempt and its CPU lease, binds the actual tag, consumes the
session-produced terminal, and calls `ReportJob::record_terminal`. No terminal,
publication, analysis line, score or assessment is fabricated by the fixture.
Responses are literal independently specified UCI lines; the canonical registry
supplies only handshake capabilities. Missing scripts or responses are errors.

Both adjacent positions complete: the Standard White root and the Black position
after `e2e4`. Searches send `position startpos` and
`position startpos moves e2e4`, each with the recorded 3000 ms budget. The tests
inspect terminal tags, histories, FENs, node identities, game/digest binding,
engine-reported White/Black perspectives, completions and job progress.

All assertions run after retained shutdown of both the scripted engine and the
original fixture engine, session/provider destruction, and fixture-directory
cleanup. Cleanup also runs before propagating analysis errors. Lease availability
and empty allocation are captured after shutdown and checked afterward. The
adapter owns no worker task or real subprocess. Paused Tokio time and bounded
event/deadline waits require no sleeps, hardware discovery or CPU diagnostics.

## Regression cases and source findings

`newest_rank_one_score_without_pv_preserves_evaluation_and_retained_candidate_best_flag`
sends rank-one exact cp 0 with PV `e2e4 e7e5`, then rank-one exact cp 400 without
a PV, followed by `bestmove e2e4`. The separate Black search reports exact cp 0
with legal PV `e7e5 g1f3`. It requires the latest evaluation, the retained top
candidate, and the separate best flag together. A separate complete before/after
control emits cp 400 with a PV. Both histories must yield loss
`0.2310585786300049`, a blunder label, one scored White move and displayed
accuracy `76.9`, while Black has no sample. The selected policy permits noisy
adjacent evaluations and marks best separately from the loss label.

Source inspection of `src/report/finding.rs` shows that `primary()` selects the
latest score-only rank-one line, while `is_best()` asks that same line for its
missing PV. The retained top PV remains in `alternatives()`. The predicted
semantic failure is the final best-flag assertion; this lane has not observed it.

`score_only_bound_followed_by_statistics_cannot_score_an_older_exact_candidate`
sends exact cp 0 with PV, cp 400 lowerbound without PV, `info nodes 100 time 1`,
and `bestmove e2e4`, then completes the separate exact Black search. It requires
`Unscored { before: Some(Bound(...)), after: None }`, the actual White lower
bound, zero scored moves, one unscored move, partial metric coverage and
unavailable accuracy. A separate bound-with-PV history proves bound handling
survives statistics; another separate exact-with-PV history proves exact results
still score. The tests do not prescribe the stored candidate score after a fix.

Source inspection of `EngineSearchSessionAnalysis::accept` in
`src/engine/search/session.rs` shows that candidates retain only score-plus-PV
lines, while every parsed info line replaces `latest`. After the statistics line,
the terminal exposes the old exact candidate and statistics. The report consumer
falls back to that candidate, predicting a scored zero-loss move and invented
`100.0` accuracy. The canonical session snapshot loses the intervening bound, so
the repair may require retained rank-one evaluation ownership in the engine
snapshot and corresponding report consumption. No repair is authored here.

## Verification and integration limits

Executed tests: **0**. No Cargo command, compilation, build, test binary, Clippy,
runtime, benchmark or CPU diagnostic ran. No semantic red or green is claimed.
Only the pinned standalone rustfmt wrapper
`/tmp/claude-chess-pgo-diagnosis-20261016/with-pinned-rust`, source/API review,
whitespace checks, dependency-file hash verification and patch checks are used.
The parent must obtain the actual semantic reds before authoring a production
repair, then compile and run the affected checks under its authorized lane.

The parent owns existing report/root registration and adds this private child
once to `tests/report/job.rs`:

```rust
#[path = "analysis_update_review.rs"]
mod analysis_update_review;
```

The existing root `EngineSearchPosition` full-FEN serializer prerequisite remains
with the parent. These new cases use the canonical Standard initial root and do
not depend on custom counters. No registration mirror or serializer change is
included in this export.

The staged binary patch and SHA-256 manifest are exported under
`/tmp/claude-report-analysis-update-review-20261017/`. The manifest records the
three owned files, incremental patch and dependency/base tree metadata. These
source checks establish neither compilation nor behavior, runtime cleanup or US7
acceptance.
