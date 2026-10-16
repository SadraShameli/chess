use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::runtime::Builder;
use tokio::time::{Instant, timeout};

use chess::board::enums::BoardEnumVariant;
use chess::board::r#move::BoardMoveError;
use chess::board::position::{BoardPosition, BoardPositionError};
use chess::board::start::{BoardStart, BoardStartError, BoardStartIndex, BoardStartIndexError};
use chess::capacity::budget::{CapacityBudget, CapacityBudgetError};
use chess::clock::duration::{ClockDuration, ClockDurationError};
use chess::clock::state::{ClockState, ClockStateError};
use chess::clock::time_control::{ClockTimeControl, ClockTimeControlError};
use chess::clock::time_provider::{ClockTimeProvider, ClockTimeProviderMonotonic};
use chess::engine::live_game::{EngineLiveGame, EngineLiveGameError};
use chess::engine::package::host::{EnginePackageHost, EnginePackageHostError};
use chess::engine::package::selector::{
    EnginePackageSelected, EnginePackageSelector, EnginePackageSelectorError,
};
use chess::engine::process_provider::{EngineNativeProcessProvider, EngineProcessLimits};
use chess::engine::session::{EngineSession, EngineSessionError};
use chess::game::engine_provider::{
    GameEngineContext, GameEngineEnumEvent, GameEngineProvider, GameEngineRequest,
    GameEngineRequestError, GameEngineSearchConfig,
};
use chess::game::enums::{GameId, GameIdentityError, GameRevision};

