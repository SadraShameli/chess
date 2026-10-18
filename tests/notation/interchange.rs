use std::error::Error;
use std::io::Cursor;

use chess::board::enums::BoardEnumVariant;
use chess::board::r#move::BoardMove;
use chess::board::position::BoardPosition;
use chess::clock::duration::ClockDuration;
use chess::game::enums::{GameIdentityError, GameMoveId};
use chess::game::move_node::GameMoveNodeAnnotations;
use chess::game::tree::GameTreeError;
use chess::notation::game::NotationGame;
use chess::notation::game_identity::NotationGameIdentity;
use chess::notation::pgn_export::{NotationPgnExport, NotationPgnExportEnumSelection};
use chess::notation::pgn_import::{
    NotationPgnImport, NotationPgnImportEnumRecovery, NotationPgnImportEnumSyntax,
};

struct NotationInterchangeFixture;

#[derive(Debug, PartialEq, Eq)]
struct NotationInterchangeNode {
    path: Vec<String>,
    children: Vec<String>,
    fen: String,
    annotations: GameMoveNodeAnnotations,
    clock: Option<ClockDuration>,
}

impl NotationInterchangeFixture {
    const ANNOTATED: &str = r#"[Event "Quotation \" and \\ path"]
[White " Straße "]
[Black "Σ"]
[Date "2024.02.??"]
[Result "*"]
[Zeta "last"]
[Alpha "first"]

{root \\ stays literal}
1. e4 $1 { main <b>text</b>[%clk 0:09:59.123456789][%csl Re4][%cal Ge2e4]}
1... e5 (1... c5 2. Nf3 (2. Nc3)) 2. Nf3
; leading {literal} \path
;
*
"#;

    fn ids() -> impl FnMut() -> Result<GameMoveId, GameIdentityError> {
        let mut next = 0_u64;
        move || {
            next += 1;
            format!("00000000-0000-4000-8000-{next:012x}").parse()
        }
    }

    fn import(input: &str) -> Result<NotationGame, Box<dyn Error>> {
        let mut importer = NotationPgnImport::new(Cursor::new(input.as_bytes()), Self::ids());
        let game = importer
            .next_game(&|| false)?
            .ok_or_else(|| std::io::Error::other("The fixed PGN must contain a game"))?;
        assert!(importer.next_game(&|| false)?.is_none());
        Ok(game)
    }

    fn single_move(input: &str) -> Result<NotationGame, Box<dyn Error>> {
        let game = Self::import(input)?;
        assert_eq!(game.tree().len(), 2);
        Ok(game)
    }

    fn movements(game: &NotationGame, tip: GameMoveId) -> Result<Vec<BoardMove>, GameTreeError> {
        game.tree()
            .path_to(tip)?
            .into_iter()
            .skip(1)
            .map(|id| {
                game.tree()
                    .node(id)?
                    .movement()
                    .ok_or(GameTreeError::BrokenRelationship)
            })
            .collect()
    }

    fn signature(game: &NotationGame) -> Result<Vec<NotationInterchangeNode>, GameTreeError> {
        let variant = game.start().position().variant();
        let mut nodes = game
            .tree()
            .nodes()
            .map(|node| {
                let path = Self::movements(game, node.id())?
                    .into_iter()
                    .map(|movement| movement.uci(variant))
                    .collect();
                let children = node
                    .children()
                    .iter()
                    .map(|id| {
                        game.tree()
                            .node(*id)?
                            .movement()
                            .map(|movement| movement.uci(variant))
                            .ok_or(GameTreeError::BrokenRelationship)
                    })
                    .collect::<Result<_, _>>()?;
                Ok(NotationInterchangeNode {
                    path,
                    children,
                    fen: node.position().fen(),
                    annotations: node.annotations().clone(),
                    clock: game.clock(node.id()),
                })
            })
            .collect::<Result<Vec<_>, GameTreeError>>()?;
        nodes.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(nodes)
    }

    fn identity(game: &NotationGame) -> Result<NotationGameIdentity, Box<dyn Error>> {
        Ok(NotationGameIdentity::new(
            game.start().position(),
            &Self::movements(game, game.mainline_tip())?,
            game.metadata(),
            Some(game.result()),
        )?)
    }
}

