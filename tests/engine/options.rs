use std::future::{Future, poll_fn};
use std::num::NonZeroU32;
use std::task::Poll;

use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::clock::duration::ClockDuration;
use chess::engine::analysis::{EngineAnalysis, EngineAnalysisStaticEvaluation};
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

use chess::engine::capabilities::{EngineCapabilities, EngineCapabilitiesVersion};
use chess::engine::enums::EngineEnumOption;
use chess::engine::options::{
    EngineOptionsEnumOutcome, EngineOptionsEnumTablebases, EngineOptionsEnumValue,
    EngineOptionsSetting, EngineOptionsSyzygyPath,
};
use chess::engine::process_provider::EngineProcessEnumPipe;
use chess::engine::resource_config::EngineResourceConfig;

impl EngineOptionsFixture {
    fn value(option: EngineEnumOption) -> EngineOptionsTestResult<EngineOptionsEnumValue> {
        Ok(match option {
            EngineEnumOption::Hash => EngineOptionsEnumValue::Hash(32),
            EngineEnumOption::Threads => EngineOptionsEnumValue::Threads(8),
            EngineEnumOption::MoveOverhead => {
                EngineOptionsEnumValue::MoveOverhead(ClockDuration::from_millis(25)?)
            }
            EngineEnumOption::Minimal => EngineOptionsEnumValue::Minimal(true),
            EngineEnumOption::ClearHash => EngineOptionsEnumValue::ClearHash,
            EngineEnumOption::Chess960 => {
                EngineOptionsEnumValue::Chess960(BoardEnumVariant::Chess960)
            }
            EngineEnumOption::MultiPv => EngineOptionsEnumValue::MultiPv(256),
            EngineEnumOption::SyzygyPath => EngineOptionsEnumValue::SyzygyPath(
                EngineOptionsSyzygyPath::new("/fixture/tables".to_owned())?,
            ),
        })
    }

    fn response(option: EngineEnumOption) -> Option<&'static str> {
        match option {
            EngineEnumOption::Hash => Some("info string set Hash to 32 MB"),
            EngineEnumOption::Threads => Some("info string set Threads to 8"),
            EngineEnumOption::MoveOverhead => Some("info string set MoveOverhead to 25 ms"),
            EngineEnumOption::Minimal => None,
            EngineEnumOption::ClearHash => Some("info string Hash cleared"),
            EngineEnumOption::Chess960 => Some("info string set UCI_Chess960 to true"),
            EngineEnumOption::MultiPv => Some("info string set MultiPV to 256"),
            EngineEnumOption::SyzygyPath => {
                Some("info string Loaded Syzygy tablebases with 6 pieces")
            }
        }
    }

    fn command(option: EngineEnumOption) -> &'static str {
        match option {
            EngineEnumOption::Hash => "setoption name Hash value 32",
            EngineEnumOption::Threads => "setoption name Threads value 8",
            EngineEnumOption::MoveOverhead => "setoption name MoveOverhead value 25",
            EngineEnumOption::Minimal => "setoption name Minimal value true",
            EngineEnumOption::ClearHash => "setoption name Clear Hash",
            EngineEnumOption::Chess960 => "setoption name UCI_Chess960 value true",
            EngineEnumOption::MultiPv => "setoption name MultiPV value 256",
            EngineEnumOption::SyzygyPath => "setoption name SyzygyPath value /fixture/tables",
        }
    }

    fn capabilities() -> EngineOptionsTestResult<EngineCapabilities> {
        let mut capabilities = EngineCapabilities::default();
        for option in EngineEnumOption::ALL {
            capabilities.discover(&format!(
                "option name {} type {}",
                option.name(),
                option.descriptor()
            ))?;
        }
        Ok(capabilities)
    }
}

