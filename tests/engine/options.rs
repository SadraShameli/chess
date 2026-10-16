use std::future::{Future, poll_fn};
use std::num::NonZeroU32;
use std::task::Poll;

use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::clock::duration::ClockDuration;
use chess::engine::process_provider::EngineProcessLimits;
use chess::engine::search::enums::{EngineSearchEnumOutput, EngineSearchEnumState};
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision,
};
use chess::engine::search::limit::{EngineSearchClock, EngineSearchLimit, EngineSearchLimitError};
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::search::stop::EngineSearchStop;
use chess::engine::session::EngineSessionError;
use tokio::time::Instant;
use uuid::Uuid;

use crate::engine_lifecycle::EngineLifecycleProvider;
use crate::engine_package_selection::EnginePackageFixture;

type EngineOptionsTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct EngineOptionsFixture;

impl EngineOptionsFixture {
    fn context(revision: u64) -> EngineOptionsTestResult<EngineSearchContext> {
        Ok(EngineSearchContext::new(
            EngineSearchGameId::try_from(Uuid::from_u128(1))?,
            EngineSearchRevision::new(revision),
        ))
    }

    fn position() -> EngineOptionsTestResult<EngineSearchPosition> {
        Ok(EngineSearchPosition::new(
            BoardPosition::standard(),
            std::iter::empty::<&str>(),
        )?)
    }
}

#[test]
fn scalar_limits_accept_both_boundaries_and_reject_empty_or_overflowing_budgets()
-> EngineOptionsTestResult {
    for depth in [1, EngineSearchLimit::MAX_DEPTH] {
        assert_eq!(
            EngineSearchLimit::depth(depth)?.command(),
            format!("go depth {depth}")
        );
    }
    for depth in [0, EngineSearchLimit::MAX_DEPTH + 1, u32::MAX] {
        assert!(matches!(
            EngineSearchLimit::depth(depth),
            Err(EngineSearchLimitError::Depth)
        ));
    }
    for budget in [1, EngineSearchLimit::MAX_NUMBER] {
        assert_eq!(
            EngineSearchLimit::nodes(budget)?.command(),
            format!("go nodes {budget}")
        );
        assert_eq!(
            EngineSearchLimit::move_time(ClockDuration::from_millis(budget)?)?.command(),
            format!("go movetime {budget}")
        );
    }
    assert!(matches!(
        EngineSearchLimit::nodes(0),
        Err(EngineSearchLimitError::EmptyBudget)
    ));
    for duration in [ClockDuration::ZERO, ClockDuration::from_nanos(999_999)] {
        assert!(matches!(
            EngineSearchLimit::move_time(duration),
            Err(EngineSearchLimitError::EmptyBudget)
        ));
    }
    for nodes in [EngineSearchLimit::MAX_NUMBER + 1, u64::MAX] {
        assert!(matches!(
            EngineSearchLimit::nodes(nodes),
            Err(EngineSearchLimitError::NumericRange)
        ));
    }
    assert!(matches!(
        EngineSearchLimit::move_time(ClockDuration::from_millis(
            EngineSearchLimit::MAX_NUMBER + 1
        )?),
        Err(EngineSearchLimitError::NumericRange)
    ));
    Ok(())
}

#[test]
fn clock_limits_preserve_both_sides_and_optional_moves_to_go_at_the_numeric_boundaries()
-> EngineOptionsTestResult {
    let maximum = ClockDuration::from_millis(EngineSearchLimit::MAX_NUMBER)?;
    let maximum_moves = u32::try_from(EngineSearchLimit::MAX_NUMBER)?;
    for moves in [None, NonZeroU32::new(1), NonZeroU32::new(maximum_moves)] {
        let clock = EngineSearchClock::new(maximum, maximum, maximum, maximum, moves)?;
        let mut expected = format!(
            "go wtime {0} btime {0} winc {0} binc {0}",
            EngineSearchLimit::MAX_NUMBER
        );
        if let Some(moves) = moves {
            expected.push_str(&format!(" movestogo {moves}"));
        }
        assert_eq!(EngineSearchLimit::clock(clock).command(), expected);
    }
    assert_eq!(
        EngineSearchLimit::clock(EngineSearchClock::new(
            ClockDuration::from_millis(1)?,
            ClockDuration::from_millis(2)?,
            ClockDuration::ZERO,
            ClockDuration::ZERO,
            None,
        )?)
        .command(),
        "go wtime 1 btime 2 winc 0 binc 0"
    );
    Ok(())
}

