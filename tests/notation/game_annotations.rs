use std::io::Cursor;

use chess::board::enums::BoardEnumColor;
use chess::board::start::BoardStart;
use chess::clock::duration::ClockDuration;
use chess::game::annotations::{GameAnnotationsEnumChange, GameAnnotationsEnumVariationRequest};
use chess::game::enums::{GameEnumMarkingColor, GameEnumResult, GameIdentityError, GameMoveId};
use chess::game::move_node::{GameMoveNodeMarking, GameMoveNodeNag};
use chess::game::session::{
    GameMetadata, GameMetadataExtension, GameMetadataKnownTags, GameMetadataText,
};
use chess::game::tree::{GameTree, GameTreeError};
use chess::notation::game::{
    NotationGame, NotationGameClock, NotationGameEnumWarning, NotationGameError, NotationGameParts,
};
use chess::notation::game_identity::NotationGameIdentityEnumResult;
use chess::notation::pgn_export::{
    NotationPgnExport, NotationPgnExportEnumSelection, NotationPgnExportError,
};
use chess::notation::pgn_import::NotationPgnImport;

struct NotationGameAnnotationsFixture;

impl NotationGameAnnotationsFixture {
    fn id(value: u128) -> Result<GameMoveId, GameIdentityError> {
        uuid::Uuid::from_u128(value).to_string().parse()
    }

    fn append(
        tree: &mut GameTree,
        parent: u128,
        id: u128,
        uci: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let parent = Self::id(parent)?;
        let movement = tree.node(parent)?.position().parse_uci(uci)?;
        tree.append(parent, Self::id(id)?, movement, None)?;
        Ok(())
    }

    fn game() -> Result<NotationGame, Box<dyn std::error::Error>> {
        let start = BoardStart::standard();
        let mut tree = GameTree::new(Self::id(1)?, start.position().clone(), None);
        for (parent, id, movement) in [
            (1, 2, "e2e4"),
            (2, 3, "e7e5"),
            (3, 4, "g1f3"),
            (1, 5, "d2d4"),
            (5, 6, "d7d5"),
            (6, 7, "b1c3"),
            (1, 8, "c2c4"),
            (5, 9, "g8f6"),
        ] {
            Self::append(&mut tree, parent, id, movement)?;
        }
        let remaining = ClockDuration::from_millis(50_000)?;
        Ok(NotationGame::new(NotationGameParts {
            start,
            tree,
            mainline_tip: Self::id(4)?,
            metadata: GameMetadata::new(
                GameMetadataKnownTags::default(),
                vec![GameMetadataExtension::new(
                    "Source".to_owned(),
                    GameMetadataText::new("Preserved".to_owned())?,
                )?],
            )?,
            result: NotationGameIdentityEnumResult::Completed(GameEnumResult::Draw),
            result_header: Some(NotationGameIdentityEnumResult::Completed(
                GameEnumResult::Draw,
            )),
            time_control: Some(GameMetadataText::new("60".to_owned())?),
            clocks: [2, 3, 5, 6, 7]
                .into_iter()
                .map(|id| Self::id(id).map(|node| NotationGameClock::new(node, remaining)))
                .collect::<Result<_, _>>()?,
            warnings: vec![
                NotationGameEnumWarning::UnknownDirective {
                    node: Self::id(2)?,
                    directive: "[%source keep]".to_owned(),
                },
                NotationGameEnumWarning::UnknownDirective {
                    node: Self::id(5)?,
                    directive: "[%source remove]".to_owned(),
                },
                NotationGameEnumWarning::UnavailableRating {
                    color: BoardEnumColor::White,
                    value: GameMetadataText::new("unavailable".to_owned())?,
                },
            ],
        })?)
    }

    fn rebuild(
        game: &NotationGame,
        tree: GameTree,
        mainline_tip: GameMoveId,
    ) -> Result<NotationGame, NotationGameError> {
        NotationGame::new(NotationGameParts {
            start: game.start().clone(),
            tree,
            mainline_tip,
            metadata: game.metadata().clone(),
            result: game.result(),
            result_header: game.result_header(),
            time_control: game.time_control().cloned(),
            clocks: game.clocks().to_vec(),
            warnings: game.warnings().to_vec(),
        })
    }

