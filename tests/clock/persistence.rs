use std::num::NonZeroU32;

use chess::board::enums::BoardEnumColor;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::stage::{ClockStage, ClockStageEnumBonus, ClockStageEnumQuota};
use chess::clock::state::{ClockState, ClockStateEnumStatus, ClockStateSnapshot};
use chess::clock::time_control::ClockTimeControl;

#[test]
fn a_running_snapshot_recovers_paused_without_charging_downtime_or_resetting_delay()
-> Result<(), Box<dyn std::error::Error>> {
    let control = ClockTimeControl::simple_delay(
        ClockDuration::from_nanos(20_000),
        ClockDuration::from_nanos(5_000),
    )?;
    let mut clock = ClockState::new(control, BoardEnumColor::White);
    clock.resume(ClockInstant::from_nanos(1_000))?;
    let snapshot = clock.snapshot(ClockInstant::from_nanos(4_000))?;
    let decoded = ClockStateSnapshot::from_persisted(snapshot.persisted())?;
    assert_eq!(snapshot, decoded);
    let mut recovered = ClockState::recover(&decoded, [true, false])?;
    assert_eq!(recovered.status(), ClockStateEnumStatus::Paused);
    assert!(recovered.warning_used(BoardEnumColor::White));
    assert!(!recovered.warning_used(BoardEnumColor::Black));
    let paused = recovered.view(ClockInstant::from_nanos(1_000_000))?;
    assert_eq!(
        paused.side(BoardEnumColor::White).delay_remaining(),
        ClockDuration::from_nanos(2_000)
    );
    recovered.resume(ClockInstant::from_nanos(1_000_000))?;
    assert_eq!(
        recovered
            .view(ClockInstant::from_nanos(1_003_000))?
            .side(BoardEnumColor::White)
            .remaining(),
        Some(ClockDuration::from_nanos(19_000))
    );
    Ok(())
}

#[test]
fn snapshot_decode_rejects_wrong_stage_counters_and_unearned_time()
-> Result<(), Box<dyn std::error::Error>> {
    let quota = NonZeroU32::new(2).ok_or("nonzero stage quota")?;
    let control = ClockTimeControl::staged(vec![
        ClockStage::new(
            ClockStageEnumQuota::Moves(quota),
            ClockDuration::from_nanos(100),
            ClockStageEnumBonus::Fischer(ClockDuration::from_nanos(5)),
        )?,
        ClockStage::new(
            ClockStageEnumQuota::Cyclic(quota),
            ClockDuration::from_nanos(50),
            ClockStageEnumBonus::None,
        )?,
    ])?;
    let clock = ClockState::new(control, BoardEnumColor::White);
    let snapshot = clock.snapshot(ClockInstant::from_nanos(0))?;
    let mut invalid = snapshot.persisted();
    invalid.sides[0].stage_index = 1;
    assert!(ClockStateSnapshot::from_persisted(invalid).is_err());
    let mut invalid = snapshot.persisted();
    invalid.sides[0].stage_moves = 2;
    assert!(ClockStateSnapshot::from_persisted(invalid).is_err());
    let mut invalid = snapshot.persisted();
    invalid.sides[0].remaining = ClockDuration::from_nanos(101);
    assert!(ClockStateSnapshot::from_persisted(invalid).is_err());
    let mut invalid = snapshot.persisted();
    invalid.sides[0].completed_moves = 1;
    assert!(ClockStateSnapshot::from_persisted(invalid).is_err());
    Ok(())
}

#[test]
fn snapshot_decode_validates_flagged_side_and_elapsed_allowance()
-> Result<(), Box<dyn std::error::Error>> {
    let control = ClockTimeControl::sudden_death(ClockDuration::from_nanos(10))?;
    let mut clock = ClockState::new(control, BoardEnumColor::White);
    clock.resume(ClockInstant::from_nanos(0))?;
    let snapshot = clock.snapshot(ClockInstant::from_nanos(10))?;
    let decoded = ClockStateSnapshot::from_persisted(snapshot.persisted())?;
    assert_eq!(
        ClockState::recover(&decoded, [false; 2])?.status(),
        ClockStateEnumStatus::Flagged(BoardEnumColor::White)
    );
    let mut invalid = snapshot.persisted();
    invalid.status = ClockStateEnumStatus::Flagged(BoardEnumColor::Black);
    assert!(ClockStateSnapshot::from_persisted(invalid).is_err());
    let mut invalid = snapshot.persisted();
    invalid.status = ClockStateEnumStatus::Running;
    assert!(ClockStateSnapshot::from_persisted(invalid).is_err());
    let mut invalid = snapshot.persisted();
    invalid.elapsed = ClockDuration::ZERO;
    assert!(ClockStateSnapshot::from_persisted(invalid).is_err());
    Ok(())
}

#[test]
fn every_saved_stage_boundary_round_trips_and_preserves_earned_cyclic_time()
-> Result<(), Box<dyn std::error::Error>> {
    let quota = NonZeroU32::new(2).ok_or("nonzero stage quota")?;
    let control = ClockTimeControl::staged(vec![
        ClockStage::new(
            ClockStageEnumQuota::Moves(quota),
            ClockDuration::from_nanos(100),
            ClockStageEnumBonus::Fischer(ClockDuration::from_nanos(5)),
        )?,
        ClockStage::new(
            ClockStageEnumQuota::Cyclic(quota),
            ClockDuration::from_nanos(50),
            ClockStageEnumBonus::Fischer(ClockDuration::from_nanos(3)),
        )?,
    ])?;
    let mut clock = ClockState::new(control, BoardEnumColor::Black);
    clock.resume(ClockInstant::from_nanos(0))?;
    for tick in 1..=16 {
        clock.complete_move(ClockInstant::from_nanos(tick))?;
        let snapshot = clock.snapshot(ClockInstant::from_nanos(tick))?;
        assert_eq!(
            ClockStateSnapshot::from_persisted(snapshot.persisted())?,
            snapshot
        );
    }
    Ok(())
}

#[test]
fn untimed_snapshots_allow_elapsed_play_but_never_flag_or_gain_timed_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let mut clock = ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::Black);
    clock.resume(ClockInstant::from_nanos(0))?;
    let snapshot = clock.snapshot(ClockInstant::from_nanos(50))?;
    assert_eq!(
        ClockStateSnapshot::from_persisted(snapshot.persisted())?,
        snapshot
    );
    let mut invalid = snapshot.persisted();
    invalid.sides[1].remaining = ClockDuration::from_nanos(1);
    assert!(ClockStateSnapshot::from_persisted(invalid).is_err());
    let mut invalid = snapshot.persisted();
    invalid.status = ClockStateEnumStatus::Flagged(BoardEnumColor::Black);
    assert!(ClockStateSnapshot::from_persisted(invalid).is_err());
    Ok(())
}
