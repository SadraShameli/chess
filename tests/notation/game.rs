use std::io::Cursor;

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::board::start::BoardStart;
use chess::clock::duration::{ClockDuration, ClockInstant};
use chess::clock::state::{ClockState, ClockStateEnumMoveOutcome};
use chess::clock::time_control::ClockTimeControl;
use chess::game::annotations::{GameAnnotationsEnumChange, GameAnnotationsEnumVariationRequest};
use chess::game::enums::{GameEnumMarkingColor, GameIdentityError, GameMoveId};
use chess::game::move_node::{GameMoveNodeMarking, GameMoveNodeNag};
use chess::game::tree::GameTreeError;
use chess::notation::game::{NotationGame, NotationGameError, NotationGameParts};
use chess::notation::pgn_export::{
    NotationPgnExport, NotationPgnExportEnumSelection, NotationPgnExportError,
};
use chess::notation::pgn_import::NotationPgnImport;

struct NotationGameStudyFixture;

impl NotationGameStudyFixture {
    fn id(value: u128) -> Result<GameMoveId, GameIdentityError> {
        uuid::Uuid::from_u128(value).to_string().parse()
    }

    fn import(pgn: &str) -> Result<NotationGame, Box<dyn std::error::Error>> {
        let mut id = 10;
        let mut importer = NotationPgnImport::new(Cursor::new(pgn.as_bytes()), || {
            id += 1;
            Self::id(id)
        });
        Ok(importer
            .next_game(&|| false)?
            .ok_or_else(|| std::io::Error::other("Missing fixed study fixture"))?)
    }

    fn game() -> Result<NotationGame, Box<dyn std::error::Error>> {
        Self::import(
            "[Event \"Preserve\"]\n[WhiteElo \" ? \"]\n[BlackElo \"999999999999999999999\"]\n[Source \"Original\"]\n[Result \"1/2-1/2\"]\n[TimeControl \"60+2\"]\n\n{Root} 1. e4 $1 {Main[%clk 0:00:57]} e5 1/2-1/2",
        )
    }

    fn export(
        game: &NotationGame,
        selection: NotationPgnExportEnumSelection,
    ) -> Result<Vec<u8>, NotationPgnExportError> {
        let mut bytes = Vec::new();
        NotationPgnExport::new(game)
            .with_selection(selection)
            .write(&mut bytes, || false)?;
        Ok(bytes)
    }

    fn snapshot(game: &NotationGame) -> String {
        format!("{game:?}")
    }
}

#[test]
fn legal_study_addition_returns_actual_node_and_effect_preserving_aggregate_content()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = NotationGameStudyFixture::game()?;
    let root = game.tree().root();
    let original = game.clone();
    let original_nodes: Vec<_> = original.tree().nodes().cloned().collect();
    let movement = game.tree().node(root)?.position().parse_uci("d2d4")?;
    let id = NotationGameStudyFixture::id(100)?;
    let applied = game.append_study_variation(root, id, movement)?;
    assert_eq!(applied.change(), GameAnnotationsEnumChange::Changed);
    assert_eq!(applied.view(), id);
    assert_eq!(applied.mainline_tip(), original.mainline_tip());
    assert_eq!(game.mainline_tip(), original.mainline_tip());
    assert_eq!(game.tree().len(), original.tree().len() + 1);
    let inserted = game.tree().node(id)?;
    assert_eq!(inserted.parent(), Some(root));
    assert_eq!(inserted.movement(), Some(movement));
    assert_eq!(inserted.san(), Some("d4"));
    assert_eq!(
        inserted.position(),
        &BoardPosition::standard().play(movement)?
    );
    assert!(inserted.clock().is_none());
    assert!(inserted.before_clock().is_none());
    assert_eq!(game.clock(id), None);
    for previous in original_nodes {
        let current = game.tree().node(previous.id())?;
        assert_eq!(current.id(), previous.id());
        assert_eq!(current.parent(), previous.parent());
        assert_eq!(current.movement(), previous.movement());
        assert_eq!(current.position(), previous.position());
        assert_eq!(current.annotations(), previous.annotations());
        assert_eq!(current.clock(), previous.clock());
        assert_eq!(current.before_clock(), previous.before_clock());
        if previous.id() != root {
            assert_eq!(current.children(), previous.children());
        }
    }
    assert_eq!(game.start(), original.start());
    assert_eq!(game.metadata(), original.metadata());
    assert_eq!(game.result(), original.result());
    assert_eq!(game.result_header(), original.result_header());
    assert_eq!(game.time_control(), original.time_control());
    assert_eq!(game.clocks(), original.clocks());
    assert_eq!(game.warnings(), original.warnings());
    assert_eq!(
        NotationGameStudyFixture::export(&game, NotationPgnExportEnumSelection::Mainline)?,
        NotationGameStudyFixture::export(&original, NotationPgnExportEnumSelection::Mainline)?
    );
    let full = NotationGameStudyFixture::export(&game, NotationPgnExportEnumSelection::Full)?;
    let restored = NotationGameStudyFixture::import(std::str::from_utf8(&full)?)?;
    assert_eq!(
        NotationGameStudyFixture::export(&restored, NotationPgnExportEnumSelection::Full)?,
        full
    );
    assert_eq!(restored.warnings(), game.warnings());
    Ok(())
}

