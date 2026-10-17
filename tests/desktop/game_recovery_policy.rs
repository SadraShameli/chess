use chess::desktop::native::lifecycle::{
    DesktopNativeLifecycle, DesktopNativeLifecycleEnumSignal, DesktopNativeLifecycleEnumWindowEvent,
    DesktopNativeLifecyclePuzzleWindowId,
};
use chess::game::enums::{GameId, GameRevision};
use chess::game::pause::{GamePauseEnumCause, GamePauseEnumFault};
use chess::game::recovery::{GameRecovery, GameRecoveryEnumAction, GameRecoveryEnumTrigger};
use chess::game::save::{GameSaveEnumAcknowledgment, GameSaveEnumStatus};
use chess::game::save_coordinator::{
    GameSaveCoordinator, GameSaveCoordinatorEnumBarrier, GameSaveCoordinatorError,
};
use chess::presentation::game::recovery::PresentationGameRecovery;

type DesktopRecoveryResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn game() -> Result<GameId, Box<dyn std::error::Error>> {
    Ok("00000000-0000-4000-8000-000000000001".parse()?)
}

fn revision(count: u8) -> Result<GameRevision, Box<dyn std::error::Error>> {
    let mut revision = GameRevision::ZERO;
    for _ in 0..count {
        revision = revision.next()?;
    }
    Ok(revision)
}

#[test]
fn focus_changes_never_pause_a_live_game_but_sleep_tab_close_quit_and_faults_do() {
    for trigger in [
        GameRecoveryEnumTrigger::FocusLost,
        GameRecoveryEnumTrigger::FocusGained,
        GameRecoveryEnumTrigger::WakeFromSleep,
    ] {
        assert_eq!(GameRecovery::decide(trigger), GameRecoveryEnumAction::KeepRunning);
    }
    assert_eq!(
        GameRecovery::pause_cause(GameRecoveryEnumTrigger::ComputerSleep),
        Some(GamePauseEnumCause::ComputerSleep)
    );
    assert_eq!(
        GameRecovery::pause_cause(GameRecoveryEnumTrigger::TabClosed),
        Some(GamePauseEnumCause::TabClosed)
    );
    assert_eq!(
        GameRecovery::pause_cause(GameRecoveryEnumTrigger::ApplicationQuit),
        Some(GamePauseEnumCause::ApplicationQuit)
    );
    assert_eq!(
        GameRecovery::decide(GameRecoveryEnumTrigger::ProcessCrash),
        GameRecoveryEnumAction::RestorePausedFromLastDurable
    );
    assert_eq!(
        GameRecovery::pause_cause(GameRecoveryEnumTrigger::Fault(GamePauseEnumFault::Engine)),
        Some(GamePauseEnumCause::Fault(GamePauseEnumFault::Engine))
    );
}

#[test]
fn faults_require_a_retry_before_resume_and_no_pause_resumes_automatically() {
    assert_eq!(
        GameRecovery::requires_retry_before_resume(GamePauseEnumCause::Fault(
            GamePauseEnumFault::Graphics
        )),
        Some(GamePauseEnumFault::Graphics)
    );
    assert_eq!(
        GameRecovery::requires_retry_before_resume(GamePauseEnumCause::AutosaveFailure),
        Some(GamePauseEnumFault::Save)
    );
    assert_eq!(
        GameRecovery::requires_retry_before_resume(GamePauseEnumCause::ComputerSleep),
        None
    );
    for cause in [
        GamePauseEnumCause::Manual,
        GamePauseEnumCause::ComputerSleep,
        GamePauseEnumCause::Relaunch,
        GamePauseEnumCause::TabClosed,
        GamePauseEnumCause::ApplicationQuit,
        GamePauseEnumCause::UpdateInstall,
        GamePauseEnumCause::AutosaveFailure,
        GamePauseEnumCause::Fault(GamePauseEnumFault::Save),
    ] {
        assert!(!GameRecovery::resumes_automatically(cause));
    }
}

