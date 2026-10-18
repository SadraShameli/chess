use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chess::board::enums::BoardEnumColor;
use chess::board::start::BoardStart;
use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::state::{ClockState, ClockStateEnumStatus};
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::native::game_engine::DesktopNativeGameEngineResources;
use chess::desktop::native::game_resources::{
    DesktopNativeGameResources, DesktopNativeGameResourcesError,
    DesktopNativeGameResourcesMemoryProvider, DesktopNativeGameResourcesOptions,
    DesktopNativeGameResourcesPolicy,
};
use chess::game::engine_provider::GameEngineContext;
use chess::game::enums::{
    GameEnumEngineReadiness, GameEnumMode, GameEnumPhase, GameEnumSeat, GameId, GameMoveId,
    GameRevision,
};
use chess::game::pause::{
    GamePauseEnumCause, GamePauseEnumEngineBarrier, GamePauseRequest, GameResumeRequest,
};
use chess::game::session::{
    GameMetadata, GameSeats, GameSession, GameSessionEnumInterruption, GameSessionOptions,
};

use crate::game_transition_fixture::{GameTransitionFixture, GameTransitionFixtureProof};

type GameConcurrentGamesResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct GameConcurrentGamesFixture;

impl GameConcurrentGamesFixture {
    fn session(value: u128) -> GameConcurrentGamesResult<GameSession> {
        let id: GameId = uuid::Uuid::from_u128(value).to_string().parse()?;
        let root: GameMoveId = uuid::Uuid::from_u128(value + 1_000).to_string().parse()?;
        let start = BoardStart::standard();
        let mut session = GameSession::new(GameSessionOptions {
            id,
            root,
            clock: ClockState::new(ClockTimeControl::initial_default(), start.position().turn()),
            start,
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
            metadata: GameMetadata::default(),
        })?;
        session.start(
            session.revision(),
            ClockInstant::from_nanos(0),
            GameEnumEngineReadiness::NotRequired,
            &GameTransitionFixtureProof,
        )?;
        Ok(session)
    }

    fn play(
        session: &mut GameSession,
        node: u128,
        uci: &str,
        millis: u64,
    ) -> GameConcurrentGamesResult {
        GameTransitionFixture::play(session, node, uci, millis)
    }

    fn pause(
        session: &mut GameSession,
        millis: u64,
        cause: GamePauseEnumCause,
    ) -> GameConcurrentGamesResult {
        session.pause(
            GamePauseRequest {
                expected_revision: session.revision(),
                cause,
                received_at: ClockInstant::from_millis(millis)?,
                engine: GamePauseEnumEngineBarrier::NoEngine,
            },
            &GameTransitionFixtureProof,
        )?;
        Ok(())
    }

    fn elapsed(session: &GameSession, millis: u64) -> GameConcurrentGamesResult<ClockDuration> {
        Ok(session
            .clock()
            .snapshot(ClockInstant::from_millis(millis)?)?
            .elapsed())
    }
}

struct GameConcurrentGamesMemory(AtomicU64);

impl DesktopNativeGameResourcesMemoryProvider for GameConcurrentGamesMemory {
    type Error = std::io::Error;

