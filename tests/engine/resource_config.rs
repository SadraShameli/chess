use std::future::Future;
use std::task::Poll;
use std::time::Duration;

use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::clock::duration::ClockDuration;
use chess::engine::enums::EngineEnumOption;
use chess::engine::resource_config::EngineResourceConfig;
use chess::engine::search::enums::EngineSearchEnumState;
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
};
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::session::EngineSessionError;
use uuid::Uuid;

use crate::engine_lifecycle::EngineLifecycleProvider;
use crate::engine_package_selection::EnginePackageFixture;

#[test]
fn resource_options_share_canonical_spin_bounds_and_keep_every_allocated_thread()
-> Result<(), Box<dyn std::error::Error>> {
    let config = EngineResourceConfig::new(512, 262_144, ClockDuration::from_millis(2_000)?)?;
    assert_eq!(config.threads(), 512);
    assert_eq!(config.hash_mib(), 262_144);
    assert_eq!(config.move_overhead(), ClockDuration::from_millis(2_000)?);
    assert_eq!(
        EngineEnumOption::Threads.descriptor(),
        "spin default 1 min 1 max 512"
    );
    assert_eq!(
        EngineEnumOption::Hash.descriptor(),
        "spin default 16 min 1 max 262144"
    );
    assert_eq!(
        EngineEnumOption::MoveOverhead.descriptor(),
        "spin default 100 min 0 max 2000"
    );
    for threads in [0, 513, usize::MAX] {
        assert!(EngineResourceConfig::new(threads, 1, ClockDuration::ZERO).is_err());
    }
    for hash_mib in [0, 262_145, u32::MAX] {
        assert!(EngineResourceConfig::new(1, hash_mib, ClockDuration::ZERO).is_err());
    }
    for overhead in [
        ClockDuration::from_millis(2_001)?,
        ClockDuration::from_nanos(1),
    ] {
        assert!(EngineResourceConfig::new(1, 1, overhead).is_err());
    }
    assert!(EngineResourceConfig::new(1, 1, ClockDuration::ZERO).is_ok());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn resource_application_waits_for_each_exact_acknowledgment_before_readiness()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    {
        let configuring = session.configure_resources(EngineResourceConfig::new(
            512,
            256,
            ClockDuration::from_millis(25)?,
        )?);
        tokio::pin!(configuring);
        for (command, acknowledgment) in [
            (
                "setoption name Threads value 512",
                "info string set Threads to 512",
            ),
            (
                "setoption name Hash value 256",
                "info string set Hash to 256 MB",
            ),
            (
                "setoption name MoveOverhead value 25",
                "info string set MoveOverhead to 25 ms",
            ),
        ] {
            let status =
                std::future::poll_fn(|context| Poll::Ready(configuring.as_mut().poll(context)))
                    .await;
            assert!(status.is_pending());
            provider.expect(command).await?;
            provider.assert_no_command();
            provider.emit(acknowledgment).await?;
        }
        configuring.await?;
    }
    provider.expect("isready").await?;
    assert_eq!(session.state(), EngineSearchEnumState::Idle);
    assert_eq!(
        session
            .resources()
            .ok_or("missing applied resources")?
            .threads(),
        512
    );
    provider.assert_no_command();
    session.shutdown().await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn active_search_refuses_resource_changes_before_any_option_is_sent()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    session
        .start_search(
            EngineSearchContext::new(
                EngineSearchGameId::try_from(Uuid::from_u128(1))?,
                EngineSearchRevision::new(1),
            ),
            EngineSearchPosition::new(BoardPosition::standard(), std::iter::empty::<&str>())?,
            EngineSearchLimit::infinite(),
        )
        .await?;
    provider.expect("position startpos").await?;
    provider.expect("go infinite").await?;
    assert!(matches!(
        session
            .configure_resources(EngineResourceConfig::new(8, 256, ClockDuration::ZERO)?)
            .await,
        Err(EngineSessionError::State(EngineSearchEnumState::Searching))
    ));
    provider.assert_no_command();
    provider.emit("bestmove e2e4").await?;
    session.next_event().await?;
    for acknowledgment in [
        "info string set Threads to 8",
        "info string set Hash to 256 MB",
        "info string set MoveOverhead to 0 ms",
    ] {
        provider.emit(acknowledgment).await?;
    }
    session
        .configure_resources(EngineResourceConfig::new(8, 256, ClockDuration::ZERO)?)
        .await?;
    provider.expect("setoption name Threads value 8").await?;
    provider.expect("setoption name Hash value 256").await?;
    provider
        .expect("setoption name MoveOverhead value 0")
        .await?;
    provider.expect("isready").await?;
    assert_eq!(session.resources().ok_or("missing resources")?.threads(), 8);
    session.shutdown().await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn missing_option_acknowledgment_times_out_and_forces_observed_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    let started = tokio::time::Instant::now();
    assert!(matches!(
        session
            .configure_resources(EngineResourceConfig::new(8, 256, ClockDuration::ZERO)?)
            .await,
        Err(EngineSessionError::Timeout("resource configuration"))
    ));
    assert_eq!(
        tokio::time::Instant::now() - started,
        Duration::from_secs(5)
    );
    assert_eq!(session.state(), EngineSearchEnumState::Failed);
    assert!(session.resources().is_none());
    assert!(session.is_shutdown_complete());
    provider.expect("setoption name Threads value 8").await?;
    provider.expect("terminate").await?;
    provider.assert_no_command();
    session.shutdown().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn premature_readyok_or_wrong_option_acknowledgment_cannot_publish_resources()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    for output in [
        "readyok",
        "info string set Threads to 7",
        "info string set Hash to 256 MB",
        "bestmove e2e4",
    ] {
        let mut provider = EngineLifecycleProvider::new();
        let mut session = provider.start(&fixture).await?;
        provider.emit(output).await?;
        assert!(
            session
                .configure_resources(EngineResourceConfig::new(8, 256, ClockDuration::ZERO)?)
                .await
                .is_err()
        );
        assert_eq!(session.state(), EngineSearchEnumState::Failed);
        assert!(session.resources().is_none());
        provider.expect("setoption name Threads value 8").await?;
        provider.expect("terminate").await?;
        provider.assert_no_command();
        session.shutdown().await?;
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_configuration_retains_preparing_state_and_forces_shutdown()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    {
        let configuring =
            session.configure_resources(EngineResourceConfig::new(8, 256, ClockDuration::ZERO)?);
        tokio::pin!(configuring);
        let state =
            std::future::poll_fn(|context| Poll::Ready(configuring.as_mut().poll(context))).await;
        assert!(state.is_pending());
        provider.expect("setoption name Threads value 8").await?;
    }
    assert_eq!(session.state(), EngineSearchEnumState::Preparing);
    assert!(session.resources().is_none());
    assert!(session.new_game(BoardEnumVariant::Standard).await.is_err());
    assert!(session.readiness().await.is_err());
    provider.assert_no_command();
    session.shutdown().await?;
    provider.expect("terminate").await?;
    provider.assert_no_command();
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn new_game_consumes_the_real_variant_acknowledgment_before_readiness_and_search()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    session.new_game(BoardEnumVariant::Chess960).await?;
    provider.expect("ucinewgame").await?;
    provider
        .expect("setoption name UCI_Chess960 value true")
        .await?;
    provider.expect("isready").await?;
    let position =
        BoardPosition::from_fen(&BoardPosition::standard().fen(), BoardEnumVariant::Chess960)?;
    let expected_position = format!("position fen {}", position.fen());
    session
        .start_search(
            EngineSearchContext::new(
                EngineSearchGameId::try_from(Uuid::from_u128(1))?,
                EngineSearchRevision::new(1),
            ),
            EngineSearchPosition::new(position, std::iter::empty::<&str>())?,
            EngineSearchLimit::depth(1)?,
        )
        .await?;
    provider.expect(&expected_position).await?;
    provider.expect("go depth 1").await?;
    provider.emit("bestmove e2e4").await?;
    let event = session.next_event().await?;
    assert!(matches!(
        event.output(),
        chess::engine::search::enums::EngineSearchEnumOutput::BestMove(_)
    ));
    session.shutdown().await?;
    provider.expect("shutdown").await?;
    Ok(())
}
