use chess::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::puzzle::definition::PuzzleDefinition;
use chess::puzzle::identity::{PuzzleDefinitionIdentity, PuzzleDefinitionIdentityError};
use chess::puzzle::metadata::{
    PuzzleDefinitionDifficulty, PuzzleDefinitionMetadata, PuzzleDefinitionMetadataError,
    PuzzleDefinitionTheme,
};
use chess::puzzle::tree::{PuzzleDefinitionTree, PuzzleDefinitionTreeError};

use crate::puzzle_fixture::PuzzleFixture;

struct PuzzleDefinitionFixture;

impl PuzzleDefinitionFixture {
    fn assert_paths(tree: &PuzzleDefinitionTree) -> Result<(), Box<dyn std::error::Error>> {
        for line in tree.lines() {
            let mut parent = tree.root();
            let mut position = tree.starting_position().clone();
            for id in line.nodes() {
                let node = tree.node(*id).ok_or("Missing solution node")?;
                let movement = node.movement().ok_or("Missing solution move")?;
                assert_eq!(node.parent(), Some(parent));
                assert_eq!(
                    tree.continuation(parent, movement)?.map(|node| node.id()),
                    Some(*id)
                );
                position = position.play(movement)?;
                assert_eq!(node.position(), &position);
                parent = *id;
            }
            let terminal = tree.node(parent).ok_or("Missing terminal node")?;
            assert!(terminal.is_terminal());
            assert!(terminal.children().is_empty());
            assert_eq!(terminal.terminal_line(), Some(line.id()));
        }
        Ok(())
    }
}

#[test]
fn retains_single_line_identity_start_and_unavailable_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    let initial = BoardPosition::standard();
    let definition = PuzzleFixture::definition(initial.clone(), &[&["e2e4", "e7e5", "g1f3"]])?;
    assert_eq!(definition.identity(), &PuzzleFixture::identity()?);
    assert_eq!(definition.starting_position(), &initial);
    assert_eq!(definition.solver(), BoardEnumColor::White);
    assert_eq!(definition.metadata().difficulty(), None);
    assert_eq!(definition.metadata().themes(), None);
    assert_eq!(definition.solution().lines().len(), 1);
    let root = definition
        .solution()
        .node(definition.solution().root())
        .ok_or("Missing root")?;
    assert_eq!(root.parent(), None);
    assert_eq!(root.movement(), None);
    assert!(!root.is_terminal());
    PuzzleDefinitionFixture::assert_paths(definition.solution())
}

#[test]
fn preserves_source_line_and_branch_order_with_shared_prefixes()
-> Result<(), Box<dyn std::error::Error>> {
    let lines: &[&[&str]] = &[
        &["e2e4", "e7e5", "g1f3"],
        &["e2e4", "c7c5", "g1f3"],
        &["e2e4", "e7e5", "f1c4"],
        &["d2d4", "d7d5", "c2c4"],
    ];
    let definition = PuzzleFixture::definition(BoardPosition::standard(), lines)?;
    let tree = definition.solution();
    let listed = tree.lines();
    assert_eq!(listed.len(), lines.len());
    assert_eq!(listed[0].nodes()[0], listed[1].nodes()[0]);
    assert_eq!(listed[0].nodes()[1], listed[2].nodes()[1]);
    assert_ne!(listed[0].nodes()[2], listed[2].nodes()[2]);
    assert_eq!(
        tree.node(tree.root()).ok_or("Missing root")?.children(),
        &[listed[0].nodes()[0], listed[3].nodes()[0]]
    );
    assert_eq!(
        tree.node(listed[0].nodes()[0])
            .ok_or("Missing trunk")?
            .children(),
        &[listed[0].nodes()[1], listed[1].nodes()[1]]
    );
    for (line, expected) in listed.iter().zip(lines) {
        let actual = line
            .nodes()
            .iter()
            .map(|id| {
                tree.node(*id)
                    .and_then(|node| node.movement())
                    .map(|movement| movement.uci(BoardEnumVariant::Standard))
                    .ok_or("Missing move")
            })
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(actual, *expected);
    }
    let rebuilt = PuzzleFixture::definition(BoardPosition::standard(), lines)?;
    assert_eq!(tree, rebuilt.solution());
    PuzzleDefinitionFixture::assert_paths(tree)
}