#[test]
fn each_clock_field_and_moves_to_go_reject_parser_overflow_independently() -> EngineOptionsTestResult
{
    let valid = ClockDuration::from_millis(1)?;
    let invalid = ClockDuration::from_millis(EngineSearchLimit::MAX_NUMBER + 1)?;
    let zero = ClockDuration::ZERO;
    for result in [
        EngineSearchClock::new(invalid, valid, zero, zero, None),
        EngineSearchClock::new(valid, invalid, zero, zero, None),
        EngineSearchClock::new(valid, valid, invalid, zero, None),
        EngineSearchClock::new(valid, valid, zero, invalid, None),
        EngineSearchClock::new(
            valid,
            valid,
            zero,
            zero,
            NonZeroU32::new(u32::try_from(EngineSearchLimit::MAX_NUMBER + 1)?),
        ),
    ] {
        assert!(matches!(result, Err(EngineSearchLimitError::NumericRange)));
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn finite_search_modes_send_one_exclusive_limit_and_correlate_natural_completion()
-> EngineOptionsTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    let clock = EngineSearchClock::new(
        ClockDuration::from_millis(60_000)?,
        ClockDuration::from_millis(59_000)?,
        ClockDuration::from_millis(1_000)?,
        ClockDuration::from_millis(2_000)?,
        NonZeroU32::new(23),
    )?;
    for (revision, (limit, expected)) in [
        (EngineSearchLimit::depth(12)?, "go depth 12"),
        (EngineSearchLimit::nodes(64)?, "go nodes 64"),
        (
            EngineSearchLimit::move_time(ClockDuration::from_millis(50)?)?,
            "go movetime 50",
        ),
        (
            EngineSearchLimit::clock(clock),
            "go wtime 60000 btime 59000 winc 1000 binc 2000 movestogo 23",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let context = EngineOptionsFixture::context(u64::try_from(revision)?)?;
        let tag = session
            .start_search(context, EngineOptionsFixture::position()?, limit)
            .await?;
        assert_eq!(tag.context(), context);
        assert_eq!(tag.generation(), session.generation());
        provider.expect("position startpos").await?;
        provider.expect(expected).await?;
        provider.assert_no_command();
        provider.emit("readyok").await?;
        let readiness = session.next_event().await?;
        assert_eq!(readiness.tag(), tag);
        assert!(matches!(
            readiness.output(),
            EngineSearchEnumOutput::Readiness
        ));
        assert_eq!(session.state(), EngineSearchEnumState::Searching);
        provider.emit("bestmove e2e4").await?;
        let completed = session.next_event().await?;
        assert_eq!(completed.tag(), tag);
        assert!(matches!(
            completed.output(),
            EngineSearchEnumOutput::BestMove(movement)
                if movement.uci(BoardEnumVariant::Standard) == "e2e4"
        ));
        assert_eq!(session.state(), EngineSearchEnumState::Idle);
        provider.assert_no_command();
    }
    session
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    provider.expect("shutdown").await?;
    assert!(session.is_shutdown_complete());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn infinite_analysis_requires_explicit_stop_and_readyok_never_completes_the_barrier()
-> EngineOptionsTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    let tag = session
        .start_search(
            EngineOptionsFixture::context(1)?,
            EngineOptionsFixture::position()?,
            EngineSearchLimit::infinite(),
        )
        .await?;
    provider.expect("position startpos").await?;
    provider.expect("go infinite").await?;
    provider.emit("readyok").await?;
    let readiness = session.next_event().await?;
    assert_eq!(readiness.tag(), tag);
    assert!(matches!(
        readiness.output(),
        EngineSearchEnumOutput::Readiness
    ));
    assert_eq!(session.state(), EngineSearchEnumState::Searching);
    provider.assert_no_command();
    {
        let stopping = session.stop(EngineSearchStop::received_at(tag, Instant::now())?);
        tokio::pin!(stopping);
        assert!(
            poll_fn(|context| { Poll::Ready(stopping.as_mut().poll(context).is_pending()) }).await
        );
        provider.expect("stop").await?;
        provider.emit("readyok").await?;
        assert!(
            poll_fn(|context| { Poll::Ready(stopping.as_mut().poll(context).is_pending()) }).await
        );
        provider.emit("bestmove e2e4").await?;
        assert_eq!(stopping.await?, tag);
    }
    assert_eq!(session.state(), EngineSearchEnumState::Idle);
    assert!(matches!(
        session.next_event().await,
        Err(EngineSessionError::State(EngineSearchEnumState::Idle))
    ));
    provider.assert_no_command();
    session
        .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
        .await?;
    provider.expect("shutdown").await?;
    assert!(session.is_shutdown_complete());
    Ok(())
}
