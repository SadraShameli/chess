use std::future::Future;
use std::task::Poll;
use std::time::Duration;

use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::clock::duration::ClockDuration;
use chess::engine::enums::EngineEnumOption;
use chess::engine::resource_config::{EngineResourceConfig, EngineResourceConfigError};
use chess::engine::search::enums::EngineSearchEnumState;
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
};
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::session::EngineSessionError;
use uuid::Uuid;

use crate::engine_lifecycle::EngineLifecycleProcess;
use crate::engine_lifecycle::EngineLifecycleProvider;
use crate::engine_package_selection::EnginePackageFixture;

struct EngineResourceConfigFixture;

impl EngineResourceConfigFixture {
    async fn apply(
        provider: &mut EngineLifecycleProvider,
        session: &mut chess::engine::session::EngineSession<EngineLifecycleProcess>,
        config: EngineResourceConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let settings = [
            ("Threads", u64::from(config.threads()), ""),
            ("Hash", u64::from(config.hash_mib()), " MB"),
            ("MoveOverhead", config.move_overhead().as_millis(), " ms"),
        ];
        for (name, value, suffix) in settings {
            provider
                .emit(&format!("info string set {name} to {value}{suffix}"))
                .await?;
        }
        session.configure_resources(config).await?;
        for (name, value, _) in settings {
            provider
                .expect(&format!("setoption name {name} value {value}"))
                .await?;
        }
        provider.expect("isready").await?;
        provider.assert_no_command();
        assert_eq!(session.resources(), Some(config));
        assert_eq!(session.state(), EngineSearchEnumState::Idle);
        Ok(())
    }
}

#[test]
fn resource_options_validate_representation_and_keep_fixed_hash_and_overhead_bounds()
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
    assert!(matches!(
        EngineResourceConfig::new(0, 1, ClockDuration::ZERO),
        Err(EngineResourceConfigError::EmptyThreads)
    ));
    for threads in [usize::try_from(u64::from(u32::MAX) + 1)?, usize::MAX] {
        assert!(matches!(
            EngineResourceConfig::new(threads, 1, ClockDuration::ZERO),
            Err(EngineResourceConfigError::Conversion(_))
        ));
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
    for threads in [513, 1024, usize::try_from(u32::MAX)?] {
        assert_eq!(
            EngineResourceConfig::new(threads, 1, ClockDuration::ZERO)?.threads(),
            u32::try_from(threads)?
        );
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn resource_application_uses_each_sessions_advertised_threads_bound()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    for (maximum, requested) in [(512, 512), (1024, 513), (1024, 1024)] {
        let mut provider = EngineLifecycleProvider::new();
        provider.advertise_threads_maximum(maximum);
        let mut session = provider.start(&fixture).await?;
        EngineResourceConfigFixture::apply(
            &mut provider,
            &mut session,
            EngineResourceConfig::new(requested, 256, ClockDuration::ZERO)?,
        )
        .await?;
        session.shutdown().await?;
        provider.expect("shutdown").await?;
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn unsupported_threads_preserve_idle_resources_and_reusability_without_io()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    for maximum in [512, 1024] {
        let mut provider = EngineLifecycleProvider::new();
        provider.advertise_threads_maximum(maximum);
        let mut session = provider.start(&fixture).await?;
        let original = EngineResourceConfig::new(1, 64, ClockDuration::ZERO)?;
        EngineResourceConfigFixture::apply(&mut provider, &mut session, original).await?;
        let unsupported =
            EngineResourceConfig::new(usize::try_from(maximum)? + 1, 256, ClockDuration::ZERO)?;
        assert!(matches!(session.configure_resources(unsupported).await,
            Err(EngineSessionError::ResourceConfig(EngineResourceConfigError::Range {
                option: EngineEnumOption::Threads,
                value,
                minimum: 1,
                maximum: advertised,
            })) if value == u64::from(maximum) + 1 && advertised == maximum
        ));
        provider.assert_no_command();
        assert_eq!(session.state(), EngineSearchEnumState::Idle);
        assert_eq!(session.resources(), Some(original));
        EngineResourceConfigFixture::apply(
            &mut provider,
            &mut session,
            EngineResourceConfig::new(usize::try_from(maximum)?, 256, ClockDuration::ZERO)?,
        )
        .await?;
        session.new_game(BoardEnumVariant::Standard).await?;
        provider.expect("ucinewgame").await?;
        provider
            .expect("setoption name UCI_Chess960 value false")
            .await?;
        provider.expect("isready").await?;
        session.shutdown().await?;
        provider.expect("shutdown").await?;
        provider.assert_no_command();
    }
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
