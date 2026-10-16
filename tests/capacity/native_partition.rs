use std::future::{Future, poll_fn};
use std::num::NonZeroUsize;
use std::task::Poll;

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::capacity::phase::{
    CapacityPhase, CapacityPhaseEnumInterruption, CapacityPhaseEnumState, CapacityPhaseError,
    CapacityPhaseGeneration,
};

struct CapacityNativePartitionFixture {
    budget: CapacityBudget,
    phase: CapacityPhase,
}

impl CapacityNativePartitionFixture {
    fn new(workers: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let budget = CapacityBudget::new(NonZeroUsize::new(workers).ok_or("zero test capacity")?)?;
        let phase = CapacityPhase::new(budget.clone())?;
        Ok(Self { budget, phase })
    }

    fn prepare(&mut self) -> Result<CapacityPhaseGeneration, CapacityPhaseError> {
        let generation = self.phase.request_preparation()?;
        self.phase.acknowledge_searches_stopped(generation)?;
        self.phase.try_enter_preparation(generation)?;
        Ok(generation)
    }

    fn enter_steady(&mut self) -> Result<(), CapacityPhaseError> {
        let generation = self.prepare()?;
        self.phase.complete_preparation(generation)
    }
}

#[test]
fn rejects_less_than_four_native_workers_without_consuming_capacity()
-> Result<(), Box<dyn std::error::Error>> {
    for workers in 1..CapacityPhase::MINIMUM_WORKERS {
        let budget = CapacityBudget::new(NonZeroUsize::new(workers).ok_or("zero test capacity")?)?;
        assert!(matches!(
            CapacityPhase::new(budget.clone()),
            Err(CapacityPhaseError::BelowNativeMinimum { available, required })
                if available == workers && required == CapacityPhase::MINIMUM_WORKERS
        ));
        assert_eq!(budget.available(), workers);
    }
    Ok(())
}

#[test]
fn refuses_initialization_without_its_host_and_compute_reservation()
-> Result<(), Box<dyn std::error::Error>> {
    let budget = CapacityBudget::new(NonZeroUsize::new(4).ok_or("zero test capacity")?)?;
    let external = budget.try_lease(3)?;
    assert!(matches!(
        CapacityPhase::new(budget.clone()),
        Err(CapacityPhaseError::Budget(_))
    ));
    assert_eq!(budget.available(), 1);
    drop(external);
    assert_eq!(budget.available(), 4);
    Ok(())
}

#[test]
fn initial_asset_preparation_freezes_workload_admission() -> Result<(), Box<dyn std::error::Error>>
{
    let mut fixture = CapacityNativePartitionFixture::new(4)?;
    assert_eq!(fixture.phase.state(), CapacityPhaseEnumState::Quiescing);
    assert_eq!(fixture.phase.total_workers(), 4);
    assert_eq!(fixture.phase.workload_workers(), 2);
    assert_eq!(fixture.phase.reserved_workers(), 2);
    assert_eq!(fixture.budget.available(), 2);
    assert!(matches!(
        fixture.phase.try_lease_workload(1),
        Err(CapacityPhaseError::InvalidPhase(
            CapacityPhaseEnumState::Quiescing
        ))
    ));
    let generation = fixture.phase.request_preparation()?;
    assert!(matches!(
        fixture.phase.try_enter_preparation(generation),
        Err(CapacityPhaseError::SearchStopPending)
    ));
    fixture.phase.acknowledge_searches_stopped(generation)?;
    fixture.phase.try_enter_preparation(generation)?;
    assert_eq!(fixture.phase.state(), CapacityPhaseEnumState::Preparing);
    assert_eq!(fixture.phase.withheld_workload_workers(), 2);
    assert_eq!(fixture.budget.available(), 0);
    fixture.phase.complete_preparation(generation)?;
    assert_eq!(fixture.phase.state(), CapacityPhaseEnumState::Steady);
    assert_eq!(fixture.phase.withheld_workload_workers(), 0);
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}

