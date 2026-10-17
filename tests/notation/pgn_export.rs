use std::cell::Cell;
use std::io::{self, Cursor, Write};

use chess::board::enums::{BoardEnumColor, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::board::start::{BoardStart, BoardStartIndex};
use chess::clock::duration::ClockDuration;
use chess::game::enums::{GameEnumMarkingColor, GameEnumResult, GameMoveId};
use chess::game::move_node::{GameMoveNodeAnnotations, GameMoveNodeMarking, GameMoveNodeNag};
use chess::game::session::{
    GameMetadata, GameMetadataDate, GameMetadataExtension, GameMetadataKnownTags,
    GameMetadataOpeningCode, GameMetadataText,
};
use chess::game::tree::GameTree;
use chess::notation::game::{
    NotationGame, NotationGameClock, NotationGameEnumWarning, NotationGameParts,
};
use chess::notation::game_identity::NotationGameIdentityEnumResult;
use chess::notation::pgn_export::{
    NotationPgnExport, NotationPgnExportEnumCommentIssue, NotationPgnExportEnumSelection,
    NotationPgnExportError,
};
use chess::notation::pgn_import::NotationPgnImport;

struct NotationPgnExportFixture {
    parts: NotationGameParts,
    next_id: u128,
}

impl NotationPgnExportFixture {
    fn id(value: u128) -> Result<GameMoveId, chess::game::enums::GameIdentityError> {
        uuid::Uuid::from_u128(value).to_string().parse()
    }

    fn new(start: BoardStart) -> Result<Self, Box<dyn std::error::Error>> {
        let root = Self::id(1)?;
        let tree = GameTree::new(root, start.position().clone(), None);
        Ok(Self {
            parts: NotationGameParts {
                start,
                tree,
                mainline_tip: root,
                metadata: GameMetadata::default(),
                result: NotationGameIdentityEnumResult::Unfinished,
                result_header: None,
                time_control: None,
                clocks: Vec::new(),
                warnings: Vec::new(),
            },
            next_id: 2,
        })
    }

    fn standard() -> Result<Self, Box<dyn std::error::Error>> {
        Self::new(BoardStart::standard())
    }

    fn custom(fen: &str, variant: BoardEnumVariant) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new(BoardStart::custom(BoardPosition::from_fen(fen, variant)?))
    }

    fn append(
        &mut self,
        parent: GameMoveId,
        uci: &str,
    ) -> Result<GameMoveId, Box<dyn std::error::Error>> {
        let id = Self::id(self.next_id)?;
        self.next_id += 1;
        let movement = self.parts.tree.node(parent)?.position().parse_uci(uci)?;
        Ok(self.parts.tree.append(parent, id, movement, None)?)
    }

    fn line(&mut self, moves: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
        for uci in moves {
            self.parts.mainline_tip = self.append(self.parts.mainline_tip, uci)?;
        }
        Ok(())
    }

    fn notes(&mut self, node: GameMoveId, notes: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.parts.tree.set_annotations(
            node,
            GameMoveNodeAnnotations::new(notes.to_owned(), Vec::new(), Vec::new())?,
        )?;
        Ok(())
    }

    fn game(self) -> Result<NotationGame, Box<dyn std::error::Error>> {
        Ok(NotationGame::new(self.parts)?)
    }

    fn output(
        game: &NotationGame,
        selection: NotationPgnExportEnumSelection,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut bytes = Vec::new();
        NotationPgnExport::new(game)
            .with_selection(selection)
            .write(&mut bytes, || false)?;
        Ok(String::from_utf8(bytes)?)
    }

    fn import(pgn: &str) -> Result<NotationGame, Box<dyn std::error::Error>> {
        let mut next = 1000;
        let mut importer = NotationPgnImport::new(Cursor::new(pgn.as_bytes()), || {
            next += 1;
            Self::id(next)
        });
        importer
            .next_game(&|| false)?
            .ok_or_else(|| io::Error::other("The fixed PGN fixture did not yield a game").into())
    }

    fn annotated() -> Result<NotationGame, Box<dyn std::error::Error>> {
        let mut fixture = Self::standard()?;
        let root = fixture.parts.tree.root();
        fixture.notes(root, "Before play")?;
        fixture.line(&["e2e4", "e7e5"])?;
        let e4 = fixture.parts.tree.node(root)?.children()[0];
        fixture.parts.tree.set_annotations(
            e4,
            GameMoveNodeAnnotations::new(
                r#"Plain <b>text</b> \"quoted\" [%mystery retained]"#.to_owned(),
                vec![GameMoveNodeNag::new(1), GameMoveNodeNag::new(146)],
                vec![
                    GameMoveNodeMarking::circle(GameEnumMarkingColor::Green, "e4".parse()?),
                    GameMoveNodeMarking::arrow(
                        GameEnumMarkingColor::Red,
                        "d1".parse()?,
                        "h5".parse()?,
                    )?,
                    GameMoveNodeMarking::circle(GameEnumMarkingColor::Blue, "b2".parse()?),
                    GameMoveNodeMarking::arrow(
                        GameEnumMarkingColor::Yellow,
                        "a1".parse()?,
                        "a8".parse()?,
                    )?,
                ],
            )?,
        )?;
        fixture.parts.clocks.push(NotationGameClock::new(
            e4,
            ClockDuration::from_nanos(3_723_123_456_789),
        ));
        fixture
            .parts
            .warnings
            .push(NotationGameEnumWarning::UnknownDirective {
                node: e4,
                directive: "[%mystery retained]".to_owned(),
            });
        fixture.game()
    }
}

