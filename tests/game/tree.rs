use std::str::FromStr;

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::game::enums::{GameEnumMarkingColor, GameMoveId};
use chess::game::move_node::{GameMoveNodeAnnotations, GameMoveNodeMarking, GameMoveNodeNag};
use chess::game::tree::GameTree;

struct GameTreeFixture;

impl GameTreeFixture {
    fn id(value: u128) -> Result<GameMoveId, Box<dyn std::error::Error>> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn tree() -> Result<GameTree, Box<dyn std::error::Error>> {
        Ok(GameTree::new(Self::id(1)?, BoardPosition::standard(), None))
    }

    fn append(
        tree: &mut GameTree,
        parent: GameMoveId,
        value: u128,
        uci: &str,
    ) -> Result<GameMoveId, Box<dyn std::error::Error>> {
        let movement = tree.node(parent)?.position().parse_uci(uci)?;
        Ok(tree.append(parent, Self::id(value)?, movement, None)?)
    }
}

#[test]
fn alternatives_keep_insertion_order_and_matching_moves_reuse_the_same_node()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tree = GameTreeFixture::tree()?;
    let root = tree.root();
    let e4 = GameTreeFixture::append(&mut tree, root, 2, "e2e4")?;
    let d4 = GameTreeFixture::append(&mut tree, root, 3, "d2d4")?;
    let reused = GameTreeFixture::append(&mut tree, root, 4, "e2e4")?;
    assert_eq!(reused, e4);
    assert_eq!(tree.len(), 3);
    assert_eq!(tree.node(root)?.children(), &[e4, d4]);
    assert_eq!(tree.node(e4)?.parent(), Some(root));
    assert_eq!(tree.node(e4)?.san(), Some("e4"));
    assert_eq!(tree.node(e4)?.position().turn(), BoardEnumColor::Black);
    assert!(tree.node(GameTreeFixture::id(4)?).is_err());
    Ok(())
}

#[test]
fn duplicate_identity_and_foreign_position_moves_leave_the_tree_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tree = GameTreeFixture::tree()?;
    let root = tree.root();
    let e4 = GameTreeFixture::append(&mut tree, root, 2, "e2e4")?;
    let black_move = tree.node(e4)?.position().parse_uci("e7e5")?;
    assert!(
        tree.append(root, GameTreeFixture::id(3)?, black_move, None)
            .is_err()
    );
    let d4 = tree.node(root)?.position().parse_uci("d2d4")?;
    assert!(tree.append(root, e4, d4, None).is_err());
    assert_eq!(tree.len(), 2);
    assert_eq!(tree.node(root)?.children(), &[e4]);
    assert_eq!(tree.node(e4)?.san(), Some("e4"));
    Ok(())
}

#[test]
fn replacing_an_ancestor_discards_all_descendant_position_and_san_caches()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tree = GameTreeFixture::tree()?;
    let root = tree.root();
    let e4 = GameTreeFixture::append(&mut tree, root, 2, "e2e4")?;
    let e5 = GameTreeFixture::append(&mut tree, e4, 3, "e7e5")?;
    let nf3 = GameTreeFixture::append(&mut tree, e5, 4, "g1f3")?;
    let c5 = GameTreeFixture::append(&mut tree, e4, 5, "c7c5")?;
    let d4 = GameTreeFixture::append(&mut tree, root, 6, "d2d4")?;
    let replacement = tree.node(root)?.position().parse_uci("c2c4")?;
    let c4 = tree.replace_branch(e4, GameTreeFixture::id(7)?, replacement)?;
    assert_eq!(tree.len(), 3);
    assert_eq!(tree.node(root)?.children(), &[c4, d4]);
    for removed in [e4, e5, nf3, c5] {
        assert!(tree.node(removed).is_err());
    }
    assert_eq!(tree.node(c4)?.san(), Some("c4"));
    assert_eq!(
        tree.node(c4)?.position(),
        &BoardPosition::standard().play_uci("c2c4")?
    );
    assert_eq!(tree.path_to(c4)?, vec![root, c4]);
    Ok(())
}

#[test]
fn invalid_replacement_preserves_the_original_branch_and_all_annotations()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tree = GameTreeFixture::tree()?;
    let root = tree.root();
    let e4 = GameTreeFixture::append(&mut tree, root, 2, "e2e4")?;
    let e5 = GameTreeFixture::append(&mut tree, e4, 3, "e7e5")?;
    let invalid = tree.node(e4)?.position().parse_uci("c7c5")?;
    assert!(
        tree.replace_branch(e4, GameTreeFixture::id(4)?, invalid)
            .is_err()
    );
    assert_eq!(tree.len(), 3);
    assert_eq!(tree.path_to(e5)?, vec![root, e4, e5]);
    assert!(
        tree.replace_branch(root, GameTreeFixture::id(5)?, invalid)
            .is_err()
    );
    Ok(())
}

#[test]
fn notes_nags_and_saved_markings_remain_attached_to_their_exact_position()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tree = GameTreeFixture::tree()?;
    let root = tree.root();
    let e4 = GameTreeFixture::append(&mut tree, root, 2, "e2e4")?;
    let markings = vec![
        GameMoveNodeMarking::circle(GameEnumMarkingColor::Green, "e4".parse()?),
        GameMoveNodeMarking::arrow(GameEnumMarkingColor::Red, "d1".parse()?, "h5".parse()?)?,
    ];
    let annotations = GameMoveNodeAnnotations::new(
        "Plain text <b>remains text</b>\nA second line".to_owned(),
        vec![GameMoveNodeNag::new(1), GameMoveNodeNag::new(146)],
        markings,
    )?;
    tree.set_annotations(e4, annotations.clone())?;
    assert_eq!(tree.node(e4)?.annotations(), &annotations);
    assert!(tree.node(root)?.annotations().notes().is_empty());
    assert_eq!(tree.node(e4)?.annotations().nags()[1].value(), 146);
    assert!(GameMoveNodeAnnotations::new("nul\0text".to_owned(), Vec::new(), Vec::new()).is_err());
    assert!(
        GameMoveNodeMarking::arrow(GameEnumMarkingColor::Blue, "e4".parse()?, "e4".parse()?)
            .is_err()
    );
    Ok(())
}