#[test]
fn steady_workloads_share_every_permitted_worker_including_one_worker_jobs()
-> Result<(), Box<dyn std::error::Error>> {
    for workers in [4, 8, 32] {
        let mut fixture = CapacityNativePartitionFixture::new(workers)?;
        fixture.enter_steady()?;
        let mut leases = Vec::new();
        for _ in 0..fixture.phase.workload_workers() {
            leases.push(fixture.phase.try_lease_workload(1)?);
        }
        assert_eq!(fixture.budget.available(), 0);
        assert!(fixture.phase.try_lease_workload(1).is_err());
        drop(leases);
        let full = fixture
            .phase
            .try_lease_workload(fixture.phase.workload_workers())?;
        assert_eq!(full.workers(), workers - fixture.phase.reserved_workers());
        assert_eq!(fixture.budget.available(), 0);
        drop(full);
        assert_eq!(fixture.budget.available(), fixture.phase.workload_workers());
        assert!(matches!(
            fixture.phase.try_lease_workload(0),
            Err(CapacityPhaseError::WorkloadRange { requested: 0, .. })
        ));
        assert!(matches!(
            fixture.phase.try_lease_workload(workers),
            Err(CapacityPhaseError::WorkloadRange { requested, maximum })
                if requested == workers && maximum == fixture.phase.workload_workers()
        ));
    }
    Ok(())
}

#[test]
fn shares_the_detected_capacity_without_an_independent_full_size_pool()
-> Result<(), Box<dyn std::error::Error>> {
    let budget = CapacityBudget::discover()?;
    let detected = std::thread::available_parallelism()?.get();
    if detected < CapacityPhase::MINIMUM_WORKERS {
        assert!(matches!(
            CapacityPhase::new(budget),
            Err(CapacityPhaseError::BelowNativeMinimum { available, .. }) if available == detected
        ));
    } else {
        let mut phase = CapacityPhase::new(budget.clone())?;
        let generation = phase.request_preparation()?;
        phase.acknowledge_searches_stopped(generation)?;
        phase.try_enter_preparation(generation)?;
        phase.complete_preparation(generation)?;
        let workload = phase.try_lease_workload(phase.workload_workers())?;
        assert_eq!(workload.workers() + phase.reserved_workers(), detected);
        assert_eq!(budget.available(), 0);
    }
    Ok(())
}

#[test]
fn preparation_waits_for_stopped_search_acknowledgment_and_every_workload_lease()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = CapacityNativePartitionFixture::new(8)?;
    fixture.enter_steady()?;
    let engine = fixture.phase.try_lease_workload(3)?;
    let rayon = fixture.budget.try_lease(1)?;
    let generation = fixture.phase.request_preparation()?;
    assert_eq!(fixture.phase.state(), CapacityPhaseEnumState::Quiescing);
    assert!(fixture.phase.try_lease_workload(1).is_err());
    assert_eq!(engine.workers(), 3);
    assert!(matches!(
        fixture.phase.try_enter_preparation(generation),
        Err(CapacityPhaseError::SearchStopPending)
    ));
    fixture.phase.acknowledge_searches_stopped(generation)?;
    assert!(matches!(
        fixture.phase.try_enter_preparation(generation),
        Err(CapacityPhaseError::QuiescencePending)
    ));
    drop(engine);
    assert!(matches!(
        fixture.phase.try_enter_preparation(generation),
        Err(CapacityPhaseError::QuiescencePending)
    ));
    drop(rayon);
    fixture.phase.try_enter_preparation(generation)?;
    assert_eq!(fixture.budget.available(), 0);
    assert!(fixture.budget.try_lease(1).is_err());
    fixture.phase.complete_preparation(generation)?;
    assert_eq!(fixture.budget.available(), fixture.phase.workload_workers());
    Ok(())
}

