use strum::IntoEnumIterator;
use std::path::Path;

use thiserror::Error;
use tokio::runtime::Builder;

use chess::board::enums::BoardEnumVariant;
use chess::capacity::budget::{CapacityBudget, CapacityBudgetError};
use chess::clock::duration::{ClockDuration, ClockDurationError};
use chess::engine::enums::EngineEnumOption;
use chess::engine::options::{
    EngineOptionsEnumOutcome, EngineOptionsEnumTablebases, EngineOptionsEnumValue,
    EngineOptionsError, EngineOptionsSetting, EngineOptionsSyzygyPath,
};
use chess::engine::package::host::{EnginePackageHost, EnginePackageHostError};
use chess::engine::package::selector::{EnginePackageSelector, EnginePackageSelectorError};
use chess::engine::process_provider::{EngineNativeProcessProvider, EngineProcessLimits};
use chess::engine::resource_config::{EngineResourceConfig, EngineResourceConfigError};
use chess::engine::session::{EngineSession, EngineSessionError};
use tokio::time::Instant;

#[derive(Debug, Error)]
pub(super) enum NativeEngineOptionsError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Capacity(#[from] CapacityBudgetError),
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
    #[error(transparent)]
    Host(#[from] EnginePackageHostError),
    #[error(transparent)]
    Package(#[from] EnginePackageSelectorError),
    #[error(transparent)]
    Resources(#[from] EngineResourceConfigError),
    #[error(transparent)]
    Options(#[from] EngineOptionsError),
    #[error(transparent)]
    Session(#[from] EngineSessionError),
    #[error("Native option acceptance violated: {0}")]
    Invariant(&'static str),
    #[error("Native option acceptance failed ({operation}); cleanup also failed ({cleanup})")]
    Cleanup {
        operation: Box<Self>,
        cleanup: Box<Self>,
    },
}

pub(super) struct NativeEngineOptionsHarness;

impl NativeEngineOptionsHarness {
    const THREADS: usize = 1;
    const HASH_MIB: u32 = 16;

    fn value(
        option: EngineEnumOption,
        path: &EngineOptionsSyzygyPath,
    ) -> Result<EngineOptionsEnumValue, NativeEngineOptionsError> {
        Ok(match option {
            EngineEnumOption::Hash => EngineOptionsEnumValue::Hash(Self::HASH_MIB),
            EngineEnumOption::Threads => EngineOptionsEnumValue::Threads(
                u32::try_from(Self::THREADS).map_err(|_conversion| {
                    NativeEngineOptionsError::Invariant("engine thread representation")
                })?,
            ),
            EngineEnumOption::MoveOverhead => {
                EngineOptionsEnumValue::MoveOverhead(ClockDuration::from_millis(25)?)
            }
            EngineEnumOption::Minimal => EngineOptionsEnumValue::Minimal(true),
            EngineEnumOption::ClearHash => EngineOptionsEnumValue::ClearHash,
            EngineEnumOption::Chess960 => {
                EngineOptionsEnumValue::Chess960(BoardEnumVariant::Chess960)
            }
            EngineEnumOption::MultiPv => EngineOptionsEnumValue::MultiPv(2),
            EngineEnumOption::SyzygyPath => EngineOptionsEnumValue::SyzygyPath(path.clone()),
        })
    }

    pub(super) fn run(directory: &Path) -> Result<(), NativeEngineOptionsError> {
        let package = EnginePackageSelector::load(directory, EnginePackageHost::detect()?)?;
        let budget = CapacityBudget::discover()?;
        let lease = budget.try_lease(Self::THREADS)?;
        let runtime = Builder::new_current_thread()
            .max_blocking_threads(budget.workers())
            .enable_all()
            .build()?;
        let empty_tables = tempfile::tempdir()?;
        let path = EngineOptionsSyzygyPath::new(
            empty_tables
                .path()
                .to_str()
                .ok_or(NativeEngineOptionsError::Invariant(
                    "tablebase fixture path must be representable",
                ))?
                .to_owned(),
        )?;
        runtime.block_on(async {
            let mut session = EngineSession::start(&EngineNativeProcessProvider, package).await?;
            let operation = async {
                session
                    .configure_resources(EngineResourceConfig::new(
                        lease.workers(),
                        Self::HASH_MIB,
                        ClockDuration::from_millis(100)?,
                    )?)
                    .await?;
                let generation = session.generation();
                let mut covered = Vec::new();
                for option in EngineEnumOption::iter() {
                    let setting = EngineOptionsSetting::new(
                        Self::value(option, &path)?,
                        session.identity().capabilities(),
                    )?;
                    let outcome = session.configure_option(setting).await?;
                    if option == EngineEnumOption::SyzygyPath {
                        if !matches!(
                            outcome,
                            EngineOptionsEnumOutcome::Tablebases(
                                EngineOptionsEnumTablebases::Unavailable { .. }
                            )
                        ) {
                            return Err(NativeEngineOptionsError::Invariant(
                                "an empty tablebase directory must be unavailable",
                            ));
                        }
                    } else if outcome != EngineOptionsEnumOutcome::Applied(option) {
                        return Err(NativeEngineOptionsError::Invariant(
                            "option acknowledgment identity",
                        ));
                    }
                    covered.push(option);
                }
                for value in [
                    EngineOptionsEnumValue::Minimal(false),
                    EngineOptionsEnumValue::Chess960(BoardEnumVariant::Standard),
                ] {
                    let setting =
                        EngineOptionsSetting::new(value, session.identity().capabilities())?;
                    session.configure_option(setting).await?;
                }
                if covered != EngineEnumOption::iter().collect::<Vec<_>>() || session.generation() != generation {
                    return Err(NativeEngineOptionsError::Invariant(
                        "registry coverage and stable process generation",
                    ));
                }
                let options = session
                    .applied_options()
                    .ok_or(NativeEngineOptionsError::Invariant("ready applied options"))?;
                if options.is_minimal()
                    || options.variant() != BoardEnumVariant::Standard
                    || options.multi_pv().get() != 2
                {
                    return Err(NativeEngineOptionsError::Invariant(
                        "applied non-resource settings",
                    ));
                }
                let resources = session
                    .resources()
                    .ok_or(NativeEngineOptionsError::Invariant(
                        "applied resource state",
                    ))?;
                if usize::try_from(resources.threads()).ok() != Some(lease.workers())
                    || resources.hash_mib() != Self::HASH_MIB
                    || resources.move_overhead() != ClockDuration::from_millis(25)?
                {
                    return Err(NativeEngineOptionsError::Invariant(
                        "resources match owned CPU/hash allocation",
                    ));
                }
                Ok::<(), NativeEngineOptionsError>(())
            }
            .await;
            let cleanup = session
                .shutdown_retained(Instant::now() + EngineProcessLimits::SHUTDOWN)
                .await
                .map_err(NativeEngineOptionsError::Session);
            if !session.is_shutdown_complete() {
                return Err(NativeEngineOptionsError::Invariant(
                    "observed engine and reader cleanup",
                ));
            }
            match (operation, cleanup) {
                (Ok(()), Ok(())) => Ok(()),
                (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
                (Err(operation), Err(cleanup)) => Err(NativeEngineOptionsError::Cleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
            }
        })?;
        println!(
            "Pinned Reckless option acceptance passed: 8/8 registry entries, exact acknowledgments, Minimal readiness, Clear Hash button, variant restoration, MultiPV, explicit empty-directory Syzygy failure, resource consistency and observed cleanup"
        );
        Ok(())
    }
}
