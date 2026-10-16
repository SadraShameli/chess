use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::game::adjudication::{
    GameAdjudicationEnumInterruption, GameAdjudicationEnumMateProof,
    GameAdjudicationEnumMateQuestion, GameAdjudicationMateProofProvider,
};
use chess::game::enums::{
    GameEnumEngineReadiness, GameEnumMode, GameEnumOutcomeReason, GameEnumPhase, GameEnumResult,
    GameEnumSeat, GameEnumTransitionKind, GameId, GameMoveId, GameRevision,
};
use chess::game::session::{
    GameMetadata, GameMetadataExtension, GameMetadataKnownTags, GameMetadataText, GameSeats,
    GameSession, GameSessionMoveRequest, GameSessionOptions,
};

struct GameSessionFixtureProof;

impl GameAdjudicationMateProofProvider for GameSessionFixtureProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

struct GameSessionFixture;

impl GameSessionFixture {
    fn id(value: u128) -> Result<GameMoveId, Box<dyn std::error::Error>> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn session(control: ClockTimeControl) -> Result<GameSession, Box<dyn std::error::Error>> {
        Ok(GameSession::new(GameSessionOptions {
            id: "123e4567-e89b-42d3-a456-426614174000".parse()?,
            root: Self::id(1)?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::HumanVersusEngine,
                GameEnumSeat::Human,
                GameEnumSeat::Reckless,
            )?,
            clock: ClockState::new(control, BoardEnumColor::White),
            metadata: GameMetadata::default(),
        })?)
    }

    fn start(session: &mut GameSession) -> Result<(), Box<dyn std::error::Error>> {
        session.start(
            session.revision(),
            ClockInstant::from_nanos(0),
            GameEnumEngineReadiness::Ready,
            &GameSessionFixtureProof,
        )?;
        Ok(())
    }

    fn request(
        session: &GameSession,
        id: u128,
        uci: &str,
        millis: u64,
    ) -> Result<GameSessionMoveRequest, Box<dyn std::error::Error>> {
        let position = session.tree().node(session.live())?.position();
        Ok(GameSessionMoveRequest {
            expected_revision: session.revision(),
            seat: session.seats().seat(position.turn()),
            color: position.turn(),
            node: Self::id(id)?,
            movement: position.parse_uci(uci)?,
            received_at: ClockInstant::from_millis(millis)?,
        })
    }
}

#[test]
fn seats_validate_mode_and_start_requires_engine_readiness()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(
        GameSeats::new(
            GameEnumMode::HumanVersusEngine,
            GameEnumSeat::Human,
            GameEnumSeat::Human
        )
        .is_err()
    );
    assert!(
        GameSeats::new(
            GameEnumMode::LocalTwoPlayer,
            GameEnumSeat::Human,
            GameEnumSeat::Reckless
        )
        .is_err()
    );
    assert!(
        GameSeats::new(
            GameEnumMode::HumanVersusEngine,
            GameEnumSeat::Reckless,
            GameEnumSeat::Reckless
        )
        .is_err()
    );
    let mut session = GameSessionFixture::session(ClockTimeControl::untimed())?;
    let revision = session.revision();
    assert!(
        session
            .start(
                revision,
                ClockInstant::from_nanos(0),
                GameEnumEngineReadiness::Unavailable,
                &GameSessionFixtureProof
            )
            .is_err()
    );
    assert_eq!(session.phase(), GameEnumPhase::Setup);
    assert_eq!(session.revision(), revision);
    assert!(!session.clock().is_running());
    GameSessionFixture::start(&mut session)?;
    assert_eq!(session.phase(), GameEnumPhase::Active);
    assert!(session.clock().is_running());
    Ok(())
}

#[test]
fn accepted_move_updates_one_revision_tree_clock_and_durable_transition_together()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameSessionFixture::session(ClockTimeControl::fischer(
        ClockDuration::from_millis(10_000)?,
        ClockDuration::from_millis(1_000)?,
    )?)?;
    GameSessionFixture::start(&mut session)?;
    let previous = session.revision();
    let request = GameSessionFixture::request(&session, 2, "e2e4", 2_000)?;
    let transition = session.accept_move(request, &GameSessionFixtureProof)?;
    assert_eq!(transition.kind(), GameEnumTransitionKind::MoveAccepted);
    assert_eq!(transition.previous_revision(), previous);
    assert_eq!(transition.revision(), previous.next()?);
    assert_eq!(session.revision(), transition.revision());
    assert_eq!(session.live(), GameSessionFixture::id(2)?);
    assert_eq!(session.view(), session.live());
    assert_eq!(session.clock().active(), BoardEnumColor::Black);
    assert_eq!(transition.clock().completed_moves(BoardEnumColor::White), 1);
    assert_eq!(
        session.tree().node(session.live())?.clock(),
        Some(transition.clock())
    );
    assert_eq!(session.tree().node(session.live())?.san(), Some("e4"));
    assert_eq!(
        transition.accepted_node().map(|node| node.id()),
        Some(session.live())
    );
    Ok(())
}