#[test]
fn repeated_preparation_requests_keep_the_original_generation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = CapacityNativePartitionFixture::new(4)?;
    let generation = fixture.phase.request_preparation()?;
    assert_eq!(fixture.phase.request_preparation()?, generation);
    fixture.phase.acknowledge_searches_stopped(generation)?;
    fixture.phase.try_enter_preparation(generation)?;
    assert_eq!(fixture.phase.request_preparation()?, generation);
    fixture.phase.complete_preparation(generation)?;
    let next = fixture.phase.request_preparation()?;
    assert_ne!(next, generation);
    assert!(matches!(
        fixture.phase.complete_preparation(generation),
        Err(CapacityPhaseError::StaleGeneration { expected: Some(current), actual })
            if current == next && actual == generation
    ));
    assert_eq!(fixture.phase.generation(), Some(next));
    assert_eq!(fixture.phase.state(), CapacityPhaseEnumState::Quiescing);
    Ok(())
}

#[test]
fn ready_requires_the_active_preparation_phase() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = CapacityNativePartitionFixture::new(4)?;
    let generation = fixture.phase.request_preparation()?;
    assert!(matches!(
        fixture.phase.complete_preparation(generation),
        Err(CapacityPhaseError::InvalidPhase(
            CapacityPhaseEnumState::Quiescing
        ))
    ));
    assert_eq!(fixture.phase.state(), CapacityPhaseEnumState::Quiescing);
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}

#[test]
fn cancellation_before_framework_work_preserves_existing_leases_and_freezes_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = CapacityNativePartitionFixture::new(4)?;
    fixture.enter_steady()?;
    let workload = fixture.phase.try_lease_workload(1)?;
    let generation = fixture.phase.request_preparation()?;
    fixture
        .phase
        .interrupt_preparation(generation, CapacityPhaseEnumInterruption::Cancelled)?;
    assert_eq!(fixture.phase.state(), CapacityPhaseEnumState::Interrupted);
    assert_eq!(
        fixture.phase.interruption(),
        Some(CapacityPhaseEnumInterruption::Cancelled)
    );
    assert_eq!(workload.workers(), 1);
    assert_eq!(fixture.budget.available(), 1);
    assert!(fixture.phase.try_lease_workload(1).is_err());
    assert!(matches!(
        fixture.phase.complete_preparation(generation),
        Err(CapacityPhaseError::StaleGeneration { expected: None, actual }) if actual == generation
    ));
    drop(workload);
    let next = fixture.prepare()?;
    assert_ne!(next, generation);
    fixture.phase.complete_preparation(next)?;
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}

#[test]
fn interruption_during_preparation_retains_all_reservations_until_framework_stop()
-> Result<(), Box<dyn std::error::Error>> {
    for reason in [
        CapacityPhaseEnumInterruption::Cancelled,
        CapacityPhaseEnumInterruption::Failed,
    ] {
        let mut fixture = CapacityNativePartitionFixture::new(8)?;
        let generation = fixture.prepare()?;
        fixture.phase.interrupt_preparation(generation, reason)?;
        assert_eq!(fixture.phase.state(), CapacityPhaseEnumState::Draining);
        assert_eq!(fixture.phase.interruption(), Some(reason));
        assert_eq!(
            fixture.phase.withheld_workload_workers(),
            fixture.phase.workload_workers()
        );
        assert_eq!(fixture.budget.available(), 0);
        assert!(fixture.budget.try_lease(1).is_err());
        assert!(fixture.phase.try_lease_workload(1).is_err());
        assert!(fixture.phase.request_preparation().is_err());
        assert!(fixture.phase.complete_preparation(generation).is_err());
        assert_eq!(fixture.budget.available(), 0);
        fixture.phase.acknowledge_preparation_stopped(generation)?;
        assert_eq!(fixture.phase.state(), CapacityPhaseEnumState::Interrupted);
        assert_eq!(fixture.phase.withheld_workload_workers(), 0);
        assert_eq!(fixture.budget.available(), fixture.phase.workload_workers());
        assert!(fixture.phase.try_lease_workload(1).is_err());
        let next = fixture.phase.request_preparation()?;
        assert_ne!(next, generation);
        assert!(
            fixture
                .phase
                .acknowledge_searches_stopped(generation)
                .is_err()
        );
        assert!(
            fixture
                .phase
                .acknowledge_preparation_stopped(generation)
                .is_err()
        );
        assert_eq!(fixture.phase.generation(), Some(next));
        fixture.phase.acknowledge_searches_stopped(next)?;
        fixture.phase.try_enter_preparation(next)?;
        fixture.phase.complete_preparation(next)?;
        assert_eq!(fixture.phase.state(), CapacityPhaseEnumState::Steady);
    }
    Ok(())
}

