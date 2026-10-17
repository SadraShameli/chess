#[path = "workbench/fixture.rs"]
mod fixture;

use std::num::NonZeroUsize;
use strum::IntoEnumIterator;

use egui_kittest::kittest::{NodeT, Queryable};

use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::desktop::identity::DesktopRequestId;
use chess::engine::enums::{EngineEnumOption, EngineOptionEnumDescriptor};
use chess::engine::options::EngineOptionsEnumValue;
use chess::presentation::workbench::configuration::PresentationWorkbenchConfiguration;
use chess::presentation::workbench::controller::{
    PresentationWorkbenchControllerContext, PresentationWorkbenchControllerEnumAction,
    PresentationWorkbenchControllerEnumAdmission, PresentationWorkbenchControllerEnumBusy,
};
use chess::presentation::workbench::snapshot::PresentationWorkbenchJobSnapshot;
use chess::workbench::enums::{
    WorkbenchEnumCompletion, WorkbenchEnumJobState, WorkbenchEnumOperation,
    WorkbenchEnumParameters, WorkbenchEnumResult,
};
use chess::workbench::job::WorkbenchJobDeadline;
use chess::workbench::parameters::{WorkbenchBenchParameters, WorkbenchPerftDepth};
use chess::workbench::result::WorkbenchBenchResult;

use fixture::{
    PresentationWorkbenchFixture, PresentationWorkbenchMounted, PresentationWorkbenchTestResult,
};

#[tokio::test]
async fn mounted_registry_covers_every_option_with_dynamic_descriptors()
-> PresentationWorkbenchTestResult {
    let fixture = PresentationWorkbenchFixture::new().await?;
    let controller = fixture.controller()?;
    let snapshot = controller.engine().ok_or("missing engine snapshot")?;
    assert_eq!(snapshot.options().count(), EngineEnumOption::iter().len());
    for option in EngineEnumOption::iter() {
        let descriptor = snapshot
            .capabilities()
            .descriptor(option)
            .ok_or("missing descriptor")?;
        if let EngineOptionEnumDescriptor::Spin(range) = descriptor {
            let setting = PresentationWorkbenchConfiguration::number(
                option,
                &range.maximum().to_string(),
                snapshot.capabilities(),
            )?;
            assert_eq!(setting.option(), option);
            assert!(
                PresentationWorkbenchConfiguration::number(
                    option,
                    &(u64::from(range.maximum()) + 1).to_string(),
                    snapshot.capabilities()
                )
                .is_err()
            );
        }
    }
    let mut harness = PresentationWorkbenchMounted::harness(controller)?;
    for option in EngineEnumOption::iter() {
        match option {
            EngineEnumOption::Hash
            | EngineEnumOption::Threads
            | EngineEnumOption::MoveOverhead
            | EngineEnumOption::MultiPv => {
                harness.get_by_label(format!("{} value", option.name()).as_str());
                harness.get_by_label(format!("Save {}", option.name()).as_str());
            }
            EngineEnumOption::Minimal => {
                harness.get_by_label("Minimal final-report output");
            }
            EngineEnumOption::ClearHash => {
                harness.get_by_label(option.name());
            }
            EngineEnumOption::Chess960 => {
                harness.get_by_label("Current game variant: Standard");
            }
            EngineEnumOption::SyzygyPath => {
                harness.get_by_label("Select tablebase directories");
            }
        }
    }
    harness.run_steps(6);
    assert!(harness.state().intents.is_empty());
    Ok(())
}