#[test]
fn versioned_settings_validate_every_registered_descriptor_and_path_before_application()
-> EngineOptionsTestResult {
    let capabilities = EngineOptionsFixture::capabilities()?;
    assert_eq!(capabilities.version(), EngineCapabilitiesVersion::CURRENT);
    assert_eq!(
        EngineCapabilitiesVersion::try_from(1)?,
        capabilities.version()
    );
    assert!(EngineCapabilitiesVersion::try_from(0).is_err());
    assert!(EngineCapabilitiesVersion::try_from(2).is_err());
    for option in EngineEnumOption::ALL {
        let setting =
            EngineOptionsSetting::new(EngineOptionsFixture::value(option)?, &capabilities)?;
        assert_eq!(setting.option(), option);
        assert_eq!(setting.version(), capabilities.version());
        assert_eq!(setting.command(), EngineOptionsFixture::command(option));
        assert!(
            EngineOptionsSetting::new(
                EngineOptionsFixture::value(option)?,
                &EngineCapabilities::default()
            )
            .is_err()
        );
    }
    for value in [
        EngineOptionsEnumValue::Hash(0),
        EngineOptionsEnumValue::Hash(262_145),
        EngineOptionsEnumValue::Threads(0),
        EngineOptionsEnumValue::Threads(513),
        EngineOptionsEnumValue::MoveOverhead(ClockDuration::from_nanos(1)),
        EngineOptionsEnumValue::MoveOverhead(ClockDuration::from_millis(2_001)?),
        EngineOptionsEnumValue::MultiPv(0),
        EngineOptionsEnumValue::MultiPv(257),
    ] {
        assert!(EngineOptionsSetting::new(value, &capabilities).is_err());
    }
    for path in [
        "",
        " ",
        "/a b",
        "/a\nb",
        "/a\rb",
        "/a\tb",
        "/a\0b",
        "/a\u{1b}b",
        "/a\u{a0}b",
    ] {
        assert!(EngineOptionsSyzygyPath::new(path.to_owned()).is_err());
    }
    assert!(EngineOptionsSyzygyPath::new("x".repeat(EngineProcessLimits::LINE_BYTES)).is_err());
    for value in [1, 256] {
        EngineOptionsSetting::new(EngineOptionsEnumValue::MultiPv(value), &capabilities)?;
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn every_registered_option_is_applied_with_its_actual_response_and_readiness()
-> EngineOptionsTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut covered = Vec::new();
    for option in EngineEnumOption::ALL {
        let mut provider = EngineLifecycleProvider::new();
        let mut session = provider.start(&fixture).await?;
        let generation = session.generation();
        let setting = EngineOptionsSetting::new(
            EngineOptionsFixture::value(option)?,
            session.identity().capabilities(),
        )?;
        if option != EngineEnumOption::Chess960
            && let Some(response) = EngineOptionsFixture::response(option)
        {
            provider.emit(response).await?;
        }
        let outcome = session.configure_option(setting).await?;
        provider
            .expect(EngineOptionsFixture::command(option))
            .await?;
        provider.expect("isready").await?;
        assert_eq!(session.state(), EngineSearchEnumState::Idle);
        assert_eq!(session.generation(), generation);
        if option == EngineEnumOption::SyzygyPath {
            assert!(
                matches!(outcome, EngineOptionsEnumOutcome::Tablebases(EngineOptionsEnumTablebases::Loaded { pieces, .. }) if pieces.get() == 6)
            );
        } else {
            assert_eq!(outcome, EngineOptionsEnumOutcome::Applied(option));
        }
        let applied = session
            .applied_options()
            .ok_or("missing applied option state")?;
        match option {
            EngineEnumOption::Minimal => assert!(applied.is_minimal()),
            EngineEnumOption::Chess960 => assert_eq!(applied.variant(), BoardEnumVariant::Chess960),
            EngineEnumOption::MultiPv => assert_eq!(applied.multi_pv().get(), 256),
            EngineEnumOption::SyzygyPath => assert!(matches!(
                applied.tablebases(),
                EngineOptionsEnumTablebases::Loaded { .. }
            )),
            EngineEnumOption::Hash
            | EngineEnumOption::Threads
            | EngineEnumOption::MoveOverhead
            | EngineEnumOption::ClearHash => {}
        }
        covered.push(option);
        provider.assert_no_command();
        session.shutdown().await?;
        provider.expect("shutdown").await?;
    }
    assert_eq!(covered, EngineEnumOption::ALL);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn active_search_refuses_every_setting_without_changing_resources_or_generation()
-> EngineOptionsTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    for line in [
        "info string set Threads to 8",
        "info string set Hash to 32 MB",
        "info string set MoveOverhead to 25 ms",
    ] {
        provider.emit(line).await?;
    }
    let resources = EngineResourceConfig::new(8, 32, ClockDuration::from_millis(25)?)?;
    session.configure_resources(resources).await?;
    for command in [
        "setoption name Threads value 8",
        "setoption name Hash value 32",
        "setoption name MoveOverhead value 25",
        "isready",
    ] {
        provider.expect(command).await?;
    }
    let tag = session
        .start_search(
            EngineOptionsFixture::context(1)?,
            EngineOptionsFixture::position()?,
            EngineSearchLimit::infinite(),
        )
        .await?;
    provider.expect("position startpos").await?;
    provider.expect("go infinite").await?;
    for option in EngineEnumOption::ALL {
        let setting = EngineOptionsSetting::new(
            EngineOptionsFixture::value(option)?,
            session.identity().capabilities(),
        )?;
        assert!(matches!(
            session.configure_option(setting).await,
            Err(EngineSessionError::State(EngineSearchEnumState::Searching))
        ));
        assert_eq!(session.resources(), Some(resources));
        assert_eq!(session.generation(), tag.generation());
        provider.assert_no_command();
    }
    provider.emit("bestmove e2e4").await?;
    assert_eq!(session.next_event().await?.tag(), tag);
    session.shutdown().await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn minimal_waits_only_for_readiness_and_cancelled_application_retains_ambiguous_ownership()
-> EngineOptionsTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    provider.set_readiness(false);
    let setting = EngineOptionsSetting::new(
        EngineOptionsEnumValue::Minimal(true),
        session.identity().capabilities(),
    )?;
    {
        let configuring = session.configure_option(setting);
        tokio::pin!(configuring);
        assert!(
            poll_fn(|context| Poll::Ready(configuring.as_mut().poll(context)))
                .await
                .is_pending()
        );
        provider.expect("setoption name Minimal value true").await?;
        provider.expect("isready").await?;
        provider.assert_no_command();
    }
    assert_eq!(session.state(), EngineSearchEnumState::Preparing);
    assert!(session.applied_options().is_none());
    assert!(!session.is_shutdown_complete());
    assert!(session.readiness().await.is_err());
    session.shutdown().await?;
    provider.expect("terminate").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn tablebase_load_outcomes_are_typed_and_readyok_alone_never_claims_success()
-> EngineOptionsTestResult {
    let fixture = EnginePackageFixture::new()?;
    for success in [true, false] {
        let mut provider = EngineLifecycleProvider::new();
        let mut session = provider.start(&fixture).await?;
        let value = EngineOptionsEnumValue::SyzygyPath(EngineOptionsSyzygyPath::new(
            "/fixture/tables".to_owned(),
        )?);
        let setting = EngineOptionsSetting::new(value, session.identity().capabilities())?;
        if success {
            provider
                .emit("info string Loaded Syzygy tablebases with 7 pieces")
                .await?;
        } else {
            provider
                .emit_pipe(
                    EngineProcessEnumPipe::Stderr,
                    "Failed to load Syzygy tablebases",
                )
                .await?;
        }
        let outcome = session.configure_option(setting).await?;
        assert!(
            matches!(outcome, EngineOptionsEnumOutcome::Tablebases(EngineOptionsEnumTablebases::Loaded { pieces, .. }) if success && pieces.get() == 7)
                || matches!(outcome, EngineOptionsEnumOutcome::Tablebases(EngineOptionsEnumTablebases::Unavailable { .. }) if !success)
        );
        provider
            .expect("setoption name SyzygyPath value /fixture/tables")
            .await?;
        provider.expect("isready").await?;
        assert_eq!(session.state(), EngineSearchEnumState::Idle);
        assert!(
            session
                .diagnostics()
                .entries()
                .any(|line| line.text().contains(if success {
                    "Loaded Syzygy"
                } else {
                    "Failed to load Syzygy"
                }))
        );
        session.shutdown().await?;
        provider.expect("shutdown").await?;
    }
    for output in [
        "readyok",
        "info string Loaded Syzygy tablebases with 0 pieces",
        "info string Loaded Syzygy tablebases with invalid pieces",
    ] {
        let mut provider = EngineLifecycleProvider::new();
        let mut session = provider.start(&fixture).await?;
        let setting = EngineOptionsSetting::new(
            EngineOptionsFixture::value(EngineEnumOption::SyzygyPath)?,
            session.identity().capabilities(),
        )?;
        provider.emit(output).await?;
        assert!(session.configure_option(setting).await.is_err());
        assert_eq!(session.state(), EngineSearchEnumState::Failed);
        assert!(session.applied_options().is_none());
        provider
            .expect("setoption name SyzygyPath value /fixture/tables")
            .await?;
        provider.expect("terminate").await?;
        provider.assert_no_command();
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn individual_resources_revalidate_the_session_descriptor_and_never_publish_an_old_config()
-> EngineOptionsTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    let mut high = EngineCapabilities::default();
    high.discover("option name Threads type spin default 1 min 1 max 1024")?;
    let setting = EngineOptionsSetting::new(EngineOptionsEnumValue::Threads(1_024), &high)?;
    assert!(session.configure_option(setting).await.is_err());
    assert_eq!(session.state(), EngineSearchEnumState::Idle);
    provider.assert_no_command();
    for line in [
        "info string set Threads to 8",
        "info string set Hash to 32 MB",
        "info string set MoveOverhead to 25 ms",
    ] {
        provider.emit(line).await?;
    }
    session
        .configure_resources(EngineResourceConfig::new(
            8,
            32,
            ClockDuration::from_millis(25)?,
        )?)
        .await?;
    for command in [
        "setoption name Threads value 8",
        "setoption name Hash value 32",
        "setoption name MoveOverhead value 25",
        "isready",
    ] {
        provider.expect(command).await?;
    }
    for (value, acknowledgment, expected) in [
        (
            EngineOptionsEnumValue::Threads(4),
            "info string set Threads to 4",
            EngineResourceConfig::new(4, 32, ClockDuration::from_millis(25)?)?,
        ),
        (
            EngineOptionsEnumValue::Hash(16),
            "info string set Hash to 16 MB",
            EngineResourceConfig::new(4, 16, ClockDuration::from_millis(25)?)?,
        ),
        (
            EngineOptionsEnumValue::MoveOverhead(ClockDuration::ZERO),
            "info string set MoveOverhead to 0 ms",
            EngineResourceConfig::new(4, 16, ClockDuration::ZERO)?,
        ),
    ] {
        let setting = EngineOptionsSetting::new(value, session.identity().capabilities())?;
        let command = setting.command();
        provider.emit(acknowledgment).await?;
        session.configure_option(setting).await?;
        assert_eq!(session.resources(), Some(expected));
        provider.expect(&command).await?;
        provider.expect("isready").await?;
    }
    let setting = EngineOptionsSetting::new(
        EngineOptionsEnumValue::Hash(64),
        session.identity().capabilities(),
    )?;
    {
        let configuring = session.configure_option(setting);
        tokio::pin!(configuring);
        assert!(
            poll_fn(|context| Poll::Ready(configuring.as_mut().poll(context)))
                .await
                .is_pending()
        );
        provider.expect("setoption name Hash value 64").await?;
    }
    assert!(session.resources().is_none());
    session.shutdown().await?;
    provider.expect("terminate").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn configured_variant_is_reconciled_with_position_and_new_game_by_the_same_owner()
-> EngineOptionsTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    let setting = EngineOptionsSetting::new(
        EngineOptionsEnumValue::Chess960(BoardEnumVariant::Chess960),
        session.identity().capabilities(),
    )?;
    session.configure_option(setting).await?;
    provider
        .expect("setoption name UCI_Chess960 value true")
        .await?;
    provider.expect("isready").await?;
    session
        .start_search(
            EngineOptionsFixture::context(1)?,
            EngineOptionsFixture::position()?,
            EngineSearchLimit::depth(1)?,
        )
        .await?;
    provider
        .expect("setoption name UCI_Chess960 value false")
        .await?;
    provider.expect("isready").await?;
    provider.expect("position startpos").await?;
    provider.expect("go depth 1").await?;
    assert_eq!(
        session
            .applied_options()
            .ok_or("missing options")?
            .variant(),
        BoardEnumVariant::Standard
    );
    provider.emit("bestmove e2e4").await?;
    session.next_event().await?;
    session.new_game(BoardEnumVariant::Chess960).await?;
    provider.expect("ucinewgame").await?;
    provider
        .expect("setoption name UCI_Chess960 value true")
        .await?;
    provider.expect("isready").await?;
    assert_eq!(
        session
            .applied_options()
            .ok_or("missing options")?
            .variant(),
        BoardEnumVariant::Chess960
    );
    session.shutdown().await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn multipv_boundaries_apply_and_unexpected_acknowledgments_fail_closed()
-> EngineOptionsTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    for value in [1, 256] {
        let setting = EngineOptionsSetting::new(
            EngineOptionsEnumValue::MultiPv(value),
            session.identity().capabilities(),
        )?;
        provider
            .emit(&format!("info string set MultiPV to {value}"))
            .await?;
        session.configure_option(setting).await?;
        provider
            .expect(&format!("setoption name MultiPV value {value}"))
            .await?;
        provider.expect("isready").await?;
        assert_eq!(
            session
                .applied_options()
                .ok_or("missing option state")?
                .multi_pv()
                .get(),
            value
        );
    }
    session.shutdown().await?;
    provider.expect("shutdown").await?;
    for option in EngineEnumOption::ALL {
        let mut provider = EngineLifecycleProvider::new();
        let mut session = provider.start(&fixture).await?;
        let setting = EngineOptionsSetting::new(
            EngineOptionsFixture::value(option)?,
            session.identity().capabilities(),
        )?;
        provider
            .emit("info string wrong option acknowledgment")
            .await?;
        assert!(matches!(
            session.configure_option(setting).await,
            Err(EngineSessionError::UnexpectedResponse)
        ));
        assert_eq!(session.state(), EngineSearchEnumState::Failed);
        assert!(session.applied_options().is_none());
        assert!(session.is_shutdown_complete());
        provider
            .expect(EngineOptionsFixture::command(option))
            .await?;
        if option == EngineEnumOption::Minimal {
            provider.expect("isready").await?;
        }
        provider.expect("terminate").await?;
        provider.assert_no_command();
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_tablebase_and_button_waits_retain_the_process_until_forced_cleanup()
-> EngineOptionsTestResult {
    let fixture = EnginePackageFixture::new()?;
    for option in [EngineEnumOption::ClearHash, EngineEnumOption::SyzygyPath] {
        let mut provider = EngineLifecycleProvider::new();
        let mut session = provider.start(&fixture).await?;
        let setting = EngineOptionsSetting::new(
            EngineOptionsFixture::value(option)?,
            session.identity().capabilities(),
        )?;
        {
            let configuring = session.configure_option(setting);
            tokio::pin!(configuring);
            assert!(
                poll_fn(|context| Poll::Ready(configuring.as_mut().poll(context)))
                    .await
                    .is_pending()
            );
            provider
                .expect(EngineOptionsFixture::command(option))
                .await?;
            provider.assert_no_command();
        }
        assert_eq!(session.state(), EngineSearchEnumState::Preparing);
        assert!(session.applied_options().is_none());
        assert!(!session.is_shutdown_complete());
        assert!(session.new_game(BoardEnumVariant::Standard).await.is_err());
        session
            .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
            .await?;
        assert!(session.is_shutdown_complete());
        provider.expect("terminate").await?;
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn failed_tablebase_replacement_clears_prior_loaded_state_without_claiming_unload()
-> EngineOptionsTestResult {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    let path = EngineOptionsSyzygyPath::new("/fixture/tables".to_owned())?;
    for loaded in [true, false] {
        let setting = EngineOptionsSetting::new(
            EngineOptionsEnumValue::SyzygyPath(path.clone()),
            session.identity().capabilities(),
        )?;
        if loaded {
            provider
                .emit("info string Loaded Syzygy tablebases with 6 pieces")
                .await?;
        } else {
            provider
                .emit_pipe(
                    EngineProcessEnumPipe::Stderr,
                    "Failed to load Syzygy tablebases",
                )
                .await?;
        }
        let outcome = session.configure_option(setting).await?;
        let EngineOptionsEnumOutcome::Tablebases(status) = outcome else {
            return Err("missing tablebase outcome".into());
        };
        assert_eq!(
            session
                .applied_options()
                .ok_or("missing applied options")?
                .tablebases(),
            &status
        );
        assert_eq!(
            matches!(status, EngineOptionsEnumTablebases::Loaded { .. }),
            loaded
        );
        assert_eq!(
            matches!(status, EngineOptionsEnumTablebases::Unavailable { .. }),
            !loaded
        );
        provider
            .expect("setoption name SyzygyPath value /fixture/tables")
            .await?;
        provider.expect("isready").await?;
    }
    provider.assert_no_command();
    session.shutdown().await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[test]
fn numeric_options_accept_both_documented_boundaries_with_exact_wire_commands()
-> EngineOptionsTestResult {
    let capabilities = EngineOptionsFixture::capabilities()?;
    for (value, command) in [
        (
            EngineOptionsEnumValue::Hash(1),
            "setoption name Hash value 1",
        ),
        (
            EngineOptionsEnumValue::Hash(262_144),
            "setoption name Hash value 262144",
        ),
        (
            EngineOptionsEnumValue::Threads(1),
            "setoption name Threads value 1",
        ),
        (
            EngineOptionsEnumValue::Threads(512),
            "setoption name Threads value 512",
        ),
        (
            EngineOptionsEnumValue::MoveOverhead(ClockDuration::ZERO),
            "setoption name MoveOverhead value 0",
        ),
        (
            EngineOptionsEnumValue::MoveOverhead(ClockDuration::from_millis(2_000)?),
            "setoption name MoveOverhead value 2000",
        ),
        (
            EngineOptionsEnumValue::Minimal(false),
            "setoption name Minimal value false",
        ),
        (
            EngineOptionsEnumValue::Chess960(BoardEnumVariant::Standard),
            "setoption name UCI_Chess960 value false",
        ),
        (
            EngineOptionsEnumValue::MultiPv(1),
            "setoption name MultiPV value 1",
        ),
    ] {
        assert_eq!(
            EngineOptionsSetting::new(value, &capabilities)?.command(),
            command
        );
    }
    Ok(())
}

#[test]
fn threads_bounds_follow_the_discovered_descriptor_in_both_directions() -> EngineOptionsTestResult {
    let mut high = EngineCapabilities::default();
    high.discover("option name Threads type spin default 1 min 1 max 1024")?;
    EngineOptionsSetting::new(EngineOptionsEnumValue::Threads(1_024), &high)?;
    assert!(EngineOptionsSetting::new(EngineOptionsEnumValue::Threads(1_025), &high).is_err());
    let mut mid = EngineCapabilities::default();
    mid.discover("option name Threads type spin default 1 min 1 max 600")?;
    EngineOptionsSetting::new(EngineOptionsEnumValue::Threads(1), &mid)?;
    EngineOptionsSetting::new(EngineOptionsEnumValue::Threads(600), &mid)?;
    assert!(EngineOptionsSetting::new(EngineOptionsEnumValue::Threads(601), &mid).is_err());
    assert!(EngineOptionsSetting::new(EngineOptionsEnumValue::Threads(0), &mid).is_err());
    Ok(())
}

#[test]
fn syzygy_paths_accept_whitespace_free_aliases_and_format_without_quoting()
-> EngineOptionsTestResult {
    let capabilities = EngineOptionsFixture::capabilities()?;
    for path in [
        "/managed/alias",
        "C:\\managed\\alias",
        "/managed/\u{e9}checs",
        "relative-alias",
    ] {
        let setting = EngineOptionsSetting::new(
            EngineOptionsEnumValue::SyzygyPath(EngineOptionsSyzygyPath::new(path.to_owned())?),
            &capabilities,
        )?;
        assert_eq!(
            setting.command(),
            format!("setoption name SyzygyPath value {path}")
        );
    }
    Ok(())
}

#[test]
fn multipv_rank_is_capped_by_the_legal_root_moves_of_the_searched_position()
-> EngineOptionsTestResult {
    let root = BoardPosition::from_fen("7k/8/8/8/8/8/8/K6R b - - 0 1", BoardEnumVariant::Standard)?;
    let legal = root.legal_moves().len();
    assert_eq!(legal, 2);
    assert!(EngineAnalysis::parse("info multipv 1 pv h8g7", &root).is_ok());
    assert!(EngineAnalysis::parse("info multipv 3 pv h8g7", &root).is_err());
    let start = BoardPosition::standard();
    let all = start.legal_moves().len();
    assert_eq!(all, 20);
    for rank in [1, 2, all] {
        assert!(EngineAnalysis::parse(&format!("info multipv {rank} pv e2e4"), &start).is_ok());
    }
    assert!(EngineAnalysis::parse(&format!("info multipv {} pv e2e4", all + 1), &start).is_err());
    assert!(EngineAnalysis::parse("info multipv 256 pv e2e4", &start).is_err());
    Ok(())
}

#[test]
fn search_scores_and_raw_evaluation_are_distinct_white_perspective_results()
-> EngineOptionsTestResult {
    let black_to_move = BoardPosition::standard().play_uci("e2e4")?;
    let analysis = EngineAnalysis::parse("info depth 9 score cp -50 pv e7e5", &black_to_move)?;
    let score = analysis
        .line()
        .and_then(|line| line.score())
        .ok_or("missing search score")?;
    assert_eq!(score.perspective(), BoardEnumColor::Black);
    assert_eq!(score.white_value(), 50);
    assert_eq!(
        EngineAnalysisStaticEvaluation::parse("-50")?.white_value(),
        -50
    );
    assert!(EngineAnalysisStaticEvaluation::parse("score cp -50").is_err());
    assert!(EngineAnalysis::parse("-50", &black_to_move).is_err());
    Ok(())
}

#[test]
fn engine_option_spellings_are_unchanged_and_the_iterator_drives_the_registry()
-> Result<(), Box<dyn std::error::Error>> {
    use chess::engine::enums::EngineEnumOption;
    use strum::IntoEnumIterator;
    let expected = [
        (EngineEnumOption::Hash, "Hash"),
        (EngineEnumOption::Threads, "Threads"),
        (EngineEnumOption::MoveOverhead, "MoveOverhead"),
        (EngineEnumOption::Minimal, "Minimal"),
        (EngineEnumOption::ClearHash, "Clear Hash"),
        (EngineEnumOption::Chess960, "UCI_Chess960"),
        (EngineEnumOption::MultiPv, "MultiPV"),
        (EngineEnumOption::SyzygyPath, "SyzygyPath"),
    ];
    assert_eq!(EngineEnumOption::iter().collect::<Vec<_>>(), EngineEnumOption::ALL);
    for (option, spelling) in expected {
        assert_eq!(option.name(), spelling);
        assert_eq!(option.to_string(), spelling);
        assert_eq!(spelling.parse::<EngineEnumOption>()?, option);
    }
    assert!("hash".parse::<EngineEnumOption>().is_err());
    Ok(())
}