#[test]
fn rejected_revision_seat_color_and_legality_preserve_all_authoritative_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameSessionFixture::session(ClockTimeControl::initial_default())?;
    GameSessionFixture::start(&mut session)?;
    let before = session.clock().snapshot(ClockInstant::from_nanos(0))?;
    let revision = session.revision();
    let root = session.live();
    let mut request = GameSessionFixture::request(&session, 2, "e2e4", 1_000)?;
    request.expected_revision = GameRevision::ZERO;
    assert!(
        session
            .accept_move(request, &GameSessionFixtureProof)
            .is_err()
    );
    let mut request = GameSessionFixture::request(&session, 2, "e2e4", 1_000)?;
    request.seat = GameEnumSeat::Reckless;
    assert!(
        session
            .accept_move(request, &GameSessionFixtureProof)
            .is_err()
    );
    let mut request = GameSessionFixture::request(&session, 2, "e2e4", 1_000)?;
    request.color = BoardEnumColor::Black;
    assert!(
        session
            .accept_move(request, &GameSessionFixtureProof)
            .is_err()
    );
    let other = BoardPosition::standard().play_uci("e2e4")?;
    let mut request = GameSessionFixture::request(&session, 2, "e2e4", 1_000)?;
    request.movement = other.parse_uci("e7e5")?;
    assert!(
        session
            .accept_move(request, &GameSessionFixtureProof)
            .is_err()
    );
    assert_eq!(session.revision(), revision);
    assert_eq!(session.live(), root);
    assert_eq!(session.tree().len(), 1);
    assert_eq!(
        session.clock().snapshot(ClockInstant::from_nanos(0))?,
        before
    );
    Ok(())
}

#[test]
fn browsing_history_keeps_live_cursor_and_clock_running_independently()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameSessionFixture::session(ClockTimeControl::untimed())?;
    GameSessionFixture::start(&mut session)?;
    let root = session.live();
    session.accept_move(
        GameSessionFixture::request(&session, 2, "e2e4", 1_000)?,
        &GameSessionFixtureProof,
    )?;
    let revision = session.revision();
    session.select_view(root)?;
    assert_eq!(session.live(), GameSessionFixture::id(2)?);
    assert_eq!(session.view(), root);
    assert_eq!(session.revision(), revision);
    session.accept_move(
        GameSessionFixture::request(&session, 3, "e7e5", 2_000)?,
        &GameSessionFixtureProof,
    )?;
    assert_eq!(session.view(), root);
    assert_eq!(session.live(), GameSessionFixture::id(3)?);
    assert!(session.clock().is_running());
    session.return_to_live();
    assert_eq!(session.view(), session.live());
    Ok(())
}

#[test]
fn exact_native_deadline_forfeits_before_a_legal_move_or_increment_can_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameSessionFixture::session(ClockTimeControl::fischer(
        ClockDuration::from_millis(1_000)?,
        ClockDuration::from_millis(5_000)?,
    )?)?;
    GameSessionFixture::start(&mut session)?;
    let root = session.live();
    let transition = session.accept_move(
        GameSessionFixture::request(&session, 2, "e2e4", 1_000)?,
        &GameSessionFixtureProof,
    )?;
    assert_eq!(transition.kind(), GameEnumTransitionKind::FlagFall);
    assert_eq!(session.live(), root);
    assert_eq!(session.tree().len(), 1);
    assert_eq!(transition.clock().completed_moves(BoardEnumColor::White), 0);
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    let outcome = session.outcome().ok_or("missing flag outcome")?;
    assert_eq!(
        outcome.result(),
        GameEnumResult::Victory(BoardEnumColor::Black)
    );
    assert_eq!(outcome.reason(), GameEnumOutcomeReason::Flagfall);
    assert_eq!(outcome.origin(), root);
    assert!(!session.clock().is_running());
    Ok(())
}