#[tokio::test]
async fn mounted_invalid_input_never_emits_and_clear_hash_is_one_shot()
-> PresentationWorkbenchTestResult {
    let fixture = PresentationWorkbenchFixture::new().await?;
    let mut harness = PresentationWorkbenchMounted::harness(fixture.controller()?)?;
    harness
        .state_mut()
        .view
        .set_number(EngineEnumOption::Threads, "2049".to_owned())?;
    harness.run_ok().ok_or("invalid threads did not settle")?;
    assert!(
        harness
            .get_by_label("Save Threads")
            .accesskit_node()
            .is_disabled()
    );
    for invalid in ["-1", "0", "1.5", "garbage", "18446744073709551616"] {
        harness
            .state_mut()
            .view
            .set_number(EngineEnumOption::Hash, invalid.to_owned())?;
        harness.run_ok().ok_or("invalid hash did not settle")?;
        assert!(
            harness
                .get_by_label("Save Hash")
                .accesskit_node()
                .is_disabled()
        );
    }
    harness.get_by_label("Clear Hash").click_accesskit();
    harness.run_ok().ok_or("clear hash did not settle")?;
    harness.run_steps(8);
    assert_eq!(harness.state().intents.len(), 1);
    let action = harness
        .state()
        .intents
        .first()
        .ok_or("missing clear hash")?
        .action();
    assert!(
        matches!(action, PresentationWorkbenchControllerEnumAction::Configure(setting) if setting.value() == &EngineOptionsEnumValue::ClearHash)
    );
    Ok(())
}

#[tokio::test]
async fn busy_and_capacity_refusals_preserve_no_pending_intent() -> PresentationWorkbenchTestResult
{
    let fixture = PresentationWorkbenchFixture::new().await?;
    let mut controller = fixture.controller()?;
    let context = controller.context();
    controller.set_admission(
        context,
        PresentationWorkbenchControllerEnumAdmission::Busy(
            PresentationWorkbenchControllerEnumBusy::LiveGame,
        ),
    )?;
    assert!(
        controller
            .configure(EngineOptionsEnumValue::ClearHash, DesktopRequestId::new())
            .is_err()
    );
    assert!(
        controller
            .select_tablebases(DesktopRequestId::new())
            .is_err()
    );
    assert!(
        controller
            .start(
                WorkbenchEnumParameters::Compiler,
                WorkbenchJobDeadline::default(),
                DesktopRequestId::new()
            )
            .is_err()
    );
    controller.set_admission(
        context,
        PresentationWorkbenchControllerEnumAdmission::Idle {
            available_workers: 0,
        },
    )?;
    assert!(
        controller
            .start(
                WorkbenchEnumParameters::Compiler,
                WorkbenchJobDeadline::default(),
                DesktopRequestId::new()
            )
            .is_err()
    );
    assert!(!controller.is_pending());
    Ok(())
}

