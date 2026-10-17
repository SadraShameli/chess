use std::io::Cursor;
use std::num::NonZeroUsize;
use std::sync::{Arc, mpsc};

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::game::adjudication::{
    GameAdjudicationEnumMateProof, GameAdjudicationEnumMateQuestion,
    GameAdjudicationMateProofProvider,
};
use chess::game::enums::{
    GameEnumEngineReadiness, GameEnumMarkingColor, GameEnumMode, GameEnumPhase, GameEnumSeat,
    GameId, GameMoveId,
};
use chess::game::move_node::{GameMoveNodeAnnotations, GameMoveNodeMarking, GameMoveNodeNag};
use chess::game::pause::GamePauseEnumCause;
use chess::game::session::{
    GameMetadata, GameMetadataDate, GameMetadataExtension, GameMetadataKnownTags,
    GameMetadataOpeningCode, GameMetadataText, GameSeats, GameSession, GameSessionEnumInterruption,
    GameSessionMoveRequest, GameSessionOptions, GameSessionRestoredMove,
};
use chess::game::storage_repository::{
    GameStorageRepository, GameStorageRevision, GameStorageSaveRequest,
};
use chess::game::takeback::GameTakebackEnumConsent;
use chess::game::tree::GameTree;
use chess::notation::game::{
    NotationGame, NotationGameClock, NotationGameEnumWarning, NotationGameParts,
};
use chess::notation::game_identity::NotationGameIdentityEnumResult;
use chess::notation::pgn_import::NotationPgnImport;
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::game_document::{
    StorageGameDocument, StorageGameDocumentError, StorageGameDocumentRepository,
};
use chess::storage::library_record::StorageLibraryEnumContent;

pub(crate) type StorageGameDocumentTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct StorageGameDocumentFixtureProof;

impl GameAdjudicationMateProofProvider for StorageGameDocumentFixtureProof {
    fn prove(
        &self,
        _position: &BoardPosition,
        _question: GameAdjudicationEnumMateQuestion,
    ) -> GameAdjudicationEnumMateProof {
        GameAdjudicationEnumMateProof::Possible
    }
}

pub(crate) struct StorageGameDocumentFixture;