#[test]
fn matching_study_continuation_returns_existing_identity_and_unchanged_effect()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = NotationGameStudyFixture::game()?;
    let root = game.tree().root();
    let e4 = game.tree().node(root)?.children()[0];
    let movement = game.tree().node(root)?.position().parse_uci("e2e4")?;
    let before = NotationGameStudyFixture::snapshot(&game);
    for proposed in [NotationGameStudyFixture::id(100)?, e4] {
        let applied = game.append_study_variation(root, proposed, movement)?;
        assert_eq!(applied.view(), e4);
        assert_eq!(applied.change(), GameAnnotationsEnumChange::Unchanged);
        assert_eq!(applied.mainline_tip(), game.mainline_tip());
        assert_eq!(NotationGameStudyFixture::snapshot(&game), before);
    }
    Ok(())
}

#[test]
fn study_continuation_reuses_real_node_clock_without_overwriting_annotations()
-> Result<(), Box<dyn std::error::Error>> {
    let mut original = NotationGameStudyFixture::game()?;
    let root = original.tree().root();
    let e4 = original.tree().node(root)?.children()[0];
    original.save_markings(
        e4,
        vec![GameMoveNodeMarking::circle(
            GameEnumMarkingColor::Green,
            "e4".parse()?,
        )],
    )?;
    original.save_notes_and_nags(
        e4,
        "Preserved played move".to_owned(),
        vec![GameMoveNodeNag::new(146)],
    )?;
    let mut tree = original.tree().clone();
    let mut clock = ClockState::new(ClockTimeControl::initial_default(), BoardEnumColor::White);
    clock.resume(ClockInstant::from_nanos(0))?;
    let ClockStateEnumMoveOutcome::Accepted(receipt) =
        clock.complete_move(ClockInstant::from_nanos(3_000_000_000))?
    else {
        return Err(std::io::Error::other("Expected timely fixture move").into());
    };
    let movement = tree.node(root)?.position().parse_uci("e2e4")?;
    assert_eq!(
        tree.append(root, e4, movement, Some(receipt.after().clone()))?,
        e4
    );
    let mut game = NotationGame::new(NotationGameParts {
        start: original.start().clone(),
        tree,
        mainline_tip: original.mainline_tip(),
        metadata: original.metadata().clone(),
        result: original.result(),
        result_header: original.result_header(),
        time_control: original.time_control().cloned(),
        clocks: original.clocks().to_vec(),
        warnings: original.warnings().to_vec(),
    })?;
    let before = NotationGameStudyFixture::snapshot(&game);
    let applied =
        game.append_study_variation(root, NotationGameStudyFixture::id(100)?, movement)?;
    assert_eq!(applied.view(), e4);
    assert_eq!(applied.change(), GameAnnotationsEnumChange::Unchanged);
    assert_eq!(game.tree().node(e4)?.clock(), Some(receipt.after()));
    assert_eq!(
        game.clock(e4),
        Some(ClockDuration::from_nanos(57_000_000_000))
    );
    assert_eq!(NotationGameStudyFixture::snapshot(&game), before);
    Ok(())
}

