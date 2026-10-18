use std::num::NonZeroU32;

use chess::board::enums::BoardEnumColor;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::stage::{ClockStage, ClockStageEnumBonus, ClockStageEnumQuota};
use chess::clock::state::{ClockState, ClockStateEnumMoveOutcome, ClockStateEnumStatus};
use chess::clock::time_control::ClockTimeControl;

type ClockStateTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct ClockStateFixture;

impl ClockStateFixture {
    fn duration(millis: u64) -> ClockStateTestResult<ClockDuration> {
        Ok(ClockDuration::from_millis(millis)?)
    }

    fn now(millis: u64) -> ClockStateTestResult<ClockInstant> {
        Ok(ClockInstant::from_millis(millis)?)
    }

    fn quota(moves: u32) -> ClockStateTestResult<NonZeroU32> {
        NonZeroU32::new(moves).ok_or_else(|| "invalid fixture quota".into())
    }

    fn accepted(clock: &mut ClockState, millis: u64) -> ClockStateTestResult {
        match clock.complete_move(Self::now(millis)?)? {
            ClockStateEnumMoveOutcome::Accepted(_) => Ok(()),
            ClockStateEnumMoveOutcome::Expired(_) => Err("unexpected fixture flag fall".into()),
        }
    }

    fn remaining(
        clock: &ClockState,
        side: BoardEnumColor,
        millis: u64,
    ) -> ClockStateTestResult<u64> {
        Ok(clock
            .view(Self::now(millis)?)?
            .side(side)
            .remaining()
            .ok_or("missing timed fixture clock")?
            .as_millis())
    }
}

#[test]
fn initial_default_is_ten_minutes_plus_five_seconds_and_controls_are_immutable_validated()
-> ClockStateTestResult {
    let control = ClockTimeControl::initial_default();
    assert!(control.is_timed());
    assert_eq!(control.stages().len(), 1);
    let stage = control.stages().first().ok_or("missing initial stage")?;
    assert_eq!(stage.base_time().as_millis(), 600_000);
    assert_eq!(
        stage.bonus(),
        ClockStageEnumBonus::Fischer(ClockStateFixture::duration(5_000)?)
    );
    assert_eq!(stage.quota(), ClockStageEnumQuota::Unlimited);
    assert!(ClockTimeControl::staged(Vec::new()).is_err());
    let unlimited = ClockStage::new(
        ClockStageEnumQuota::Unlimited,
        ClockStateFixture::duration(1_000)?,
        ClockStageEnumBonus::None,
    )?;
    assert!(ClockTimeControl::staged(vec![unlimited, unlimited]).is_err());
    let moves = ClockStage::new(
        ClockStageEnumQuota::Moves(ClockStateFixture::quota(2)?),
        ClockStateFixture::duration(1_000)?,
        ClockStageEnumBonus::None,
    )?;
    assert!(ClockTimeControl::staged(vec![moves]).is_err());
    let cyclic = ClockStage::new(
        ClockStageEnumQuota::Cyclic(ClockStateFixture::quota(2)?),
        ClockStateFixture::duration(1_000)?,
        ClockStageEnumBonus::None,
    )?;
    assert!(ClockTimeControl::staged(vec![cyclic, unlimited]).is_err());
    Ok(())
}

#[test]
fn rejects_overflowing_stage_allowances_and_finite_control_totals() -> ClockStateTestResult {
    assert!(
        ClockStage::new(
            ClockStageEnumQuota::Unlimited,
            ClockDuration::from_nanos(u64::MAX),
            ClockStageEnumBonus::SimpleDelay(ClockDuration::from_nanos(1))
        )
        .is_err()
    );
    let first = ClockStage::new(
        ClockStageEnumQuota::Moves(ClockStateFixture::quota(2)?),
        ClockDuration::from_nanos(u64::MAX - 1),
        ClockStageEnumBonus::None,
    )?;
    let last = ClockStage::new(
        ClockStageEnumQuota::Unlimited,
        ClockDuration::from_nanos(2),
        ClockStageEnumBonus::None,
    )?;
    assert!(ClockTimeControl::staged(vec![first, last]).is_err());
    let increment = ClockStage::new(
        ClockStageEnumQuota::Moves(ClockStateFixture::quota(2)?),
        ClockDuration::from_nanos(1),
        ClockStageEnumBonus::Fischer(ClockDuration::from_nanos(u64::MAX / 2)),
    )?;
    assert!(ClockTimeControl::staged(vec![increment, last]).is_err());
    Ok(())
}