#[test]
fn full_interchange_preserves_independent_annotated_tree_and_source_metadata()
-> Result<(), Box<dyn Error>> {
    let original = NotationInterchangeFixture::import(NotationInterchangeFixture::ANNOTATED)?;
    assert_eq!(original.tree().len(), 7);
    assert_eq!(
        original
            .metadata()
            .known()
            .event
            .as_ref()
            .map(|text| text.as_str()),
        Some("Quotation \" and \\ path")
    );
    assert_eq!(
        original
            .metadata()
            .extensions()
            .iter()
            .map(|tag| tag.name())
            .collect::<Vec<_>>(),
        ["Zeta", "Alpha"]
    );
    let path = original.tree().path_to(original.mainline_tip())?;
    assert_eq!(
        original.tree().node(path[0])?.annotations().notes(),
        r"root \\ stays literal"
    );
    let first = original.tree().node(path[1])?;
    assert_eq!(first.annotations().notes(), " main <b>text</b>");
    assert_eq!(first.annotations().nags()[0].value(), 1);
    assert_eq!(first.annotations().markings().len(), 2);
    assert_eq!(
        original.clock(first.id()),
        Some(ClockDuration::from_nanos(599_123_456_789))
    );
    assert_eq!(
        original
            .tree()
            .node(original.mainline_tip())?
            .annotations()
            .notes(),
        " leading {literal} \\path\n"
    );
    let mut output = Vec::new();
    NotationPgnExport::new(&original).write(&mut output, || false)?;
    let restored = NotationInterchangeFixture::import(std::str::from_utf8(&output)?)?;
    assert_eq!(
        NotationInterchangeFixture::signature(&original)?,
        NotationInterchangeFixture::signature(&restored)?
    );
    assert_eq!(original.metadata(), restored.metadata());
    assert_eq!(original.result_header(), restored.result_header());
    assert_eq!(
        NotationInterchangeFixture::identity(&original)?,
        NotationInterchangeFixture::identity(&restored)?
    );
    Ok(())
}

#[test]
fn mainline_interchange_keeps_selected_moves_and_notes_without_side_branches()
-> Result<(), Box<dyn Error>> {
    let original = NotationInterchangeFixture::import(NotationInterchangeFixture::ANNOTATED)?;
    let mut output = Vec::new();
    NotationPgnExport::new(&original)
        .with_selection(NotationPgnExportEnumSelection::Mainline)
        .write(&mut output, || false)?;
    let restored = NotationInterchangeFixture::import(std::str::from_utf8(&output)?)?;
    assert_eq!(restored.tree().len(), 4);
    assert_eq!(
        NotationInterchangeFixture::movements(&restored, restored.mainline_tip())?
            .into_iter()
            .map(|movement| movement.uci(BoardEnumVariant::Standard))
            .collect::<Vec<_>>(),
        ["e2e4", "e7e5", "g1f3"]
    );
    assert_eq!(
        NotationInterchangeFixture::identity(&original)?,
        NotationInterchangeFixture::identity(&restored)?
    );
    assert_eq!(
        original.tree().node(original.mainline_tip())?.annotations(),
        restored.tree().node(restored.mainline_tip())?.annotations()
    );
    Ok(())
}