impl StorageGameDocumentFixture {
    pub(crate) fn node(value: u128) -> StorageGameDocumentTestResult<GameMoveId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    pub(crate) fn session() -> StorageGameDocumentTestResult<GameSession> {
        let control = ClockTimeControl::bronstein_delay(
            ClockDuration::from_millis(60_000)?,
            ClockDuration::from_millis(2_000)?,
        )?;
        let known = GameMetadataKnownTags {
            white: Some(GameMetadataText::new("White player".to_owned())?),
            black: Some(GameMetadataText::new("Black player".to_owned())?),
            date: Some(GameMetadataDate::new(Some(2026), Some(10), Some(16))?),
            eco: Some(GameMetadataOpeningCode::new("C20".to_owned())?),
            ..GameMetadataKnownTags::default()
        };
        let mut session = GameSession::new(GameSessionOptions {
            id: "c1b5029c-f686-4f18-b45a-1af832163c24".parse()?,
            root: Self::node(1)?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::HumanVersusEngine,
                GameEnumSeat::Human,
                GameEnumSeat::Reckless,
            )?,
            clock: ClockState::new(control, BoardEnumColor::White),
            metadata: GameMetadata::new(
                known,
                vec![GameMetadataExtension::new(
                    "Study".to_owned(),
                    GameMetadataText::new("private offline study".to_owned())?,
                )?],
            )?,
        })?;
        session.start(
            session.revision(),
            ClockInstant::from_nanos(0),
            GameEnumEngineReadiness::Ready,
            &StorageGameDocumentFixtureProof,
        )?;
        let movement = session
            .tree()
            .node(session.live())?
            .position()
            .parse_uci("e2e4")?;
        session.accept_move(
            GameSessionMoveRequest {
                expected_revision: session.revision(),
                seat: GameEnumSeat::Human,
                color: BoardEnumColor::White,
                node: Self::node(2)?,
                movement,
                received_at: ClockInstant::from_millis(500)?,
            },
            &StorageGameDocumentFixtureProof,
        )?;
        Ok(session)
    }

    pub(crate) fn annotated() -> StorageGameDocumentTestResult<GameSession> {
        let session = Self::session()?;
        let mut snapshot = session.restoration(ClockInstant::from_millis(2_000)?)?;
        snapshot.root_annotations = GameMoveNodeAnnotations::new(
            "Root notes".to_owned(),
            vec![GameMoveNodeNag::new(10)],
            vec![GameMoveNodeMarking::circle(
                GameEnumMarkingColor::Blue,
                "d4".parse()?,
            )],
        )?;
        snapshot
            .moves
            .first_mut()
            .ok_or("missing accepted move")?
            .annotations = GameMoveNodeAnnotations::new(
            "Center control".to_owned(),
            vec![GameMoveNodeNag::new(1)],
            vec![GameMoveNodeMarking::arrow(
                GameEnumMarkingColor::Green,
                "e2".parse()?,
                "e4".parse()?,
            )?],
        )?;
        snapshot.moves.push(GameSessionRestoredMove {
            id: Self::node(3)?,
            parent: snapshot.session.root,
            movement: snapshot.session.start.position().parse_uci("d2d4")?,
            annotations: GameMoveNodeAnnotations::default(),
            before_clock: None,
            clock: None,
        });
        snapshot.view = snapshot.session.root;
        snapshot.warnings_used = [true, false];
        snapshot.takeback_consent = GameTakebackEnumConsent::NotRequired;
        snapshot.premoves_enabled = true;
        Ok(GameSession::restore(snapshot)?)
    }

    pub(crate) fn library_id() -> StorageGameDocumentTestResult<GameId> {
        Ok("e7734e83-6f41-4863-8f37-578ed3f2036b".parse()?)
    }

    pub(crate) fn notation() -> StorageGameDocumentTestResult<NotationGame> {
        let root = Self::node(101)?;
        let start = BoardStart::standard();
        let mut tree = GameTree::new(root, start.position().clone(), None);
        for (parent, id, uci) in [
            (101, 102, "e2e4"),
            (101, 104, "d2d4"),
            (104, 105, "d7d5"),
            (102, 103, "e7e5"),
        ] {
            let parent = Self::node(parent)?;
            let movement = tree.node(parent)?.position().parse_uci(uci)?;
            tree.append(parent, Self::node(id)?, movement, None)?;
        }
        tree.set_annotations(
            root,
            GameMoveNodeAnnotations::new(
                "Root <plain> notes".to_owned(),
                vec![GameMoveNodeNag::new(10)],
                Vec::new(),
            )?,
        )?;
        tree.set_annotations(
            Self::node(102)?,
            GameMoveNodeAnnotations::new(
                "[%unknown retained] \"quoted\"".to_owned(),
                vec![GameMoveNodeNag::new(1)],
                vec![GameMoveNodeMarking::arrow(
                    GameEnumMarkingColor::Green,
                    "e2".parse()?,
                    "e4".parse()?,
                )?],
            )?,
        )?;
        Ok(NotationGame::new(NotationGameParts {
            start,
            tree,
            mainline_tip: Self::node(105)?,
            metadata: GameMetadata::new(
                GameMetadataKnownTags {
                    white: Some(GameMetadataText::new("  Élodie  ".to_owned())?),
                    black: Some(GameMetadataText::new("Opponent".to_owned())?),
                    date: Some(GameMetadataDate::new(Some(2026), None, Some(16))?),
                    ..GameMetadataKnownTags::default()
                },
                vec![
                    GameMetadataExtension::new(
                        "SourceNote".to_owned(),
                        GameMetadataText::new("Ordered unknown tag".to_owned())?,
                    )?,
                    GameMetadataExtension::new(
                        "SecondTag".to_owned(),
                        GameMetadataText::new("second".to_owned())?,
                    )?,
                ],
            )?,
            result: NotationGameIdentityEnumResult::Completed(
                chess::game::enums::GameEnumResult::Draw,
            ),
            result_header: None,
            time_control: Some(GameMetadataText::new("40/7200:3600+30".to_owned())?),
            clocks: vec![NotationGameClock::new(
                Self::node(102)?,
                ClockDuration::from_nanos(3_723_123_456_789),
            )],
            warnings: vec![
                NotationGameEnumWarning::UnknownDirective {
                    node: Self::node(102)?,
                    directive: "[%unknown retained]".to_owned(),
                },
                NotationGameEnumWarning::UnavailableRating {
                    color: BoardEnumColor::White,
                    value: GameMetadataText::new("?".to_owned())?,
                },
            ],
        })?)
    }

    fn notation_json() -> StorageGameDocumentTestResult<String> {
        Ok(
            StorageGameDocument::capture_notation(Self::library_id()?, &Self::notation()?)?
                .json()?,
        )
    }

    fn interior_notation() -> StorageGameDocumentTestResult<NotationGame> {
        let original = Self::notation()?;
        Ok(NotationGame::new(NotationGameParts {
            start: original.start().clone(),
            tree: original.tree().clone(),
            mainline_tip: Self::node(102)?,
            metadata: original.metadata().clone(),
            result: original.result(),
            result_header: original.result_header(),
            time_control: original.time_control().cloned(),
            clocks: original.clocks().to_vec(),
            warnings: original.warnings().to_vec(),
        })?)
    }

    fn fixed_legacy_interior(version: u32) -> StorageGameDocumentTestResult<String> {
        Ok(Self::fixed_legacy(version)?
            .replacen(
                "\"live\":\"00000000-0000-0000-0000-000000000001\"",
                "\"live\":\"00000000-0000-0000-0000-000000000002\"",
                1,
            )
            .replacen("\"active\":\"white\"", "\"active\":\"black\"", 1)
            .replacen("\"completed_moves\":0", "\"completed_moves\":1", 1))
    }

    fn fixed_legacy(version: u32) -> StorageGameDocumentTestResult<String> {
        let options = match version {
            1 => "",
            2 => {
                ",\"play_options\":{\"untimed_move_time_nanos\":5000000000,\"move_overhead_nanos\":10000000,\"is_auto_flip_enabled\":false}"
            }
            _ => return Err("unsupported fixture version".into()),
        };
        Ok(format!(
            r#"{{"schema_version":{version},"game":{{"id":"c1b5029c-f686-4f18-b45a-1af832163c24","revision":"7","root":"00000000-0000-0000-0000-000000000001","start":{{"selection":{{"kind":"standard"}},"initial_fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"}},"mode":"local_two_player","white":"human","black":"human","time_control":[],"root_annotations":{{"notes":"legacy root","nags":[],"markings":[]}},"root_clock":null,"moves":[{{"id":"00000000-0000-0000-0000-000000000002","parent":"00000000-0000-0000-0000-000000000001","uci":"e2e4","annotations":{{"notes":"retained after takeback","nags":[],"markings":[]}},"before_clock":null,"clock":null}},{{"id":"00000000-0000-0000-0000-000000000003","parent":"00000000-0000-0000-0000-000000000002","uci":"e7e5","annotations":{{"notes":"legacy leaf","nags":[],"markings":[]}},"before_clock":null,"clock":null}},{{"id":"00000000-0000-0000-0000-000000000004","parent":"00000000-0000-0000-0000-000000000001","uci":"d2d4","annotations":{{"notes":"alternative","nags":[],"markings":[]}},"before_clock":null,"clock":null}}],"live":"00000000-0000-0000-0000-000000000001","view":"00000000-0000-0000-0000-000000000004","clock":{{"sides":[{{"remaining_nanos":0,"completed_moves":0,"stage_index":0,"stage_moves":0}},{{"remaining_nanos":0,"completed_moves":0,"stage_index":0,"stage_moves":0}}],"active":"white","elapsed_nanos":0,"status":"paused"}},"warnings_used":[false,false],"state":{{"analysis":null}},"metadata":{{"event":null,"site":null,"date":null,"round":null,"white":null,"black":null,"white_elo":null,"black_elo":null,"opening":null,"variation":null,"eco":null,"annotator":null,"extensions":[]}},"takeback_consent":"required","premoves_enabled":false{options}}}}}"#
        ))
    }
}