#[derive(Debug, Error)]
pub(super) enum NativeEngineLiveGameError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Capacity(#[from] CapacityBudgetError),
    #[error(transparent)]
    Host(#[from] EnginePackageHostError),
    #[error(transparent)]
    Package(#[from] EnginePackageSelectorError),
    #[error(transparent)]
    Session(#[from] EngineSessionError),
    #[error(transparent)]
    Engine(#[from] EngineLiveGameError),
    #[error(transparent)]
    Request(#[from] GameEngineRequestError),
    #[error(transparent)]
    Clock(#[from] ClockStateError),
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
    #[error(transparent)]
    TimeControl(#[from] ClockTimeControlError),
    #[error(transparent)]
    Position(#[from] BoardPositionError),
    #[error(transparent)]
    Move(#[from] BoardMoveError),
    #[error(transparent)]
    Start(#[from] BoardStartError),
    #[error(transparent)]
    Index(#[from] BoardStartIndexError),
    #[error(transparent)]
    Identity(#[from] GameIdentityError),
    #[error("The real pinned engine violated live-game acceptance: {0}")]
    Invariant(&'static str),
    #[error("The real pinned engine acceptance exceeded its deadline: {0}")]
    Deadline(#[from] tokio::time::error::Elapsed),
    #[error("Native engine acceptance failed ({operation}); owned cleanup also failed ({cleanup})")]
    Cleanup {
        #[source]
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
}

type NativeEngineLiveGameActor = EngineLiveGame<
    chess::engine::process_provider::EngineNativeProcess,
    ClockTimeProviderMonotonic,
>;

struct NativeEngineLiveGameCase {
    initial: BoardPosition,
    history: Vec<&'static str>,
    control: ClockTimeControl,
}

impl NativeEngineLiveGameCase {
    fn request(
        &self,
        clock: &ClockTimeProviderMonotonic,
    ) -> Result<GameEngineRequest, NativeEngineLiveGameError> {
        let mut current = self.initial.clone();
        let mut history = Vec::with_capacity(self.history.len());
        for value in &self.history {
            let movement = current.parse_uci(value)?;
            current = current.play(movement)?;
            history.push(movement);
        }
        let mut state = ClockState::new(self.control.clone(), current.turn());
        state.resume(clock.now()?)?;
        Ok(GameEngineRequest::new(
            GameEngineContext::new(
                "541a31b7-45bd-4ddb-b0f1-d6ee1f33944b".parse::<GameId>()?,
                GameRevision::ZERO,
            ),
            self.initial.clone(),
            history,
            state,
            GameEngineSearchConfig::new(
                NativeEngineLiveGameHarness::MOVE_TIME,
                GameEngineSearchConfig::initial_default().move_overhead(),
            )?,
        )?)
    }
}

pub(super) struct NativeEngineLiveGameHarness;

impl NativeEngineLiveGameHarness {
    const MOVE_TIME: ClockDuration = ClockDuration::from_nanos(100_000_000);
    const RESULT_DEADLINE: Duration = Duration::from_secs(5);
    const CASE_DEADLINE: Duration = Duration::from_secs(15);
    const ENGINE_THREADS: usize = 1;

    fn cases() -> Result<Vec<NativeEngineLiveGameCase>, NativeEngineLiveGameError> {
        let delay_base = ClockDuration::from_millis(3_000)?;
        let delay = ClockDuration::from_millis(100)?;
        Ok(vec![
            NativeEngineLiveGameCase {
                initial: BoardPosition::standard(),
                history: vec!["g1f3", "g8f6", "f3g1", "f6g8"],
                control: ClockTimeControl::untimed(),
            },
            NativeEngineLiveGameCase {
                initial: BoardStart::chess960(BoardStartIndex::STANDARD)?
                    .position()
                    .clone(),
                history: Vec::new(),
                control: ClockTimeControl::untimed(),
            },
            NativeEngineLiveGameCase {
                initial: BoardStart::dfrc(
                    BoardStartIndex::try_from(1)?,
                    BoardStartIndex::try_from(958)?,
                )?
                .position()
                .clone(),
                history: Vec::new(),
                control: ClockTimeControl::untimed(),
            },
            NativeEngineLiveGameCase {
                initial: BoardPosition::from_fen(
                    "4k3/8/8/8/8/8/8/6KR w H - 0 1",
                    BoardEnumVariant::Chess960,
                )?,
                history: vec!["g1h1"],
                control: ClockTimeControl::untimed(),
            },
            NativeEngineLiveGameCase {
                initial: BoardPosition::standard(),
                history: Vec::new(),
                control: ClockTimeControl::simple_delay(delay_base, delay)?,
            },
            NativeEngineLiveGameCase {
                initial: BoardPosition::standard(),
                history: Vec::new(),
                control: ClockTimeControl::bronstein_delay(delay_base, delay)?,
            },
        ])
    }

    async fn accept_move(
        engine: &mut NativeEngineLiveGameActor,
        request: GameEngineRequest,
    ) -> Result<(), NativeEngineLiveGameError> {
        let position = request.current().clone();
        let context = request.context();
        let identity = engine.start(request).await?;
        timeout(Self::RESULT_DEADLINE, async {
            loop {
                match engine.next_event(context).await? {
                    GameEngineEnumEvent::Info {
                        identity: actual, ..
                    }
                    | GameEngineEnumEvent::Diagnostic {
                        identity: actual, ..
                    }
                    | GameEngineEnumEvent::Readiness { identity: actual } => {
                        if actual != identity {
                            return Err(NativeEngineLiveGameError::Invariant(
                                "telemetry retained its search identity",
                            ));
                        }
                    }
                    GameEngineEnumEvent::Move(movement) => {
                        if movement.identity() != identity {
                            return Err(NativeEngineLiveGameError::Invariant(
                                "bestmove was legal and search-correlated",
                            ));
                        }
                        position.play(movement.movement())?;
                        if !engine.is_idle() {
                            return Err(NativeEngineLiveGameError::Invariant(
                                "final bestmove opened the idle barrier",
                            ));
                        }
                        return Ok(());
                    }
                    GameEngineEnumEvent::Stopped(_) | GameEngineEnumEvent::Discarded { .. } => {
                        return Err(NativeEngineLiveGameError::Invariant(
                            "a timely legal search produced a move",
                        ));
                    }
                }
            }
        })
        .await?
    }

    async fn cancellation(
        engine: &mut NativeEngineLiveGameActor,
        case: &NativeEngineLiveGameCase,
        clock: &ClockTimeProviderMonotonic,
    ) -> Result<(), NativeEngineLiveGameError> {
        let request = case.request(clock)?;
        let context = request.context();
        let old = engine.start(request).await?;
        let proof = engine.quiesce(context, Instant::now().into_std()).await?;
        if proof.context() != context
            || proof.stopped_search() != Some(old)
            || proof.generation() != old.generation()
            || !engine.is_idle()
        {
            return Err(NativeEngineLiveGameError::Invariant(
                "stop consumed the final old bestmove before replacement",
            ));
        }
        let next = case.request(clock)?;
        Self::accept_move(engine, next).await
    }

    async fn shutdown(
        engine: &mut NativeEngineLiveGameActor,
    ) -> Result<(), NativeEngineLiveGameError> {
        let first = engine
            .shutdown_retained((Instant::now() + EngineProcessLimits::SHUTDOWN).into_std())
            .await;
        let observed = if engine.is_shutdown_complete() {
            Ok(())
        } else {
            engine
                .shutdown_retained((Instant::now() + EngineProcessLimits::SHUTDOWN).into_std())
                .await
        };
        match (first, observed) {
            (Ok(()), Ok(())) if engine.is_shutdown_complete() => Ok(()),
            (Ok(()), Ok(())) => Err(NativeEngineLiveGameError::Invariant(
                "the child and pipe readers were observed complete",
            )),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error.into()),
            (Err(operation), Err(cleanup)) => Err(NativeEngineLiveGameError::Cleanup {
                operation: Box::new(operation.into()),
                cleanup: Box::new(cleanup.into()),
            }),
        }
    }

    async fn run_case(
        package: EnginePackageSelected,
        case: NativeEngineLiveGameCase,
    ) -> Result<(), NativeEngineLiveGameError> {
        let session = EngineSession::start(&EngineNativeProcessProvider, package).await?;
        let clock = Arc::new(ClockTimeProviderMonotonic::new());
        let mut engine = EngineLiveGame::new(session, Arc::clone(&clock));
        let operation = timeout(Self::CASE_DEADLINE, async {
            Self::accept_move(&mut engine, case.request(&clock)?).await?;
            Self::cancellation(&mut engine, &case, &clock).await
        })
        .await
        .map_err(NativeEngineLiveGameError::Deadline)
        .and_then(|result| result);
        let cleanup = Self::shutdown(&mut engine).await;
        match (operation, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(operation), Err(cleanup)) => Err(NativeEngineLiveGameError::Cleanup {
                operation: Box::new(operation),
                cleanup: Box::new(cleanup),
            }),
        }
    }

    pub(super) fn run(directory: &Path) -> Result<(), NativeEngineLiveGameError> {
        let package = EnginePackageSelector::load(directory, EnginePackageHost::detect()?)?;
        let budget = CapacityBudget::discover()?;
        let _engine_lease = budget.try_lease(Self::ENGINE_THREADS)?;
        let runtime = Builder::new_current_thread()
            .max_blocking_threads(budget.workers())
            .enable_all()
            .build()?;
        let cases = Self::cases()?;
        let count = cases.len();
        runtime.block_on(async {
            for case in cases {
                Self::run_case(package.clone(), case).await?;
            }
            Ok::<(), NativeEngineLiveGameError>(())
        })?;
        println!(
            "Pinned Reckless live-game acceptance passed for {count} cases: Standard repetition history, Chess960, DFRC, castling history, both delay modes, legal correlated bestmoves, cancellation barriers, replacement searches, and observed child cleanup"
        );
        Ok(())
    }
}