#[test]
fn unexpected_framework_stop_acknowledgment_does_not_release_preparation_reservations()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = CapacityNativePartitionFixture::new(4)?;
    let generation = fixture.prepare()?;
    assert!(matches!(
        fixture.phase.acknowledge_preparation_stopped(generation),
        Err(CapacityPhaseError::InvalidPhase(
            CapacityPhaseEnumState::Preparing
        ))
    ));
    assert_eq!(fixture.budget.available(), 0);
    assert_eq!(fixture.phase.state(), CapacityPhaseEnumState::Preparing);
    fixture.phase.complete_preparation(generation)?;
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}

#[test]
fn dropping_the_phase_releases_only_its_owned_reservations()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = CapacityNativePartitionFixture::new(8)?;
    fixture.enter_steady()?;
    let external = fixture.budget.try_lease(3)?;
    drop(fixture.phase);
    assert_eq!(fixture.budget.available(), 5);
    assert_eq!(external.workers(), 3);
    drop(external);
    assert_eq!(fixture.budget.available(), 8);
    let mut phase = CapacityPhase::new(fixture.budget.clone())?;
    let generation = phase.request_preparation()?;
    phase.acknowledge_searches_stopped(generation)?;
    phase.try_enter_preparation(generation)?;
    assert_eq!(fixture.budget.available(), 0);
    drop(phase);
    assert_eq!(fixture.budget.available(), 8);
    Ok(())
}

#[test]
fn raw_budget_admission_cannot_bypass_any_nonsteady_phase() -> Result<(), Box<dyn std::error::Error>>
{
    let mut fixture = CapacityNativePartitionFixture::new(8)?;
    assert!(fixture.budget.try_lease(1).is_err());
    let generation = fixture.prepare()?;
    assert!(fixture.budget.try_lease(1).is_err());
    fixture
        .phase
        .interrupt_preparation(generation, CapacityPhaseEnumInterruption::Cancelled)?;
    assert!(fixture.budget.try_lease(1).is_err());
    fixture.phase.acknowledge_preparation_stopped(generation)?;
    assert_eq!(fixture.budget.available(), fixture.phase.workload_workers());
    assert!(fixture.budget.try_lease(1).is_err());
    fixture.enter_steady()?;
    let admitted = fixture.budget.try_lease(1)?;
    fixture.phase.request_preparation()?;
    assert!(fixture.budget.try_lease(1).is_err());
    drop(admitted);
    Ok(())
}

#[tokio::test]
async fn async_raw_budget_admission_waits_for_the_shared_phase_gate()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = CapacityNativePartitionFixture::new(4)?;
    let budget = fixture.budget.clone();
    let mut waiting = std::pin::pin!(budget.lease(1));
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    let generation = fixture.prepare()?;
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    fixture.phase.complete_preparation(generation)?;
    let admitted = waiting.await?;
    assert_eq!(admitted.workers(), 1);
    assert_eq!(fixture.budget.available(), 1);
    drop(admitted);
    assert_eq!(fixture.budget.available(), 2);
    Ok(())
}