#[test]
fn schema_three_is_discriminated_and_preserves_explicit_played_study_live_and_view()
-> StorageGameDocumentTestResult {
    let session = StorageGameDocumentFixture::annotated()?;
    let mut snapshot = session.restoration(ClockInstant::from_millis(2_000)?)?;
    snapshot.study_mainline_tip = StorageGameDocumentFixture::node(3)?;
    let session = GameSession::restore(snapshot)?;
    let json = StorageGameDocument::capture(&session, ClockInstant::from_millis(2_000)?)?.json()?;
    assert!(json.starts_with("{\"schema_version\":3,\"kind\":\"played\",\"game\":"));
    let recovered = StorageGameDocument::restore(&json)?;
    assert_eq!(
        recovered.study_mainline_tip(),
        StorageGameDocumentFixture::node(3)?
    );
    assert_eq!(recovered.live(), StorageGameDocumentFixture::node(2)?);
    assert_eq!(recovered.view(), recovered.tree().root());
    assert_eq!(recovered.live_moves()?, session.live_moves()?);
    assert!(matches!(
        StorageGameDocument::recover_record(&json)?.into_content(),
        StorageLibraryEnumContent::Played(_)
    ));
    Ok(())
}

#[test]
fn actual_legacy_shapes_derive_the_first_ordered_leaf_without_extending_played_history()
-> StorageGameDocumentTestResult {
    for version in [1, 2] {
        let json = StorageGameDocumentFixture::fixed_legacy(version)?;
        let original = json.clone();
        let recovered = StorageGameDocument::restore(&json)?;
        assert_eq!(json, original);
        assert_eq!(recovered.revision().value(), 7);
        assert_eq!(recovered.live(), recovered.tree().root());
        assert!(recovered.live_moves()?.is_empty());
        assert_eq!(
            recovered.study_mainline_tip(),
            StorageGameDocumentFixture::node(3)?
        );
        assert_eq!(recovered.view(), StorageGameDocumentFixture::node(4)?);
        assert_eq!(
            recovered
                .tree()
                .node(StorageGameDocumentFixture::node(2)?)?
                .annotations()
                .notes(),
            "retained after takeback"
        );
        assert!(recovered.outcome().is_none());
        assert!(!recovered.clock().is_running());
        let latest =
            StorageGameDocument::capture(&recovered, ClockInstant::from_nanos(0))?.json()?;
        assert!(latest.starts_with("{\"schema_version\":3,\"kind\":\"played\","));
        assert_eq!(
            StorageGameDocument::restore(&latest)?.study_mainline_tip(),
            recovered.study_mainline_tip()
        );
    }
    Ok(())
}

#[test]
fn legacy_takeback_at_an_interior_live_node_keeps_the_retained_study_line_out_of_played_identity()
-> StorageGameDocumentTestResult {
    for version in [1, 2] {
        let json = StorageGameDocumentFixture::fixed_legacy_interior(version)?;
        let record = StorageGameDocument::recover_record(&json)?;
        assert_eq!(
            record.identity()?.structural_key().mainline_uci(),
            &["e2e4"]
        );
        assert_eq!(record.identity_line_len()?, 1);
        let StorageLibraryEnumContent::Played(session) = record.into_content() else {
            return Err("legacy became notation".into());
        };
        assert_eq!(session.live(), StorageGameDocumentFixture::node(2)?);
        assert_eq!(
            session.study_mainline_tip(),
            StorageGameDocumentFixture::node(3)?
        );
        assert_eq!(session.view(), StorageGameDocumentFixture::node(4)?);
        assert_eq!(
            session
                .clock()
                .snapshot(ClockInstant::from_nanos(0))?
                .completed_moves(BoardEnumColor::White),
            1
        );
        assert_eq!(session.clock().active(), BoardEnumColor::Black);
        assert!(session.outcome().is_none());
        assert_eq!(session.revision().value(), 7);
    }
    Ok(())
}