struct NotationPgnExportWriter {
    bytes: Vec<u8>,
    chunk: usize,
    largest_request: usize,
    fail_after: Option<usize>,
    interruptions: usize,
    flushes: usize,
}

impl NotationPgnExportWriter {
    fn new(chunk: usize) -> Self {
        Self {
            bytes: Vec::new(),
            chunk,
            largest_request: 0,
            fail_after: None,
            interruptions: 0,
            flushes: 0,
        }
    }
}

impl Write for NotationPgnExportWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.largest_request = self.largest_request.max(bytes.len());
        if self.interruptions > 0 {
            self.interruptions -= 1;
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        let remaining = match self.fail_after {
            Some(limit) => limit.saturating_sub(self.bytes.len()),
            None => usize::MAX,
        };
        if remaining == 0 {
            return Err(io::Error::other("Fixed destination failure"));
        }
        let length = bytes.len().min(self.chunk).min(remaining);
        self.bytes.extend_from_slice(&bytes[..length]);
        Ok(length)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

#[test]
fn default_full_export_has_a_fixed_canonical_standard_fixture()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    fixture.line(&["e2e4", "e7e5", "g1f3"])?;
    let game = fixture.game()?;
    let mut bytes = Vec::new();
    NotationPgnExport::new(&game).write(&mut bytes, || false)?;
    assert_eq!(
        String::from_utf8(bytes)?,
        "[Variant \"Standard\"]\n\n1. e4 e5 2. Nf3 *\n"
    );
    Ok(())
}

#[test]
fn absent_result_header_remains_absent_but_movetext_always_has_a_result()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    fixture.parts.result = NotationGameIdentityEnumResult::Completed(GameEnumResult::Draw);
    assert_eq!(
        NotationPgnExportFixture::output(&fixture.game()?, NotationPgnExportEnumSelection::Full)?,
        "[Variant \"Standard\"]\n\n1/2-1/2\n"
    );
    Ok(())
}

#[test]
fn explicit_result_header_and_each_result_token_are_preserved()
-> Result<(), Box<dyn std::error::Error>> {
    for (result, expected) in [
        (
            NotationGameIdentityEnumResult::Unfinished,
            "[Result \"*\"]\n[Variant \"Standard\"]\n\n*\n",
        ),
        (
            NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(
                BoardEnumColor::White,
            )),
            "[Result \"1-0\"]\n[Variant \"Standard\"]\n\n1-0\n",
        ),
        (
            NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(
                BoardEnumColor::Black,
            )),
            "[Result \"0-1\"]\n[Variant \"Standard\"]\n\n0-1\n",
        ),
        (
            NotationGameIdentityEnumResult::Completed(GameEnumResult::Draw),
            "[Result \"1/2-1/2\"]\n[Variant \"Standard\"]\n\n1/2-1/2\n",
        ),
    ] {
        let mut fixture = NotationPgnExportFixture::standard()?;
        fixture.parts.result = result;
        fixture.parts.result_header = Some(result);
        assert_eq!(
            NotationPgnExportFixture::output(
                &fixture.game()?,
                NotationPgnExportEnumSelection::Full
            )?,
            expected
        );
    }
    Ok(())
}

#[test]
fn display_strings_and_ordered_unknown_tags_escape_quotes_and_backslashes()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    fixture.parts.metadata = GameMetadata::new(
        GameMetadataKnownTags {
            white: Some(GameMetadataText::new("  Élodie ".to_owned())?),
            black: Some(GameMetadataText::new("Player \"B\"".to_owned())?),
            ..GameMetadataKnownTags::default()
        },
        vec![
            GameMetadataExtension::new(
                "Zulu".to_owned(),
                GameMetadataText::new("C:\\chess {raw}".to_owned())?,
            )?,
            GameMetadataExtension::new(
                "Alpha".to_owned(),
                GameMetadataText::new("say \"hello\"".to_owned())?,
            )?,
        ],
    )?;
    assert_eq!(
        NotationPgnExportFixture::output(&fixture.game()?, NotationPgnExportEnumSelection::Full)?,
        "[White \"  Élodie \"]\n[Black \"Player \\\"B\\\"\"]\n[Variant \"Standard\"]\n[Zulu \"C:\\\\chess {raw}\"]\n[Alpha \"say \\\"hello\\\"\"]\n\n*\n"
    );
    Ok(())
}