#[test]
fn a_move_received_before_deadline_is_accepted_and_duplicate_identity_is_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameSessionFixture::session(ClockTimeControl::sudden_death(
        ClockDuration::from_millis(1_000)?,
    )?)?;
    GameSessionFixture::start(&mut session)?;
    let root = session.live();
    let mut invalid = GameSessionFixture::request(&session, 2, "e2e4", 999)?;
    invalid.node = root;
    assert!(
        session
            .accept_move(invalid, &GameSessionFixtureProof)
            .is_err()
    );
    assert_eq!(session.tree().len(), 1);
    assert_eq!(session.clock().active(), BoardEnumColor::White);
    let transition = session.accept_move(
        GameSessionFixture::request(&session, 2, "e2e4", 999)?,
        &GameSessionFixtureProof,
    )?;
    assert_eq!(transition.kind(), GameEnumTransitionKind::MoveAccepted);
    assert_eq!(session.phase(), GameEnumPhase::Active);
    Ok(())
}

#[test]
fn checkmate_is_derived_from_live_position_and_blocks_later_moves()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameSessionFixture::session(ClockTimeControl::untimed())?;
    GameSessionFixture::start(&mut session)?;
    for (value, uci) in [(2, "f2f3"), (3, "e7e5"), (4, "g2g4"), (5, "d8h4")] {
        session.accept_move(
            GameSessionFixture::request(&session, value, uci, 1_000)?,
            &GameSessionFixtureProof,
        )?;
    }
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    let outcome = session.outcome().ok_or("missing checkmate")?;
    assert_eq!(
        outcome.result(),
        GameEnumResult::Victory(BoardEnumColor::Black)
    );
    assert_eq!(outcome.reason(), GameEnumOutcomeReason::Checkmate);
    assert_eq!(outcome.origin(), GameSessionFixture::id(5)?);
    assert!(!session.clock().is_running());
    let revision = session.revision();
    let previous_movement = BoardPosition::standard().parse_uci("e2e4")?;
    assert!(
        session
            .accept_move(
                GameSessionMoveRequest {
                    expected_revision: revision,
                    seat: GameEnumSeat::Human,
                    color: BoardEnumColor::White,
                    node: GameSessionFixture::id(6)?,
                    movement: previous_movement,
                    received_at: ClockInstant::from_millis(2_000)?,
                },
                &GameSessionFixtureProof
            )
            .is_err()
    );
    assert_eq!(session.revision(), revision);
    assert_eq!(session.tree().len(), 5);
    Ok(())
}

#[test]
fn metadata_preserves_ordered_extension_tags_and_rejects_invalid_or_duplicate_names()
-> Result<(), Box<dyn std::error::Error>> {
    let metadata = GameMetadata::new(
        GameMetadataKnownTags::default(),
        vec![
            GameMetadataExtension::new(
                "Source".to_owned(),
                GameMetadataText::new("offline".to_owned())?,
            )?,
            GameMetadataExtension::new(
                "SourceId".to_owned(),
                GameMetadataText::new("42".to_owned())?,
            )?,
        ],
    )?;
    assert_eq!(metadata.extensions()[0].name(), "Source");
    assert_eq!(metadata.extensions()[1].value().as_str(), "42");
    assert!(
        GameMetadataExtension::new(
            "White".to_owned(),
            GameMetadataText::new("name".to_owned())?
        )
        .is_err()
    );
    assert!(
        GameMetadataExtension::new(
            "bad tag".to_owned(),
            GameMetadataText::new("value".to_owned())?
        )
        .is_err()
    );
    let repeated = GameMetadataExtension::new(
        "Source".to_owned(),
        GameMetadataText::new("offline".to_owned())?,
    )?;
    assert!(
        GameMetadata::new(
            GameMetadataKnownTags::default(),
            vec![repeated.clone(), repeated]
        )
        .is_err()
    );
    assert!(GameMetadataText::new("invalid\0value".to_owned()).is_err());
    Ok(())
}

#[test]
fn game_identity_and_revisions_keep_validated_external_encodings()
-> Result<(), Box<dyn std::error::Error>> {
    let id: GameId = "123e4567-e89b-42d3-a456-426614174000".parse()?;
    assert_eq!(
        serde_json::from_str::<GameId>(&serde_json::to_string(&id)?)?,
        id
    );
    assert!(
        "00000000-0000-0000-0000-000000000000"
            .parse::<GameId>()
            .is_err()
    );
    let maximum: GameRevision = u64::MAX.to_string().parse()?;
    assert!(maximum.next().is_err());
    assert_eq!(
        serde_json::from_str::<GameRevision>(&serde_json::to_string(&maximum)?)?,
        maximum
    );
    for invalid in ["01", "-1", "+1", "18446744073709551616"] {
        assert!(invalid.parse::<GameRevision>().is_err());
    }
    Ok(())
}

struct GameSessionUnresolvedAfterWhiteMove;