#[test]
fn excluded_illegal_variation_cannot_leak_a_legal_prefix_into_the_next_game()
-> Result<(), Box<dyn Error>> {
    let input =
        "[Event \"excluded\"]\n\n1. e4 e5 (1... Qa9) 2. Nf3 *\n\n[Event \"kept\"]\n\n1. d4 d5 *\n";
    let mut importer = NotationPgnImport::new(
        Cursor::new(input.as_bytes()),
        NotationInterchangeFixture::ids(),
    );
    let rejected = importer.next_game(&|| false);
    assert!(rejected.is_err());
    let error = rejected
        .err()
        .ok_or_else(|| std::io::Error::other("The illegal branch must be excluded"))?;
    assert_eq!(error.game_ordinal(), 1);
    assert_eq!(error.recovery(), NotationPgnImportEnumRecovery::NextGame);
    let restored = importer
        .next_game(&|| false)?
        .ok_or_else(|| std::io::Error::other("The second game must remain available"))?;
    assert_eq!(
        restored
            .metadata()
            .known()
            .event
            .as_ref()
            .map(|value| value.as_str()),
        Some("kept")
    );
    assert_eq!(restored.tree().len(), 3);
    assert_eq!(
        NotationInterchangeFixture::movements(&restored, restored.mainline_tip())?
            .into_iter()
            .map(|movement| movement.uci(BoardEnumVariant::Standard))
            .collect::<Vec<_>>(),
        ["d2d4", "d7d5"]
    );
    assert!(importer.next_game(&|| false)?.is_none());
    Ok(())
}

#[test]
fn header_absence_and_present_unfinished_result_roundtrip_independently()
-> Result<(), Box<dyn Error>> {
    for input in ["1. e4 *\n", "[Result \"*\"]\n\n1. e4 *\n"] {
        let original = NotationInterchangeFixture::import(input)?;
        let mut output = Vec::new();
        NotationPgnExport::new(&original).write(&mut output, || false)?;
        let restored = NotationInterchangeFixture::import(std::str::from_utf8(&output)?)?;
        assert_eq!(original.result_header(), restored.result_header());
        assert_eq!(original.result(), restored.result());
    }
    Ok(())
}

#[test]
fn adjacent_asterisk_after_move_terminates_the_game() -> Result<(), Box<dyn Error>> {
    NotationInterchangeFixture::single_move("1.e4*")?;
    Ok(())
}

#[test]
fn adjacent_asterisk_after_nag_terminates_the_game() -> Result<(), Box<dyn Error>> {
    let game = NotationInterchangeFixture::single_move("1.e4$1*")?;
    assert_eq!(
        game.tree().node(game.mainline_tip())?.annotations().nags()[0].value(),
        1
    );
    Ok(())
}

#[test]
fn adjacent_asterisk_before_next_game_is_a_complete_empty_game() -> Result<(), Box<dyn Error>> {
    let mut importer =
        NotationPgnImport::new(Cursor::new(b"*1.e4*"), NotationInterchangeFixture::ids());
    let first = importer
        .next_game(&|| false)?
        .ok_or("The first record must be present")?;
    assert_eq!(first.tree().len(), 1);
    let second = importer
        .next_game(&|| false)?
        .ok_or("The second record must be present")?;
    assert_eq!(second.tree().len(), 2);
    assert!(importer.next_game(&|| false)?.is_none());
    Ok(())
}

#[test]
fn failed_error_drain_retains_the_illegal_branch_cause_and_original_location()
-> Result<(), Box<dyn Error>> {
    let input = "1. e4 (1. d4 d5 2. Ke3) {unterminated";
    let mut importer = NotationPgnImport::new(
        Cursor::new(input.as_bytes()),
        NotationInterchangeFixture::ids(),
    );
    let error = importer
        .next_game(&|| false)
        .err()
        .ok_or("The illegal branch must be excluded")?;
    let mut branch = BoardPosition::standard();
    for san in ["d4", "d5"] {
        branch = branch.play(branch.parse_san(san)?)?;
    }
    let primary = branch
        .parse_san("Ke3")
        .err()
        .ok_or("The independent branch must reject Ke3")?;
    let displayed = error.to_string();
    assert!(displayed.contains(&primary.to_string()), "{displayed}");
    assert!(
        displayed.contains(&NotationPgnImportEnumSyntax::UnterminatedComment.to_string()),
        "{displayed}"
    );
    assert_eq!(
        error.location().byte_offset(),
        u64::try_from(input.find("Ke3").ok_or("Missing fixed illegal move")?)?
    );
    assert_eq!(error.recovery(), NotationPgnImportEnumRecovery::Terminated);
    assert!(
        std::iter::successors(Some(&error as &dyn Error), |&cause| cause.source())
            .any(|cause| cause.to_string().contains(&primary.to_string()))
    );
    Ok(())
}