#[test]
fn known_metadata_and_partial_date_use_fixed_pgn_tag_names()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    fixture.parts.metadata = GameMetadata::new(
        GameMetadataKnownTags {
            event: Some(GameMetadataText::new("Study".to_owned())?),
            site: Some(GameMetadataText::new("Home".to_owned())?),
            date: Some(GameMetadataDate::new(Some(2026), None, Some(7))?),
            round: Some(GameMetadataText::new("?".to_owned())?),
            white_elo: Some(2400),
            black_elo: Some(2399),
            opening: Some(GameMetadataText::new("King Pawn".to_owned())?),
            variation: Some(GameMetadataText::new("Study line".to_owned())?),
            eco: Some(GameMetadataOpeningCode::new("C20".to_owned())?),
            annotator: Some(GameMetadataText::new("Owner".to_owned())?),
            ..GameMetadataKnownTags::default()
        },
        Vec::new(),
    )?;
    fixture.parts.time_control = Some(GameMetadataText::new("40/7200:3600+30".to_owned())?);
    assert_eq!(
        NotationPgnExportFixture::output(&fixture.game()?, NotationPgnExportEnumSelection::Full)?,
        "[Event \"Study\"]\n[Site \"Home\"]\n[Date \"2026.??.07\"]\n[Round \"?\"]\n[WhiteElo \"2400\"]\n[BlackElo \"2399\"]\n[Opening \"King Pawn\"]\n[Variation \"Study line\"]\n[ECO \"C20\"]\n[Annotator \"Owner\"]\n[TimeControl \"40/7200:3600+30\"]\n[Variant \"Standard\"]\n\n*\n"
    );
    Ok(())
}

#[test]
fn full_export_selects_the_explicit_path_and_keeps_alternative_sibling_order()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    let root = fixture.parts.tree.root();
    let e4 = fixture.append(root, "e2e4")?;
    fixture.append(e4, "e7e5")?;
    let d4 = fixture.append(root, "d2d4")?;
    let d5 = fixture.append(d4, "d7d5")?;
    let nf6 = fixture.append(d4, "g8f6")?;
    fixture.append(d5, "c2c4")?;
    fixture.append(root, "c2c4")?;
    fixture.parts.mainline_tip = nf6;
    let game = fixture.game()?;
    assert_eq!(
        NotationPgnExportFixture::output(&game, NotationPgnExportEnumSelection::Full)?,
        "[Variant \"Standard\"]\n\n1. d4 (1. e4 e5) (1. c4) 1... Nf6 (1... d5 2. c4) *\n"
    );
    assert_eq!(
        game.tree().node(root)?.children(),
        &[e4, d4, NotationPgnExportFixture::id(8)?]
    );
    Ok(())
}

#[test]
fn mainline_selection_excludes_other_branches_and_their_annotations()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    let root = fixture.parts.tree.root();
    let e4 = fixture.append(root, "e2e4")?;
    fixture.notes(e4, "Excluded branch")?;
    let d4 = fixture.append(root, "d2d4")?;
    fixture.parts.mainline_tip = fixture.append(d4, "d7d5")?;
    assert_eq!(
        NotationPgnExportFixture::output(
            &fixture.game()?,
            NotationPgnExportEnumSelection::Mainline
        )?,
        "[Variant \"Standard\"]\n\n1. d4 d5 *\n"
    );
    Ok(())
}

#[test]
fn full_refuses_continuations_beyond_the_selected_tip_and_mainline_stops_there()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    fixture.line(&["e2e4"])?;
    let tip = fixture.parts.mainline_tip;
    fixture.append(tip, "e7e5")?;
    let game = fixture.game()?;
    let mut bytes = Vec::new();
    assert!(
        matches!(NotationPgnExport::new(&game).write(&mut bytes, || false), Err(NotationPgnExportError::MainlineHasContinuations { node }) if node == tip)
    );
    assert!(bytes.is_empty());
    assert_eq!(
        NotationPgnExportFixture::output(&game, NotationPgnExportEnumSelection::Mainline)?,
        "[Variant \"Standard\"]\n\n1. e4 *\n"
    );
    Ok(())
}

#[test]
fn annotations_preserve_plaintext_nags_marking_order_and_nanosecond_clocks()
-> Result<(), Box<dyn std::error::Error>> {
    let game = NotationPgnExportFixture::annotated()?;
    assert_eq!(
        NotationPgnExportFixture::output(&game, NotationPgnExportEnumSelection::Full)?,
        concat!(
            "[Variant \"Standard\"]\n\n{Before play} 1. e4 $1 $146 {",
            r#"Plain <b>text</b> \"quoted\" [%mystery retained]"#,
            "[%csl Ge4][%cal Rd1h5][%csl Bb2][%cal Ya1a8][%clk 1:02:03.123456789]} 1... e5 *\n"
        )
    );
    assert_eq!(game.warnings().len(), 1);
    Ok(())
}

#[test]
fn root_nags_markings_and_clock_are_not_dropped() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    let root = fixture.parts.tree.root();
    fixture.parts.tree.set_annotations(
        root,
        GameMoveNodeAnnotations::new(
            String::new(),
            vec![GameMoveNodeNag::new(0), GameMoveNodeNag::new(255)],
            vec![GameMoveNodeMarking::circle(
                GameEnumMarkingColor::Yellow,
                "a1".parse()?,
            )],
        )?,
    )?;
    fixture
        .parts
        .clocks
        .push(NotationGameClock::new(root, ClockDuration::ZERO));
    assert_eq!(
        NotationPgnExportFixture::output(&fixture.game()?, NotationPgnExportEnumSelection::Full)?,
        "[Variant \"Standard\"]\n\n$0 $255 {[%csl Ya1][%clk 0:00:00]} *\n"
    );
    Ok(())
}