    fn export(
        game: &NotationGame,
        selection: NotationPgnExportEnumSelection,
    ) -> Result<Vec<u8>, NotationPgnExportError> {
        let mut output = Vec::new();
        NotationPgnExport::new(game)
            .with_selection(selection)
            .write(&mut output, || false)?;
        Ok(output)
    }
}

#[test]
fn notation_delete_confirmation_refuses_notes_saved_after_preview()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = NotationGameAnnotationsFixture::game()?;
    let root = NotationGameAnnotationsFixture::id(5)?;
    let confirmation = game
        .preview_variation(GameAnnotationsEnumVariationRequest::Delete { root })?
        .confirm();
    game.save_notes_and_nags(root, "saved after the preview".to_owned(), vec![])?;
    let result = game.apply_variation(confirmation, root);
    assert!(matches!(
        result,
        Err(NotationGameError::Tree(GameTreeError::StaleVariation))
    ));
    assert_eq!(
        game.tree().node(root)?.annotations().notes(),
        "saved after the preview"
    );
    Ok(())
}

#[test]
fn notation_delete_confirmation_refuses_markings_saved_after_preview()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = NotationGameAnnotationsFixture::game()?;
    let root = NotationGameAnnotationsFixture::id(5)?;
    let confirmation = game
        .preview_variation(GameAnnotationsEnumVariationRequest::Delete { root })?
        .confirm();
    let marking = GameMoveNodeMarking::circle(GameEnumMarkingColor::Red, "d4".parse()?);
    game.save_markings(root, vec![marking])?;
    let result = game.apply_variation(confirmation, root);
    assert!(matches!(
        result,
        Err(NotationGameError::Tree(GameTreeError::StaleVariation))
    ));
    assert_eq!(game.tree().node(root)?.annotations().markings(), &[marking]);
    Ok(())
}

#[test]
fn notation_explicit_saves_preserve_counterparts_and_accept_interior_selected_tips()
-> Result<(), Box<dyn std::error::Error>> {
    let source = NotationGameAnnotationsFixture::game()?;
    let node = NotationGameAnnotationsFixture::id(2)?;
    let mut game = NotationGameAnnotationsFixture::rebuild(&source, source.tree().clone(), node)?;
    let markings = vec![GameMoveNodeMarking::circle(
        GameEnumMarkingColor::Blue,
        "e4".parse()?,
    )];
    assert_eq!(
        game.save_markings(node, markings.clone())?,
        GameAnnotationsEnumChange::Changed
    );
    let nags = vec![GameMoveNodeNag::new(1), GameMoveNodeNag::new(255)];
    assert_eq!(
        game.save_notes_and_nags(node, "<b>literal</b>".to_owned(), nags.clone())?,
        GameAnnotationsEnumChange::Changed
    );
    assert_eq!(
        game.save_markings(node, markings.clone())?,
        GameAnnotationsEnumChange::Unchanged
    );
    assert_eq!(
        game.save_notes_and_nags(node, "<b>literal</b>".to_owned(), nags.clone())?,
        GameAnnotationsEnumChange::Unchanged
    );
    assert_eq!(game.tree().node(node)?.annotations().markings(), markings);
    game.save_markings(node, Vec::new())?;
    assert_eq!(game.tree().node(node)?.annotations().nags(), nags);
    assert!(
        game.save_notes_and_nags(node, "bad\0note".to_owned(), Vec::new())
            .is_err()
    );
    assert!(
        game.save_markings(NotationGameAnnotationsFixture::id(99)?, Vec::new())
            .is_err()
    );
    assert_eq!(
        game.tree().node(node)?.annotations().notes(),
        "<b>literal</b>"
    );
    assert_eq!(game.mainline_tip(), node);
    assert_eq!(game.metadata(), source.metadata());
    assert_eq!(game.warnings(), source.warnings());
    assert_eq!(game.clocks(), source.clocks());
    Ok(())
}

