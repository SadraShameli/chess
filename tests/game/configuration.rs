use rand::{SeedableRng, rngs::StdRng};

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::board::start::{BoardStartEnumSelection, BoardStartIndex};
use chess::clock::duration::ClockDuration;
use chess::clock::time_control::ClockTimeControl;
use chess::game::configuration::{
    GameConfiguration, GameConfigurationEnumIndex, GameConfigurationEnumOpponent,
    GameConfigurationEnumPreset, GameConfigurationEnumStart, GameConfigurationOptions,
};
use chess::game::enums::{GameEnumMode, GameEnumSeat};

type GameConfigurationTestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn new_game_defaults_preserve_canonical_clock_engine_budget_and_seats()
-> GameConfigurationTestResult {
    let configuration = GameConfiguration::initial_default();
    assert_eq!(
        configuration.time_control(),
        &ClockTimeControl::initial_default()
    );
    let seats = configuration.seats()?;
    assert_eq!(seats.mode(), GameEnumMode::HumanVersusEngine);
    assert_eq!(seats.seat(BoardEnumColor::White), GameEnumSeat::Human);
    assert_eq!(seats.seat(BoardEnumColor::Black), GameEnumSeat::Reckless);
    assert_eq!(configuration.start(), &GameConfigurationEnumStart::Standard);
    let options = configuration.options();
    assert_eq!(
        options.engine_search().untimed_move_time(),
        ClockDuration::from_millis(3000)?
    );
    assert!(options.requires_local_takeback_consent());
    assert!(!options.is_premove_enabled());
    assert!(!options.is_auto_flip_enabled());
    Ok(())
}

#[test]
fn every_selected_preset_has_the_approved_base_and_exclusive_fischer_increment()
-> GameConfigurationTestResult {
    let expected = [
        None,
        Some((60_000, 0)),
        Some((120_000, 1000)),
        Some((180_000, 0)),
        Some((180_000, 2000)),
        Some((300_000, 0)),
        Some((300_000, 3000)),
        Some((600_000, 0)),
        Some((600_000, 5000)),
        Some((900_000, 10_000)),
        Some((1_800_000, 0)),
    ];
    assert_eq!(GameConfigurationEnumPreset::ALL.len(), expected.len());
    for (preset, expected) in GameConfigurationEnumPreset::ALL.into_iter().zip(expected) {
        let control = preset.time_control()?;
        match expected {
            None => assert!(!control.is_timed()),
            Some((base, increment)) => {
                assert_eq!(control.stages().len(), 1);
                let stage = control.stages().first().ok_or("missing preset stage")?;
                assert_eq!(stage.base_time().as_millis(), base, "{}", preset.label());
                assert_eq!(
                    stage.bonus().increment().as_millis(),
                    increment,
                    "{}",
                    preset.label()
                );
                assert_eq!(stage.bonus().delay(), ClockDuration::ZERO);
            }
        }
    }
    Ok(())
}

#[test]
fn numbered_and_mixed_random_starts_resolve_only_with_an_injected_generator()
-> GameConfigurationTestResult {
    let numbered = BoardStartIndex::try_from(42)?;
    let selection = GameConfigurationEnumStart::Dfrc {
        white: GameConfigurationEnumIndex::Numbered(numbered),
        black: GameConfigurationEnumIndex::Random,
    };
    let mut random = StdRng::seed_from_u64(69);
    let mut expected_random = StdRng::seed_from_u64(69);
    let expected_black = BoardStartIndex::random(&mut expected_random);
    let start = selection.resolve(&mut random)?;
    assert_eq!(
        start.selection(),
        BoardStartEnumSelection::Dfrc {
            white_index: numbered,
            black_index: expected_black,
        }
    );
    let frc = GameConfigurationEnumStart::Chess960 {
        index: GameConfigurationEnumIndex::Numbered(BoardStartIndex::STANDARD),
    }
    .resolve(&mut random)?;
    assert_eq!(
        frc.selection(),
        BoardStartEnumSelection::Chess960 {
            index: BoardStartIndex::STANDARD
        }
    );
    assert_eq!(
        frc.position().placement(),
        BoardPosition::standard().placement()
    );
    Ok(())
}

#[test]
fn custom_start_and_local_opponents_retain_their_authoritative_domain_values()
-> GameConfigurationTestResult {
    let position = BoardPosition::standard();
    let configuration = GameConfiguration::new(
        GameConfigurationEnumStart::Custom(position.clone()),
        GameConfigurationEnumOpponent::LocalTwoPlayer,
        ClockTimeControl::untimed(),
        GameConfigurationOptions::initial_default(),
    );
    let seats = configuration.seats()?;
    assert_eq!(seats.mode(), GameEnumMode::LocalTwoPlayer);
    for color in [BoardEnumColor::White, BoardEnumColor::Black] {
        assert_eq!(seats.seat(color), GameEnumSeat::Human);
    }
    let mut random = StdRng::seed_from_u64(7);
    assert_eq!(
        configuration.start().resolve(&mut random)?.position(),
        &position
    );
    Ok(())
}