#[test]
fn schema_kind_combinations_and_unknown_envelope_fields_are_refused()
-> StorageGameDocumentTestResult {
    let notation = StorageGameDocumentFixture::notation_json()?;
    for invalid in [
        notation.replacen(",\"kind\":\"notation\"", "", 1),
        notation.replacen("\"kind\":\"notation\"", "\"kind\":null", 1),
        notation.replacen("\"notation\"", "\"other\"", 1),
        notation.replacen("\"notation\"", "\"played\"", 1),
        notation.replacen("\"schema_version\":3", "\"schema_version\":2", 1),
        notation.replacen("\"game\":", "\"unexpected\":true,\"game\":", 1),
    ] {
        assert!(StorageGameDocument::recover_record(&invalid).is_err());
    }
    for version in [1, 2] {
        let legacy = StorageGameDocumentFixture::fixed_legacy(version)?;
        for kind in ["null", "\"played\""] {
            let invalid = legacy.replacen("\"game\":", &format!("\"kind\":{kind},\"game\":"), 1);
            assert!(StorageGameDocument::recover_record(&invalid).is_err());
        }
    }
    assert!(matches!(
        StorageGameDocument::recover_record(r#"{"schema_version":0,"game":{}}"#),
        Err(StorageGameDocumentError::UnsupportedSchema(0))
    ));
    Ok(())
}

#[test]
fn clockless_notation_roundtrip_preserves_tree_source_extras_and_external_identity()
-> StorageGameDocumentTestResult {
    let notation = StorageGameDocumentFixture::notation()?;
    let json = StorageGameDocument::capture_notation(
        StorageGameDocumentFixture::library_id()?,
        &notation,
    )?
    .json()?;
    assert!(json.starts_with("{\"schema_version\":3,\"kind\":\"notation\",\"game\":"));
    assert!(!json.contains("\"white\":\"human\""));
    assert!(!json.contains("\"state\":"));
    assert!(!json.contains("\"completed_moves\":"));
    assert!(matches!(
        StorageGameDocument::restore(&json),
        Err(StorageGameDocumentError::ContentKind { .. })
    ));
    let record = StorageGameDocument::recover_record(&json)?;
    assert_eq!(record.id(), StorageGameDocumentFixture::library_id()?);
    assert_eq!(record.mode(), None);
    let StorageLibraryEnumContent::Notation(recovered) = record.into_content() else {
        return Err("notation was converted to played content".into());
    };
    assert_eq!(recovered.metadata(), notation.metadata());
    assert_eq!(recovered.start(), notation.start());
    assert_eq!(recovered.result(), notation.result());
    assert_eq!(recovered.result_header(), None);
    assert_eq!(recovered.time_control(), notation.time_control());
    assert_eq!(recovered.clocks(), notation.clocks());
    assert_eq!(recovered.warnings(), notation.warnings());
    assert_eq!(recovered.mainline_tip(), notation.mainline_tip());
    assert_eq!(recovered.tree().len(), notation.tree().len());
    assert_eq!(
        recovered
            .tree()
            .nodes()
            .map(|node| node.id())
            .collect::<Vec<_>>(),
        notation
            .tree()
            .nodes()
            .map(|node| node.id())
            .collect::<Vec<_>>()
    );
    for expected in notation.tree().nodes() {
        let actual = recovered.tree().node(expected.id())?;
        assert_eq!(actual.parent(), expected.parent());
        assert_eq!(actual.children(), expected.children());
        assert_eq!(actual.movement(), expected.movement());
        assert_eq!(actual.position(), expected.position());
        assert_eq!(actual.annotations(), expected.annotations());
        assert!(actual.clock().is_none());
        assert!(actual.before_clock().is_none());
    }
    Ok(())
}

#[test]
fn imported_pgn_is_persisted_without_a_runtime_or_result_header_invention()
-> StorageGameDocumentTestResult {
    let pgn = "[WhiteElo \"?\"]\n[TimeControl \"unavailable source\"]\n[Custom \"ordered\"]\n\n{root} 1. e4 {[%clk 1:02:03.123456789] [%unknown yes]} (1. d4 d5) e5 1/2-1/2\n";
    let mut next = 200;
    let mut importer = NotationPgnImport::new(Cursor::new(pgn.as_bytes()), || {
        next += 1;
        uuid::Uuid::from_u128(next).to_string().parse()
    });
    let game = importer
        .next_game(&|| false)?
        .ok_or("missing imported game")?;
    let json =
        StorageGameDocument::capture_notation(StorageGameDocumentFixture::library_id()?, &game)?
            .json()?;
    let record = StorageGameDocument::recover_record(&json)?;
    let StorageLibraryEnumContent::Notation(actual) = record.into_content() else {
        return Err("expected imported notation".into());
    };
    assert_eq!(actual.result_header(), None);
    assert_eq!(actual.result(), game.result());
    assert_eq!(actual.warnings(), game.warnings());
    assert_eq!(actual.clocks(), game.clocks());
    assert_eq!(actual.metadata(), game.metadata());
    Ok(())
}

#[test]
fn notation_corruption_is_rejected_before_a_library_record_is_returned()
-> StorageGameDocumentTestResult {
    let json = StorageGameDocumentFixture::notation_json()?;
    let id = |value| StorageGameDocumentFixture::node(value).map(|id| id.as_uuid().to_string());
    for invalid in [
        json.replace(
            &format!("\"id\":\"{}\"", id(103)?),
            &format!("\"id\":\"{}\"", id(102)?),
        ),
        json.replace("\"uci\":\"d2d4\"", "\"uci\":\"e2e4\""),
        json.replace(
            &format!("\"parent\":\"{}\"", id(104)?),
            &format!("\"parent\":\"{}\"", id(105)?),
        ),
        json.replace(
            &format!("\"mainline_tip\":\"{}\"", id(105)?),
            &format!("\"mainline_tip\":\"{}\"", id(999)?),
        ),
        json.replace(
            &format!("\"node\":\"{}\"", id(102)?),
            &format!("\"node\":\"{}\"", id(999)?),
        ),
        json.replacen(
            "\"result_header\":null",
            "\"result_header\":\"unfinished\"",
            1,
        ),
        json.replacen(
            "\"root_annotations\":",
            "\"seats\":[],\"root_annotations\":",
            1,
        ),
    ] {
        assert_ne!(invalid, json);
        assert!(StorageGameDocument::recover_record(&invalid).is_err());
    }
    Ok(())
}

#[test]
fn notation_notes_metadata_warning_and_body_limits_are_enforced() -> StorageGameDocumentTestResult {
    let json = StorageGameDocumentFixture::notation_json()?;
    let oversized = "x".repeat(1024 * 1024 + 1);
    for original in [
        "Root <plain> notes",
        "Ordered unknown tag",
        "[%unknown retained]",
    ] {
        let invalid = json.replacen(original, &oversized, 1);
        assert_ne!(invalid, json);
        assert!(StorageGameDocument::recover_record(&invalid).is_err());
    }
    let oversized_body = " ".repeat(StorageDocumentStore::MEMORY_BUDGET + 1);
    assert!(matches!(
        StorageGameDocument::recover_record(&oversized_body),
        Err(StorageGameDocumentError::TooLarge)
    ));
    Ok(())
}

#[derive(serde::Deserialize)]
struct StorageGameDocumentFixtureEnvelope<'a> {
    #[serde(borrow)]
    game: StorageGameDocumentFixtureMoves<'a>,
}