#[test]
fn brace_notes_use_semicolon_lines_with_exact_empty_lines_and_no_brace_escape()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    fixture.line(&["e2e4", "e7e5"])?;
    let e4 = fixture
        .parts
        .tree
        .node(fixture.parts.tree.root())?
        .children()[0];
    fixture.notes(e4, "\n{literal} \\}\n\nlast\n")?;
    fixture
        .parts
        .clocks
        .push(NotationGameClock::new(e4, ClockDuration::from_nanos(1)));
    assert_eq!(
        NotationPgnExportFixture::output(&fixture.game()?, NotationPgnExportEnumSelection::Full)?,
        "[Variant \"Standard\"]\n\n1. e4 ;\n;{literal} \\}\n;\n;last\n;[%clk 0:00:00.000000001]\n1... e5 *\n"
    );
    Ok(())
}

#[test]
fn recognized_directive_text_is_refused_instead_of_becoming_an_annotation()
-> Result<(), Box<dyn std::error::Error>> {
    for notes in [
        "literal [%clk 0:00:01]",
        "literal [%csl Ge4]",
        "literal [%cal Ga1a8]",
        "literal [%clk\u{00a0}0:00:01]",
        "literal [%csl\u{2003}Ge4]",
    ] {
        let mut fixture = NotationPgnExportFixture::standard()?;
        let root = fixture.parts.tree.root();
        fixture.notes(root, notes)?;
        let game = fixture.game()?;
        assert!(
            matches!(NotationPgnExport::new(&game).write(&mut Vec::new(), || false), Err(NotationPgnExportError::UnsupportedComment { node, reason: NotationPgnExportEnumCommentIssue::RecognizedDirective }) if node == root)
        );
    }
    Ok(())
}

#[test]
fn carriage_returns_in_brace_notes_are_refused_instead_of_normalized()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    let root = fixture.parts.tree.root();
    fixture.notes(root, "{literal}\r\nnext")?;
    let game = fixture.game()?;
    assert!(matches!(
        NotationPgnExport::new(&game).write(&mut Vec::new(), || false),
        Err(NotationPgnExportError::UnsupportedComment {
            reason: NotationPgnExportEnumCommentIssue::CarriageReturnInLineComment,
            ..
        })
    ));
    Ok(())
}

#[test]
fn tag_line_breaks_are_refused_instead_of_rewriting_display_text()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    fixture.parts.metadata = GameMetadata::new(
        GameMetadataKnownTags {
            event: Some(GameMetadataText::new("first\nsecond".to_owned())?),
            ..GameMetadataKnownTags::default()
        },
        Vec::new(),
    )?;
    let game = fixture.game()?;
    assert!(
        matches!(NotationPgnExport::new(&game).write(&mut Vec::new(), || false), Err(NotationPgnExportError::UnsupportedTagValue { name }) if name == "Event")
    );
    Ok(())
}

#[test]
fn custom_black_start_has_setup_fen_and_correct_move_numbers()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::custom(
        "4k3/8/8/8/8/8/8/4K3 b - - 7 23",
        BoardEnumVariant::Standard,
    )?;
    fixture.line(&["e8f8", "e1f1"])?;
    assert_eq!(
        NotationPgnExportFixture::output(&fixture.game()?, NotationPgnExportEnumSelection::Full)?,
        "[Variant \"Standard\"]\n[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 b - - 7 23\"]\n\n23... Kf8 24. Kf1 *\n"
    );
    Ok(())
}

#[test]
fn standard_and_chess960_castling_use_the_board_canonical_san()
-> Result<(), Box<dyn std::error::Error>> {
    for (variant, fen, moves, expected) in [
        (
            BoardEnumVariant::Standard,
            "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
            ["e1g1", "e8c8"],
            "[Variant \"Standard\"]\n[SetUp \"1\"]\n[FEN \"r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1\"]\n\n1. O-O O-O-O *\n",
        ),
        (
            BoardEnumVariant::Chess960,
            "r3k2r/8/8/8/8/8/8/R3K2R w HAha - 0 1",
            ["e1h1", "e8a8"],
            "[Variant \"Chess960\"]\n[SetUp \"1\"]\n[FEN \"r3k2r/8/8/8/8/8/8/R3K2R w HAha - 0 1\"]\n\n1. O-O O-O-O *\n",
        ),
    ] {
        let mut fixture = NotationPgnExportFixture::custom(fen, variant)?;
        fixture.line(&moves)?;
        assert_eq!(
            NotationPgnExportFixture::output(
                &fixture.game()?,
                NotationPgnExportEnumSelection::Full
            )?,
            expected
        );
    }
    Ok(())
}