#[test]
fn refuses_empty_solutions_and_empty_listed_branches() -> Result<(), Box<dyn std::error::Error>> {
    let initial = BoardPosition::standard();
    assert!(matches!(
        PuzzleDefinition::new(
            PuzzleFixture::identity()?,
            initial.clone(),
            Vec::new(),
            PuzzleDefinitionMetadata::default()
        ),
        Err(PuzzleDefinitionTreeError::EmptySolutions)
    ));
    assert!(matches!(
        PuzzleDefinition::new(
            PuzzleFixture::identity()?,
            initial,
            vec![Vec::new()],
            PuzzleDefinitionMetadata::default()
        ),
        Err(PuzzleDefinitionTreeError::EmptyLine { line: 0 })
    ));
    Ok(())
}

#[test]
fn rejects_duplicate_complete_lines_without_silent_deduplication()
-> Result<(), Box<dyn std::error::Error>> {
    let initial = BoardPosition::standard();
    let line = PuzzleFixture::line(&initial, &["e2e4", "e7e5", "g1f3"])?;
    let result = PuzzleDefinition::new(
        PuzzleFixture::identity()?,
        initial,
        vec![line.clone(), line],
        PuzzleDefinitionMetadata::default(),
    );
    assert!(matches!(
        result,
        Err(PuzzleDefinitionTreeError::DuplicateLine { line: 1, .. })
    ));
    Ok(())
}

#[test]
fn rejects_a_terminal_prefix_in_either_source_order() -> Result<(), Box<dyn std::error::Error>> {
    for reverse in [false, true] {
        let initial = BoardPosition::standard();
        let mut lines = vec![
            PuzzleFixture::line(&initial, &["e2e4"])?,
            PuzzleFixture::line(&initial, &["e2e4", "e7e5", "g1f3"])?,
        ];
        if reverse {
            lines.reverse();
        }
        let result = PuzzleDefinition::new(
            PuzzleFixture::identity()?,
            initial,
            lines,
            PuzzleDefinitionMetadata::default(),
        );
        assert!(matches!(
            result,
            Err(PuzzleDefinitionTreeError::TerminalPrefix { line: 1, .. })
        ));
    }
    Ok(())
}

#[test]
fn validates_each_ply_against_its_actual_preceding_board() -> Result<(), Box<dyn std::error::Error>>
{
    let initial = BoardPosition::standard();
    let valid = PuzzleFixture::line(&initial, &["e2e4", "e7e5", "g1f3"])?;
    let developed = initial.play_uci("e2e4")?.play_uci("e7e5")?;
    let illegal = [
        developed.parse_uci("f1b5")?,
        initial.parse_uci("a2a3")?,
        initial.parse_uci("e2e4")?,
    ];
    for (ply, movement) in illegal.into_iter().enumerate() {
        let mut line = valid.clone();
        line[ply] = movement;
        let result = PuzzleDefinition::new(
            PuzzleFixture::identity()?,
            initial.clone(),
            vec![line],
            PuzzleDefinitionMetadata::default(),
        );
        assert!(
            matches!(result, Err(PuzzleDefinitionTreeError::IllegalMove { line: 0, ply: failed_ply, source: _ }) if failed_ply == ply)
        );
    }
    Ok(())
}

#[test]
fn rejects_illegal_later_branch_without_borrowing_another_branch_history()
-> Result<(), Box<dyn std::error::Error>> {
    let initial = BoardPosition::standard();
    let capture = PuzzleFixture::line(&initial, &["e2e4", "d7d5", "e4d5"])?;
    let mut inconsistent = capture.clone();
    inconsistent[1] = initial.play_uci("e2e4")?.parse_uci("e7e5")?;
    let result = PuzzleDefinition::new(
        PuzzleFixture::identity()?,
        initial,
        vec![capture, inconsistent],
        PuzzleDefinitionMetadata::default(),
    );
    assert!(matches!(
        result,
        Err(PuzzleDefinitionTreeError::IllegalMove {
            line: 1,
            ply: 2,
            ..
        })
    ));
    Ok(())
}