#[derive(serde::Deserialize)]
struct StorageGameDocumentFixtureMoves<'a> {
    #[serde(borrow)]
    moves: Vec<&'a serde_json::value::RawValue>,
}

#[derive(serde::Deserialize)]
struct StorageGameDocumentFixtureRuntimeClock<'a> {
    #[serde(borrow)]
    clock: &'a serde_json::value::RawValue,
}

#[test]
fn notation_refuses_injected_runtime_snapshots_instead_of_adopting_played_clocks()
-> StorageGameDocumentTestResult {
    let session = StorageGameDocumentFixture::session()?;
    let played =
        StorageGameDocument::capture(&session, ClockInstant::from_millis(2_000)?)?.json()?;
    let envelope: StorageGameDocumentFixtureEnvelope<'_> = serde_json::from_str(&played)?;
    let node = envelope.game.moves.first().ok_or("missing played move")?;
    let runtime: StorageGameDocumentFixtureRuntimeClock<'_> = serde_json::from_str(node.get())?;
    let notation = StorageGameDocumentFixture::notation_json()?;
    let invalid = notation.replacen(
        "\"uci\":\"e2e4\"",
        &format!("\"clock\":{},\"uci\":\"e2e4\"", runtime.clock.get()),
        1,
    );
    assert_ne!(invalid, notation);
    assert!(matches!(
        StorageGameDocument::recover_record(&invalid),
        Err(StorageGameDocumentError::NotationRuntimeClock)
    ));
    Ok(())
}

#[test]
fn notation_explicit_result_header_and_declared_start_survive_the_same_codec()
-> StorageGameDocumentTestResult {
    let id = StorageGameDocumentFixture::library_id()?;
    for start in [
        BoardStart::standard(),
        BoardStart::chess960(0_u16.try_into()?)?,
        BoardStart::dfrc(0_u16.try_into()?, 959_u16.try_into()?)?,
    ] {
        let root = StorageGameDocumentFixture::node(500)?;
        let game = NotationGame::new(NotationGameParts {
            tree: GameTree::new(root, start.position().clone(), None),
            start,
            mainline_tip: root,
            metadata: GameMetadata::default(),
            result: NotationGameIdentityEnumResult::Unfinished,
            result_header: Some(NotationGameIdentityEnumResult::Unfinished),
            time_control: None,
            clocks: Vec::new(),
            warnings: Vec::new(),
        })?;
        let json = StorageGameDocument::capture_notation(id, &game)?.json()?;
        let record = StorageGameDocument::recover_record(&json)?;
        assert_eq!(record.start(), game.start());
        let StorageLibraryEnumContent::Notation(restored) = record.into_content() else {
            return Err("expected notation".into());
        };
        assert_eq!(restored.result_header(), game.result_header());
    }
    Ok(())
}