#[tokio::test]
async fn closing_admission_releases_existing_async_semaphore_waiters_before_preparation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = CapacityNativePartitionFixture::new(4)?;
    fixture.enter_steady()?;
    let held = fixture.budget.try_lease(fixture.phase.workload_workers())?;
    let budget = fixture.budget.clone();
    let mut waiting = std::pin::pin!(budget.lease(1));
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    let generation = fixture.phase.request_preparation()?;
    poll_fn(|context| {
        assert!(waiting.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    fixture.phase.acknowledge_searches_stopped(generation)?;
    drop(held);
    fixture.phase.try_enter_preparation(generation)?;
    assert_eq!(fixture.budget.available(), 0);
    fixture.phase.complete_preparation(generation)?;
    let admitted = waiting.await?;
    assert_eq!(admitted.workers(), 1);
    Ok(())
}

#[tokio::test]
async fn raw_executor_submission_obeys_the_same_phase_gate()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = CapacityNativePartitionFixture::new(4)?;
    let executor = CapacityExecutor::new(fixture.budget.clone())?;
    assert!(executor.submit(|_cancellation| 1).is_err());
    fixture.enter_steady()?;
    assert_eq!(executor.submit(|_cancellation| 7)?.join().await?, 7);
    let generation = fixture.phase.request_preparation()?;
    assert!(executor.submit(|_cancellation| 1).is_err());
    fixture
        .phase
        .interrupt_preparation(generation, CapacityPhaseEnumInterruption::Failed)?;
    assert!(executor.submit(|_cancellation| 1).is_err());
    fixture.enter_steady()?;
    assert_eq!(executor.submit(|_cancellation| 9)?.join().await?, 9);
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn admitted_executor_work_observes_phase_quiescence_at_its_cooperative_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = CapacityNativePartitionFixture::new(4)?;
    fixture.enter_steady()?;
    let executor = CapacityExecutor::new(fixture.budget.clone())?;
    let (entered_sender, entered_receiver) = std::sync::mpsc::sync_channel(1);
    let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
    let job = executor.submit(
        move |cancellation| -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
            entered_sender
                .send(())
                .map_err(|_value| "entry receiver closed")?;
            release_receiver.recv()?;
            Ok(cancellation.is_cancelled())
        },
    )?;
    loop {
        match entered_receiver.try_recv() {
            Ok(()) => break,
            Err(std::sync::mpsc::TryRecvError::Empty) => tokio::task::yield_now().await,
            Err(error @ std::sync::mpsc::TryRecvError::Disconnected) => return Err(error.into()),
        }
    }
    let generation = fixture.phase.request_preparation()?;
    fixture.phase.acknowledge_searches_stopped(generation)?;
    assert!(matches!(
        fixture.phase.try_enter_preparation(generation),
        Err(CapacityPhaseError::QuiescencePending)
    ));
    release_sender
        .send(())
        .map_err(|_value| "release receiver closed")?;
    assert!(job.join().await?.map_err(std::io::Error::other)?);
    fixture.phase.try_enter_preparation(generation)?;
    fixture.phase.complete_preparation(generation)?;
    executor.shutdown().await?;
    Ok(())
}

#[test]
fn one_shared_budget_cannot_have_competing_phase_controllers()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = CapacityNativePartitionFixture::new(8)?;
    fixture.enter_steady()?;
    assert!(CapacityPhase::new(fixture.budget.clone()).is_err());
    assert_eq!(fixture.budget.available(), fixture.phase.workload_workers());
    let lease = fixture.phase.try_lease_workload(1)?;
    drop(lease);
    drop(fixture.phase);
    let mut replacement = CapacityPhase::new(fixture.budget.clone())?;
    let generation = replacement.request_preparation()?;
    replacement.acknowledge_searches_stopped(generation)?;
    replacement.try_enter_preparation(generation)?;
    replacement.complete_preparation(generation)?;
    assert_eq!(fixture.budget.available(), replacement.workload_workers());
    Ok(())
}
