use std::future::Future;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::task::Poll;
use std::time::Duration;

use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::clock::duration::{ClockDuration, ClockDurationError, ClockInstant};
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::clock::time_provider::ClockTimeProvider;
use chess::engine::live_game::{EngineLiveGame, EngineLiveGameSearchPlan};
use chess::engine::process_provider::EngineProcessLimits;
use chess::game::engine_provider::{
    GameEngineContext, GameEngineEnumDiscard, GameEngineEnumEvent, GameEngineProvider,
    GameEngineRequest, GameEngineSearchConfig,
};
use chess::game::enums::{GameId, GameRevision};
use tokio::time::Instant;

use crate::engine_lifecycle::EngineLifecycleProvider;
use crate::engine_package_selection::EnginePackageFixture;

type EngineLiveGameTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct EngineLiveGameFixtureClock(AtomicU64);

impl EngineLiveGameFixtureClock {
    fn set_millis(&self, millis: u64) -> EngineLiveGameTestResult {
        self.0.store(
            ClockDuration::from_millis(millis)?.as_nanos(),
            Ordering::SeqCst,
        );
        Ok(())
    }
}

impl ClockTimeProvider for EngineLiveGameFixtureClock {
    fn now(&self) -> Result<ClockInstant, ClockDurationError> {
        Ok(ClockInstant::from_nanos(self.0.load(Ordering::SeqCst)))
    }
}

struct EngineLiveGameFixture;

impl EngineLiveGameFixture {
    fn context(revision: GameRevision) -> EngineLiveGameTestResult<GameEngineContext> {
        Ok(GameEngineContext::new(
            "00000000-0000-0000-0000-000000000001".parse::<GameId>()?,
            revision,
        ))
    }

    fn request(
        initial: BoardPosition,
        history: &[&str],
        control: ClockTimeControl,
    ) -> EngineLiveGameTestResult<GameEngineRequest> {
        let mut current = initial.clone();
        let mut moves = Vec::new();
        for uci in history {
            let movement = current.parse_uci(uci)?;
            current = current.play(movement)?;
            moves.push(movement);
        }
        let mut clock = ClockState::new(control, current.turn());
        clock.resume(ClockInstant::from_nanos(0))?;
        Ok(GameEngineRequest::new(
            Self::context(GameRevision::ZERO)?,
            initial,
            moves,
            clock,
            GameEngineSearchConfig::initial_default(),
        )?)
    }

    fn standard() -> EngineLiveGameTestResult<GameEngineRequest> {
        Self::request(BoardPosition::standard(), &[], ClockTimeControl::untimed())
    }

    async fn expect_search(
        provider: &mut EngineLifecycleProvider,
        position: &str,
        limit: &str,
    ) -> EngineLiveGameTestResult {
        provider.expect("ucinewgame").await?;
        provider
            .expect("setoption name UCI_Chess960 value false")
            .await?;
        provider.expect("isready").await?;
        provider.expect(position).await?;
        provider.expect(limit).await?;
        Ok(())
    }
}

#[test]
fn planning_keeps_full_history_and_uses_fen_for_every_chess960_start() -> EngineLiveGameTestResult {
    let history = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let request = EngineLiveGameFixture::request(
        BoardPosition::standard(),
        &history,
        ClockTimeControl::untimed(),
    )?;
    let plan = EngineLiveGameSearchPlan::new(&request, ClockInstant::from_nanos(0))?;
    assert_eq!(
        plan.position().command(),
        "position startpos moves g1f3 g8f6 f3g1 f6g8"
    );
    assert_eq!(plan.limit().command(), "go movetime 3000");
    assert_eq!(plan.deadline(), None);
    let frc =
        BoardPosition::from_fen(&BoardPosition::standard().fen(), BoardEnumVariant::Chess960)?;
    let request =
        EngineLiveGameFixture::request(frc.clone(), &history, ClockTimeControl::untimed())?;
    assert_eq!(
        EngineLiveGameSearchPlan::new(&request, ClockInstant::from_nanos(0))?
            .position()
            .command(),
        format!("position fen {} moves g1f3 g8f6 f3g1 f6g8", frc.fen())
    );
    Ok(())
}