#[tokio::test]
async fn job_defaults_progress_results_export_and_wrong_identity_are_typed()
-> PresentationWorkbenchTestResult {
    let fixture = PresentationWorkbenchFixture::new().await?;
    let mut controller = fixture.controller()?;
    let parameters = PresentationWorkbenchConfiguration::default().parameters(
        WorkbenchEnumOperation::Bench,
        controller.engine().ok_or("missing engine")?.capabilities(),
        controller.position(),
    )?;
    let WorkbenchEnumParameters::Bench(bench) = &parameters else {
        return Err("expected bench".into());
    };
    assert_eq!(
        (bench.hash_mib(), bench.threads(), bench.depth()),
        (16, 1, WorkbenchBenchParameters::DEFAULT_DEPTH)
    );
    let bench = *bench;
    let request = DesktopRequestId::new();
    assert!(
        controller
            .start(parameters, WorkbenchJobDeadline::default(), request)?
            .is_some()
    );
    let mut job = fixture.job(WorkbenchEnumParameters::Bench(bench), 7)?;
    let context = controller.context();
    controller.accept_job(
        context,
        request,
        PresentationWorkbenchJobSnapshot::capture(&job)?,
    )?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    job.admit(job.id(), budget.try_lease(1)?, ClockInstant::from_nanos(1))?;
    job.start(job.id(), ClockInstant::from_nanos(2))?;
    job.advance(job.id(), 17, ClockInstant::from_nanos(3))?;
    let old_snapshot = PresentationWorkbenchJobSnapshot::capture(&job)?;
    controller.receive_job(context, PresentationWorkbenchJobSnapshot::capture(&job)?)?;
    assert_eq!(
        controller
            .job()
            .ok_or("missing job")?
            .progress()
            .completed(),
        17
    );
    let other = fixture.job(WorkbenchEnumParameters::Compiler, 8)?;
    assert!(
        controller
            .cancel(other.id(), DesktopRequestId::new())
            .is_err()
    );
    assert!(
        controller
            .receive_job(context, PresentationWorkbenchJobSnapshot::capture(&other)?)
            .is_err()
    );
    assert!(
        controller
            .export(job.id(), DesktopRequestId::new())
            .is_err()
    );
    job.finish_reaped(
        job.id(),
        WorkbenchEnumCompletion::Result(WorkbenchEnumResult::Bench(WorkbenchBenchResult::new(
            45,
            u64::MAX,
            ClockDuration::from_millis(1)?,
            u64::MAX,
        )?)),
        ClockInstant::from_nanos(4),
    )?;
    controller.receive_job(context, PresentationWorkbenchJobSnapshot::capture(&job)?)?;
    let mut harness = PresentationWorkbenchMounted::harness(controller)?;
    assert!(
        harness
            .state_mut()
            .controller
            .receive_job(context, old_snapshot)
            .is_err()
    );
    assert_eq!(
        harness
            .state()
            .controller
            .job()
            .ok_or("missing job")?
            .rows()
            .iter()
            .filter(|row| row.value() == u64::MAX.to_string())
            .count(),
        2
    );
    harness
        .get_by_label("Export diagnostic result")
        .click_accesskit();
    harness.run_ok().ok_or("export did not settle")?;
    assert_eq!(harness.state().intents.len(), 1);
    assert!(
        matches!(harness.state().intents[0].action(), PresentationWorkbenchControllerEnumAction::Export { job: id } if *id == job.id())
    );
    let request = harness.state().intents[0].request();
    assert!(
        harness
            .state_mut()
            .controller
            .exported(context, DesktopRequestId::new(), job.id())
            .is_err()
    );
    harness
        .state_mut()
        .controller
        .exported(context, request, job.id())?;
    harness
        .run_ok()
        .ok_or("export acknowledgment did not settle")?;
    harness.get_by_label("Diagnostic export completed by the backend.");
    Ok(())
}

#[tokio::test]
async fn mounted_minimal_save_retains_draft_after_typed_backend_refusal()
-> PresentationWorkbenchTestResult {
    use chess::desktop::error::{DesktopEnumFailureCode, DesktopFailure};

    let fixture = PresentationWorkbenchFixture::new().await?;
    let mut harness = PresentationWorkbenchMounted::harness(fixture.controller()?)?;
    harness
        .get_by_label("Minimal final-report output")
        .click_accesskit();
    harness.run_ok().ok_or("minimal input did not settle")?;
    assert!(harness.state().intents.is_empty());
    harness.get_by_label("Save Minimal").click_accesskit();
    harness.run_ok().ok_or("minimal save did not settle")?;
    harness.run_steps(4);
    assert_eq!(harness.state().intents.len(), 1);
    let intent = &harness.state().intents[0];
    assert!(
        matches!(intent.action(), PresentationWorkbenchControllerEnumAction::Configure(setting) if setting.value() == &EngineOptionsEnumValue::Minimal(true))
    );
    let context = intent.context();
    let request = intent.request();
    harness.state_mut().controller.reject(
        context,
        request,
        DesktopFailure::new(
            DesktopEnumFailureCode::ResourceRefusal,
            "Configuration barrier is busy".to_owned(),
            true,
        ),
    )?;
    harness
        .run_ok()
        .ok_or("configuration refusal did not settle")?;
    harness.get_by_label("Configuration barrier is busy");
    harness.get_by_label("Save Minimal").click_accesskit();
    harness.run_ok().ok_or("minimal retry did not settle")?;
    assert_eq!(harness.state().intents.len(), 2);
    assert!(
        matches!(harness.state().intents[1].action(), PresentationWorkbenchControllerEnumAction::Configure(setting) if setting.value() == &EngineOptionsEnumValue::Minimal(true))
    );
    Ok(())
}