#[test]
fn chess960_standard_index_still_declares_variant_and_shredder_fen()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = NotationPgnExportFixture::new(BoardStart::chess960(BoardStartIndex::STANDARD)?)?;
    assert_eq!(
        NotationPgnExportFixture::output(&fixture.game()?, NotationPgnExportEnumSelection::Full)?,
        "[Variant \"Chess960\"]\n[SetUp \"1\"]\n[FEN \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w HAha - 0 1\"]\n\n*\n"
    );
    Ok(())
}

#[test]
fn checkmate_survives_canonical_san_export() -> Result<(), Box<dyn std::error::Error>> {
    let mut mate = NotationPgnExportFixture::standard()?;
    mate.line(&["f2f3", "e7e5", "g2g4", "d8h4"])?;
    mate.parts.result =
        NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(BoardEnumColor::Black));
    assert_eq!(
        NotationPgnExportFixture::output(&mate.game()?, NotationPgnExportEnumSelection::Full)?,
        "[Variant \"Standard\"]\n\n1. f3 e5 2. g4 Qh4# 0-1\n"
    );
    Ok(())
}

#[test]
fn cancellation_before_output_leaves_the_injected_destination_empty()
-> Result<(), Box<dyn std::error::Error>> {
    let game = NotationPgnExportFixture::standard()?.game()?;
    let mut writer = NotationPgnExportWriter::new(1);
    assert!(matches!(
        NotationPgnExport::new(&game).write(&mut writer, || true),
        Err(NotationPgnExportError::Cancelled)
    ));
    assert!(writer.bytes.is_empty());
    assert_eq!(writer.flushes, 0);
    Ok(())
}

#[test]
fn cancellation_during_partial_writes_stops_with_only_a_canonical_prefix()
-> Result<(), Box<dyn std::error::Error>> {
    let game = NotationPgnExportFixture::standard()?.game()?;
    let mut writer = NotationPgnExportWriter::new(1);
    let polls = Cell::new(0);
    let result = NotationPgnExport::new(&game).write(&mut writer, || {
        polls.set(polls.get() + 1);
        polls.get() >= 12
    });
    assert!(matches!(result, Err(NotationPgnExportError::Cancelled)));
    let expected = b"[Variant \"Standard\"]\n\n*\n";
    assert!(!writer.bytes.is_empty());
    assert!(writer.bytes.len() < expected.len());
    assert!(expected.starts_with(&writer.bytes));
    assert_eq!(writer.flushes, 0);
    Ok(())
}

#[test]
fn partial_writes_and_interrupted_io_are_completed_without_flush_or_close()
-> Result<(), Box<dyn std::error::Error>> {
    let game = NotationPgnExportFixture::standard()?.game()?;
    let mut writer = NotationPgnExportWriter::new(1);
    writer.interruptions = 3;
    NotationPgnExport::new(&game).write(&mut writer, || false)?;
    assert_eq!(writer.bytes, b"[Variant \"Standard\"]\n\n*\n");
    assert_eq!(writer.flushes, 0);
    writer.write_all(b"after")?;
    assert!(writer.bytes.ends_with(b"after"));
    Ok(())
}

#[test]
fn destination_failure_stops_at_the_first_error_without_a_result_marker()
-> Result<(), Box<dyn std::error::Error>> {
    let game = NotationPgnExportFixture::standard()?.game()?;
    let mut writer = NotationPgnExportWriter::new(3);
    writer.fail_after = Some(9);
    assert!(
        matches!(NotationPgnExport::new(&game).write(&mut writer, || false), Err(NotationPgnExportError::Io(error)) if error.kind() == io::ErrorKind::Other)
    );
    assert_eq!(writer.bytes, b"[Variant ");
    assert_eq!(writer.flushes, 0);
    Ok(())
}

#[test]
fn zero_length_writes_are_typed_failures() -> Result<(), Box<dyn std::error::Error>> {
    let game = NotationPgnExportFixture::standard()?.game()?;
    assert!(
        matches!(NotationPgnExport::new(&game).write(&mut NotationPgnExportWriter::new(0), || false), Err(NotationPgnExportError::Io(error)) if error.kind() == io::ErrorKind::WriteZero)
    );
    Ok(())
}

#[test]
fn annotated_full_export_round_trips_through_the_real_importer()
-> Result<(), Box<dyn std::error::Error>> {
    let game = NotationPgnExportFixture::annotated()?;
    let pgn = NotationPgnExportFixture::output(&game, NotationPgnExportEnumSelection::Full)?;
    let restored = NotationPgnExportFixture::import(&pgn)?;
    assert_eq!(
        NotationPgnExportFixture::output(&restored, NotationPgnExportEnumSelection::Full)?,
        pgn
    );
    assert_eq!(restored.result_header(), None);
    assert_eq!(restored.warnings().len(), 1);
    assert_eq!(restored.tree().len(), game.tree().len());
    let original_move = game.tree().path_to(game.mainline_tip())?[1];
    let restored_move = restored.tree().path_to(restored.mainline_tip())?[1];
    assert_eq!(
        game.tree().node(original_move)?.annotations(),
        restored.tree().node(restored_move)?.annotations()
    );
    assert_eq!(
        restored.clock(restored_move),
        Some(ClockDuration::from_nanos(3_723_123_456_789))
    );
    Ok(())
}