#[test]
fn rejects_a_move_that_exposes_its_king_even_when_valid_in_another_position()
-> Result<(), Box<dyn std::error::Error>> {
    let unpinned = BoardPosition::from_fen(
        "k7/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
        BoardEnumVariant::Standard,
    )?;
    let pinned = BoardPosition::from_fen(
        "k3r3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
        BoardEnumVariant::Standard,
    )?;
    let movement = unpinned.parse_uci("e5d6")?;
    assert!(matches!(
        PuzzleDefinition::new(
            PuzzleFixture::identity()?,
            pinned,
            vec![vec![movement]],
            PuzzleDefinitionMetadata::default()
        ),
        Err(PuzzleDefinitionTreeError::IllegalMove {
            line: 0,
            ply: 0,
            ..
        })
    ));
    Ok(())
}

#[test]
fn keeps_transposed_positions_on_their_distinct_listed_paths()
-> Result<(), Box<dyn std::error::Error>> {
    let definition = PuzzleFixture::definition(
        BoardPosition::standard(),
        &[
            &["g1f3", "g8f6", "b1c3", "b8c6"],
            &["b1c3", "b8c6", "g1f3", "g8f6"],
        ],
    )?;
    let tree = definition.solution();
    let first = tree
        .node(tree.lines()[0].nodes()[3])
        .ok_or("Missing first leaf")?;
    let second = tree
        .node(tree.lines()[1].nodes()[3])
        .ok_or("Missing second leaf")?;
    assert_eq!(first.position(), second.position());
    assert_ne!(first.id(), second.id());
    assert_ne!(first.terminal_line(), second.terminal_line());
    PuzzleDefinitionFixture::assert_paths(tree)
}

#[test]
fn keeps_legal_unlisted_moves_outside_the_accepted_tree() -> Result<(), Box<dyn std::error::Error>>
{
    let definition =
        PuzzleFixture::definition(BoardPosition::standard(), &[&["e2e4", "e7e5", "g1f3"]])?;
    let initial = definition.starting_position();
    let tree = definition.solution();
    assert!(
        tree.continuation(tree.root(), initial.parse_uci("e2e4")?)?
            .is_some()
    );
    let unlisted = initial.parse_uci("d2d4")?;
    assert!(initial.play(unlisted).is_ok());
    assert!(tree.continuation(tree.root(), unlisted)?.is_none());
    assert!(initial.parse_uci("e2e5").is_err());
    Ok(())
}

#[test]
fn uses_the_supplied_solver_position_without_applying_an_extra_setup_move()
-> Result<(), Box<dyn std::error::Error>> {
    let initial = BoardPosition::standard().play_uci("e2e4")?;
    let definition = PuzzleFixture::definition(initial.clone(), &[&["c7c5", "g1f3"]])?;
    assert_eq!(definition.solver(), BoardEnumColor::Black);
    assert_eq!(definition.starting_position(), &initial);
    let first = definition.solution().lines()[0].nodes()[0];
    assert_eq!(
        definition
            .solution()
            .node(first)
            .ok_or("Missing first move")?
            .movement(),
        Some(initial.parse_uci("c7c5")?)
    );
    PuzzleDefinitionFixture::assert_paths(definition.solution())
}

#[test]
fn preserves_en_passant_and_counters_in_validated_positions()
-> Result<(), Box<dyn std::error::Error>> {
    let initial = BoardPosition::from_fen(
        "4k3/8/8/3pP3/8/8/8/4K3 w - d6 19 42",
        BoardEnumVariant::Standard,
    )?;
    let definition = PuzzleFixture::definition(initial.clone(), &[&["e5d6", "e8d7", "e1d2"]])?;
    assert_eq!(definition.starting_position(), &initial);
    let first = definition.solution().lines()[0].nodes()[0];
    let position = definition
        .solution()
        .node(first)
        .ok_or("Missing capture")?
        .position();
    assert_eq!(position.placement(), "4k3/8/3P4/8/8/8/8/4K3");
    assert_eq!(position.fullmoves(), 42);
    assert_eq!(position.halfmoves(), 0);
    PuzzleDefinitionFixture::assert_paths(definition.solution())
}

