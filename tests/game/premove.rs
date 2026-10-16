use chess::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::game::enums::GameEnumMode;
use chess::game::premove::{
    GamePremove, GamePremoveEnumClearReason, GamePremoveEnumResolution, GamePremoveIntent,
};

type GamePremoveTestResult = Result<(), Box<dyn std::error::Error>>;

struct GamePremoveFixture;

impl GamePremoveFixture {
    fn intent(
        origin: &str,
        destination: &str,
        promotion: Option<BoardEnumPieceKind>,
    ) -> Result<GamePremoveIntent, Box<dyn std::error::Error>> {
        Ok(GamePremoveIntent::new(
            origin.parse()?,
            destination.parse()?,
            promotion,
        )?)
    }

    fn position() -> Result<BoardPosition, Box<dyn std::error::Error>> {
        Ok(BoardPosition::standard().play_uci("e2e4")?)
    }
}

#[test]
fn premoves_are_disabled_initially_and_cannot_be_queued_during_the_human_turn()
-> GamePremoveTestResult {
    let mut premove = GamePremove::default();
    let intent = GamePremoveFixture::intent("g1", "f3", None)?;
    let position = GamePremoveFixture::position()?;
    assert!(!premove.is_enabled());
    assert!(
        premove
            .queue(
                &position,
                GameEnumMode::HumanVersusEngine,
                BoardEnumColor::White,
                intent
            )
            .is_err()
    );
    premove.set_enabled(true);
    assert!(
        premove
            .queue(
                &BoardPosition::standard(),
                GameEnumMode::HumanVersusEngine,
                BoardEnumColor::White,
                intent,
            )
            .is_err()
    );
    assert!(
        premove
            .queue(
                &position,
                GameEnumMode::LocalTwoPlayer,
                BoardEnumColor::White,
                intent
            )
            .is_err()
    );
    assert!(premove.pending().is_none());
    Ok(())
}

#[test]
fn one_queued_intent_is_replaced_and_executes_only_after_legal_revalidation()
-> GamePremoveTestResult {
    let mut premove = GamePremove::default();
    premove.set_enabled(true);
    let position = GamePremoveFixture::position()?;
    let first = GamePremoveFixture::intent("g1", "f3", None)?;
    let second = GamePremoveFixture::intent("b1", "c3", None)?;
    assert_eq!(
        premove.queue(
            &position,
            GameEnumMode::HumanVersusEngine,
            BoardEnumColor::White,
            first
        )?,
        None
    );
    assert_eq!(
        premove.queue(
            &position,
            GameEnumMode::HumanVersusEngine,
            BoardEnumColor::White,
            second
        )?,
        Some(first)
    );
    assert!(premove.resolve(&position, BoardEnumColor::White).is_err());
    assert_eq!(premove.pending(), Some(second));
    let position = position.play_uci("e7e5")?;
    let GamePremoveEnumResolution::Execute(movement) =
        premove.resolve(&position, BoardEnumColor::White)?
    else {
        return Err("expected legal queued move".into());
    };
    assert_eq!(movement.uci(position.variant()), "b1c3");
    assert!(premove.pending().is_none());
    assert_eq!(position.turn(), BoardEnumColor::White);
    Ok(())
}

#[test]
fn an_illegal_premove_is_cleared_without_submitting_or_changing_the_position()
-> GamePremoveTestResult {
    let mut premove = GamePremove::default();
    premove.set_enabled(true);
    let position = GamePremoveFixture::position()?;
    let intent = GamePremoveFixture::intent("g1", "g5", None)?;
    premove.queue(
        &position,
        GameEnumMode::HumanVersusEngine,
        BoardEnumColor::White,
        intent,
    )?;
    let position = position.play_uci("e7e5")?;
    let before = position.fen();
    assert!(matches!(
        premove.resolve(&position, BoardEnumColor::White)?,
        GamePremoveEnumResolution::ClearedIllegal { intent: rejected, .. } if rejected == intent
    ));
    assert!(premove.pending().is_none());
    assert_eq!(position.fen(), before);
    Ok(())
}

#[test]
fn promotion_requires_a_preselected_piece_and_preserves_all_four_choices() -> GamePremoveTestResult
{
    let position =
        BoardPosition::from_fen("7k/P7/8/8/8/8/8/7K b - - 0 1", BoardEnumVariant::Standard)?;
    let mut premove = GamePremove::default();
    premove.set_enabled(true);
    let unchosen = GamePremoveFixture::intent("a7", "a8", None)?;
    assert!(
        premove
            .queue(
                &position,
                GameEnumMode::HumanVersusEngine,
                BoardEnumColor::White,
                unchosen
            )
            .is_err()
    );
    assert!(GamePremoveFixture::intent("a7", "a8", Some(BoardEnumPieceKind::King)).is_err());
    for promotion in [
        BoardEnumPieceKind::Knight,
        BoardEnumPieceKind::Bishop,
        BoardEnumPieceKind::Rook,
        BoardEnumPieceKind::Queen,
    ] {
        let intent = GamePremoveFixture::intent("a7", "a8", Some(promotion))?;
        premove.queue(
            &position,
            GameEnumMode::HumanVersusEngine,
            BoardEnumColor::White,
            intent,
        )?;
        let after = position.play_uci("h8g8")?;
        let GamePremoveEnumResolution::Execute(movement) =
            premove.resolve(&after, BoardEnumColor::White)?
        else {
            return Err("expected preselected promotion".into());
        };
        assert_eq!(movement.promotion(), Some(promotion));
    }
    Ok(())
}

#[test]
fn every_interruption_and_disabling_clear_the_queued_intent() -> GamePremoveTestResult {
    let position = GamePremoveFixture::position()?;
    let intent = GamePremoveFixture::intent("g1", "f3", None)?;
    let mut premove = GamePremove::default();
    premove.set_enabled(true);
    for reason in GamePremoveEnumClearReason::ALL {
        premove.queue(
            &position,
            GameEnumMode::HumanVersusEngine,
            BoardEnumColor::White,
            intent,
        )?;
        assert_eq!(premove.clear(reason), Some(intent));
        assert!(premove.pending().is_none());
        assert!(premove.is_enabled());
    }
    premove.queue(
        &position,
        GameEnumMode::HumanVersusEngine,
        BoardEnumColor::White,
        intent,
    )?;
    premove.set_enabled(false);
    assert!(!premove.is_enabled());
    assert!(premove.pending().is_none());
    assert!(matches!(
        premove.resolve(&position, BoardEnumColor::Black)?,
        GamePremoveEnumResolution::Empty
    ));
    Ok(())
}

#[test]
fn a_queue_cannot_reference_an_opponents_piece_or_attach_promotion_to_a_knight()
-> GamePremoveTestResult {
    let position = GamePremoveFixture::position()?;
    let mut premove = GamePremove::default();
    premove.set_enabled(true);
    for intent in [
        GamePremoveFixture::intent("g8", "f6", None)?,
        GamePremoveFixture::intent("g1", "f3", Some(BoardEnumPieceKind::Queen))?,
        GamePremoveFixture::intent("g3", "f5", None)?,
    ] {
        assert!(
            premove
                .queue(
                    &position,
                    GameEnumMode::HumanVersusEngine,
                    BoardEnumColor::White,
                    intent
                )
                .is_err()
        );
    }
    assert!(premove.pending().is_none());
    Ok(())
}