#[test]
fn semicolon_brace_notes_round_trip_leading_trailing_and_blank_lines()
-> Result<(), Box<dyn std::error::Error>> {
    for notes in [
        "{literal}",
        "\n{literal}\n\n",
        "  {literal} \\} ; text  ",
        "}only close",
        "only {open",
    ] {
        let mut fixture = NotationPgnExportFixture::standard()?;
        let root = fixture.parts.tree.root();
        fixture.notes(root, notes)?;
        fixture
            .parts
            .clocks
            .push(NotationGameClock::new(root, ClockDuration::from_nanos(42)));
        let pgn = NotationPgnExportFixture::output(
            &fixture.game()?,
            NotationPgnExportEnumSelection::Full,
        )?;
        let restored = NotationPgnExportFixture::import(&pgn)?;
        assert_eq!(
            restored
                .tree()
                .node(restored.tree().root())?
                .annotations()
                .notes(),
            notes
        );
        assert_eq!(
            restored.clock(restored.tree().root()),
            Some(ClockDuration::from_nanos(42))
        );
    }
    Ok(())
}

#[test]
fn fixed_variation_fixture_round_trips_full_and_selected_mainline()
-> Result<(), Box<dyn std::error::Error>> {
    let pgn =
        "[Variant \"Standard\"]\n\n1. d4 (1. e4 e5 (1... c5)) (1. c4) 1... Nf6 (1... d5 2. c4) *\n";
    let game = NotationPgnExportFixture::import(pgn)?;
    assert_eq!(
        NotationPgnExportFixture::output(&game, NotationPgnExportEnumSelection::Full)?,
        pgn
    );
    let mainline =
        NotationPgnExportFixture::output(&game, NotationPgnExportEnumSelection::Mainline)?;
    assert_eq!(mainline, "[Variant \"Standard\"]\n\n1. d4 Nf6 *\n");
    let restored = NotationPgnExportFixture::import(&mainline)?;
    assert_eq!(restored.tree().len(), 3);
    Ok(())
}

#[test]
fn escaped_metadata_round_trip_preserves_order_display_and_missing_result_header()
-> Result<(), Box<dyn std::error::Error>> {
    let pgn = "[White \" Élodie \"]\n[Variant \"Standard\"]\n[Zulu \"C:\\\\chess {raw}\"]\n[Alpha \"say \\\"hello\\\"\"]\n\n*\n";
    let game = NotationPgnExportFixture::import(pgn)?;
    assert_eq!(
        NotationPgnExportFixture::output(&game, NotationPgnExportEnumSelection::Full)?,
        pgn
    );
    assert_eq!(
        game.metadata()
            .known()
            .white
            .as_ref()
            .map(GameMetadataText::as_str),
        Some(" Élodie ")
    );
    assert_eq!(game.metadata().extensions()[0].name(), "Zulu");
    assert_eq!(game.metadata().extensions()[1].name(), "Alpha");
    assert_eq!(game.result_header(), None);
    Ok(())
}

#[test]
fn clock_fraction_boundaries_round_trip_without_losing_native_precision()
-> Result<(), Box<dyn std::error::Error>> {
    for (nanos, expected) in [
        (0, "[Variant \"Standard\"]\n\n{[%clk 0:00:00]} *\n"),
        (
            1,
            "[Variant \"Standard\"]\n\n{[%clk 0:00:00.000000001]} *\n",
        ),
        (
            1_230_000_000,
            "[Variant \"Standard\"]\n\n{[%clk 0:00:01.23]} *\n",
        ),
        (
            u64::MAX,
            "[Variant \"Standard\"]\n\n{[%clk 5124095:34:33.709551615]} *\n",
        ),
    ] {
        let mut fixture = NotationPgnExportFixture::standard()?;
        fixture.parts.clocks.push(NotationGameClock::new(
            fixture.parts.tree.root(),
            ClockDuration::from_nanos(nanos),
        ));
        let pgn = NotationPgnExportFixture::output(
            &fixture.game()?,
            NotationPgnExportEnumSelection::Full,
        )?;
        assert_eq!(pgn, expected);
        let restored = NotationPgnExportFixture::import(&pgn)?;
        assert_eq!(
            restored.clock(restored.tree().root()),
            Some(ClockDuration::from_nanos(nanos))
        );
    }
    Ok(())
}

#[test]
fn semicolon_notes_at_a_variation_end_do_not_comment_out_the_closing_parenthesis()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    fixture.line(&["e2e4", "e7e5"])?;
    let root = fixture.parts.tree.root();
    let d4 = fixture.append(root, "d2d4")?;
    fixture.notes(d4, "{branch}\n")?;
    let game = fixture.game()?;
    let expected = "[Variant \"Standard\"]\n\n1. e4 (1. d4 ;{branch}\n;\n) 1... e5 *\n";
    let pgn = NotationPgnExportFixture::output(&game, NotationPgnExportEnumSelection::Full)?;
    assert_eq!(pgn, expected);
    let restored = NotationPgnExportFixture::import(&pgn)?;
    assert_eq!(
        NotationPgnExportFixture::output(&restored, NotationPgnExportEnumSelection::Full)?,
        expected
    );
    Ok(())
}

