use std::error::Error;
#[cfg(unix)]
use std::future::Future;
#[cfg(unix)]
use std::num::NonZeroUsize;
use std::path::PathBuf;
#[cfg(unix)]
use std::task::Poll;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use chess::capacity::budget::CapacityBudget;
#[cfg(unix)]
use chess::capacity::executor::CapacityExecutor;
use chess::engine::package::manifest::EnginePackageEnumTarget;
#[cfg(unix)]
use tokio::process::Command;

use crate::engine::pgo::ToolsEnginePgoEnumInstrumentation;
#[cfg(unix)]
use crate::engine::pgo::{
    ToolsEnginePgoCancellation, ToolsEnginePgoEnumInput, ToolsEnginePgoEnumStage,
    ToolsEnginePgoProcess, ToolsEnginePgoProcessRequest, ToolsEnginePgoResources,
};
use crate::engine::pgo::{ToolsEnginePgoEnumBuild, ToolsEnginePgoError, ToolsEnginePgoInputs};
use crate::engine::spsa_check::ToolsEngineSpsaCheck;

#[test]
fn actual_queued_fen_preserves_source_backed_fullmove_discrepancy() -> Result<(), Box<dyn Error>> {
    let position = chess::engine::search::position::EngineSearchPosition::new(
        chess::board::position::BoardPosition::standard(),
        ["e2e4"],
    )?;
    let actual = "FEN: rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 2\n-16\n";
    let observation =
        crate::engine::pgo::ToolsEnginePgoQueuedPosition::from_output(actual, position.current())?;
    assert!(matches!(
        observation.fullmove(),
        crate::engine::pgo::ToolsEnginePgoEnumFullmove::UpstreamAfterWhite {
            expected: 1,
            observed: 2,
            ..
        }
    ));
    assert!(
        crate::engine::pgo::ToolsEnginePgoQueuedPosition::from_output(
            actual,
            &chess::board::position::BoardPosition::standard()
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn encoded_profile_flags_keep_space_bearing_paths_in_one_argument() -> Result<(), Box<dyn Error>> {
    let profile = PathBuf::from("/owner chosen directory/merged.profdata");
    for target in [
        EnginePackageEnumTarget::MacosArm,
        EnginePackageEnumTarget::LinuxX64,
        EnginePackageEnumTarget::WindowsX64,
    ] {
        let flags = ToolsEnginePgoEnumBuild::Optimize.flags(
            target,
            &profile,
            ToolsEnginePgoEnumInstrumentation::Full,
        )?;
        let arguments: Vec<_> = flags.split('\u{1f}').collect();
        assert!(flags.starts_with(target.portable_flags()));
        assert!(arguments.contains(&"profile-use=/owner chosen directory/merged.profdata"));
        assert!(!flags.contains("target-cpu=native"));
    }
    Ok(())
}

#[test]
fn profile_flag_separator_is_rejected_at_the_path_boundary() {
    assert!(matches!(
        ToolsEnginePgoEnumBuild::Instrument.flags(
            EnginePackageEnumTarget::MacosArm,
            &PathBuf::from("profile\u{1f}injection"),
            ToolsEnginePgoEnumInstrumentation::Full
        ),
        Err(ToolsEnginePgoError::InvalidPath)
    ));
}

#[test]
fn counts_only_is_explicit_and_changes_only_profile_build_flags() -> Result<(), Box<dyn Error>> {
    let inputs = ToolsEnginePgoInputs::new(
        PathBuf::from("source"),
        PathBuf::from("model"),
        PathBuf::from("output"),
    );
    assert_eq!(
        inputs.instrumentation(),
        ToolsEnginePgoEnumInstrumentation::Full
    );
    let inputs = inputs.with_instrumentation(ToolsEnginePgoEnumInstrumentation::CountsOnly);
    assert_eq!(
        inputs.instrumentation(),
        ToolsEnginePgoEnumInstrumentation::CountsOnly
    );
    let profile = PathBuf::from("/owner chosen output/profile");
    let target = EnginePackageEnumTarget::MacosArm;
    for build in [
        ToolsEnginePgoEnumBuild::Baseline,
        ToolsEnginePgoEnumBuild::Spsa,
    ] {
        assert_eq!(
            build.flags(target, &profile, ToolsEnginePgoEnumInstrumentation::Full)?,
            build.flags(
                target,
                &profile,
                ToolsEnginePgoEnumInstrumentation::CountsOnly
            )?
        );
    }
    for build in [
        ToolsEnginePgoEnumBuild::Instrument,
        ToolsEnginePgoEnumBuild::Optimize,
    ] {
        let full = build.flags(target, &profile, ToolsEnginePgoEnumInstrumentation::Full)?;
        let counts = build.flags(
            target,
            &profile,
            ToolsEnginePgoEnumInstrumentation::CountsOnly,
        )?;
        assert!(!full.contains("--disable-vp"));
        assert_eq!(
            counts.strip_suffix("\u{1f}-C\u{1f}llvm-args=--disable-vp"),
            Some(full.as_str())
        );
    }
    Ok(())
}

#[test]
fn unrelated_compile_failure_does_not_become_upstream_spsa_unavailable() {
    assert!(!ToolsEngineSpsaCheck::is_upstream_unavailable(
        "error: failed to find libclang"
    ));
    assert!(!ToolsEngineSpsaCheck::is_upstream_unavailable(
        "error[E0425]: cannot find function `print_options` in module `crate::parameters`"
    ));
    assert!(ToolsEngineSpsaCheck::is_upstream_unavailable(
        "error[E0425]: cannot find function `print_options` in module `crate::parameters`\nerror[E0425]: cannot find function `set_parameter` in module `crate::parameters`"
    ));
    assert!(!ToolsEngineSpsaCheck::is_upstream_unavailable(
        "error[E0425]: cannot find function `print_options` in module `crate::parameters`\nerror[E0425]: cannot find function `set_parameter` in module `crate::parameters`\nerror[E0308]: mismatched types"
    ));
}

#[tokio::test]
async fn existing_output_is_refused_without_changing_owner_contents() -> Result<(), Box<dyn Error>>
{
    let directory = tempfile::tempdir()?;
    let output = directory.path().join("existing");
    tokio::fs::create_dir(&output).await?;
    tokio::fs::write(output.join("owner-data"), b"preserve").await?;
    let inputs = ToolsEnginePgoInputs::new(
        directory.path().to_path_buf(),
        directory.path().join("model"),
        output.clone(),
    );
    assert!(matches!(
        inputs.claim_output().await,
        Err(ToolsEnginePgoError::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists
    ));
    assert_eq!(
        tokio::fs::read(output.join("owner-data")).await?,
        b"preserve"
    );
    directory.close()?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn bounded_process_reaps_output_flood_and_releases_capacity() -> Result<(), Box<dyn Error>> {
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("Invalid fixture budget")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let cancellation = ToolsEnginePgoCancellation::new();
    let resources = ToolsEnginePgoResources::new(&budget, &executor, &cancellation);
    let mut process = ToolsEnginePgoProcess::new(resources);
    let receipt = process
        .execute(
            Command::new("/usr/bin/yes").arg("bounded diagnostic fixture"),
            ToolsEnginePgoProcessRequest::new(
                ToolsEnginePgoEnumStage::QueuedAutoExit,
                1,
                ToolsEnginePgoEnumInput::HoldOpen,
                Duration::from_secs(10),
            ),
        )
        .await?;
    assert!(matches!(
        receipt.require_success(),
        Err(ToolsEnginePgoError::OutputBound)
    ));
    assert_eq!(budget.available(), budget.workers());
    drop(process);
    executor.shutdown().await?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_reaps_waiting_child_and_releases_capacity() -> Result<(), Box<dyn Error>> {
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("Invalid fixture budget")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let cancellation = ToolsEnginePgoCancellation::new();
    let resources = ToolsEnginePgoResources::new(&budget, &executor, &cancellation);
    let mut process = ToolsEnginePgoProcess::new(resources);
    let mut command = Command::new("/bin/cat");
    let operation = process.execute(
        &mut command,
        ToolsEnginePgoProcessRequest::new(
            ToolsEnginePgoEnumStage::QueuedAutoExit,
            1,
            ToolsEnginePgoEnumInput::HoldOpen,
            Duration::from_secs(10),
        ),
    );
    let cancel = async {
        tokio::task::yield_now().await;
        assert_eq!(budget.available(), 0);
        cancellation.cancel();
    };
    let (receipt, ()) = tokio::join!(operation, cancel);
    assert!(matches!(
        receipt?.require_success(),
        Err(ToolsEnginePgoError::Cancelled)
    ));
    assert_eq!(budget.available(), budget.workers());
    drop(process);
    executor.shutdown().await?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn deadline_reaps_waiting_child_and_releases_capacity() -> Result<(), Box<dyn Error>> {
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("Invalid fixture budget")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let cancellation = ToolsEnginePgoCancellation::new();
    let mut process = ToolsEnginePgoProcess::new(ToolsEnginePgoResources::new(
        &budget,
        &executor,
        &cancellation,
    ));
    let mut command = Command::new("/bin/cat");
    tokio::time::pause();
    let mut operation = Box::pin(process.execute(
        &mut command,
        ToolsEnginePgoProcessRequest::new(
            ToolsEnginePgoEnumStage::QueuedAutoExit,
            1,
            ToolsEnginePgoEnumInput::HoldOpen,
            Duration::from_secs(10),
        ),
    ));
    std::future::poll_fn(|context| {
        assert!(operation.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    tokio::time::advance(Duration::from_secs(10)).await;
    tokio::time::resume();
    let receipt = operation.await?;
    assert!(matches!(
        receipt.require_success(),
        Err(ToolsEnginePgoError::Deadline)
    ));
    assert_eq!(budget.available(), budget.workers());
    drop(process);
    executor.shutdown().await?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn dropping_wait_retains_child_capacity_until_observed_cleanup() -> Result<(), Box<dyn Error>>
{
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("Invalid fixture budget")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let cancellation = ToolsEnginePgoCancellation::new();
    let mut process = ToolsEnginePgoProcess::new(ToolsEnginePgoResources::new(
        &budget,
        &executor,
        &cancellation,
    ));
    let mut command = Command::new("/bin/cat");
    let mut waiting = Box::pin(process.execute(
        &mut command,
        ToolsEnginePgoProcessRequest::new(
            ToolsEnginePgoEnumStage::QueuedAutoExit,
            1,
            ToolsEnginePgoEnumInput::HoldOpen,
            Duration::from_secs(10),
        ),
    ));
    std::future::poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(budget.available(), 0);
    drop(waiting);
    assert_eq!(
        budget.available(),
        0,
        "A dropped wait released unreaped child capacity"
    );
    process
        .shutdown_retained(
            tokio::time::Instant::now()
                + chess::engine::process_provider::EngineProcessLimits::SHUTDOWN,
        )
        .await?;
    assert_eq!(budget.available(), budget.workers());
    drop(process);
    executor.shutdown().await?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn resumed_wait_keeps_its_original_absolute_deadline() -> Result<(), Box<dyn Error>> {
    let budget = CapacityBudget::new(NonZeroUsize::new(1).ok_or("Invalid fixture budget")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let cancellation = ToolsEnginePgoCancellation::new();
    let mut process = ToolsEnginePgoProcess::new(ToolsEnginePgoResources::new(
        &budget,
        &executor,
        &cancellation,
    ));
    let mut command = Command::new("/bin/cat");
    tokio::time::pause();
    let mut waiting = Box::pin(process.execute(
        &mut command,
        ToolsEnginePgoProcessRequest::new(
            ToolsEnginePgoEnumStage::QueuedAutoExit,
            1,
            ToolsEnginePgoEnumInput::HoldOpen,
            Duration::from_secs(10),
        ),
    ));
    std::future::poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    assert_eq!(budget.available(), 0);
    tokio::time::advance(Duration::from_secs(10)).await;
    tokio::time::resume();
    let receipt = process.observe().await?;
    assert!(matches!(
        receipt.require_success(),
        Err(ToolsEnginePgoError::Deadline)
    ));
    assert_eq!(budget.available(), budget.workers());
    drop(process);
    executor.shutdown().await?;
    Ok(())
}