#[test]
fn notation_delete_confirmation_refuses_descendant_nag_edits_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = NotationGameAnnotationsFixture::game()?;
    let root = NotationGameAnnotationsFixture::id(5)?;
    let descendant = NotationGameAnnotationsFixture::id(7)?;
    let confirmation = game
        .preview_variation(GameAnnotationsEnumVariationRequest::Delete { root })?
        .confirm();
    let clocks = game.clocks().to_vec();
    let warnings = game.warnings().to_vec();
    let previous_tip = game.mainline_tip();
    let nags = vec![GameMoveNodeNag::new(1), GameMoveNodeNag::new(146)];
    game.save_notes_and_nags(descendant, String::new(), nags.clone())?;
    assert!(matches!(
        game.apply_variation(confirmation, descendant),
        Err(NotationGameError::Tree(GameTreeError::StaleVariation))
    ));
    assert_eq!(game.tree().len(), 9);
    assert_eq!(game.tree().node(descendant)?.annotations().nags(), nags);
    assert_eq!(game.clocks(), clocks);
    assert_eq!(game.warnings(), warnings);
    assert_eq!(game.mainline_tip(), previous_tip);
    Ok(())
}

#[test]
fn notation_delete_confirmation_survives_noop_saves_and_outside_scope_edits()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = NotationGameAnnotationsFixture::game()?;
    let root = NotationGameAnnotationsFixture::id(5)?;
    let marking = GameMoveNodeMarking::circle(GameEnumMarkingColor::Red, "d4".parse()?);
    game.save_markings(root, vec![marking])?;
    let confirmation = game
        .preview_variation(GameAnnotationsEnumVariationRequest::Delete { root })?
        .confirm();
    assert_eq!(
        game.save_markings(root, vec![marking])?,
        GameAnnotationsEnumChange::Unchanged
    );
    assert_eq!(
        game.save_notes_and_nags(root, String::new(), Vec::new())?,
        GameAnnotationsEnumChange::Unchanged
    );
    let survivor = game.mainline_tip();
    game.save_notes_and_nags(
        survivor,
        "Surviving notes".to_owned(),
        vec![GameMoveNodeNag::new(1)],
    )?;
    let applied = game.apply_variation(confirmation, survivor)?;
    assert_eq!(applied.change(), GameAnnotationsEnumChange::Changed);
    assert_eq!(applied.view(), survivor);
    assert_eq!(
        game.tree().node(survivor)?.annotations().notes(),
        "Surviving notes"
    );
    assert!(game.tree().node(root).is_err());
    Ok(())
}

#[test]
fn notation_delete_confirmation_distinguishes_marking_color_shape_and_endpoint()
-> Result<(), Box<dyn std::error::Error>> {
    let root = NotationGameAnnotationsFixture::id(5)?;
    let original =
        GameMoveNodeMarking::arrow(GameEnumMarkingColor::Red, "d2".parse()?, "d4".parse()?)?;
    for replacement in [
        GameMoveNodeMarking::arrow(GameEnumMarkingColor::Blue, "d2".parse()?, "d4".parse()?)?,
        GameMoveNodeMarking::arrow(GameEnumMarkingColor::Red, "d2".parse()?, "d5".parse()?)?,
        GameMoveNodeMarking::arrow(GameEnumMarkingColor::Red, "d1".parse()?, "d4".parse()?)?,
        GameMoveNodeMarking::circle(GameEnumMarkingColor::Red, "d2".parse()?),
    ] {
        let mut game = NotationGameAnnotationsFixture::game()?;
        game.save_markings(root, vec![original])?;
        let confirmation = game
            .preview_variation(GameAnnotationsEnumVariationRequest::Delete { root })?
            .confirm();
        game.save_markings(root, vec![replacement])?;
        assert!(matches!(
            game.apply_variation(confirmation, root),
            Err(NotationGameError::Tree(GameTreeError::StaleVariation))
        ));
        assert_eq!(
            game.tree().node(root)?.annotations().markings(),
            &[replacement]
        );
    }
    Ok(())
}