    async fn available_bytes(&self) -> Result<u64, Self::Error> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

fn context(index: u8) -> GameConcurrentGamesResult<GameEngineContext> {
    let game = format!("00000000-0000-4000-8000-{index:012}").parse()?;
    Ok(GameEngineContext::new(game, GameRevision::ZERO))
}

#[test]
fn simultaneous_games_keep_independent_identities_revisions_and_clocks() -> GameConcurrentGamesResult
{
    let mut first = GameConcurrentGamesFixture::session(1)?;
    let mut second = GameConcurrentGamesFixture::session(2)?;
    assert_ne!(first.id(), second.id());
    let second_revision = second.revision();
    let second_live = second.live();
    GameConcurrentGamesFixture::play(&mut first, 10, "e2e4", 1_000)?;
    assert_ne!(first.revision(), second_revision);
    assert_eq!(second.revision(), second_revision);
    assert_eq!(second.live(), second_live);
    assert_eq!(second.clock().active(), BoardEnumColor::White);
    assert_eq!(first.clock().active(), BoardEnumColor::Black);
    assert_eq!(
        GameConcurrentGamesFixture::elapsed(&second, 7_000)?,
        ClockDuration::from_millis(7_000)?
    );
    GameConcurrentGamesFixture::play(&mut second, 20, "d2d4", 7_000)?;
    assert_eq!(second.clock().active(), BoardEnumColor::Black);
    assert_eq!(first.clock().active(), BoardEnumColor::Black);
    assert_ne!(
        GameEngineContext::new(first.id(), first.revision()),
        GameEngineContext::new(second.id(), first.revision())
    );
    assert_ne!(
        GameEngineContext::new(first.id(), first.revision()),
        GameEngineContext::new(first.id(), GameRevision::ZERO)
    );
    Ok(())
}

#[test]
fn pausing_one_game_freezes_only_that_clock_and_resume_never_charges_the_pause()
-> GameConcurrentGamesResult {
    let mut first = GameConcurrentGamesFixture::session(1)?;
    let second = GameConcurrentGamesFixture::session(2)?;
    GameConcurrentGamesFixture::pause(&mut first, 2_000, GamePauseEnumCause::Manual)?;
    assert_eq!(first.phase(), GameEnumPhase::Interrupted);
    assert_eq!(second.phase(), GameEnumPhase::Active);
    assert_eq!(
        GameConcurrentGamesFixture::elapsed(&first, 90_000)?,
        GameConcurrentGamesFixture::elapsed(&first, 2_000)?
    );
    assert_eq!(
        GameConcurrentGamesFixture::elapsed(&second, 90_000)?,
        ClockDuration::from_millis(90_000)?
    );
    let revision = first.revision();
    first.resume(
        GameResumeRequest {
            expected_revision: revision,
            received_at: ClockInstant::from_millis(90_000)?,
            readiness: GameEnumEngineReadiness::NotRequired,
        },
        &GameTransitionFixtureProof,
    )?;
    assert_eq!(first.phase(), GameEnumPhase::Active);
    assert_eq!(
        GameConcurrentGamesFixture::elapsed(&first, 90_000)?,
        ClockDuration::from_millis(2_000)?
    );
    Ok(())
}

#[test]
fn quitting_with_many_games_restores_each_one_paused_at_its_own_revision()
-> GameConcurrentGamesResult {
    let mut first = GameConcurrentGamesFixture::session(1)?;
    let mut second = GameConcurrentGamesFixture::session(2)?;
    GameConcurrentGamesFixture::play(&mut first, 10, "e2e4", 1_000)?;
    GameConcurrentGamesFixture::play(&mut second, 20, "d2d4", 3_000)?;
    GameConcurrentGamesFixture::pause(&mut second, 4_000, GamePauseEnumCause::ApplicationQuit)?;
    let quit_at = ClockInstant::from_millis(5_000)?;
    let first_saved = first.restoration(quit_at)?;
    let second_saved = second.restoration(quit_at)?;
    let first_clock = first_saved.clock.clone();
    let restored_first = GameSession::restore(first_saved)?;
    let restored_second = GameSession::restore(second_saved)?;
    assert_eq!(restored_first.id(), first.id());
    assert_eq!(restored_second.id(), second.id());
    assert_eq!(restored_first.revision(), first.revision());
    assert_eq!(restored_second.revision(), second.revision());
    for restored in [&restored_first, &restored_second] {
        assert_eq!(restored.phase(), GameEnumPhase::Interrupted);
        assert!(!restored.clock().is_running());
        assert_eq!(restored.clock().status(), ClockStateEnumStatus::Paused);
    }
    assert_eq!(
        restored_first.interruption(),
        Some(GameSessionEnumInterruption::Paused(
            GamePauseEnumCause::Relaunch
        ))
    );
    assert_eq!(
        restored_first
            .clock()
            .snapshot(ClockInstant::from_millis(900_000)?)?
            .elapsed(),
        first_clock.elapsed()
    );
    Ok(())
}

#[tokio::test]
async fn insufficient_memory_refuses_a_further_game_without_disturbing_admitted_hash()
-> GameConcurrentGamesResult {
    let budget = CapacityBudget::new(NonZeroUsize::new(4).ok_or("empty budget")?)?;
    let memory = Arc::new(GameConcurrentGamesMemory(AtomicU64::new(
        64 * 1024 * 1024 * 1024,
    )));
    let resources = DesktopNativeGameResources::new(DesktopNativeGameResourcesOptions {
        budget: budget.clone(),
        policy: DesktopNativeGameResourcesPolicy::new(4, 64)?,
        memory: Arc::clone(&memory),
    })?;
    let first = context(1)?;
    let _registration = resources.register(first).await?;
    let allocation = resources.allocate(first).await?;
    let configured = allocation.config().hash_mib();
    resources.acknowledge(first, allocation.config()).await?;
    drop(allocation);
    memory.0.store(64 * 1024 * 1024, Ordering::SeqCst);
    assert!(matches!(
        resources.register(context(2)?).await,
        Err(DesktopNativeGameResourcesError::MemoryAdmission { .. })
    ));
    let again = resources.allocate(first).await?;
    assert_eq!(again.config().hash_mib(), configured);
    assert_eq!(resources.policy().hash_mib(), 64);
    Ok(())
}

#[tokio::test]
async fn closing_a_game_releases_its_reservation_so_another_can_be_recreated()
-> GameConcurrentGamesResult {
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("empty budget")?)?;
    let memory = Arc::new(GameConcurrentGamesMemory(AtomicU64::new(
        64 * 1024 * 1024 * 1024,
    )));
    let resources = DesktopNativeGameResources::new(DesktopNativeGameResourcesOptions {
        budget: budget.clone(),
        policy: DesktopNativeGameResourcesPolicy::new(1, 64)?,
        memory,
    })?;
    let registration = resources.register(context(1)?).await?;
    assert!(matches!(
        resources.register(context(2)?).await,
        Err(DesktopNativeGameResourcesError::ThreadAdmission { .. })
    ));
    drop(registration);
    let _recreated = resources.register(context(2)?).await?;
    assert_eq!(budget.available(), 1);
    Ok(())
}