#[tokio::test]
async fn cancellation_and_stale_contexts_cannot_replace_current_state()
-> PresentationWorkbenchTestResult {
    let fixture = PresentationWorkbenchFixture::new().await?;
    let mut controller = fixture.controller()?;
    let context = controller.context();
    let request = DesktopRequestId::new();
    assert!(
        controller
            .start(
                WorkbenchEnumParameters::Compiler,
                WorkbenchJobDeadline::default(),
                request,
            )?
            .is_some()
    );
    let mut job = fixture.job(WorkbenchEnumParameters::Compiler, 1)?;
    controller.accept_job(
        context,
        request,
        PresentationWorkbenchJobSnapshot::capture(&job)?,
    )?;
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    job.admit(job.id(), budget.try_lease(1)?, ClockInstant::from_nanos(1))?;
    job.start(job.id(), ClockInstant::from_nanos(2))?;
    controller.receive_job(context, PresentationWorkbenchJobSnapshot::capture(&job)?)?;
    let mut harness = PresentationWorkbenchMounted::harness(controller)?;
    harness
        .get_by_label("Cancel diagnostic job")
        .click_accesskit();
    harness.run_ok().ok_or("cancel did not settle")?;
    harness.run_steps(6);
    assert_eq!(harness.state().intents.len(), 1);
    job.cancel(job.id(), ClockInstant::from_nanos(3))?;
    harness
        .state_mut()
        .controller
        .receive_job(context, PresentationWorkbenchJobSnapshot::capture(&job)?)?;
    assert_eq!(
        harness
            .state()
            .controller
            .job()
            .ok_or("missing cancelling job")?
            .state(),
        WorkbenchEnumJobState::Cancelling
    );
    job.finish_reaped(
        job.id(),
        WorkbenchEnumCompletion::Terminated,
        ClockInstant::from_nanos(4),
    )?;
    harness
        .state_mut()
        .controller
        .receive_job(context, PresentationWorkbenchJobSnapshot::capture(&job)?)?;
    assert_eq!(
        harness
            .state()
            .controller
            .job()
            .ok_or("missing job")?
            .state(),
        WorkbenchEnumJobState::Cancelled
    );
    let other = PresentationWorkbenchFixture::new().await?;
    let stale = PresentationWorkbenchControllerContext::new(
        other.engine.generation(),
        context.configuration(),
        context.selection(),
    );
    assert!(
        harness
            .state_mut()
            .controller
            .receive_engine(stale, other.snapshot()?)
            .is_err()
    );
    let stale = PresentationWorkbenchControllerContext::new(
        context.generation(),
        context.configuration().next()?,
        context.selection(),
    );
    assert!(
        harness
            .state_mut()
            .controller
            .receive_job(stale, PresentationWorkbenchJobSnapshot::capture(&job)?)
            .is_err()
    );
    assert_eq!(WorkbenchPerftDepth::default().value(), 4);
    Ok(())
}