#[test]
fn notation_promotion_confirmation_refuses_changed_selected_or_previous_path_notes()
-> Result<(), Box<dyn std::error::Error>> {
    let tip = NotationGameAnnotationsFixture::id(9)?;
    for changed_node in [
        NotationGameAnnotationsFixture::id(5)?,
        NotationGameAnnotationsFixture::id(4)?,
    ] {
        let mut game = NotationGameAnnotationsFixture::game()?;
        let previous_tip = game.mainline_tip();
        let confirmation = game
            .preview_variation(GameAnnotationsEnumVariationRequest::Promote { tip })?
            .confirm();
        game.save_notes_and_nags(changed_node, "Saved after preview".to_owned(), Vec::new())?;
        assert!(matches!(
            game.apply_variation(confirmation, previous_tip),
            Err(NotationGameError::Tree(GameTreeError::StaleVariation))
        ));
        assert_eq!(game.mainline_tip(), previous_tip);
        assert_eq!(
            game.tree().node(changed_node)?.annotations().notes(),
            "Saved after preview"
        );
    }
    Ok(())
}

#[test]
fn notation_promotion_preserves_explicit_interior_tip_and_stable_sibling_order()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = NotationGameAnnotationsFixture::game()?;
    let tip = NotationGameAnnotationsFixture::id(5)?;
    let previous = game.mainline_tip();
    let preview = game.preview_variation(GameAnnotationsEnumVariationRequest::Promote { tip })?;
    assert_eq!(preview.previous_tip(), previous);
    assert_eq!(
        preview.previous_path(),
        &[
            NotationGameAnnotationsFixture::id(1)?,
            NotationGameAnnotationsFixture::id(2)?,
            NotationGameAnnotationsFixture::id(3)?,
            previous
        ]
    );
    assert_eq!(
        preview.selected_path(),
        Some([game.tree().root(), tip].as_slice())
    );
    assert_eq!(game.mainline_tip(), previous);
    let applied = game.apply_variation(preview.confirm(), previous)?;
    assert_eq!(applied.change(), GameAnnotationsEnumChange::Changed);
    assert_eq!(applied.view(), previous);
    assert_eq!(applied.mainline_tip(), tip);
    assert_eq!(game.mainline_tip(), tip);
    assert_eq!(
        game.tree().node(game.tree().root())?.children(),
        &[
            tip,
            NotationGameAnnotationsFixture::id(2)?,
            NotationGameAnnotationsFixture::id(8)?
        ]
    );
    assert!(
        matches!(NotationGameAnnotationsFixture::export(&game, NotationPgnExportEnumSelection::Full), Err(NotationPgnExportError::MainlineHasContinuations { node }) if node == tip)
    );
    let mainline = String::from_utf8(NotationGameAnnotationsFixture::export(
        &game,
        NotationPgnExportEnumSelection::Mainline,
    )?)?;
    assert!(mainline.contains("1. d4"));
    assert!(!mainline.contains("d5"));
    let preview = game.preview_variation(GameAnnotationsEnumVariationRequest::Promote { tip })?;
    assert_eq!(
        game.apply_variation(preview.confirm(), tip)?.change(),
        GameAnnotationsEnumChange::Unchanged
    );
    let tip = NotationGameAnnotationsFixture::id(9)?;
    let preview = game.preview_variation(GameAnnotationsEnumVariationRequest::Promote { tip })?;
    game.apply_variation(preview.confirm(), previous)?;
    assert_eq!(
        game.tree()
            .node(NotationGameAnnotationsFixture::id(5)?)?
            .children(),
        &[tip, NotationGameAnnotationsFixture::id(6)?]
    );
    assert!(game.tree().node(previous).is_ok());
    Ok(())
}