#[test]
fn bare_backslashes_and_whitespace_in_brace_comments_round_trip_literally()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    let notes = "  \\keep\\path\n\nlast  \n";
    fixture.notes(fixture.parts.tree.root(), notes)?;
    let pgn =
        NotationPgnExportFixture::output(&fixture.game()?, NotationPgnExportEnumSelection::Full)?;
    assert_eq!(
        pgn,
        "[Variant \"Standard\"]\n\n{  \\keep\\path\n\nlast  \n} *\n"
    );
    let restored = NotationPgnExportFixture::import(&pgn)?;
    assert_eq!(
        restored
            .tree()
            .node(restored.tree().root())?
            .annotations()
            .notes(),
        notes
    );
    Ok(())
}

#[test]
fn custom_black_start_and_chess960_castling_round_trip_positions()
-> Result<(), Box<dyn std::error::Error>> {
    for (fen, variant, moves) in [
        (
            "4k3/8/8/8/8/8/8/4K3 b - - 7 23",
            BoardEnumVariant::Standard,
            ["e8f8", "e1f1"],
        ),
        (
            "r3k2r/8/8/8/8/8/8/R3K2R w HAha - 0 1",
            BoardEnumVariant::Chess960,
            ["e1h1", "e8a8"],
        ),
    ] {
        let mut fixture = NotationPgnExportFixture::custom(fen, variant)?;
        fixture.line(&moves)?;
        let game = fixture.game()?;
        let pgn = NotationPgnExportFixture::output(&game, NotationPgnExportEnumSelection::Full)?;
        let restored = NotationPgnExportFixture::import(&pgn)?;
        assert_eq!(restored.start().position(), game.start().position());
        assert_eq!(
            restored.tree().node(restored.mainline_tip())?.position(),
            game.tree().node(game.mainline_tip())?.position()
        );
        assert_eq!(
            NotationPgnExportFixture::output(&restored, NotationPgnExportEnumSelection::Full)?,
            pgn
        );
    }
    Ok(())
}

#[test]
fn large_notes_are_streamed_as_bounded_write_requests() -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    let notes = "界".repeat(8193);
    fixture.notes(fixture.parts.tree.root(), &notes)?;
    let game = fixture.game()?;
    let mut writer = NotationPgnExportWriter::new(usize::MAX);
    NotationPgnExport::new(&game).write(&mut writer, || false)?;
    assert!(writer.largest_request <= 4096);
    let pgn = String::from_utf8(writer.bytes)?;
    let restored = NotationPgnExportFixture::import(&pgn)?;
    assert_eq!(
        restored
            .tree()
            .node(restored.tree().root())?
            .annotations()
            .notes(),
        notes
    );
    Ok(())
}

#[test]
fn annotations_on_excluded_branches_do_not_fail_mainline_export()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    fixture.line(&["d2d4"])?;
    let root = fixture.parts.tree.root();
    let alternative = fixture.append(root, "e2e4")?;
    fixture.notes(alternative, "literal [%clk 0:00:01]")?;
    let game = fixture.game()?;
    assert_eq!(
        NotationPgnExportFixture::output(&game, NotationPgnExportEnumSelection::Mainline)?,
        "[Variant \"Standard\"]\n\n1. d4 *\n"
    );
    assert!(
        matches!(NotationPgnExport::new(&game).write(&mut Vec::new(), || false), Err(NotationPgnExportError::UnsupportedComment { node, .. }) if node == alternative)
    );
    Ok(())
}

#[test]
fn each_promotion_choice_retains_standard_san_letters() -> Result<(), Box<dyn std::error::Error>> {
    for (uci, expected) in [
        (
            "a7a8q",
            "[Variant \"Standard\"]\n[SetUp \"1\"]\n[FEN \"7k/P7/8/8/8/8/8/7K w - - 0 1\"]\n\n1. a8=Q+ *\n",
        ),
        (
            "a7a8r",
            "[Variant \"Standard\"]\n[SetUp \"1\"]\n[FEN \"7k/P7/8/8/8/8/8/7K w - - 0 1\"]\n\n1. a8=R+ *\n",
        ),
        (
            "a7a8b",
            "[Variant \"Standard\"]\n[SetUp \"1\"]\n[FEN \"7k/P7/8/8/8/8/8/7K w - - 0 1\"]\n\n1. a8=B *\n",
        ),
        (
            "a7a8n",
            "[Variant \"Standard\"]\n[SetUp \"1\"]\n[FEN \"7k/P7/8/8/8/8/8/7K w - - 0 1\"]\n\n1. a8=N *\n",
        ),
    ] {
        let mut fixture = NotationPgnExportFixture::custom(
            "7k/P7/8/8/8/8/8/7K w - - 0 1",
            BoardEnumVariant::Standard,
        )?;
        fixture.line(&[uci])?;
        let pgn = NotationPgnExportFixture::output(
            &fixture.game()?,
            NotationPgnExportEnumSelection::Full,
        )?;
        assert_eq!(pgn, expected);
        let restored = NotationPgnExportFixture::import(&pgn)?;
        assert_eq!(
            NotationPgnExportFixture::output(&restored, NotationPgnExportEnumSelection::Full)?,
            expected
        );
    }
    Ok(())
}