#[test]
fn untimed_games_still_track_turns_moves_and_pauses_without_a_deadline() -> ClockStateTestResult {
    let mut clock = ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White);
    assert_eq!(clock.status(), ClockStateEnumStatus::Paused);
    clock.resume(ClockStateFixture::now(0)?)?;
    assert!(clock.is_running());
    assert_eq!(clock.view(ClockStateFixture::now(1_000)?)?.deadline(), None);
    ClockStateFixture::accepted(&mut clock, 1_000)?;
    assert_eq!(clock.active(), BoardEnumColor::Black);
    let view = clock.view(ClockStateFixture::now(1_000)?)?;
    assert_eq!(view.side(BoardEnumColor::White).completed_moves(), 1);
    assert_eq!(view.side(BoardEnumColor::White).remaining(), None);
    clock.pause(ClockStateFixture::now(2_000)?)?;
    clock.resume(ClockStateFixture::now(100_000)?)?;
    assert_eq!(
        clock
            .view(ClockStateFixture::now(100_000)?)?
            .elapsed()
            .as_millis(),
        1_000
    );
    Ok(())
}

#[test]
fn a_move_exactly_at_the_nanosecond_deadline_is_rejected_before_any_bonus_or_turn_change()
-> ClockStateTestResult {
    let control =
        ClockTimeControl::fischer(ClockDuration::from_nanos(10), ClockDuration::from_nanos(3))?;
    let mut clock = ClockState::new(control.clone(), BoardEnumColor::White);
    clock.resume(ClockInstant::from_nanos(100))?;
    let view = clock.view(ClockInstant::from_nanos(109))?;
    assert_eq!(view.deadline(), Some(ClockInstant::from_nanos(110)));
    assert!(matches!(
        clock.complete_move(ClockInstant::from_nanos(110))?,
        ClockStateEnumMoveOutcome::Expired(_)
    ));
    assert_eq!(clock.active(), BoardEnumColor::White);
    assert_eq!(
        clock.status(),
        ClockStateEnumStatus::Flagged(BoardEnumColor::White)
    );
    assert_eq!(
        clock
            .view(ClockInstant::from_nanos(110))?
            .side(BoardEnumColor::White)
            .completed_moves(),
        0
    );
    let mut timely = ClockState::new(control, BoardEnumColor::White);
    timely.resume(ClockInstant::from_nanos(100))?;
    assert!(matches!(
        timely.complete_move(ClockInstant::from_nanos(109))?,
        ClockStateEnumMoveOutcome::Accepted(_)
    ));
    assert_eq!(
        timely
            .view(ClockInstant::from_nanos(109))?
            .side(BoardEnumColor::White)
            .remaining(),
        Some(ClockDuration::from_nanos(4))
    );
    Ok(())
}

#[test]
fn sudden_death_charges_only_the_active_side_and_polling_never_adds_increment()
-> ClockStateTestResult {
    let mut clock = ClockState::new(
        ClockTimeControl::sudden_death(ClockStateFixture::duration(20_000)?)?,
        BoardEnumColor::White,
    );
    clock.resume(ClockStateFixture::now(0)?)?;
    clock.poll(ClockStateFixture::now(1_000)?)?;
    clock.poll(ClockStateFixture::now(2_000)?)?;
    ClockStateFixture::accepted(&mut clock, 3_000)?;
    assert_eq!(
        ClockStateFixture::remaining(&clock, BoardEnumColor::White, 4_000)?,
        17_000
    );
    assert_eq!(
        ClockStateFixture::remaining(&clock, BoardEnumColor::Black, 4_000)?,
        19_000
    );
    Ok(())
}