#[tokio::test]
async fn option_acknowledgments_reject_old_revision_request_generation_and_variant_edits()
-> PresentationWorkbenchTestResult {
    let fixture = PresentationWorkbenchFixture::new().await?;
    let mut controller = fixture.controller()?;
    let context = controller.context();
    let request = DesktopRequestId::new();
    assert!(
        controller
            .configure(EngineOptionsEnumValue::ClearHash, request)?
            .is_some()
    );
    assert!(
        controller
            .configure(EngineOptionsEnumValue::ClearHash, request)?
            .is_none()
    );
    assert!(
        controller
            .configuration_applied(
                context,
                DesktopRequestId::new(),
                context.configuration().next()?,
                fixture.snapshot()?
            )
            .is_err()
    );
    assert!(
        controller
            .configuration_applied(
                context,
                request,
                context.configuration(),
                fixture.snapshot()?
            )
            .is_err()
    );
    assert!(controller.is_pending());
    controller.configuration_applied(
        context,
        request,
        context.configuration().next()?,
        fixture.snapshot()?,
    )?;
    assert!(
        controller
            .receive_engine(context, fixture.snapshot()?)
            .is_err()
    );
    assert!(
        controller
            .configure(EngineOptionsEnumValue::ClearHash, request)?
            .is_none()
    );
    assert!(
        controller
            .configure(
                EngineOptionsEnumValue::Chess960(chess::board::enums::BoardEnumVariant::Chess960),
                DesktopRequestId::new()
            )
            .is_err()
    );
    assert!(
        controller
            .configure(
                EngineOptionsEnumValue::SyzygyPath(
                    chess::engine::options::EngineOptionsSyzygyPath::new("/tmp/tables".to_owned())?
                ),
                DesktopRequestId::new()
            )
            .is_err()
    );
    assert!(!controller.is_pending());
    Ok(())
}

#[tokio::test]
async fn mounted_busy_configuration_and_invalid_diagnostic_parameters_emit_nothing()
-> PresentationWorkbenchTestResult {
    let fixture = PresentationWorkbenchFixture::new().await?;
    let mut controller = fixture.controller()?;
    controller.set_admission(
        controller.context(),
        PresentationWorkbenchControllerEnumAdmission::Busy(
            PresentationWorkbenchControllerEnumBusy::Search,
        ),
    )?;
    let mut harness = PresentationWorkbenchMounted::harness(controller)?;
    assert!(
        harness
            .get_by_label("Clear Hash")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        harness
            .get_by_label("Select tablebase directories")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        harness
            .get_by_label("Run diagnostic")
            .accesskit_node()
            .is_disabled()
    );
    let context = harness.state().controller.context();
    harness.state_mut().controller.set_admission(
        context,
        PresentationWorkbenchControllerEnumAdmission::Idle {
            available_workers: 4,
        },
    )?;
    harness
        .state_mut()
        .view
        .set_operation(WorkbenchEnumOperation::Perft);
    for depth in ["0", "13", "4.5", "-1", ""] {
        harness
            .state_mut()
            .view
            .configuration_mut()
            .set_perft_depth(depth.to_owned());
        harness.run_ok().ok_or("perft validation did not settle")?;
        assert!(
            harness
                .get_by_label("Run diagnostic")
                .accesskit_node()
                .is_disabled()
        );
    }
    harness
        .state_mut()
        .view
        .set_operation(WorkbenchEnumOperation::Bench);
    harness.state_mut().view.configuration_mut().set_bench(
        "".to_owned(),
        "5".to_owned(),
        "".to_owned(),
    );
    harness.run_ok().ok_or("bench capacity did not settle")?;
    assert!(
        harness
            .get_by_label("Run diagnostic")
            .accesskit_node()
            .is_disabled()
    );
    harness.state_mut().view.configuration_mut().set_bench(
        "".to_owned(),
        "".to_owned(),
        "".to_owned(),
    );
    for deadline in ["0", "600001", "18446744073709551616", "1.5"] {
        harness
            .state_mut()
            .view
            .configuration_mut()
            .set_deadline_millis(deadline.to_owned());
        harness
            .run_ok()
            .ok_or("deadline validation did not settle")?;
        assert!(
            harness
                .get_by_label("Run diagnostic")
                .accesskit_node()
                .is_disabled()
        );
    }
    assert!(harness.state().intents.is_empty());
    Ok(())
}