#[test]
fn node_limit_and_parent_before_child_order_are_checked_by_the_canonical_decoder()
-> StorageGameDocumentTestResult {
    let json = StorageGameDocumentFixture::notation_json()?;
    let envelope: StorageGameDocumentFixtureEnvelope<'_> = serde_json::from_str(&json)?;
    let moves = envelope.game.moves;
    let original = format!(
        "[{}]",
        moves
            .iter()
            .map(|node| node.get())
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut reordered = moves.iter().map(|node| node.get()).collect::<Vec<_>>();
    reordered.swap(0, 3);
    let invalid_order = json.replacen(&original, &format!("[{}]", reordered.join(",")), 1);
    assert_ne!(invalid_order, json);
    assert!(StorageGameDocument::recover_record(&invalid_order).is_err());
    let first = moves.first().ok_or("missing notation move")?.get();
    let oversized = format!(
        "[{}]",
        std::iter::repeat_n(first, StorageGameDocument::MAX_MOVES + 1)
            .collect::<Vec<_>>()
            .join(",")
    );
    let invalid_count = json.replacen(&original, &oversized, 1);
    assert_ne!(invalid_count, json);
    let error = StorageGameDocument::recover_record(&invalid_count)
        .err()
        .ok_or("oversized move list was accepted")?;
    assert!(error.to_string().contains("100000"));
    Ok(())
}

#[test]
fn capture_applies_the_same_text_limit_to_played_and_notation_notes()
-> StorageGameDocumentTestResult {
    let mut notation = StorageGameDocumentFixture::notation()?;
    let notes = "x".repeat(1024 * 1024 + 1);
    let annotations = GameMoveNodeAnnotations::new(notes, Vec::new(), Vec::new())?;
    let mut tree = notation.tree().clone();
    tree.set_annotations(tree.root(), annotations.clone())?;
    notation = NotationGame::new(NotationGameParts {
        start: notation.start().clone(),
        tree,
        mainline_tip: notation.mainline_tip(),
        metadata: notation.metadata().clone(),
        result: notation.result(),
        result_header: notation.result_header(),
        time_control: notation.time_control().cloned(),
        clocks: notation.clocks().to_vec(),
        warnings: notation.warnings().to_vec(),
    })?;
    assert!(matches!(
        StorageGameDocument::capture_notation(StorageGameDocumentFixture::library_id()?, &notation),
        Err(StorageGameDocumentError::TextTooLarge)
    ));
    let mut snapshot =
        StorageGameDocumentFixture::session()?.restoration(ClockInstant::from_millis(2_000)?)?;
    snapshot.root_annotations = annotations;
    let played = GameSession::restore(snapshot)?;
    assert!(matches!(
        StorageGameDocument::capture(&played, ClockInstant::from_millis(2_000)?),
        Err(StorageGameDocumentError::TextTooLarge)
    ));
    Ok(())
}

#[test]
fn played_tip_presence_relationship_and_version_are_revalidated() -> StorageGameDocumentTestResult {
    let session = StorageGameDocumentFixture::session()?;
    let json = StorageGameDocument::capture(&session, ClockInstant::from_millis(2_000)?)?.json()?;
    let tip = format!(
        ",\"study_mainline_tip\":\"{}\"",
        session.study_mainline_tip().as_uuid()
    );
    assert!(json.contains(&tip));
    assert!(matches!(
        StorageGameDocument::restore(&json.replacen(&tip, "", 1)),
        Err(StorageGameDocumentError::MissingStudyTip)
    ));
    for replacement in [
        session.tree().root(),
        StorageGameDocumentFixture::node(999)?,
    ] {
        let invalid = json.replacen(
            &tip,
            &format!(",\"study_mainline_tip\":\"{}\"", replacement.as_uuid()),
            1,
        );
        assert!(StorageGameDocument::restore(&invalid).is_err());
    }
    for version in [1, 2] {
        let invalid = StorageGameDocumentFixture::fixed_legacy(version)?.replacen(
            "\"live\":",
            &format!(
                "\"study_mainline_tip\":\"{}\",\"live\":",
                session.study_mainline_tip().as_uuid()
            ),
            1,
        );
        assert!(matches!(
            StorageGameDocument::restore(&invalid),
            Err(StorageGameDocumentError::LegacyStudyTip)
        ));
    }
    let future = r#"{"schema_version":4,"kind":"future-content","game":{"invalid":true}}"#;
    assert!(matches!(
        StorageGameDocument::recover_record(future),
        Err(StorageGameDocumentError::FutureSchema {
            found: 4,
            supported: 3
        })
    ));
    Ok(())
}

#[tokio::test]
async fn legacy_read_preserves_the_original_database_body_identity_and_revision()
-> StorageGameDocumentTestResult {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("invalid fixture capacity")?)?;
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget.clone())
            .await?;
    let executor = CapacityExecutor::new(budget)?;
    let repository = StorageGameDocumentRepository::new(store.client(), &executor);
    for version in [1, 2] {
        let json = StorageGameDocumentFixture::fixed_legacy(version)?;
        let game = StorageGameDocument::recover_record(&json)?.id();
        let saved = store
            .client()
            .save(game.as_uuid(), u64::from(version - 1), json.clone())
            .await?;
        let recovered = repository
            .load(game)
            .await?
            .ok_or("missing legacy record")?;
        assert_eq!(recovered.storage_revision().value(), saved.revision());
        assert_eq!(recovered.session().id(), game);
        let after = store
            .client()
            .read(game.as_uuid())
            .await?
            .ok_or("missing stored body")?;
        assert_eq!(after.body(), json);
        assert_eq!(after.revision(), saved.revision());
    }
    store.close().await?;
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn cancellation_refuses_codec_work_and_the_owned_executor_job_is_joined()
-> StorageGameDocumentTestResult {
    let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("invalid fixture capacity")?)?;
    let executor = CapacityExecutor::new(budget)?;
    let ready = Arc::new(tokio::sync::Notify::new());
    let worker_ready = Arc::clone(&ready);
    let (release_sender, release_receiver) = mpsc::channel();
    let game = StorageGameDocumentFixture::notation()?;
    let game_id = StorageGameDocumentFixture::library_id()?;
    let json = StorageGameDocumentFixture::notation_json()?;
    let document = StorageGameDocument::capture_notation(game_id, &game)?;
    let job = executor
        .submit_waiting(move |cancellation| {
            worker_ready.notify_one();
            let released = release_receiver.recv();
            (
                released,
                matches!(
                    StorageGameDocument::capture_notation_cancellable(game_id, &game, cancellation),
                    Err(StorageGameDocumentError::Cancelled)
                ),
                matches!(
                    StorageGameDocument::recover_record_cancellable(&json, cancellation),
                    Err(StorageGameDocumentError::Cancelled)
                ),
                matches!(
                    document.json_cancellable(cancellation),
                    Err(StorageGameDocumentError::Cancelled)
                ),
            )
        })
        .await?;
    ready.notified().await;
    job.cancel();
    release_sender.send(())?;
    let (released, capture, recover, encode) = job.join().await?;
    released?;
    assert!(capture && recover && encode);
    executor.shutdown().await?;
    Ok(())
}