#[test]
fn notation_deletion_cleans_only_removed_references_and_repairs_the_actual_caller_view()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = NotationGameAnnotationsFixture::game()?;
    let source = game.clone();
    let root = NotationGameAnnotationsFixture::id(5)?;
    let preview = game.preview_variation(GameAnnotationsEnumVariationRequest::Delete { root })?;
    assert_eq!(
        preview.deleted_nodes(),
        &[
            root,
            NotationGameAnnotationsFixture::id(6)?,
            NotationGameAnnotationsFixture::id(7)?,
            NotationGameAnnotationsFixture::id(9)?
        ]
    );
    assert_eq!(preview.parent(), Some(game.tree().root()));
    assert_eq!(preview.resulting_tip(), game.mainline_tip());
    let applied =
        game.apply_variation(preview.confirm(), NotationGameAnnotationsFixture::id(7)?)?;
    assert_eq!(applied.view(), game.tree().root());
    assert_eq!(
        game.clocks()
            .iter()
            .map(|clock| clock.node())
            .collect::<Vec<_>>(),
        [
            NotationGameAnnotationsFixture::id(2)?,
            NotationGameAnnotationsFixture::id(3)?
        ]
    );
    for clock in game.clocks() {
        assert_eq!(game.clock(clock.node()), Some(clock.remaining()));
    }
    for id in [5, 6, 7, 9] {
        assert!(
            game.clock(NotationGameAnnotationsFixture::id(id)?)
                .is_none()
        );
    }
    assert_eq!(
        game.warnings(),
        &[source.warnings()[0].clone(), source.warnings()[2].clone()]
    );
    assert_eq!(game.metadata(), source.metadata());
    assert_eq!(game.result(), source.result());
    assert_eq!(game.result_header(), source.result_header());
    assert_eq!(game.time_control(), source.time_control());
    let preview = game.preview_variation(GameAnnotationsEnumVariationRequest::Delete {
        root: NotationGameAnnotationsFixture::id(8)?,
    })?;
    let view = NotationGameAnnotationsFixture::id(3)?;
    assert_eq!(game.apply_variation(preview.confirm(), view)?.view(), view);
    Ok(())
}

#[test]
fn deleting_selected_line_uses_surviving_sibling_then_parent_when_no_siblings_remain()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = NotationGameAnnotationsFixture::game()?;
    for (root, expected_tip, view) in [(2, 7, 4), (5, 8, 7), (8, 1, 8)] {
        let preview = game.preview_variation(GameAnnotationsEnumVariationRequest::Delete {
            root: NotationGameAnnotationsFixture::id(root)?,
        })?;
        assert_eq!(
            preview.resulting_tip(),
            NotationGameAnnotationsFixture::id(expected_tip)?
        );
        let applied =
            game.apply_variation(preview.confirm(), NotationGameAnnotationsFixture::id(view)?)?;
        assert_eq!(
            applied.mainline_tip(),
            NotationGameAnnotationsFixture::id(expected_tip)?
        );
        assert_eq!(applied.view(), game.tree().root());
    }
    assert_eq!(game.tree().len(), 1);
    assert!(
        game.preview_variation(GameAnnotationsEnumVariationRequest::Delete {
            root: game.tree().root()
        })
        .is_err()
    );
    Ok(())
}