#[tokio::test]
async fn all_operations_keep_real_parameters_and_results_with_bounded_diagnostics()
-> PresentationWorkbenchTestResult {
    use chess::engine::analysis::EngineAnalysisStaticEvaluation;
    use chess::engine::process_provider::{EngineProcessEnumPipe, EngineProcessLine};
    use chess::workbench::result::{
        WorkbenchCompilerResult, WorkbenchPerftResult, WorkbenchPerftRoot,
    };

    let fixture = PresentationWorkbenchFixture::new().await?;
    let controller = fixture.controller()?;
    let mut configuration = PresentationWorkbenchConfiguration::default();
    configuration.set_perft_depth(WorkbenchPerftDepth::MINIMUM.to_string());
    let budget = CapacityBudget::new(NonZeroUsize::MIN)?;
    assert!(!WorkbenchEnumOperation::ALL.is_empty());
    for operation in WorkbenchEnumOperation::ALL {
        let parameters = configuration.parameters(
            operation,
            controller.engine().ok_or("missing engine")?.capabilities(),
            controller.position(),
        )?;
        let result = match &parameters {
            WorkbenchEnumParameters::Inspector(position) => {
                WorkbenchEnumResult::Inspector(position.current().clone())
            }
            WorkbenchEnumParameters::RawEvaluation(_) => WorkbenchEnumResult::RawEvaluation(
                EngineAnalysisStaticEvaluation::parse("-2147483648")?,
            ),
            WorkbenchEnumParameters::Compiler => {
                WorkbenchEnumResult::Compiler(WorkbenchCompilerResult::new(
                    "rustc fixture".to_owned(),
                    fixture.engine.package().target(),
                    String::new(),
                )?)
            }
            WorkbenchEnumParameters::Perft(parameters) => {
                let roots: Vec<_> = parameters
                    .position()
                    .current()
                    .legal_moves()
                    .into_iter()
                    .map(|movement| WorkbenchPerftRoot::new(movement, 1))
                    .collect();
                WorkbenchEnumResult::Perft(WorkbenchPerftResult::new(
                    parameters,
                    roots.clone(),
                    u64::try_from(roots.len())?,
                    ClockDuration::ZERO,
                    0,
                )?)
            }
            WorkbenchEnumParameters::Bench(_) => {
                WorkbenchEnumResult::Bench(WorkbenchBenchResult::new(
                    WorkbenchBenchParameters::POSITIONS,
                    0,
                    ClockDuration::ZERO,
                    0,
                )?)
            }
        };
        let mut job = fixture.job(parameters, 42)?;
        job.admit(job.id(), budget.try_lease(1)?, ClockInstant::from_nanos(1))?;
        job.start(job.id(), ClockInstant::from_nanos(2))?;
        for index in 0..PresentationWorkbenchJobSnapshot::MAX_DIAGNOSTIC_LINES + 10 {
            job.record(
                job.id(),
                &EngineProcessLine::new(
                    EngineProcessEnumPipe::Stdout,
                    format!("bounded line {index}"),
                )?,
                ClockInstant::from_nanos(3),
            )?;
        }
        job.finish_reaped(
            job.id(),
            WorkbenchEnumCompletion::Result(result),
            ClockInstant::from_nanos(4),
        )?;
        let snapshot = PresentationWorkbenchJobSnapshot::capture(&job)?;
        assert_eq!(snapshot.state(), WorkbenchEnumJobState::Succeeded);
        assert!(!snapshot.rows().is_empty());
        assert!(snapshot.rows().len() <= PresentationWorkbenchJobSnapshot::MAX_RESULT_ROWS);
        assert_eq!(
            snapshot.diagnostics().len(),
            PresentationWorkbenchJobSnapshot::MAX_DIAGNOSTIC_LINES
        );
        assert_eq!(snapshot.omitted_lines(), 10);
        if operation == WorkbenchEnumOperation::RawEvaluation {
            assert!(
                snapshot
                    .rows()
                    .iter()
                    .any(|row| row.value() == i32::MIN.to_string())
            );
        }
        if operation == WorkbenchEnumOperation::Compiler {
            assert!(
                snapshot
                    .rows()
                    .iter()
                    .any(|row| row.value() == "No target features reported")
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn failed_timed_out_loading_and_missing_position_states_are_honest()
-> PresentationWorkbenchTestResult {
    use chess::desktop::error::{DesktopEnumFailureCode, DesktopFailure};
    use chess::presentation::workbench::controller::PresentationWorkbenchController;
    use chess::workbench::job::WorkbenchJobFailure;

    let fixture = PresentationWorkbenchFixture::new().await?;
    let mut controller = fixture.controller()?;
    let context = controller.context();
    let mut loading =
        PresentationWorkbenchMounted::harness(PresentationWorkbenchController::new(context, None))?;
    loading.get_by_label("Loading engine identity and capabilities...");
    loading.state_mut().controller.engine_unavailable(
        context,
        DesktopFailure::new(
            DesktopEnumFailureCode::FileUnavailable,
            "Restore the selected engine package".to_owned(),
            true,
        ),
    )?;
    loading.run_ok().ok_or("unavailable state did not settle")?;
    loading.get_by_label("Restore the selected engine package");
    let configuration = PresentationWorkbenchConfiguration::default();
    for operation in WorkbenchEnumOperation::ALL {
        let parameters =
            configuration.parameters(operation, fixture.engine.identity().capabilities(), None);
        assert_eq!(
            parameters.is_ok(),
            matches!(
                operation,
                WorkbenchEnumOperation::Compiler | WorkbenchEnumOperation::Bench
            )
        );
    }
    let request = DesktopRequestId::new();
    assert!(
        controller
            .start(
                WorkbenchEnumParameters::Compiler,
                WorkbenchJobDeadline::default(),
                request
            )?
            .is_some()
    );
    let mut job = fixture.job(WorkbenchEnumParameters::Compiler, 1)?;
    controller.accept_job(
        context,
        request,
        PresentationWorkbenchJobSnapshot::capture(&job)?,
    )?;
    job.fail_before_start(
        job.id(),
        WorkbenchJobFailure::UnexpectedTermination,
        ClockInstant::from_nanos(1),
    )?;
    controller.receive_job(context, PresentationWorkbenchJobSnapshot::capture(&job)?)?;
    assert!(controller.can_export());
    assert!(!controller.can_cancel());
    let request = DesktopRequestId::new();
    assert!(
        controller
            .start(
                WorkbenchEnumParameters::Compiler,
                WorkbenchJobDeadline::default(),
                request
            )?
            .is_some()
    );
    let mut job = fixture.job(WorkbenchEnumParameters::Compiler, 2)?;
    controller.accept_job(
        context,
        request,
        PresentationWorkbenchJobSnapshot::capture(&job)?,
    )?;
    job.expire(job.id(), job.deadline())?;
    controller.receive_job(context, PresentationWorkbenchJobSnapshot::capture(&job)?)?;
    let snapshot = controller.job().ok_or("missing timed out job")?;
    assert_eq!(snapshot.state(), WorkbenchEnumJobState::TimedOut);
    assert!(snapshot.remaining().is_none());
    assert!(snapshot.rows().is_empty());
    assert!(controller.can_export());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn tablebase_selection_configuration_loaded_cardinality_and_failure_remain_distinct()
-> PresentationWorkbenchTestResult {
    use chess::engine::options::{EngineOptionsEnumOutcome, EngineOptionsEnumTablebases};
    use chess::engine::tablebase::location::{
        EngineTablebaseLocation, EngineTablebaseLocationPolicy, EngineTablebaseLocationSelection,
    };
    use chess::engine::tablebase::status::EngineTablebaseStatus;
    use chess::presentation::workbench::snapshot::{
        PresentationWorkbenchSnapshot, PresentationWorkbenchTablebaseEnumState,
    };
    use std::num::NonZeroU32;

    let fixture = PresentationWorkbenchFixture::new().await?;
    let directory = tempfile::Builder::new()
        .prefix("chess-ui-tb-")
        .tempdir_in("/tmp")?;
    let source = directory.path().join("source with spaces");
    std::fs::create_dir(&source)?;
    std::fs::write(source.join("KQvK.rtbw"), b"typed status fixture")?;
    let mut location = EngineTablebaseLocation::prepare_blocking(
        EngineTablebaseLocationSelection::new(vec![source])?,
        EngineTablebaseLocationPolicy::unix_temporary(directory.path().to_path_buf()),
        || false,
    )?;
    let mut status = EngineTablebaseStatus::configured(&location);
    let snapshot = |status: &EngineTablebaseStatus| {
        PresentationWorkbenchSnapshot::capture(
            fixture.engine.package(),
            fixture.engine.identity().capabilities(),
            fixture.engine.applied_options(),
            Some(status),
        )
    };
    let mut controller = fixture.controller()?;
    let context = controller.context();
    let request = DesktopRequestId::new();
    let intent = controller
        .select_tablebases(request)?
        .ok_or("missing selection intent")?;
    assert!(matches!(
        intent.action(),
        PresentationWorkbenchControllerEnumAction::SelectTablebaseDirectories
    ));
    controller.tablebases_selected(context, request, snapshot(&status)?)?;
    assert!(matches!(
        controller
            .engine()
            .ok_or("missing engine")?
            .tablebases()
            .state(),
        PresentationWorkbenchTablebaseEnumState::Configured
    ));
    let mut harness = PresentationWorkbenchMounted::harness(controller)?;
    harness.get_by_label("Tablebases: directories prepared, engine loading not confirmed.");
    harness
        .get_by_label("Configure selected tablebases")
        .click_accesskit();
    harness
        .run_ok()
        .ok_or("tablebase configure did not settle")?;
    assert!(matches!(
        harness.state().intents[0].action(),
        PresentationWorkbenchControllerEnumAction::ConfigureTablebases
    ));
    let request = harness.state().intents[0].request();
    status.record_configuration(Ok(EngineOptionsEnumOutcome::Tablebases(
        EngineOptionsEnumTablebases::Loaded {
            path: location.engine_path().clone(),
            pieces: NonZeroU32::new(6).ok_or("invalid cardinality")?,
        },
    )));
    harness.state_mut().controller.configuration_applied(
        context,
        request,
        context.configuration().next()?,
        snapshot(&status)?,
    )?;
    harness
        .run_ok()
        .ok_or("loaded cardinality did not settle")?;
    harness.get_by_label("Tablebases: loaded, up to 6 pieces.");
    status.record_configuration(Ok(EngineOptionsEnumOutcome::Tablebases(
        EngineOptionsEnumTablebases::Unavailable {
            path: location.engine_path().clone(),
        },
    )));
    let context = harness.state().controller.context();
    harness
        .state_mut()
        .controller
        .receive_engine(context, snapshot(&status)?)?;
    assert!(matches!(
        harness
            .state()
            .controller
            .engine()
            .ok_or("missing engine")?
            .tablebases()
            .state(),
        PresentationWorkbenchTablebaseEnumState::Unavailable
    ));
    status.record_configuration(Ok(EngineOptionsEnumOutcome::Applied(
        EngineEnumOption::Minimal,
    )));
    harness
        .state_mut()
        .controller
        .receive_engine(context, snapshot(&status)?)?;
    assert!(matches!(
        harness
            .state()
            .controller
            .engine()
            .ok_or("missing engine")?
            .tablebases()
            .state(),
        PresentationWorkbenchTablebaseEnumState::Failed(_)
    ));
    location.cleanup_blocking()?;
    Ok(())
}