#[test]
fn en_passant_capture_uses_existing_board_san_and_round_trips()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::custom(
        "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
        BoardEnumVariant::Standard,
    )?;
    fixture.line(&["e5d6"])?;
    let game = fixture.game()?;
    let pgn = NotationPgnExportFixture::output(&game, NotationPgnExportEnumSelection::Full)?;
    assert_eq!(
        pgn,
        "[Variant \"Standard\"]\n[SetUp \"1\"]\n[FEN \"4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1\"]\n\n1. exd6 *\n"
    );
    let restored = NotationPgnExportFixture::import(&pgn)?;
    assert_eq!(
        restored.tree().node(restored.mainline_tip())?.position(),
        game.tree().node(game.mainline_tip())?.position()
    );
    Ok(())
}

#[test]
fn unavailable_ratings_preserve_the_exact_raw_tag_value() -> Result<(), Box<dyn std::error::Error>>
{
    let mut fixture = NotationPgnExportFixture::standard()?;
    fixture
        .parts
        .warnings
        .push(NotationGameEnumWarning::UnavailableRating {
            color: BoardEnumColor::White,
            value: GameMetadataText::new(" ? ".to_owned())?,
        });
    fixture
        .parts
        .warnings
        .push(NotationGameEnumWarning::UnavailableRating {
            color: BoardEnumColor::Black,
            value: GameMetadataText::new(r#"invalid "rating" \raw"#.to_owned())?,
        });
    let game = fixture.game()?;
    assert_eq!(game.metadata().known().white_elo, None);
    assert_eq!(game.metadata().known().black_elo, None);
    assert_eq!(
        NotationPgnExportFixture::output(&game, NotationPgnExportEnumSelection::Full)?,
        "[WhiteElo \" ? \"]\n[BlackElo \"invalid \\\"rating\\\" \\\\raw\"]\n[Variant \"Standard\"]\n\n*\n"
    );
    Ok(())
}

#[test]
fn unavailable_rating_and_unknown_directive_warnings_round_trip_together()
-> Result<(), Box<dyn std::error::Error>> {
    let pgn = "[WhiteElo \"?\"]\n[BlackElo \"999999999999999999999999\"]\n[Variant \"Standard\"]\n\n{[%mystery retained]} *\n";
    let game = NotationPgnExportFixture::import(pgn)?;
    assert_eq!(game.metadata().known().white_elo, None);
    assert_eq!(game.metadata().known().black_elo, None);
    assert_eq!(game.warnings().len(), 3);
    assert!(game.warnings().iter().any(|warning| matches!(warning, NotationGameEnumWarning::UnknownDirective { directive, .. } if directive == "[%mystery retained]")));
    for color in [BoardEnumColor::White, BoardEnumColor::Black] {
        assert!(game.warnings().iter().any(|warning| matches!(warning, NotationGameEnumWarning::UnavailableRating { color: actual, .. } if *actual == color)));
    }
    assert_eq!(
        NotationPgnExportFixture::output(&game, NotationPgnExportEnumSelection::Full)?,
        pgn
    );
    Ok(())
}

#[test]
fn incomplete_unknown_directive_does_not_swallow_appended_clock_data()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    let root = fixture.parts.tree.root();
    let notes = "raw [%unfinished ";
    fixture.notes(root, notes)?;
    fixture
        .parts
        .clocks
        .push(NotationGameClock::new(root, ClockDuration::from_nanos(42)));
    let pgn =
        NotationPgnExportFixture::output(&fixture.game()?, NotationPgnExportEnumSelection::Full)?;
    assert_eq!(
        pgn,
        "[Variant \"Standard\"]\n\n{raw [%unfinished [%clk 0:00:00.000000042]} *\n"
    );
    let restored = NotationPgnExportFixture::import(&pgn)?;
    assert_eq!(
        restored
            .tree()
            .node(restored.tree().root())?
            .annotations()
            .notes(),
        notes
    );
    assert_eq!(
        restored.clock(restored.tree().root()),
        Some(ClockDuration::from_nanos(42))
    );
    Ok(())
}

#[test]
fn unknown_directive_names_that_extend_known_names_remain_plaintext()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NotationPgnExportFixture::standard()?;
    let notes = "[%clkExtra same] [%cslOther keep] [%calOther keep]";
    fixture.notes(fixture.parts.tree.root(), notes)?;
    let pgn =
        NotationPgnExportFixture::output(&fixture.game()?, NotationPgnExportEnumSelection::Full)?;
    assert_eq!(
        pgn,
        "[Variant \"Standard\"]\n\n{[%clkExtra same] [%cslOther keep] [%calOther keep]} *\n"
    );
    let restored = NotationPgnExportFixture::import(&pgn)?;
    assert_eq!(
        restored
            .tree()
            .node(restored.tree().root())?
            .annotations()
            .notes(),
        notes
    );
    assert_eq!(restored.warnings().len(), 3);
    Ok(())
}