#[test]
fn accepted_notation_interior_tip_can_be_wrapped() -> StorageGameDocumentTestResult {
    let game = StorageGameDocumentFixture::interior_notation()?;
    assert!(!game.tree().node(game.mainline_tip())?.children().is_empty());
    let record = chess::storage::library_record::StorageLibraryRecord::new(
        StorageGameDocumentFixture::library_id()?,
        StorageLibraryEnumContent::Notation(Box::new(game)),
    )?;
    assert_eq!(record.identity_line_len()?, 1);
    assert_eq!(
        record.identity()?.structural_key().mainline_uci(),
        &["e2e4"]
    );
    Ok(())
}

#[test]
fn accepted_notation_interior_tip_can_be_captured() -> StorageGameDocumentTestResult {
    let game = StorageGameDocumentFixture::interior_notation()?;
    let document =
        StorageGameDocument::capture_notation(StorageGameDocumentFixture::library_id()?, &game)?;
    let recovered = StorageGameDocument::recover_record(&document.json()?)?;
    let StorageLibraryEnumContent::Notation(actual) = recovered.into_content() else {
        return Err("expected notation content".into());
    };
    assert_eq!(actual.mainline_tip(), game.mainline_tip());
    assert_eq!(actual.tree().len(), game.tree().len());
    assert_eq!(actual.clocks(), game.clocks());
    assert_eq!(actual.warnings(), game.warnings());
    Ok(())
}

#[test]
fn accepted_notation_interior_tip_can_be_recovered() -> StorageGameDocumentTestResult {
    let json = StorageGameDocumentFixture::notation_json()?.replace(
        &format!(
            "\"mainline_tip\":\"{}\"",
            StorageGameDocumentFixture::node(105)?.as_uuid()
        ),
        &format!(
            "\"mainline_tip\":\"{}\"",
            StorageGameDocumentFixture::node(102)?.as_uuid()
        ),
    );
    let record = StorageGameDocument::recover_record(&json)?;
    assert_eq!(record.identity_line_len()?, 1);
    let StorageLibraryEnumContent::Notation(actual) = record.into_content() else {
        return Err("expected notation content".into());
    };
    assert_eq!(
        actual.mainline_tip(),
        StorageGameDocumentFixture::node(102)?
    );
    assert!(
        !actual
            .tree()
            .node(actual.mainline_tip())?
            .children()
            .is_empty()
    );
    Ok(())
}

#[test]
fn future_schema_is_rejected_before_parsing_the_game_payload() -> StorageGameDocumentTestResult {
    let json = format!(
        "{{\"schema_version\":{},\"game\":{{\"moves\":42,\"clock\":\"unsupported\"}}}}",
        StorageGameDocument::SCHEMA_VERSION + 1
    );
    assert!(matches!(
        StorageGameDocument::restore(&json),
        Err(StorageGameDocumentError::FutureSchema { .. })
    ));
    Ok(())
}

#[test]
fn running_recovery_is_paused_without_charging_downtime() -> StorageGameDocumentTestResult {
    let session = StorageGameDocumentFixture::session()?;
    let original = session
        .clock()
        .snapshot(ClockInstant::from_millis(2_000)?)?;
    let document = StorageGameDocument::capture(&session, ClockInstant::from_millis(2_000)?)?;
    let recovered = StorageGameDocument::restore(&document.json()?)?;
    assert_eq!(recovered.id(), session.id());
    assert_eq!(recovered.revision(), session.revision());
    assert_eq!(recovered.live(), session.live());
    assert_eq!(recovered.phase(), GameEnumPhase::Interrupted);
    assert_eq!(
        recovered.interruption(),
        Some(GameSessionEnumInterruption::Paused(
            GamePauseEnumCause::Relaunch
        ))
    );
    assert!(!recovered.clock().is_running());
    let later = recovered
        .clock()
        .snapshot(ClockInstant::from_millis(999_000)?)?;
    assert_eq!(later.elapsed(), original.elapsed());
    assert_eq!(later.active(), original.active());
    assert_eq!(later.persisted().sides, original.persisted().sides);
    assert_eq!(later.control(), original.control());
    Ok(())
}