#[test]
fn stages_advance_independently_per_side_and_add_the_next_budget_once() -> ClockStateTestResult {
    let control = ClockTimeControl::staged(vec![
        ClockStage::new(
            ClockStageEnumQuota::Moves(ClockStateFixture::quota(2)?),
            ClockStateFixture::duration(20_000)?,
            ClockStageEnumBonus::Fischer(ClockStateFixture::duration(1_000)?),
        )?,
        ClockStage::new(
            ClockStageEnumQuota::Unlimited,
            ClockStateFixture::duration(30_000)?,
            ClockStageEnumBonus::None,
        )?,
    ])?;
    let mut clock = ClockState::new(control, BoardEnumColor::White);
    clock.resume(ClockStateFixture::now(0)?)?;
    for time in [1_000, 2_000, 3_000] {
        ClockStateFixture::accepted(&mut clock, time)?;
    }
    let view = clock.view(ClockStateFixture::now(3_000)?)?;
    assert_eq!(view.side(BoardEnumColor::White).stage_index(), Some(1));
    assert_eq!(
        view.side(BoardEnumColor::White)
            .remaining()
            .ok_or("missing white time")?
            .as_millis(),
        50_000
    );
    assert_eq!(view.side(BoardEnumColor::White).moves_to_go(), None);
    assert_eq!(view.side(BoardEnumColor::Black).stage_index(), Some(0));
    assert_eq!(
        view.side(BoardEnumColor::Black).moves_to_go(),
        Some(ClockStateFixture::quota(1)?)
    );
    ClockStateFixture::accepted(&mut clock, 4_000)?;
    ClockStateFixture::accepted(&mut clock, 5_000)?;
    assert_eq!(
        ClockStateFixture::remaining(&clock, BoardEnumColor::White, 5_000)?,
        49_000
    );
    Ok(())
}

#[test]
fn final_cyclic_stage_repeats_its_quota_and_budget() -> ClockStateTestResult {
    let control = ClockTimeControl::staged(vec![ClockStage::new(
        ClockStageEnumQuota::Cyclic(ClockStateFixture::quota(2)?),
        ClockStateFixture::duration(10_000)?,
        ClockStageEnumBonus::None,
    )?])?;
    let mut clock = ClockState::new(control, BoardEnumColor::White);
    clock.resume(ClockStateFixture::now(0)?)?;
    for time in 1..=7 {
        ClockStateFixture::accepted(&mut clock, time * 1_000)?;
    }
    let white = clock
        .view(ClockStateFixture::now(7_000)?)?
        .side(BoardEnumColor::White);
    assert_eq!(white.completed_moves(), 4);
    assert_eq!(
        white.remaining().ok_or("missing white time")?.as_millis(),
        26_000
    );
    assert_eq!(white.moves_to_go(), Some(ClockStateFixture::quota(2)?));
    assert_eq!(white.stage_index(), Some(0));
    Ok(())
}

#[test]
fn simple_delay_defers_main_clock_charge_and_exposes_unused_delay() -> ClockStateTestResult {
    let mut clock = ClockState::new(
        ClockTimeControl::simple_delay(
            ClockStateFixture::duration(5_000)?,
            ClockStateFixture::duration(2_000)?,
        )?,
        BoardEnumColor::White,
    );
    clock.resume(ClockStateFixture::now(0)?)?;
    let early = clock.view(ClockStateFixture::now(1_000)?)?;
    assert_eq!(
        early
            .side(BoardEnumColor::White)
            .displayed()
            .ok_or("missing display")?
            .as_millis(),
        5_000
    );
    assert_eq!(
        early
            .side(BoardEnumColor::White)
            .delay_remaining()
            .as_millis(),
        1_000
    );
    assert_eq!(
        early.side(BoardEnumColor::White).pending_refund(),
        ClockDuration::ZERO
    );
    ClockStateFixture::accepted(&mut clock, 4_000)?;
    assert_eq!(
        ClockStateFixture::remaining(&clock, BoardEnumColor::White, 4_000)?,
        3_000
    );
    Ok(())
}

#[test]
fn bronstein_transient_zero_is_not_flag_fall_and_refund_arrives_only_on_timely_move()
-> ClockStateTestResult {
    let control = ClockTimeControl::bronstein_delay(
        ClockStateFixture::duration(5_000)?,
        ClockStateFixture::duration(2_000)?,
    )?;
    let mut clock = ClockState::new(control.clone(), BoardEnumColor::White);
    clock.resume(ClockStateFixture::now(0)?)?;
    let view = clock.view(ClockStateFixture::now(6_000)?)?;
    assert_eq!(view.status(), ClockStateEnumStatus::Running);
    assert_eq!(
        view.side(BoardEnumColor::White).displayed(),
        Some(ClockDuration::ZERO)
    );
    assert_eq!(
        view.side(BoardEnumColor::White).remaining(),
        Some(ClockStateFixture::duration(1_000)?)
    );
    assert_eq!(
        view.side(BoardEnumColor::White).pending_refund(),
        ClockStateFixture::duration(2_000)?
    );
    ClockStateFixture::accepted(&mut clock, 6_000)?;
    assert_eq!(
        ClockStateFixture::remaining(&clock, BoardEnumColor::White, 6_000)?,
        1_000
    );
    let mut late = ClockState::new(control, BoardEnumColor::White);
    late.resume(ClockStateFixture::now(0)?)?;
    assert!(matches!(
        late.complete_move(ClockStateFixture::now(7_000)?)?,
        ClockStateEnumMoveOutcome::Expired(_)
    ));
    Ok(())
}