#[test]
fn native_signals_map_to_game_pauses_without_pausing_for_focus() -> DesktopRecoveryResult {
    let window = DesktopNativeLifecyclePuzzleWindowId::new(std::num::NonZeroU64::MIN);
    assert_eq!(
        DesktopNativeLifecycle::game_interruption(DesktopNativeLifecycleEnumSignal::Suspend),
        Some(GamePauseEnumCause::ComputerSleep)
    );
    assert_eq!(
        DesktopNativeLifecycle::game_interruption(DesktopNativeLifecycleEnumSignal::Resumed),
        None
    );
    assert_eq!(
        DesktopNativeLifecycle::game_interruption(DesktopNativeLifecycleEnumSignal::Window {
            window,
            event: DesktopNativeLifecycleEnumWindowEvent::FocusLost,
        }),
        None
    );
    Ok(())
}

#[test]
fn recovery_headlines_distinguish_pending_retry_from_a_recovered_pause() {
    let fault = GamePauseEnumCause::Fault(GamePauseEnumFault::Engine);
    assert!(PresentationGameRecovery::pause_headline(fault, false).contains("Retry first"));
    assert!(PresentationGameRecovery::pause_headline(fault, true).contains("stays paused"));
    assert!(
        PresentationGameRecovery::pause_headline(GamePauseEnumCause::Manual, false)
            .contains("Resume")
    );
}

#[test]
fn unsaved_moves_block_close_quit_and_trash_until_the_newest_revision_is_durable()
-> DesktopRecoveryResult {
    let mut coordinator = GameSaveCoordinator::new(game()?, revision(3)?);
    assert_eq!(coordinator.status(), GameSaveEnumStatus::Required);
    for barrier in [
        GameSaveCoordinatorEnumBarrier::CloseGame,
        GameSaveCoordinatorEnumBarrier::QuitApplication,
        GameSaveCoordinatorEnumBarrier::MoveToTrash,
    ] {
        assert!(matches!(
            coordinator.require_saved(barrier),
            Err(GameSaveCoordinatorError::UnsavedChanges { .. })
        ));
    }
    assert_eq!(
        coordinator.acknowledge(game()?, revision(2)?)?,
        GameSaveEnumAcknowledgment::Older
    );
    assert_eq!(coordinator.status(), GameSaveEnumStatus::Required);
    assert_eq!(
        coordinator.acknowledge(game()?, revision(3)?)?,
        GameSaveEnumAcknowledgment::Current
    );
    assert_eq!(coordinator.status(), GameSaveEnumStatus::Current);
    coordinator.require_saved(GameSaveCoordinatorEnumBarrier::QuitApplication)?;
    Ok(())
}

#[test]
fn a_failed_save_keeps_memory_state_and_retry_targets_the_newest_revision()
-> DesktopRecoveryResult {
    let mut coordinator = GameSaveCoordinator::new(game()?, revision(1)?);
    coordinator.acknowledge(game()?, revision(1)?)?;
    coordinator.observe_memory(revision(4)?);
    coordinator.record_failure("disk full".to_owned());
    assert_eq!(coordinator.newest_to_save(), revision(4)?);
    assert_eq!(coordinator.memory_revision(), revision(4)?);
    let headline = PresentationGameRecovery::save_headline(&coordinator)
        .ok_or("a failure has a headline")?;
    assert!(headline.contains("disk full") && headline.contains("Retry Save"));
    coordinator.acknowledge(game()?, revision(4)?)?;
    assert!(coordinator.failure().is_none());
    assert!(PresentationGameRecovery::save_headline(&coordinator).is_none());
    Ok(())
}

#[test]
fn receipts_for_another_game_or_a_future_revision_are_rejected() -> DesktopRecoveryResult {
    let mut coordinator = GameSaveCoordinator::new(game()?, revision(2)?);
    let other: GameId = "00000000-0000-4000-8000-000000000002".parse()?;
    assert!(matches!(
        coordinator.acknowledge(other, revision(1)?),
        Err(GameSaveCoordinatorError::ForeignGame { .. })
    ));
    assert!(matches!(
        coordinator.acknowledge(game()?, revision(3)?),
        Err(GameSaveCoordinatorError::FutureRevision { .. })
    ));
    Ok(())
}
