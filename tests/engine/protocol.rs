use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::engine::search::enums::{EngineSearchEnumOutput, EngineSearchEnumState};
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
};
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::search::stop::EngineSearchStop;
use chess::engine::session::EngineSessionError;
use tokio::time::Instant;
use uuid::Uuid;

use crate::engine_lifecycle::EngineLifecycleProvider;
use crate::engine_package_selection::EnginePackageFixture;

type EngineProtocolTestResult = Result<(), Box<dyn std::error::Error>>;

struct EngineProtocolFixture;

impl EngineProtocolFixture {
    const STALE_CYCLES: u64 = 1_000;

    fn context(revision: u64) -> Result<EngineSearchContext, Box<dyn std::error::Error>> {
        Ok(EngineSearchContext::new(
            EngineSearchGameId::try_from(Uuid::from_u128(1))?,
            EngineSearchRevision::new(revision),
        ))
    }

    fn position() -> Result<EngineSearchPosition, Box<dyn std::error::Error>> {
        Ok(EngineSearchPosition::new(
            BoardPosition::standard(),
            std::iter::empty::<&str>(),
        )?)
    }
}

#[tokio::test(start_paused = true)]
async fn one_thousand_stale_result_cycles_never_reach_the_engine_or_a_newer_search()
-> EngineProtocolTestResult {
    let package = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&package).await?;
    let mut previous = None;
    for revision in 1..=EngineProtocolFixture::STALE_CYCLES {
        let tag = session
            .start_search(
                EngineProtocolFixture::context(revision)?,
                EngineProtocolFixture::position()?,
                EngineSearchLimit::infinite(),
            )
            .await?;
        provider.expect("position startpos").await?;
        provider.expect("go infinite").await?;
        if let Some(old) = previous {
            assert_ne!(old, tag);
            assert!(matches!(
                session
                    .stop(EngineSearchStop::received_at(old, Instant::now())?)
                    .await,
                Err(EngineSessionError::StaleSearch)
            ));
            provider.assert_no_command();
            assert_eq!(session.state(), EngineSearchEnumState::Searching);
        }
        provider.emit("bestmove e2e4").await?;
        let result = session.next_event().await?;
        assert_eq!(result.tag(), tag);
        assert!(
            matches!(result.output(), EngineSearchEnumOutput::BestMove(movement)
            if movement.uci(BoardEnumVariant::Standard) == "e2e4")
        );
        assert_eq!(session.state(), EngineSearchEnumState::Idle);
        previous = Some(tag);
    }
    session.shutdown().await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn readiness_is_never_completion_for_depth_node_and_infinite_searches()
-> EngineProtocolTestResult {
    let package = EnginePackageFixture::new()?;
    for (limit, command) in [
        (EngineSearchLimit::depth(5)?, "go depth 5"),
        (EngineSearchLimit::nodes(4_096)?, "go nodes 4096"),
        (EngineSearchLimit::infinite(), "go infinite"),
    ] {
        let mut provider = EngineLifecycleProvider::new();
        let mut session = provider.start(&package).await?;
        let tag = session
            .start_search(
                EngineProtocolFixture::context(1)?,
                EngineProtocolFixture::position()?,
                limit,
            )
            .await?;
        provider.expect("position startpos").await?;
        provider.expect(command).await?;
        provider.emit("readyok").await?;
        let readiness = session.next_event().await?;
        assert_eq!(readiness.tag(), tag);
        assert!(matches!(
            readiness.output(),
            EngineSearchEnumOutput::Readiness
        ));
        assert_eq!(session.state(), EngineSearchEnumState::Searching);
        provider.emit("bestmove d2d4").await?;
        let result = session.next_event().await?;
        assert!(matches!(
            result.output(),
            EngineSearchEnumOutput::BestMove(_)
        ));
        assert_eq!(session.state(), EngineSearchEnumState::Idle);
        session.shutdown().await?;
        provider.expect("shutdown").await?;
    }
    Ok(())
}