#[test]
fn pausing_preserves_elapsed_turn_and_never_grants_a_second_delay() -> ClockStateTestResult {
    for bonus in [
        ClockStageEnumBonus::SimpleDelay(ClockStateFixture::duration(2_000)?),
        ClockStageEnumBonus::BronsteinDelay(ClockStateFixture::duration(2_000)?),
    ] {
        let control = ClockTimeControl::staged(vec![ClockStage::new(
            ClockStageEnumQuota::Unlimited,
            ClockStateFixture::duration(5_000)?,
            bonus,
        )?])?;
        let mut clock = ClockState::new(control, BoardEnumColor::White);
        clock.resume(ClockStateFixture::now(0)?)?;
        clock.pause(ClockStateFixture::now(1_500)?)?;
        let before = clock.view(ClockStateFixture::now(50_000)?)?;
        assert_eq!(before.elapsed(), ClockStateFixture::duration(1_500)?);
        assert_eq!(
            before.side(BoardEnumColor::White).delay_remaining(),
            ClockStateFixture::duration(500)?
        );
        clock.resume(ClockStateFixture::now(50_000)?)?;
        assert_eq!(
            clock.view(ClockStateFixture::now(50_000)?)?.deadline(),
            Some(ClockStateFixture::now(55_500)?)
        );
        clock.pause(ClockStateFixture::now(51_000)?)?;
        clock.resume(ClockStateFixture::now(100_000)?)?;
        ClockStateFixture::accepted(&mut clock, 101_000)?;
        assert_eq!(
            ClockStateFixture::remaining(&clock, BoardEnumColor::White, 101_000)?,
            3_500
        );
    }
    Ok(())
}

#[test]
fn takeback_restores_complete_pre_move_accounting_but_never_rewinds_used_warning_flags()
-> ClockStateTestResult {
    let mut clock = ClockState::new(
        ClockTimeControl::fischer(
            ClockStateFixture::duration(12_000)?,
            ClockStateFixture::duration(5_000)?,
        )?,
        BoardEnumColor::White,
    );
    clock.resume(ClockStateFixture::now(0)?)?;
    assert_eq!(
        clock.poll(ClockStateFixture::now(2_000)?)?.new_warning(),
        None
    );
    assert_eq!(
        clock.poll(ClockStateFixture::now(2_001)?)?.new_warning(),
        Some(BoardEnumColor::White)
    );
    let receipt = match clock.complete_move(ClockStateFixture::now(3_000)?)? {
        ClockStateEnumMoveOutcome::Accepted(receipt) => receipt,
        ClockStateEnumMoveOutcome::Expired(_) => return Err("fixture unexpectedly flagged".into()),
    };
    assert_eq!(
        ClockStateFixture::remaining(&clock, BoardEnumColor::White, 3_000)?,
        14_000
    );
    assert!(clock.warning_used(BoardEnumColor::White));
    clock.restore(receipt.before())?;
    assert_eq!(clock.status(), ClockStateEnumStatus::Paused);
    assert_eq!(clock.active(), BoardEnumColor::White);
    assert_eq!(
        ClockStateFixture::remaining(&clock, BoardEnumColor::White, 20_000)?,
        9_000
    );
    assert_eq!(
        clock
            .view(ClockStateFixture::now(20_000)?)?
            .side(BoardEnumColor::White)
            .completed_moves(),
        0
    );
    assert!(clock.warning_used(BoardEnumColor::White));
    assert_eq!(
        clock.resume(ClockStateFixture::now(20_000)?)?.new_warning(),
        None
    );
    ClockStateFixture::accepted(&mut clock, 20_000)?;
    assert_eq!(
        ClockStateFixture::remaining(&clock, BoardEnumColor::White, 20_000)?,
        14_000
    );
    Ok(())
}