#[test]
fn complete_snapshot_preserves_ordered_variations_annotations_metadata_and_warning_flags()
-> StorageGameDocumentTestResult {
    let session = StorageGameDocumentFixture::annotated()?;
    let root = session.tree().root();
    let document = StorageGameDocument::capture(&session, ClockInstant::from_millis(2_000)?)?;
    let recovered = StorageGameDocument::restore(&document.json()?)?;
    assert_eq!(recovered.tree().len(), 3);
    assert_eq!(recovered.tree().root(), root);
    assert_eq!(recovered.live(), session.live());
    assert_eq!(recovered.view(), root);
    assert_eq!(recovered.start_position(), session.start_position());
    assert_eq!(recovered.seats(), session.seats());
    assert_eq!(recovered.metadata(), session.metadata());
    assert_eq!(
        recovered.tree().node(root)?.children(),
        session.tree().node(root)?.children()
    );
    for id in recovered.tree().path_to(recovered.live())? {
        let actual = recovered.tree().node(id)?;
        let expected = session.tree().node(id)?;
        assert_eq!(actual.annotations(), expected.annotations());
        assert_eq!(actual.before_clock(), expected.before_clock());
        assert_eq!(actual.clock(), expected.clock());
        assert_eq!(actual.position(), expected.position());
    }
    assert!(recovered.clock().warning_used(BoardEnumColor::White));
    assert!(!recovered.clock().warning_used(BoardEnumColor::Black));
    assert_eq!(
        recovered.takeback().consent(),
        GameTakebackEnumConsent::NotRequired
    );
    assert_eq!(recovered.takeback().pending(), None);
    assert!(recovered.premove().is_enabled());
    assert_eq!(recovered.premove().pending(), None);
    Ok(())
}

#[test]
fn corrupted_moves_and_noncanonical_identity_never_become_a_recovered_game()
-> StorageGameDocumentTestResult {
    let session = StorageGameDocumentFixture::session()?;
    let json = StorageGameDocument::capture(&session, ClockInstant::from_millis(2_000)?)?.json()?;
    assert!(json.contains("e2e4"));
    assert!(StorageGameDocument::restore(&json.replace("e2e4", "e2e5")).is_err());
    assert!(
        StorageGameDocument::restore(
            &json.replace(&session.id().as_uuid().to_string(), "INVALID_GAME_ID")
        )
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn autosave_commits_one_complete_revision_and_conflict_preserves_it()
-> StorageGameDocumentTestResult {
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("invalid fixture capacity")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget).await?;
    let session = StorageGameDocumentFixture::annotated()?;
    let receipt = {
        let repository = StorageGameDocumentRepository::new(store.client(), &executor);
        let request = GameStorageSaveRequest::capture(
            &session,
            ClockInstant::from_millis(2_000)?,
            GameStorageRevision::INITIAL,
        )?;
        let receipt = repository.save(request).await?;
        assert_eq!(receipt.game(), session.id());
        assert_eq!(receipt.game_revision(), session.revision());
        assert_eq!(receipt.storage_revision().value(), 1);
        let stale = GameStorageSaveRequest::capture(
            &session,
            ClockInstant::from_millis(2_000)?,
            GameStorageRevision::INITIAL,
        )?;
        assert!(repository.save(stale).await.is_err());
        let saved = repository
            .load(session.id())
            .await?
            .ok_or("missing game document")?;
        assert_eq!(saved.storage_revision(), receipt.storage_revision());
        assert_eq!(saved.session().revision(), session.revision());
        assert_eq!(saved.session().live(), session.live());
        assert_eq!(saved.session().tree().len(), session.tree().len());
        receipt
    };
    store.close().await?;
    executor.shutdown().await?;
    let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("invalid fixture capacity")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget).await?;
    {
        let repository = StorageGameDocumentRepository::new(store.client(), &executor);
        let recovered = repository
            .load(session.id())
            .await?
            .ok_or("missing durable game")?;
        assert_eq!(recovered.storage_revision(), receipt.storage_revision());
        assert_eq!(recovered.session().metadata(), session.metadata());
        assert_eq!(recovered.session().phase(), GameEnumPhase::Interrupted);
        assert!(!recovered.session().clock().is_running());
    }
    store.close().await?;
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn a_document_under_the_wrong_database_identity_is_rejected() -> StorageGameDocumentTestResult
{
    let directory = tempfile::tempdir()?;
    let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("invalid fixture capacity")?)?;
    let executor = CapacityExecutor::new(budget.clone())?;
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget).await?;
    let session = StorageGameDocumentFixture::session()?;
    let wrong: chess::game::enums::GameId = "0f6c64aa-4b6a-479f-92bb-7cbb29a73633".parse()?;
    let json = StorageGameDocument::capture(&session, ClockInstant::from_millis(2_000)?)?.json()?;
    store.client().save(wrong.as_uuid(), 0, json).await?;
    {
        let repository = StorageGameDocumentRepository::new(store.client(), &executor);
        assert!(repository.load(wrong).await.is_err());
        assert!(repository.load(session.id()).await?.is_none());
    }
    store.close().await?;
    executor.shutdown().await?;
    Ok(())
}