impl GameAdjudicationMateProofProvider for GameSessionUnresolvedAfterWhiteMove {
    fn prove(
        &self,
        position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        match position.turn() {
            BoardEnumColor::White => GameAdjudicationEnumMateProof::Possible,
            BoardEnumColor::Black => {
                GameAdjudicationEnumMateProof::Unknown(GameAdjudicationEnumInterruption::Unresolved)
            }
        }
    }
}

#[test]
fn unresolved_adjudication_preserves_the_accepted_position_and_interrupts_clocks()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameSessionFixture::session(ClockTimeControl::initial_default())?;
    GameSessionFixture::start(&mut session)?;
    let transition = session.accept_move(
        GameSessionFixture::request(&session, 2, "e2e4", 1_000)?,
        &GameSessionUnresolvedAfterWhiteMove,
    )?;
    assert_eq!(transition.kind(), GameEnumTransitionKind::MoveAccepted);
    assert_eq!(session.phase(), GameEnumPhase::Interrupted);
    assert_eq!(session.live(), GameSessionFixture::id(2)?);
    assert!(session.outcome().is_none());
    assert!(session.interruption().is_some());
    assert!(!session.clock().is_running());
    Ok(())
}

#[test]
fn a_native_deadline_event_finishes_without_waiting_for_a_submitted_move()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = GameSessionFixture::session(ClockTimeControl::sudden_death(
        ClockDuration::from_millis(1_000)?,
    )?)?;
    GameSessionFixture::start(&mut session)?;
    let revision = session.revision();
    let early = session.poll_deadline(
        revision,
        ClockInstant::from_millis(999)?,
        &GameSessionFixtureProof,
    )?;
    assert!(early.transition().is_none());
    assert_eq!(session.revision(), revision);
    let expired = session.poll_deadline(
        revision,
        ClockInstant::from_millis(1_000)?,
        &GameSessionFixtureProof,
    )?;
    assert_eq!(
        expired
            .transition()
            .ok_or("missing terminal transition")?
            .kind(),
        GameEnumTransitionKind::FlagFall
    );
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    assert_eq!(session.tree().len(), 1);
    Ok(())
}

#[test]
fn partial_dates_validate_calendar_days_and_opening_codes_are_typed()
-> Result<(), Box<dyn std::error::Error>> {
    use chess::game::session::{GameMetadataDate, GameMetadataOpeningCode};

    assert!(GameMetadataDate::new(Some(2024), Some(2), Some(29)).is_ok());
    assert!(GameMetadataDate::new(Some(2023), Some(2), Some(29)).is_err());
    assert!(GameMetadataDate::new(None, Some(2), Some(29)).is_ok());
    assert!(GameMetadataDate::new(Some(2024), Some(4), Some(31)).is_err());
    assert!(GameMetadataDate::new(Some(0), None, None).is_err());
    assert!(GameMetadataDate::new(None, Some(13), None).is_err());
    assert_eq!(
        GameMetadataOpeningCode::new("B20".to_owned())?.as_str(),
        "B20"
    );
    for invalid in ["F20", "b20", "B2", "B2x"] {
        assert!(GameMetadataOpeningCode::new(invalid.to_owned()).is_err());
    }
    Ok(())
}

#[test]
fn human_claims_stay_explicit_and_the_next_reckless_turn_claims_automatically()
-> Result<(), Box<dyn std::error::Error>> {
    use chess::board::enums::BoardEnumVariant;

    let mut session = GameSession::new(GameSessionOptions {
        id: "123e4567-e89b-42d3-a456-426614174000".parse()?,
        root: GameSessionFixture::id(1)?,
        start: BoardStart::custom(BoardPosition::from_fen(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 100 1",
            BoardEnumVariant::Standard,
        )?),
        seats: GameSeats::new(
            GameEnumMode::HumanVersusEngine,
            GameEnumSeat::Human,
            GameEnumSeat::Reckless,
        )?,
        clock: ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White),
        metadata: GameMetadata::default(),
    })?;
    GameSessionFixture::start(&mut session)?;
    assert_eq!(session.phase(), GameEnumPhase::Active);
    session.accept_move(
        GameSessionFixture::request(&session, 2, "g1f3", 1_000)?,
        &GameSessionFixtureProof,
    )?;
    assert_eq!(session.phase(), GameEnumPhase::Completed);
    assert_eq!(
        session.outcome().ok_or("missing automatic claim")?.reason(),
        GameEnumOutcomeReason::FiftyMove
    );
    assert_eq!(session.live(), GameSessionFixture::id(2)?);
    Ok(())
}