#[test]
fn restored_delay_and_stage_snapshots_preserve_both_sides_and_completed_quotas()
-> ClockStateTestResult {
    let control = ClockTimeControl::staged(vec![
        ClockStage::new(
            ClockStageEnumQuota::Moves(ClockStateFixture::quota(1)?),
            ClockStateFixture::duration(5_000)?,
            ClockStageEnumBonus::SimpleDelay(ClockStateFixture::duration(2_000)?),
        )?,
        ClockStage::new(
            ClockStageEnumQuota::Unlimited,
            ClockStateFixture::duration(20_000)?,
            ClockStageEnumBonus::None,
        )?,
    ])?;
    let mut clock = ClockState::new(control, BoardEnumColor::White);
    clock.resume(ClockStateFixture::now(0)?)?;
    let snapshot = clock.snapshot(ClockStateFixture::now(3_000)?)?;
    ClockStateFixture::accepted(&mut clock, 3_000)?;
    ClockStateFixture::accepted(&mut clock, 4_000)?;
    clock.restore(&snapshot)?;
    clock.resume(ClockStateFixture::now(100_000)?)?;
    let view = clock.view(ClockStateFixture::now(100_000)?)?;
    assert_eq!(view.side(BoardEnumColor::White).stage_index(), Some(0));
    assert_eq!(
        view.side(BoardEnumColor::White).moves_to_go(),
        Some(ClockStateFixture::quota(1)?)
    );
    assert_eq!(
        view.side(BoardEnumColor::Black).remaining(),
        Some(ClockStateFixture::duration(5_000)?)
    );
    assert_eq!(
        view.side(BoardEnumColor::White).delay_remaining(),
        ClockDuration::ZERO
    );
    assert_eq!(view.deadline(), Some(ClockStateFixture::now(104_000)?));
    Ok(())
}

#[test]
fn backwards_timestamps_invalid_phases_and_wrong_control_restore_leave_state_unchanged()
-> ClockStateTestResult {
    let mut clock = ClockState::new(ClockTimeControl::initial_default(), BoardEnumColor::White);
    let original = clock.clone();
    assert!(clock.complete_move(ClockStateFixture::now(0)?).is_err());
    assert_eq!(clock, original);
    clock.resume(ClockStateFixture::now(100)?)?;
    assert!(clock.resume(ClockStateFixture::now(101)?).is_err());
    clock.poll(ClockStateFixture::now(200)?)?;
    let before = clock.clone();
    assert!(clock.complete_move(ClockStateFixture::now(199)?).is_err());
    assert_eq!(clock, before);
    let other = ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White);
    assert!(
        clock
            .restore(&other.snapshot(ClockStateFixture::now(0)?)?)
            .is_err()
    );
    assert_eq!(clock, before);
    Ok(())
}

#[test]
fn runtime_bonus_overflow_is_a_typed_error_with_no_partial_turn_or_counter_commit()
-> ClockStateTestResult {
    let control = ClockTimeControl::fischer(
        ClockDuration::from_nanos(u64::MAX - 1),
        ClockDuration::from_nanos(1),
    )?;
    let mut clock = ClockState::new(control, BoardEnumColor::White);
    let now = ClockInstant::from_nanos(0);
    clock.resume(now)?;
    assert!(matches!(
        clock.complete_move(now)?,
        ClockStateEnumMoveOutcome::Accepted(_)
    ));
    assert!(matches!(
        clock.complete_move(now)?,
        ClockStateEnumMoveOutcome::Accepted(_)
    ));
    let before = clock.clone();
    assert!(clock.complete_move(now).is_err());
    assert_eq!(clock, before);
    Ok(())
}

#[test]
fn persisted_warning_flags_merge_monotonically_and_do_not_replay_after_relaunch()
-> ClockStateTestResult {
    let control = ClockTimeControl::sudden_death(ClockStateFixture::duration(5_000)?)?;
    let mut clock = ClockState::new(control.clone(), BoardEnumColor::White);
    clock.merge_warning_used(BoardEnumColor::White);
    assert_eq!(
        clock.resume(ClockStateFixture::now(0)?)?.new_warning(),
        None
    );
    let snapshot = clock.snapshot(ClockStateFixture::now(1_000)?)?;
    let mut reopened = ClockState::new(control, BoardEnumColor::Black);
    reopened.merge_warning_used(BoardEnumColor::White);
    reopened.restore(&snapshot)?;
    assert_eq!(reopened.active(), BoardEnumColor::White);
    assert_eq!(
        reopened.resume(ClockStateFixture::now(100)?)?.new_warning(),
        None
    );
    assert!(reopened.warning_used(BoardEnumColor::White));
    Ok(())
}
