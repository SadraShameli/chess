use chess::board::enums::BoardEnumColor;
use chess::clock::duration::ClockDuration;
use chess::desktop::native::audio::{
    DesktopNativeAudio, DesktopNativeAudioError, DesktopNativeAudioPort,
};
use chess::desktop::preferences::{
    DesktopPreferences, DesktopPreferencesEnumAudioCategory, DesktopPreferencesEnumPatch,
};
use chess::game::alerts::GameAlertsLowTime;
use chess::game::enums::GameId;
use chess::presentation::audio::router::{
    PresentationAudioContext, PresentationAudioEvent, PresentationAudioRouter,
};

type Category = DesktopPreferencesEnumAudioCategory;

fn game(value: u128) -> Result<GameId, Box<dyn std::error::Error>> {
    Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
}

fn context(selected: Option<GameId>, focused: bool, now_ms: u64) -> PresentationAudioContext {
    PresentationAudioContext {
        selected_game: selected,
        app_focused: focused,
        now_ms,
    }
}

#[test]
fn move_and_check_follow_the_selected_game_while_results_cover_all()
-> Result<(), Box<dyn std::error::Error>> {
    let selected = game(1)?;
    let other = game(2)?;
    let preferences = DesktopPreferences::default();
    let mut router = PresentationAudioRouter::default();
    assert!(
        router
            .submit(
                &[PresentationAudioEvent::new(other, Category::Move)],
                preferences,
                context(Some(selected), true, 0),
            )
            .is_none()
    );
    let result = router.submit(
        &[PresentationAudioEvent::new(other, Category::LowTime)],
        preferences,
        context(Some(selected), true, 10),
    );
    assert!(result.is_some_and(|playback| playback.category() == Category::LowTime));
    Ok(())
}

#[test]
fn priority_is_result_then_low_time_then_check_then_move() -> Result<(), Box<dyn std::error::Error>>
{
    let id = game(1)?;
    let events = [
        PresentationAudioEvent::new(id, Category::Move),
        PresentationAudioEvent::new(id, Category::Check),
        PresentationAudioEvent::new(id, Category::LowTime),
        PresentationAudioEvent::new(id, Category::Result),
    ];
    let mut router = PresentationAudioRouter::default();
    let playback = router.submit(
        &events,
        DesktopPreferences::default(),
        context(Some(id), true, 0),
    );
    assert!(playback.is_some_and(|playback| playback.category() == Category::Result));
    Ok(())
}

#[test]
fn same_category_events_coalesce_within_one_hundred_milliseconds()
-> Result<(), Box<dyn std::error::Error>> {
    let id = game(1)?;
    let event = [PresentationAudioEvent::new(id, Category::Move)];
    let preferences = DesktopPreferences::default();
    let mut router = PresentationAudioRouter::default();
    assert!(
        router
            .submit(&event, preferences, context(Some(id), true, 1_000))
            .is_some()
    );
    assert!(
        router
            .submit(&event, preferences, context(Some(id), true, 1_099))
            .is_none()
    );
    assert!(
        router
            .submit(&event, preferences, context(Some(id), true, 1_200))
            .is_some()
    );
    Ok(())
}

#[test]
fn an_overlapping_lower_priority_sound_is_dropped_not_queued()
-> Result<(), Box<dyn std::error::Error>> {
    let id = game(1)?;
    let preferences = DesktopPreferences::default();
    let mut router = PresentationAudioRouter::default();
    assert!(
        router
            .submit(
                &[PresentationAudioEvent::new(id, Category::Result)],
                preferences,
                context(Some(id), true, 0),
            )
            .is_some()
    );
    let during = [PresentationAudioEvent::new(id, Category::Move)];
    assert!(
        router
            .submit(&during, preferences, context(Some(id), true, 300))
            .is_none()
    );
    assert!(
        router
            .submit(&during, preferences, context(Some(id), true, 5_000))
            .is_some()
    );
    Ok(())
}

#[test]
fn muted_and_unfocused_events_are_discarded_and_never_replayed()
-> Result<(), Box<dyn std::error::Error>> {
    let id = game(1)?;
    let event = [PresentationAudioEvent::new(id, Category::Check)];
    let mut router = PresentationAudioRouter::default();
    let preferences = DesktopPreferences::default();
    assert!(
        router
            .submit(&event, preferences, context(Some(id), false, 0))
            .is_none()
    );
    let mut muted = preferences;
    muted.apply(DesktopPreferencesEnumPatch::CategoryEnabled(
        Category::Check,
        false,
    ));
    assert!(
        router
            .submit(&event, muted, context(Some(id), true, 500))
            .is_none()
    );
    assert!(
        router
            .submit(&[], preferences, context(Some(id), true, 1_000))
            .is_none()
    );
    Ok(())
}

struct MissingDevice;

impl DesktopNativeAudioPort for MissingDevice {
    fn play(&mut self, _category: Category, _gain: f32) -> Result<(), DesktopNativeAudioError> {
        Err(DesktopNativeAudioError::DeviceUnavailable)
    }
}

#[test]
fn a_missing_device_warns_without_changing_preferences_or_queueing()
-> Result<(), Box<dyn std::error::Error>> {
    let id = game(1)?;
    let preferences = DesktopPreferences::default();
    let mut router = PresentationAudioRouter::default();
    let mut audio = DesktopNativeAudio::new(MissingDevice);
    if let Some(playback) = router.submit(
        &[PresentationAudioEvent::new(id, Category::Move)],
        preferences,
        context(Some(id), true, 0),
    ) {
        audio.play(playback);
    }
    assert!(audio.warning().is_some());
    assert_eq!(preferences, DesktopPreferences::default());
    Ok(())
}

#[test]
fn low_time_flags_fire_once_per_side_and_survive_restore() -> Result<(), Box<dyn std::error::Error>>
{
    let mut alerts = GameAlertsLowTime::default();
    let low = ClockDuration::from_millis(5_000)?;
    assert!(alerts.observe(BoardEnumColor::White, low));
    assert!(!alerts.observe(BoardEnumColor::White, low));
    assert!(alerts.observe(BoardEnumColor::Black, low));
    let (white, black) = alerts.flags();
    let mut restored = GameAlertsLowTime::restore(white, black);
    assert!(!restored.observe(BoardEnumColor::White, ClockDuration::from_millis(1)?));
    assert!(!restored.observe(BoardEnumColor::Black, low));
    Ok(())
}