#[test]
fn notation_confirmation_revalidates_subtree_order_and_selected_tip_before_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let source = NotationGameAnnotationsFixture::game()?;
    let request = GameAnnotationsEnumVariationRequest::Delete {
        root: NotationGameAnnotationsFixture::id(5)?,
    };
    let mut tree = source.tree().clone();
    NotationGameAnnotationsFixture::append(&mut tree, 7, 10, "g8f6")?;
    let mut changed =
        NotationGameAnnotationsFixture::rebuild(&source, tree, source.mainline_tip())?;
    let confirmation = source.preview_variation(request)?.confirm();
    assert!(matches!(
        changed.apply_variation(confirmation, changed.mainline_tip()),
        Err(NotationGameError::Tree(GameTreeError::StaleVariation))
    ));
    assert_eq!(changed.tree().len(), 10);
    let mut changed = source.clone();
    let promotion = changed.preview_variation(GameAnnotationsEnumVariationRequest::Promote {
        tip: NotationGameAnnotationsFixture::id(9)?,
    })?;
    changed.apply_variation(promotion.confirm(), changed.mainline_tip())?;
    let restore_tip = changed.preview_variation(GameAnnotationsEnumVariationRequest::Promote {
        tip: source.mainline_tip(),
    })?;
    changed.apply_variation(restore_tip.confirm(), changed.mainline_tip())?;
    assert_eq!(changed.mainline_tip(), source.mainline_tip());
    assert!(
        changed
            .apply_variation(
                source.preview_variation(request)?.confirm(),
                changed.mainline_tip()
            )
            .is_err()
    );
    let mut unchanged = source.clone();
    assert!(
        unchanged
            .apply_variation(
                source.preview_variation(request)?.confirm(),
                NotationGameAnnotationsFixture::id(99)?
            )
            .is_err()
    );
    assert_eq!(unchanged.tree().len(), source.tree().len());
    Ok(())
}

#[test]
fn promotion_confirmation_rejects_changed_path_ordering_without_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let source = NotationGameAnnotationsFixture::game()?;
    let request = GameAnnotationsEnumVariationRequest::Promote {
        tip: NotationGameAnnotationsFixture::id(9)?,
    };
    let mut tree = source.tree().clone();
    NotationGameAnnotationsFixture::append(&mut tree, 1, 10, "g1f3")?;
    let mut changed =
        NotationGameAnnotationsFixture::rebuild(&source, tree, source.mainline_tip())?;
    let order = changed
        .tree()
        .node(changed.tree().root())?
        .children()
        .to_vec();
    assert!(matches!(
        changed.apply_variation(
            source.preview_variation(request)?.confirm(),
            source.mainline_tip()
        ),
        Err(NotationGameError::Tree(GameTreeError::StaleVariation))
    ));
    assert_eq!(
        changed.tree().node(changed.tree().root())?.children(),
        order
    );
    assert_eq!(changed.mainline_tip(), source.mainline_tip());
    Ok(())
}

#[test]
fn edited_leaf_study_roundtrips_annotations_and_order_through_real_pgn_codec()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = NotationGameAnnotationsFixture::game()?;
    let tip = NotationGameAnnotationsFixture::id(9)?;
    game.save_notes_and_nags(
        tip,
        "literal <b>notes</b>".to_owned(),
        vec![GameMoveNodeNag::new(1)],
    )?;
    game.save_markings(
        tip,
        vec![GameMoveNodeMarking::circle(
            GameEnumMarkingColor::Red,
            "f6".parse()?,
        )],
    )?;
    let preview = game.preview_variation(GameAnnotationsEnumVariationRequest::Promote { tip })?;
    game.apply_variation(preview.confirm(), game.tree().root())?;
    let bytes =
        NotationGameAnnotationsFixture::export(&game, NotationPgnExportEnumSelection::Full)?;
    let mut next = 100_u128;
    let mut importer = NotationPgnImport::new(Cursor::new(bytes), move || {
        next += 1;
        NotationGameAnnotationsFixture::id(next)
    });
    let restored = importer
        .next_game(&|| false)?
        .ok_or_else(|| std::io::Error::other("The exported fixture must contain a game"))?;
    assert!(importer.next_game(&|| false)?.is_none());
    assert_eq!(restored.tree().len(), game.tree().len());
    assert_eq!(
        restored.tree().node(restored.mainline_tip())?.annotations(),
        game.tree().node(tip)?.annotations()
    );
    assert_eq!(
        restored.tree().node(restored.mainline_tip())?.position(),
        game.tree().node(tip)?.position()
    );
    assert_eq!(restored.metadata(), game.metadata());
    assert_eq!(restored.result(), game.result());
    Ok(())
}