#[test]
fn study_addition_at_selected_tip_keeps_mainline_endpoint_and_full_export_refusal()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = NotationGameStudyFixture::game()?;
    let tip = game.mainline_tip();
    let before = NotationGameStudyFixture::export(&game, NotationPgnExportEnumSelection::Mainline)?;
    let movement = game.tree().node(tip)?.position().parse_uci("g1f3")?;
    let applied = game.append_study_variation(tip, NotationGameStudyFixture::id(100)?, movement)?;
    assert_eq!(applied.mainline_tip(), tip);
    assert_eq!(game.mainline_tip(), tip);
    assert_eq!(
        NotationGameStudyFixture::export(&game, NotationPgnExportEnumSelection::Mainline)?,
        before
    );
    assert!(
        matches!(NotationGameStudyFixture::export(&game, NotationPgnExportEnumSelection::Full), Err(NotationPgnExportError::MainlineHasContinuations { node }) if node == tip)
    );
    Ok(())
}

#[test]
fn illegal_wrong_turn_and_colliding_study_additions_are_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = NotationGameStudyFixture::game()?;
    let root = game.tree().root();
    let tip = game.mainline_tip();
    let e4 = game.tree().node(root)?.children()[0];
    let stale_e4 = BoardPosition::standard().parse_uci("e2e4")?;
    let black_move = game.tree().node(e4)?.position().parse_uci("c7c5")?;
    let legal_d4 = game.tree().node(root)?.position().parse_uci("d2d4")?;
    let before = NotationGameStudyFixture::snapshot(&game);
    for movement in [stale_e4, black_move] {
        assert!(matches!(
            game.append_study_variation(tip, NotationGameStudyFixture::id(100)?, movement),
            Err(NotationGameError::Tree(GameTreeError::Move(_)))
        ));
        assert_eq!(NotationGameStudyFixture::snapshot(&game), before);
    }
    assert!(matches!(
        game.append_study_variation(root, tip, legal_d4),
        Err(NotationGameError::Tree(GameTreeError::DuplicateIdentity))
    ));
    assert_eq!(NotationGameStudyFixture::snapshot(&game), before);
    Ok(())
}

#[test]
fn removed_study_parent_rejects_stale_continuation_without_reusing_deleted_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = NotationGameStudyFixture::game()?;
    let root = game.tree().root();
    let branch = NotationGameStudyFixture::id(100)?;
    let d4 = game.tree().node(root)?.position().parse_uci("d2d4")?;
    game.append_study_variation(root, branch, d4)?;
    let stale = game.tree().node(branch)?.position().parse_uci("d7d5")?;
    let confirmation = game
        .preview_variation(GameAnnotationsEnumVariationRequest::Delete { root: branch })?
        .confirm();
    game.apply_variation(confirmation, root)?;
    let before = NotationGameStudyFixture::snapshot(&game);
    assert!(matches!(
        game.append_study_variation(branch, NotationGameStudyFixture::id(101)?, stale),
        Err(NotationGameError::Tree(GameTreeError::UnknownNode))
    ));
    assert_eq!(NotationGameStudyFixture::snapshot(&game), before);
    Ok(())
}

#[test]
fn custom_and_chess960_study_continuations_use_actual_parent_legality()
-> Result<(), Box<dyn std::error::Error>> {
    for start in [
        BoardStart::custom(BoardPosition::from_fen(
            "4k3/8/8/8/8/8/4P3/4K3 w - - 0 7",
            chess::board::enums::BoardEnumVariant::Standard,
        )?),
        BoardStart::chess960(chess::board::start::BoardStartIndex::try_from(0)?)?,
    ] {
        let root = NotationGameStudyFixture::id(1)?;
        let mut game = NotationGame::new(NotationGameParts {
            tree: chess::game::tree::GameTree::new(root, start.position().clone(), None),
            mainline_tip: root,
            metadata: chess::game::session::GameMetadata::default(),
            result: chess::notation::game_identity::NotationGameIdentityEnumResult::Unfinished,
            result_header: None,
            time_control: None,
            clocks: vec![],
            warnings: vec![],
            start,
        })?;
        let movement = game.start().position().legal_moves()[0];
        let expected = game.start().position().play(movement)?;
        let applied =
            game.append_study_variation(root, NotationGameStudyFixture::id(2)?, movement)?;
        assert_eq!(applied.change(), GameAnnotationsEnumChange::Changed);
        assert_eq!(applied.mainline_tip(), root);
        assert_eq!(game.tree().node(applied.view())?.position(), &expected);
        assert!(game.tree().node(applied.view())?.clock().is_none());
    }
    Ok(())
}