#[test]
fn deleting_a_branch_keeps_siblings_ordered_and_rejects_root_deletion()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tree = GameTreeFixture::tree()?;
    let root = tree.root();
    let e4 = GameTreeFixture::append(&mut tree, root, 2, "e2e4")?;
    let e5 = GameTreeFixture::append(&mut tree, e4, 3, "e7e5")?;
    let d4 = GameTreeFixture::append(&mut tree, root, 4, "d2d4")?;
    assert_eq!(tree.delete_branch(e4)?, 2);
    assert_eq!(tree.node(root)?.children(), &[d4]);
    assert!(tree.node(e5).is_err());
    assert!(tree.delete_branch(root).is_err());
    assert_eq!(tree.len(), 2);
    Ok(())
}

#[test]
fn move_identifiers_reject_nil_and_noncanonical_external_strings()
-> Result<(), Box<dyn std::error::Error>> {
    let id = GameMoveId::from_str("123e4567-e89b-42d3-a456-426614174000")?;
    assert_eq!(GameMoveId::try_from(id.as_uuid())?, id);
    assert!(GameMoveId::try_from(uuid::Uuid::nil()).is_err());
    assert_eq!(
        serde_json::from_str::<GameMoveId>(&serde_json::to_string(&id)?)?,
        id
    );
    for invalid in [
        "00000000-0000-0000-0000-000000000000",
        "123E4567-E89B-42D3-A456-426614174000",
        "123e4567e89b42d3a456426614174000",
    ] {
        assert!(GameMoveId::from_str(invalid).is_err());
    }
    Ok(())
}

#[test]
fn replacing_with_an_existing_sibling_reuses_its_subtree_annotations_and_order()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tree = GameTreeFixture::tree()?;
    let root = tree.root();
    let e4 = GameTreeFixture::append(&mut tree, root, 2, "e2e4")?;
    let e5 = GameTreeFixture::append(&mut tree, e4, 3, "e7e5")?;
    let d4 = GameTreeFixture::append(&mut tree, root, 4, "d2d4")?;
    let d5 = GameTreeFixture::append(&mut tree, d4, 5, "d7d5")?;
    let c4 = GameTreeFixture::append(&mut tree, root, 6, "c2c4")?;
    let notes =
        GameMoveNodeAnnotations::new("Keep the existing study".to_owned(), Vec::new(), Vec::new())?;
    tree.set_annotations(d4, notes.clone())?;
    let movement = tree.node(root)?.position().parse_uci("d2d4")?;
    assert_eq!(
        tree.replace_branch(e4, GameTreeFixture::id(7)?, movement)?,
        d4
    );
    assert_eq!(tree.node(root)?.children(), &[d4, c4]);
    assert_eq!(tree.node(d4)?.annotations(), &notes);
    assert_eq!(tree.path_to(d5)?, vec![root, d4, d5]);
    assert_eq!(tree.len(), 4);
    for deleted in [e4, e5, GameTreeFixture::id(7)?] {
        assert!(tree.node(deleted).is_err());
    }
    Ok(())
}

#[test]
fn replacing_a_branch_with_its_own_move_preserves_all_descendants()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tree = GameTreeFixture::tree()?;
    let root = tree.root();
    let e4 = GameTreeFixture::append(&mut tree, root, 2, "e2e4")?;
    let e5 = GameTreeFixture::append(&mut tree, e4, 3, "e7e5")?;
    let notes = GameMoveNodeAnnotations::new(
        "Keep the original branch".to_owned(),
        Vec::new(),
        Vec::new(),
    )?;
    tree.set_annotations(e4, notes.clone())?;
    let movement = tree.node(root)?.position().parse_uci("e2e4")?;
    assert_eq!(
        tree.replace_branch(e4, GameTreeFixture::id(4)?, movement)?,
        e4
    );
    assert_eq!(tree.len(), 3);
    assert_eq!(tree.path_to(e5)?, vec![root, e4, e5]);
    assert_eq!(tree.node(e4)?.annotations(), &notes);
    assert!(tree.node(GameTreeFixture::id(4)?).is_err());
    Ok(())
}

#[test]
fn matching_sibling_does_not_allow_an_unrelated_replacement_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut tree = GameTreeFixture::tree()?;
    let root = tree.root();
    let e4 = GameTreeFixture::append(&mut tree, root, 2, "e2e4")?;
    let e5 = GameTreeFixture::append(&mut tree, e4, 3, "e7e5")?;
    let d4 = GameTreeFixture::append(&mut tree, root, 4, "d2d4")?;
    let d5 = GameTreeFixture::append(&mut tree, d4, 5, "d7d5")?;
    let movement = tree.node(root)?.position().parse_uci("d2d4")?;
    assert!(tree.replace_branch(e4, d5, movement).is_err());
    assert_eq!(tree.node(root)?.children(), &[e4, d4]);
    assert_eq!(tree.path_to(e5)?, vec![root, e4, e5]);
    assert_eq!(tree.path_to(d5)?, vec![root, d4, d5]);
    assert_eq!(tree.len(), 5);
    Ok(())
}