#[test]
fn timed_planning_uses_both_remaining_clocks_and_the_current_stage_increment()
-> EngineLiveGameTestResult {
    let request = EngineLiveGameFixture::request(
        BoardPosition::standard(),
        &[],
        ClockTimeControl::initial_default(),
    )?;
    let plan = EngineLiveGameSearchPlan::new(&request, ClockInstant::from_millis(1_000)?)?;
    assert_eq!(
        plan.limit().command(),
        "go wtime 599000 btime 600000 winc 5000 binc 5000"
    );
    assert_eq!(plan.deadline(), Some(ClockInstant::from_millis(600_000)?));
    Ok(())
}

#[test]
fn both_delay_kinds_use_only_movetime_with_unused_delay_and_one_overhead_deduction()
-> EngineLiveGameTestResult {
    let base = ClockDuration::from_millis(60_000)?;
    let delay = ClockDuration::from_millis(2_000)?;
    for control in [
        ClockTimeControl::simple_delay(base, delay)?,
        ClockTimeControl::bronstein_delay(base, delay)?,
    ] {
        let request = EngineLiveGameFixture::request(BoardPosition::standard(), &[], control)?;
        let early = EngineLiveGameSearchPlan::new(&request, ClockInstant::from_millis(500)?)?;
        assert_eq!(early.limit().command(), "go movetime 3400");
        assert_eq!(early.deadline(), Some(ClockInstant::from_millis(62_000)?));
        assert_eq!(
            EngineLiveGameSearchPlan::new(&request, ClockInstant::from_millis(3_000)?)?
                .limit()
                .command(),
            "go movetime 1866"
        );
        assert!(
            EngineLiveGameSearchPlan::new(&request, ClockInstant::from_millis(61_950)?).is_err()
        );
        assert!(
            EngineLiveGameSearchPlan::new(&request, ClockInstant::from_millis(62_000)?).is_err()
        );
    }
    Ok(())
}

#[test]
fn paused_mismatched_terminal_and_invalid_histories_never_become_search_requests()
-> EngineLiveGameTestResult {
    let initial = BoardPosition::standard();
    let context = EngineLiveGameFixture::context(GameRevision::ZERO)?;
    let config = GameEngineSearchConfig::initial_default();
    assert!(
        GameEngineRequest::new(
            context,
            initial.clone(),
            Vec::new(),
            ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White),
            config
        )
        .is_err()
    );
    let mut wrong_side = ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::Black);
    wrong_side.resume(ClockInstant::from_nanos(0))?;
    assert!(
        GameEngineRequest::new(context, initial.clone(), Vec::new(), wrong_side, config).is_err()
    );
    assert!(
        EngineLiveGameFixture::request(
            BoardPosition::from_fen("7k/6Q1/5K2/8/8/8/8/8 b - - 0 1", BoardEnumVariant::Standard)?,
            &[],
            ClockTimeControl::untimed()
        )
        .is_err()
    );
    let mut clock = ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White);
    clock.resume(ClockInstant::from_nanos(0))?;
    let move_white = initial.parse_uci("e2e4")?;
    assert!(
        GameEngineRequest::new(
            context,
            initial,
            vec![move_white, move_white],
            clock,
            config
        )
        .is_err()
    );
    assert!(GameEngineSearchConfig::new(ClockDuration::ZERO, ClockDuration::ZERO).is_err());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn live_turn_sequences_new_game_readiness_full_history_and_correlated_legal_result()
-> EngineLiveGameTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let session = provider.start(&fixture).await?;
    let clock = Arc::new(EngineLiveGameFixtureClock::default());
    let mut engine = EngineLiveGame::new(session, clock);
    let request = EngineLiveGameFixture::request(
        BoardPosition::standard(),
        &["g1f3", "g8f6", "f3g1", "f6g8"],
        ClockTimeControl::untimed(),
    )?;
    let context = request.context();
    let identity = engine.start(request).await?;
    EngineLiveGameFixture::expect_search(
        &mut provider,
        "position startpos moves g1f3 g8f6 f3g1 f6g8",
        "go movetime 3000",
    )
    .await?;
    assert_eq!(identity.context(), context);
    assert!(!identity.search().is_nil());
    assert!(!identity.generation().is_nil());
    provider.emit("info depth 1 score cp 0 pv e2e4").await?;
    assert!(
        matches!(engine.next_event(context).await?, GameEngineEnumEvent::Info { identity: actual, .. } if actual == identity)
    );
    provider.emit("bestmove e2e4").await?;
    let movement = match engine.next_event(context).await? {
        GameEngineEnumEvent::Move(movement) => movement,
        event @ (GameEngineEnumEvent::Info { .. }
        | GameEngineEnumEvent::Diagnostic { .. }
        | GameEngineEnumEvent::Readiness { .. }
        | GameEngineEnumEvent::Stopped(_)
        | GameEngineEnumEvent::Discarded { .. }) => {
            return Err(format!("expected legal move, received {event:?}").into());
        }
    };
    assert_eq!(movement.identity(), identity);
    assert_eq!(movement.movement().uci(BoardEnumVariant::Standard), "e2e4");
    assert_eq!(movement.received_at(), ClockInstant::from_nanos(0));
    assert!(engine.is_idle());
    assert_eq!(
        engine
            .stop(identity, Instant::now().into_std())
            .await?
            .identity(),
        identity
    );
    provider.assert_no_command();
    engine
        .shutdown_retained((Instant::now() + EngineProcessLimits::SHUTDOWN).into_std())
        .await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn illegal_bestmove_fails_and_reaps_without_emitting_a_game_move() -> EngineLiveGameTestResult
{
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let session = provider.start(&fixture).await?;
    let mut engine = EngineLiveGame::new(session, Arc::new(EngineLiveGameFixtureClock::default()));
    let request = EngineLiveGameFixture::standard()?;
    let context = request.context();
    engine.start(request).await?;
    EngineLiveGameFixture::expect_search(&mut provider, "position startpos", "go movetime 3000")
        .await?;
    provider.emit("bestmove e2e5").await?;
    assert!(engine.next_event(context).await.is_err());
    provider.expect("terminate").await?;
    assert!(!engine.is_idle());
    engine
        .shutdown_retained((Instant::now() + EngineProcessLimits::SHUTDOWN).into_std())
        .await?;
    provider.assert_no_command();
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_bestmove_at_the_native_deadline_is_discarded_after_the_stop_barrier()
-> EngineLiveGameTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let session = provider.start(&fixture).await?;
    let clock = Arc::new(EngineLiveGameFixtureClock::default());
    let mut engine = EngineLiveGame::new(session, Arc::clone(&clock));
    let request = EngineLiveGameFixture::request(
        BoardPosition::standard(),
        &[],
        ClockTimeControl::sudden_death(ClockDuration::from_millis(1_000)?)?,
    )?;
    let context = request.context();
    let identity = engine.start(request).await?;
    EngineLiveGameFixture::expect_search(
        &mut provider,
        "position startpos",
        "go wtime 1000 btime 1000 winc 0 binc 0",
    )
    .await?;
    clock.set_millis(1_000)?;
    provider.emit("bestmove e2e4").await?;
    assert!(
        matches!(engine.next_event(context).await?, GameEngineEnumEvent::Discarded { identity: actual, reason: GameEngineEnumDiscard::Deadline } if actual == identity)
    );
    provider.expect("stop").await?;
    assert!(engine.is_idle());
    engine
        .shutdown_retained((Instant::now() + EngineProcessLimits::SHUTDOWN).into_std())
        .await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn stale_game_revisions_cannot_receive_an_old_search_move() -> EngineLiveGameTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let session = provider.start(&fixture).await?;
    let mut engine = EngineLiveGame::new(session, Arc::new(EngineLiveGameFixtureClock::default()));
    let identity = engine.start(EngineLiveGameFixture::standard()?).await?;
    EngineLiveGameFixture::expect_search(&mut provider, "position startpos", "go movetime 3000")
        .await?;
    provider.emit("bestmove e2e4").await?;
    let newer = EngineLiveGameFixture::context(GameRevision::ZERO.next()?)?;
    assert!(
        matches!(engine.next_event(newer).await?, GameEngineEnumEvent::Discarded { identity: actual, reason: GameEngineEnumDiscard::StaleContext } if actual == identity)
    );
    assert!(engine.is_idle());
    engine
        .shutdown_retained((Instant::now() + EngineProcessLimits::SHUTDOWN).into_std())
        .await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn dropping_stop_wait_invalidates_move_delivery_and_blocks_replacement_until_final_bestmove()
-> EngineLiveGameTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let session = provider.start(&fixture).await?;
    let mut engine = EngineLiveGame::new(session, Arc::new(EngineLiveGameFixtureClock::default()));
    let context = EngineLiveGameFixture::context(GameRevision::ZERO)?;
    let identity = engine.start(EngineLiveGameFixture::standard()?).await?;
    EngineLiveGameFixture::expect_search(&mut provider, "position startpos", "go movetime 3000")
        .await?;
    {
        let waiting = engine.stop(identity, Instant::now().into_std());
        tokio::pin!(waiting);
        assert!(
            std::future::poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        provider.expect("stop").await?;
    }
    assert!(
        engine
            .start(EngineLiveGameFixture::standard()?)
            .await
            .is_err()
    );
    provider.assert_no_command();
    provider.emit("readyok").await?;
    assert!(matches!(
        engine.next_event(context).await?,
        GameEngineEnumEvent::Readiness { .. }
    ));
    assert!(!engine.is_idle());
    provider.emit("bestmove e2e4").await?;
    assert!(
        matches!(engine.next_event(context).await?, GameEngineEnumEvent::Stopped(receipt) if receipt.identity() == identity)
    );
    assert!(engine.is_idle());
    let replacement = engine.start(EngineLiveGameFixture::standard()?).await?;
    assert_ne!(replacement.search(), identity.search());
    provider.expect("position startpos").await?;
    provider.expect("go movetime 3000").await?;
    provider.emit("bestmove d2d4").await?;
    assert!(matches!(
        engine.next_event(context).await?,
        GameEngineEnumEvent::Move(_)
    ));
    engine
        .shutdown_retained((Instant::now() + EngineProcessLimits::SHUTDOWN).into_std())
        .await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn expired_clock_never_reaches_position_or_go_and_timed_wait_enforces_its_deadline()
-> EngineLiveGameTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let session = provider.start(&fixture).await?;
    let clock = Arc::new(EngineLiveGameFixtureClock::default());
    let mut engine = EngineLiveGame::new(session, Arc::clone(&clock));
    let request = EngineLiveGameFixture::request(
        BoardPosition::standard(),
        &[],
        ClockTimeControl::sudden_death(ClockDuration::from_millis(1_000)?)?,
    )?;
    clock.set_millis(1_000)?;
    assert!(engine.start(request).await.is_err());
    provider.assert_no_command();
    clock.set_millis(0)?;
    let request = EngineLiveGameFixture::request(
        BoardPosition::standard(),
        &[],
        ClockTimeControl::sudden_death(ClockDuration::from_millis(1_000)?)?,
    )?;
    let context = request.context();
    engine.start(request).await?;
    EngineLiveGameFixture::expect_search(
        &mut provider,
        "position startpos",
        "go wtime 1000 btime 1000 winc 0 binc 0",
    )
    .await?;
    let event = {
        let waiting = engine.next_event(context);
        tokio::pin!(waiting);
        assert!(
            std::future::poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        tokio::time::advance(Duration::from_secs(1)).await;
        clock.set_millis(1_000)?;
        assert!(
            std::future::poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        provider.expect("stop").await?;
        provider.emit("bestmove e2e4").await?;
        waiting.await?
    };
    assert!(matches!(
        event,
        GameEngineEnumEvent::Discarded {
            reason: GameEngineEnumDiscard::Deadline,
            ..
        }
    ));
    engine
        .shutdown_retained((Instant::now() + EngineProcessLimits::SHUTDOWN).into_std())
        .await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn quiescence_proof_distinguishes_idle_from_observed_search_completion()
-> EngineLiveGameTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let session = provider.start(&fixture).await?;
    let mut engine = EngineLiveGame::new(session, Arc::new(EngineLiveGameFixtureClock::default()));
    let context = EngineLiveGameFixture::context(GameRevision::ZERO)?;
    let idle = engine.quiesce(context, Instant::now().into_std()).await?;
    assert_eq!(idle.context(), context);
    assert!(!idle.generation().is_nil());
    assert_eq!(idle.stopped_search(), None);
    provider.assert_no_command();
    let identity = engine.start(EngineLiveGameFixture::standard()?).await?;
    EngineLiveGameFixture::expect_search(&mut provider, "position startpos", "go movetime 3000")
        .await?;
    let newer = EngineLiveGameFixture::context(GameRevision::ZERO.next()?)?;
    assert!(
        engine
            .quiesce(newer, Instant::now().into_std())
            .await
            .is_err()
    );
    provider.assert_no_command();
    let stopped = {
        let waiting = engine.quiesce(context, Instant::now().into_std());
        tokio::pin!(waiting);
        assert!(
            std::future::poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        provider.expect("stop").await?;
        provider.emit("readyok").await?;
        assert!(
            std::future::poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        provider.emit("bestmove e2e4").await?;
        waiting.await?
    };
    assert_eq!(stopped.context(), context);
    assert_eq!(stopped.generation(), identity.generation());
    assert_eq!(stopped.stopped_search(), Some(identity));
    assert!(engine.is_idle());
    engine
        .shutdown_retained((Instant::now() + EngineProcessLimits::SHUTDOWN).into_std())
        .await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn untimed_watchdog_fault_stops_discards_and_reaps_before_allowing_explicit_retry()
-> EngineLiveGameTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let session = provider.start(&fixture).await?;
    let mut engine = EngineLiveGame::new(session, Arc::new(EngineLiveGameFixtureClock::default()));
    let request = EngineLiveGameFixture::standard()?;
    let context = request.context();
    engine.start(request).await?;
    EngineLiveGameFixture::expect_search(&mut provider, "position startpos", "go movetime 3000")
        .await?;
    let error = {
        let waiting = engine.next_event(context);
        tokio::pin!(waiting);
        assert!(
            std::future::poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        tokio::time::advance(
            GameEngineSearchConfig::initial_default()
                .untimed_move_time()
                .as_duration()
                + chess::engine::search::stop::EngineSearchStop::SAFETY,
        )
        .await;
        assert!(
            std::future::poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        provider.expect("stop").await?;
        provider.emit("bestmove e2e4").await?;
        waiting
            .await
            .err()
            .ok_or("A silent untimed engine must fail rather than delivering its late move")?
    };
    assert!(matches!(
        error,
        chess::engine::live_game::EngineLiveGameError::SearchDeadline { .. }
    ));
    provider.expect("shutdown").await?;
    assert!(engine.is_shutdown_complete());
    assert!(!engine.is_idle());
    assert!(
        engine
            .start(EngineLiveGameFixture::standard()?)
            .await
            .is_err()
    );
    provider.assert_no_command();
    Ok(())
}

#[test]
fn positive_submillisecond_delay_budget_uses_the_approved_minimum_without_extending_the_clock()
-> EngineLiveGameTestResult {
    let base = ClockDuration::from_millis(60_000)?;
    let delay = ClockDuration::from_millis(2_000)?;
    for control in [
        ClockTimeControl::simple_delay(base, delay)?,
        ClockTimeControl::bronstein_delay(base, delay)?,
    ] {
        let request = EngineLiveGameFixture::request(BoardPosition::standard(), &[], control)?;
        let plan =
            EngineLiveGameSearchPlan::new(&request, ClockInstant::from_nanos(61_899_500_000))?;
        assert_eq!(plan.limit().command(), "go movetime 1");
        assert_eq!(plan.deadline(), Some(ClockInstant::from_millis(62_000)?));
    }
    Ok(())
}