#[test]
fn preserves_each_listed_promotion_without_adding_a_queen_default()
-> Result<(), Box<dyn std::error::Error>> {
    let initial =
        BoardPosition::from_fen("7k/P7/8/8/8/8/8/7K w - - 0 1", BoardEnumVariant::Standard)?;
    let definition = PuzzleFixture::definition(
        initial.clone(),
        &[&["a7a8n"], &["a7a8b"], &["a7a8r"], &["a7a8q"]],
    )?;
    assert!(initial.parse_uci("a7a8").is_err());
    let promotions = definition
        .solution()
        .lines()
        .iter()
        .map(|line| {
            definition
                .solution()
                .node(line.nodes()[0])
                .and_then(|node| node.movement())
                .and_then(|movement| movement.promotion())
                .ok_or("Missing promotion")
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        promotions,
        [
            BoardEnumPieceKind::Knight,
            BoardEnumPieceKind::Bishop,
            BoardEnumPieceKind::Rook,
            BoardEnumPieceKind::Queen
        ]
    );
    PuzzleDefinitionFixture::assert_paths(definition.solution())
}

#[test]
fn retains_chess960_castling_variant_rook_identity_and_stationary_king()
-> Result<(), Box<dyn std::error::Error>> {
    for (fen, movement, placement) in [
        (
            "4k3/8/8/8/8/8/8/6KR w H - 0 1",
            "g1h1",
            "4k3/8/8/8/8/8/8/5RK1",
        ),
        (
            "4k3/8/8/8/8/8/8/4KR2 w F - 0 1",
            "e1f1",
            "4k3/8/8/8/8/8/8/5RK1",
        ),
    ] {
        let initial = BoardPosition::from_fen(fen, BoardEnumVariant::Chess960)?;
        let definition = PuzzleFixture::definition(initial, &[&[movement]])?;
        let node = definition
            .solution()
            .node(definition.solution().lines()[0].nodes()[0])
            .ok_or("Missing castle")?;
        assert_eq!(node.position().variant(), BoardEnumVariant::Chess960);
        assert_eq!(node.position().placement(), placement);
        assert_eq!(
            node.movement()
                .ok_or("Missing castling move")?
                .uci(BoardEnumVariant::Chess960),
            movement
        );
        assert!(
            node.movement()
                .ok_or("Missing castling move")?
                .castling_rook()
                .is_some()
        );
        PuzzleDefinitionFixture::assert_paths(definition.solution())?;
    }
    Ok(())
}

#[test]
fn refuses_counter_overflow_and_moves_after_checkmate() -> Result<(), Box<dyn std::error::Error>> {
    let maximum = BoardPosition::from_fen(
        "4k3/8/8/8/8/8/8/4K3 b - - 0 1000000",
        BoardEnumVariant::Standard,
    )?;
    let movement = maximum.parse_uci("e8e7")?;
    assert!(matches!(
        PuzzleDefinition::new(
            PuzzleFixture::identity()?,
            maximum,
            vec![vec![movement]],
            PuzzleDefinitionMetadata::default()
        ),
        Err(PuzzleDefinitionTreeError::IllegalMove {
            line: 0,
            ply: 0,
            ..
        })
    ));
    let initial = BoardPosition::standard();
    let mut mate = PuzzleFixture::line(&initial, &["f2f3", "e7e5", "g2g4", "d8h4"])?;
    mate.push(initial.parse_uci("a2a3")?);
    assert!(matches!(
        PuzzleDefinition::new(
            PuzzleFixture::identity()?,
            initial,
            vec![mate],
            PuzzleDefinitionMetadata::default()
        ),
        Err(PuzzleDefinitionTreeError::IllegalMove {
            line: 0,
            ply: 4,
            ..
        })
    ));
    Ok(())
}

#[test]
fn preserves_exact_source_identity_without_trimming_or_folding()
-> Result<(), Box<dyn std::error::Error>> {
    let identity = PuzzleDefinitionIdentity::new(
        "personal/source".into(),
        "version-2".into(),
        " Åß id ".into(),
    )?;
    assert_eq!(identity.source(), "personal/source");
    assert_eq!(identity.version(), "version-2");
    assert_eq!(identity.puzzle_id(), " Åß id ");
    for other in [
        PuzzleDefinitionIdentity::new("other".into(), "version-2".into(), " Åß id ".into())?,
        PuzzleDefinitionIdentity::new(
            "personal/source".into(),
            "version-3".into(),
            " Åß id ".into(),
        )?,
        PuzzleDefinitionIdentity::new(
            "personal/source".into(),
            "version-2".into(),
            "Åß id".into(),
        )?,
    ] {
        assert_ne!(identity, other);
    }
    assert!(matches!(
        PuzzleDefinitionIdentity::new(String::new(), "v1".into(), "id".into()),
        Err(PuzzleDefinitionIdentityError::EmptySource)
    ));
    assert!(matches!(
        PuzzleDefinitionIdentity::new("source".into(), String::new(), "id".into()),
        Err(PuzzleDefinitionIdentityError::EmptyVersion)
    ));
    assert!(matches!(
        PuzzleDefinitionIdentity::new("source".into(), "v1".into(), String::new()),
        Err(PuzzleDefinitionIdentityError::EmptyPuzzleId)
    ));
    Ok(())
}

#[test]
fn validates_unsigned_difficulty_without_inventing_a_rating_range()
-> Result<(), Box<dyn std::error::Error>> {
    for (raw, value) in [("0", 0), ("4294967295", u32::MAX), ("001250", 1250)] {
        assert_eq!(raw.parse::<PuzzleDefinitionDifficulty>()?.rating(), value);
    }
    for raw in [
        "",
        "-1",
        "+1",
        " 1200",
        "1200 ",
        "1.5",
        "4294967296",
        "unknown",
        "１２",
    ] {
        assert!(raw.parse::<PuzzleDefinitionDifficulty>().is_err(), "{raw}");
    }
    let metadata =
        PuzzleDefinitionMetadata::new(Some(PuzzleDefinitionDifficulty::new(1250)), None)?;
    let initial = BoardPosition::standard();
    let line = PuzzleFixture::line(&initial, &["e2e4"])?;
    let definition =
        PuzzleDefinition::new(PuzzleFixture::identity()?, initial, vec![line], metadata)?;
    assert_eq!(
        definition
            .metadata()
            .difficulty()
            .map(PuzzleDefinitionDifficulty::rating),
        Some(1250)
    );
    assert_eq!(definition.metadata().themes(), None);
    Ok(())
}

#[test]
fn validates_optional_theme_tokens_preserving_case_and_order()
-> Result<(), Box<dyn std::error::Error>> {
    for raw in ["", " ", "two words", "fork\t", "fork\n"] {
        assert!(raw.parse::<PuzzleDefinitionTheme>().is_err());
    }
    let themes = ["fork", "Fork", "échec", "non\u{a0}breaking"]
        .into_iter()
        .map(str::parse)
        .collect::<Result<Vec<PuzzleDefinitionTheme>, _>>()?;
    let metadata = PuzzleDefinitionMetadata::new(None, Some(themes))?;
    assert_eq!(
        metadata
            .themes()
            .ok_or("Missing themes")?
            .iter()
            .map(PuzzleDefinitionTheme::as_str)
            .collect::<Vec<_>>(),
        ["fork", "Fork", "échec", "non\u{a0}breaking"]
    );
    assert_eq!(metadata.difficulty(), None);
    assert!(matches!(
        PuzzleDefinitionMetadata::new(None, Some(Vec::new())),
        Err(PuzzleDefinitionMetadataError::EmptyThemes)
    ));
    assert!(matches!(
        PuzzleDefinitionMetadata::new(None, Some(vec!["fork".parse()?, "fork".parse()?])),
        Err(PuzzleDefinitionMetadataError::DuplicateTheme { .. })
    ));
    assert_eq!(PuzzleDefinitionMetadata::default().themes(), None);
    Ok(())
}
