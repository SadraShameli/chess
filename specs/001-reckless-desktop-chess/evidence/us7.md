# User Story 7 acceptance evidence

Status: source and tests written; the consolidated compile and test run is
outstanding, so no pass counts are recorded here yet. Record them from that run.

## Scenario map

| Scenario | Owning tests |
| --- | --- |
| Report scoring: cp clamp, logistic expected score, thresholds, forced exclusion, mates, bounds, per-player accuracy | `tests/report/scoring.rs` |
| Report job identity, progress, partial and outdated states, relaunch | `tests/report/job.rs` |
| FIFO scheduling, pause for live games, restart with full budget | `tests/report/scheduler.rs` |
| Automatic played-game reports, explicit imported analysis, no report on reopen/import/metadata edit | `tests/report/service.rs` |
| Report start/cancel/read/list and confirmed replacement preview | `tests/report/commands.rs` |
| Report persistence | `tests/storage/report_repository.rs` |
| Position identity, first occurrence per game, transpositions | `tests/opening/index.rs`, `tests/opening/statistics.rs`, `tests/opening/explorer.rs` |
| Combined-source deduplication and denominators | `tests/opening/statistics.rs`, `tests/opening/explorer.rs` |
| Date/rating/player/time-control/result filters, partial dates, missing metadata | `tests/opening/query.rs`, `tests/opening/explorer.rs` |
| Deepest opening label and 50-sample pages | `tests/opening/explorer.rs`, `tests/opening/statistics.rs` |
| Polyglot decoding, legal lookup, zero and scaled weights, study-only use | `tests/opening/book.rs` |
| Indexed opening storage and continuation counts | `tests/storage/opening_repository.rs` |
| Study continuation reuses or adds a variation, never submits a live move | `tests/game/study.rs` (implemented on `GameSession`) |
| Report presentation: findings, labels, accuracy, outdated and partial labels, graph/finding selection, confirmed replacement | `tests/presentation/report.rs` |
| Explorer and book presentation: statistics, unavailable data, study-only wording, shared selected position across tabs | `tests/presentation/opening.rs` |

## Exact metric references

Metric and statistic expectations are asserted exactly in the tests above:
accuracy is the one-decimal mean-loss value, outcome shares use known-result
denominators, continuation frequency uses next-move denominators, and Polyglot
shares are one-decimal normalized values with unavailable shares for all-zero
legal weights.

## Outstanding

- Live bootstrap wiring (`src/desktop/native/bootstrap.rs`) of the report
  completion trigger to a running report worker and engine, and wiring the
  opening index into a mounted explorer data source, is not done. The typed
  trigger entry points (`DesktopReportCommands::observe_game_completed`,
  `observe_game_line`) and the presentation owner (`PresentationApp::study_mut`)
  exist for that wiring.
- Corpus and book preview/apply handlers need the dataset installation flow.
- Real engine analysis timing for reports needs external evidence.

## Report search interruption when a live game starts (written, not compiled or run)

The report worker used to learn about a started live game only after its current
queue drain returned, so a running report search kept its engine and lease until it
finished. `DesktopNativeReportHost::run_next` now reads the live game watch between
engine events, updates the scheduler demand when the running count changes, and when
the scheduler answers Stop for the bound search it stops that search through the
session with the receipt time and records the actual terminal. The job then pauses and
resumes automatically once the live game ends. The check runs between engine events, so
the stop latency is one event interval. Test: `report_demand_counts_only_active_games`
in `tests/presentation/app.rs` covers the demand count; the stop path itself needs a
real or fake engine process.

Still not wired: `DesktopNativeReportHost::pump` has no caller, so the report panel,
explorer and dataset intents are not exchanged with the live window. Mounting it needs
a two-phase main-thread exchange (take intents, send them to the async host, apply the
returned views) and the packaged starter manifest, which does not exist yet.
