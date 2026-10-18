use std::io::{self, BufRead, Cursor, Read};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};

use tokio::sync::Notify;

use chess::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::{CapacityExecutor, CapacityExecutorJob};
use chess::capacity::phase::{
    CapacityPhase, CapacityPhaseEnumState, CapacityPhaseError, CapacityPhaseGeneration,
};
use chess::puzzle::csv_import::{
    PuzzleCsvImport, PuzzleCsvImportEnumContinuation, PuzzleCsvImportEnumField,
    PuzzleCsvImportError, PuzzleCsvImportIssueError, PuzzleCsvImportRow,
};
use chess::puzzle::definition::PuzzleDefinition;
use chess::puzzle::enums::PuzzleEnumCountdownMode;
use chess::puzzle::identity::PuzzleDefinitionIdentity;
use chess::puzzle::persistence::PuzzlePersistenceRun;
use chess::puzzle::random_provider::PuzzleRandomProviderSeeded;
use chess::puzzle::selection::{PuzzleSelectionDataset, PuzzleSelectionFilter};
use chess::puzzle::session::{PuzzleSessionConfig, PuzzleSessionId};

struct PuzzleCsvImportFixture;

impl PuzzleCsvImportFixture {
    const HEADER: &str =
        "PuzzleId,FEN,Moves,Rating,RatingDeviation,Popularity,NbPlays,Themes,GameUrl,OpeningTags";
    const FEN: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
    const START: &str = "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2";

    fn parser(
        bytes: Vec<u8>,
    ) -> Result<PuzzleCsvImport<Cursor<Vec<u8>>>, Box<dyn std::error::Error>> {
        Ok(PuzzleCsvImport::new(Cursor::new(bytes), Self::dataset()?))
    }

    fn dataset() -> Result<PuzzleSelectionDataset, Box<dyn std::error::Error>> {
        Ok(PuzzleSelectionDataset::from_identity(
            &PuzzleDefinitionIdentity::new(
                "personal/lichess".into(),
                "snapshot-1".into(),
                "seed".into(),
            )?,
        ))
    }

    fn row(moves: &str) -> String {
        format!("abc12,{},{}", Self::FEN, moves)
    }

    fn persisted(
        definition: PuzzleDefinition,
    ) -> Result<PuzzlePersistenceRun, Box<dyn std::error::Error>> {
        Ok(PuzzlePersistenceRun::new(
            PuzzleSessionId::new(std::num::NonZeroU64::MIN),
            PuzzleSessionConfig::defaults(PuzzleEnumCountdownMode::Session),
            vec![definition],
            PuzzleSelectionFilter::default(),
            PuzzleRandomProviderSeeded::new(5),
        )?)
    }

    fn cycle_moves(solution_plies: usize) -> String {
        let continuation = ["g1f3", "g8f6", "f3g1", "f6g8"]
            .into_iter()
            .cycle()
            .take(solution_plies)
            .collect::<Vec<_>>()
            .join(" ");
        format!("e7e5 {continuation}")
    }

    fn one(csv: String) -> Result<PuzzleCsvImportRow, Box<dyn std::error::Error>> {
        let mut parser = Self::parser(csv.into_bytes())?;
        let row = parser.next_row(|| false)?.ok_or("Missing validated row")?;
        assert!(parser.next_row(|| false)?.is_none());
        Ok(row)
    }

    fn assert_issue(
        csv: Vec<u8>,
        field: Option<PuzzleCsvImportEnumField>,
        matches: impl FnOnce(&PuzzleCsvImportIssueError) -> bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let error = Self::parser(csv)?
            .next_row(|| false)
            .err()
            .ok_or("Expected typed CSV issue")?;
        assert_eq!(error.field(), field);
        assert!(matches(error.issue()), "{error}");
        Ok(())
    }
}

#[test]
fn old_header_applies_black_setup_and_validates_entire_player_line()
-> Result<(), Box<dyn std::error::Error>> {
    let row = PuzzleCsvImportFixture::one(format!(
        "{}\r\n{},1250,75,-10,123,fork opening,https://example.invalid/game,Italian_Game\r\n",
        PuzzleCsvImportFixture::HEADER,
        PuzzleCsvImportFixture::row("e7e5 g1f3 b8c6 f1b5")
    ))?;
    let definition = row.definition();
    assert_eq!(row.logical_row(), 2);
    assert_eq!(
        definition.starting_position().fen(),
        PuzzleCsvImportFixture::START
    );
    assert_eq!(definition.solver(), BoardEnumColor::White);
    assert_eq!(definition.starting_position().halfmoves(), 0);
    assert_eq!(definition.starting_position().fullmoves(), 2);
    assert_eq!(row.setup_move().uci(BoardEnumVariant::Standard), "e7e5");
    assert_eq!(definition.identity().source(), "personal/lichess");
    assert_eq!(definition.identity().version(), "snapshot-1");
    assert_eq!(definition.identity().puzzle_id(), "abc12");
    assert_eq!(
        definition
            .metadata()
            .difficulty()
            .map(|rating| rating.rating()),
        Some(1250)
    );
    assert_eq!(
        definition
            .metadata()
            .themes()
            .ok_or("Missing themes")?
            .iter()
            .map(|theme| theme.as_str())
            .collect::<Vec<_>>(),
        ["fork", "opening"]
    );
    assert_eq!(row.source_metadata().rating_deviation(), Some(75));
    assert_eq!(row.source_metadata().popularity(), Some(-10));
    assert_eq!(row.source_metadata().plays(), Some(123));
    assert_eq!(row.source_metadata().daily_date(), None);
    let tree = definition.solution();
    assert_eq!(tree.lines().len(), 1);
    let nodes = tree.lines()[0].nodes();
    assert_eq!(nodes.len(), 3);
    let first = tree.node(nodes[0]).ok_or("Missing first solution node")?;
    assert_eq!(
        first
            .movement()
            .ok_or("Missing player move")?
            .uci(BoardEnumVariant::Standard),
        "g1f3"
    );
    assert_eq!(
        first.position().fen(),
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2"
    );
    let final_node = tree.node(nodes[2]).ok_or("Missing final node")?;
    assert!(final_node.is_terminal());
    assert_eq!(
        final_node.position().fen(),
        "r1bqkbnr/pppp1ppp/2n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 3 3"
    );
    Ok(())
}

#[test]
fn current_header_preserves_daily_date_and_all_raw_fields() -> Result<(), Box<dyn std::error::Error>>
{
    let row = PuzzleCsvImportFixture::one(format!(
        "{},DailyDate\n{},001250,075,+10,000123,fork,https://example.invalid/game,Italian_Game,2026-10-05\n",
        PuzzleCsvImportFixture::HEADER,
        PuzzleCsvImportFixture::row("e7e5 g1f3")
    ))?;
    let metadata = row.source_metadata();
    assert_eq!(metadata.daily_date(), Some("2026-10-05"));
    assert_eq!(metadata.game_url(), Some("https://example.invalid/game"));
    assert_eq!(metadata.opening_tags(), Some("Italian_Game"));
    assert_eq!(metadata.fields().len(), 11);
    assert_eq!(
        metadata.fields()[3].header().field(),
        Some(PuzzleCsvImportEnumField::Rating)
    );
    assert_eq!(metadata.fields()[3].value(), "001250");
    assert_eq!(metadata.popularity(), Some(10));
    Ok(())
}

#[test]
fn quoted_utf8_unknown_columns_preserve_commas_quotes_and_multiline_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let csv = format!(
        "Moves,\"méta,未知\",PuzzleId,FEN,Notes\r\n\"e7e5 g1f3\",\"café,\"\"quoted\"\"\",Åß,{},\"line one\r\nline two\n第三行\"\r\n",
        PuzzleCsvImportFixture::FEN
    );
    let mut parser = PuzzleCsvImportFixture::parser(csv.into_bytes())?;
    let row = parser.next_row(|| false)?.ok_or("Missing row")?;
    let headers = parser.headers().ok_or("Missing parsed headers")?;
    assert_eq!(headers[1].name(), "méta,未知");
    assert_eq!(headers[1].field(), None);
    let fields = row.source_metadata().fields();
    assert_eq!(fields[1].header(), &headers[1]);
    assert_eq!(fields[1].value(), "café,\"quoted\"");
    assert_eq!(fields[4].value(), "line one\r\nline two\n第三行");
    assert_eq!(row.definition().identity().puzzle_id(), "Åß");
    assert!(parser.next_row(|| false)?.is_none());
    Ok(())
}

#[test]
fn omitted_or_empty_optional_metadata_is_unavailable_but_zero_is_available()
-> Result<(), Box<dyn std::error::Error>> {
    for (header, suffix) in [
        ("PuzzleId,FEN,Moves", ""),
        (PuzzleCsvImportFixture::HEADER, ",,,,,,,"),
    ] {
        let row = PuzzleCsvImportFixture::one(format!(
            "{header}\n{}{suffix}",
            PuzzleCsvImportFixture::row("e7e5 g1f3")
        ))?;
        assert_eq!(row.definition().metadata().difficulty(), None);
        assert_eq!(row.definition().metadata().themes(), None);
        assert_eq!(row.source_metadata().rating_deviation(), None);
        assert_eq!(row.source_metadata().popularity(), None);
        assert_eq!(row.source_metadata().plays(), None);
    }
    let row = PuzzleCsvImportFixture::one(format!(
        "PuzzleId,FEN,Moves,Rating\n{},0",
        PuzzleCsvImportFixture::row("e7e5 g1f3")
    ))?;
    assert_eq!(
        row.definition()
            .metadata()
            .difficulty()
            .map(|rating| rating.rating()),
        Some(0)
    );
    Ok(())
}

#[test]
fn white_setup_retains_black_solver_and_nonzero_counters() -> Result<(), Box<dyn std::error::Error>>
{
    let row = PuzzleCsvImportFixture::one(
        "PuzzleId,FEN,Moves\nid,rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 17 42,g1f3 g8f6".into(),
    )?;
    assert_eq!(row.definition().solver(), BoardEnumColor::Black);
    assert_eq!(
        row.definition().starting_position().fen(),
        "rnbqkbnr/pppppppp/8/8/8/5N2/PPPPPPPP/RNBQKB1R b KQkq - 18 42"
    );
    Ok(())
}

#[test]
fn accepts_underpromotion_in_setup_and_solution_without_queen_substitution()
-> Result<(), Box<dyn std::error::Error>> {
    for (fen, moves, setup_promotion, solution_promotion, expected_start) in [
        (
            "7k/P7/8/8/8/8/8/7K b - - 9 23",
            "h8g7 a7a8n",
            None,
            Some(BoardEnumPieceKind::Knight),
            "8/P5k1/8/8/8/8/8/7K w - - 10 24",
        ),
        (
            "7k/P7/8/8/8/8/8/7K w - - 9 23",
            "a7a8b h8g7",
            Some(BoardEnumPieceKind::Bishop),
            None,
            "B6k/8/8/8/8/8/8/7K b - - 0 23",
        ),
    ] {
        let row = PuzzleCsvImportFixture::one(format!("PuzzleId,FEN,Moves\nid,{fen},{moves}"))?;
        assert_eq!(row.setup_move().promotion(), setup_promotion);
        assert_eq!(row.definition().starting_position().fen(), expected_start);
        let tree = row.definition().solution();
        let movement = tree
            .node(tree.lines()[0].nodes()[0])
            .and_then(|node| node.movement())
            .ok_or("Missing promotion move")?;
        assert_eq!(movement.promotion(), solution_promotion);
    }
    Ok(())
}

#[test]
fn rejects_missing_duplicate_and_empty_headers_before_any_row()
-> Result<(), Box<dyn std::error::Error>> {
    for header in [
        "FEN,Moves",
        "PuzzleId,Moves",
        "PuzzleId,FEN",
        "puzzleid,FEN,Moves",
    ] {
        PuzzleCsvImportFixture::assert_issue(header.as_bytes().to_vec(), None, |issue| {
            matches!(issue, PuzzleCsvImportIssueError::MissingHeader { .. })
        })?;
    }
    for name in ["PuzzleId", "Extra"] {
        let csv = format!("PuzzleId,FEN,Moves,{name},{name}\n");
        PuzzleCsvImportFixture::assert_issue(csv.into_bytes(), None, |issue| {
            matches!(issue, PuzzleCsvImportIssueError::DuplicateHeader { .. })
        })?;
    }
    PuzzleCsvImportFixture::assert_issue(b"PuzzleId,FEN,Moves,".to_vec(), None, |issue| {
        matches!(issue, PuzzleCsvImportIssueError::EmptyHeader)
    })?;
    PuzzleCsvImportFixture::assert_issue(Vec::new(), None, |issue| {
        matches!(issue, PuzzleCsvImportIssueError::MissingHeader { .. })
    })
}

#[test]
fn excludes_bad_width_and_late_illegal_move_then_previews_next_complete_row()
-> Result<(), Box<dyn std::error::Error>> {
    let csv = format!(
        "PuzzleId,FEN,Moves\nbad,width\n{}\n{}\n",
        PuzzleCsvImportFixture::row("e7e5 g1f3 b8c6 f1b5 a8a1"),
        PuzzleCsvImportFixture::row("e7e5 g1f3")
    );
    let mut parser = PuzzleCsvImportFixture::parser(csv.into_bytes())?;
    let cancel = AtomicBool::new(false);
    let width = parser
        .next_row(|| cancel.load(Ordering::Relaxed))
        .err()
        .ok_or("Missing width issue")?;
    assert_eq!(width.logical_row(), 2);
    assert!(matches!(
        width.issue(),
        PuzzleCsvImportIssueError::Width {
            expected: 3,
            actual: 2
        }
    ));
    assert_eq!(
        width.continuation(),
        PuzzleCsvImportEnumContinuation::NextRow
    );
    let illegal = parser
        .next_row(|| cancel.load(Ordering::Relaxed))
        .err()
        .ok_or("Missing late move issue")?;
    assert_eq!(illegal.logical_row(), 3);
    assert_eq!(illegal.field(), Some(PuzzleCsvImportEnumField::Moves));
    assert!(matches!(
        illegal.issue(),
        PuzzleCsvImportIssueError::Move { ply: 4, .. }
    ));
    assert_eq!(
        illegal.continuation(),
        PuzzleCsvImportEnumContinuation::NextRow
    );
    assert_eq!(
        parser
            .next_row(|| cancel.load(Ordering::Relaxed))?
            .ok_or("Missing next valid row")?
            .logical_row(),
        4
    );
    assert!(
        parser
            .next_row(|| cancel.load(Ordering::Relaxed))?
            .is_none()
    );
    Ok(())
}

#[test]
fn rejects_invalid_fen_identity_setup_short_lines_and_move_tokens()
-> Result<(), Box<dyn std::error::Error>> {
    PuzzleCsvImportFixture::assert_issue(
        format!(
            "PuzzleId,FEN,Moves\n,{},e7e5 g1f3",
            PuzzleCsvImportFixture::FEN
        )
        .into_bytes(),
        Some(PuzzleCsvImportEnumField::PuzzleId),
        |issue| matches!(issue, PuzzleCsvImportIssueError::Identity(_)),
    )?;
    for fen in [
        "not a fen".to_owned(),
        "x".repeat(513),
        "8/8/8/8/8/8/8/8 w - - 0 1".to_owned(),
    ] {
        PuzzleCsvImportFixture::assert_issue(
            format!("PuzzleId,FEN,Moves\nid,{fen},e7e5 g1f3").into_bytes(),
            Some(PuzzleCsvImportEnumField::Fen),
            |issue| matches!(issue, PuzzleCsvImportIssueError::Fen(_)),
        )?;
    }
    for moves in ["", "e7e5"] {
        PuzzleCsvImportFixture::assert_issue(
            format!("PuzzleId,FEN,Moves\n{}", PuzzleCsvImportFixture::row(moves)).into_bytes(),
            Some(PuzzleCsvImportEnumField::Moves),
            |issue| matches!(issue, PuzzleCsvImportIssueError::TooFewMoves),
        )?;
    }
    for (moves, ply) in [
        ("e7e4 g1f3", 0),
        ("e7e5 g1f4", 1),
        ("e7e5 g1f3 b8c6 broken", 3),
    ] {
        PuzzleCsvImportFixture::assert_issue(
            format!("PuzzleId,FEN,Moves\n{}", PuzzleCsvImportFixture::row(moves)).into_bytes(),
            Some(PuzzleCsvImportEnumField::Moves),
            |issue| matches!(issue, PuzzleCsvImportIssueError::Move { ply: found, .. } if *found == ply),
        )?;
    }
    for moves in ["e7e5\tg1f3", "e7e5  g1f3", " e7e5 g1f3", "e7e5 g1f3 "] {
        PuzzleCsvImportFixture::assert_issue(
            format!("PuzzleId,FEN,Moves\n{}", PuzzleCsvImportFixture::row(moves)).into_bytes(),
            Some(PuzzleCsvImportEnumField::Moves),
            |issue| matches!(issue, PuzzleCsvImportIssueError::MoveSeparator),
        )?;
    }
    Ok(())
}

#[test]
fn rejects_integer_syntax_range_and_duplicate_themes_with_field_context()
-> Result<(), Box<dyn std::error::Error>> {
    for (name, field, invalid) in [
        ("Rating", PuzzleCsvImportEnumField::Rating, "-1"),
        ("Rating", PuzzleCsvImportEnumField::Rating, "4294967296"),
        (
            "RatingDeviation",
            PuzzleCsvImportEnumField::RatingDeviation,
            "-1",
        ),
        (
            "RatingDeviation",
            PuzzleCsvImportEnumField::RatingDeviation,
            "4294967296",
        ),
        ("Popularity", PuzzleCsvImportEnumField::Popularity, "1.5"),
        (
            "Popularity",
            PuzzleCsvImportEnumField::Popularity,
            "2147483648",
        ),
        ("NbPlays", PuzzleCsvImportEnumField::NbPlays, "-1"),
        (
            "NbPlays",
            PuzzleCsvImportEnumField::NbPlays,
            "18446744073709551616",
        ),
        ("NbPlays", PuzzleCsvImportEnumField::NbPlays, "１２"),
    ] {
        let mut parser = PuzzleCsvImportFixture::parser(
            format!(
                "PuzzleId,FEN,Moves,{name}\n{},{invalid}",
                PuzzleCsvImportFixture::row("e7e5 g1f3")
            )
            .into_bytes(),
        )?;
        let error = parser
            .next_row(|| false)
            .err()
            .ok_or("Accepted invalid integer")?;
        assert_eq!(error.logical_row(), 2);
        assert_eq!(error.column(), Some(4));
        assert_eq!(error.field(), Some(field));
        assert_eq!(
            error.continuation(),
            PuzzleCsvImportEnumContinuation::NextRow
        );
        assert!(matches!(
            error.issue(),
            PuzzleCsvImportIssueError::IntegerSyntax
                | PuzzleCsvImportIssueError::Integer(_)
                | PuzzleCsvImportIssueError::Metadata(_)
        ));
    }
    PuzzleCsvImportFixture::assert_issue(
        format!(
            "PuzzleId,FEN,Moves,Themes\n{},fork fork",
            PuzzleCsvImportFixture::row("e7e5 g1f3")
        )
        .into_bytes(),
        Some(PuzzleCsvImportEnumField::Themes),
        |issue| matches!(issue, PuzzleCsvImportIssueError::Metadata(_)),
    )
}

#[test]
fn malformed_quote_or_record_delimiter_terminates_without_guessing_a_next_row()
-> Result<(), Box<dyn std::error::Error>> {
    for malformed in [
        "bad\"quote",
        "\"closed\"tail",
        "\"unclosed\nnext,line",
        "bare\rcarriage",
    ] {
        let mut parser = PuzzleCsvImportFixture::parser(
            format!(
                "PuzzleId,FEN,Moves\n{malformed}\n{}",
                PuzzleCsvImportFixture::row("e7e5 g1f3")
            )
            .into_bytes(),
        )?;
        let error = parser
            .next_row(|| false)
            .err()
            .ok_or("Accepted malformed CSV")?;
        assert_eq!(error.logical_row(), 2);
        assert_eq!(
            error.continuation(),
            PuzzleCsvImportEnumContinuation::Terminated
        );
        assert!(matches!(
            error.issue(),
            PuzzleCsvImportIssueError::UnexpectedQuote
                | PuzzleCsvImportIssueError::TrailingQuotedData
                | PuzzleCsvImportIssueError::UnclosedQuote
                | PuzzleCsvImportIssueError::BareCarriageReturn
        ));
        assert!(parser.next_row(|| false)?.is_none());
    }
    Ok(())
}

#[test]
fn invalid_utf8_is_typed_and_only_a_fully_framed_row_can_continue()
-> Result<(), Box<dyn std::error::Error>> {
    let mut csv = b"PuzzleId,FEN,Moves,Extra\n".to_vec();
    csv.extend_from_slice(PuzzleCsvImportFixture::row("e7e5 g1f3").as_bytes());
    csv.extend_from_slice(b",\xff\n");
    csv.extend_from_slice(PuzzleCsvImportFixture::row("e7e5 g1f3").as_bytes());
    csv.extend_from_slice(b",ok\n");
    let mut parser = PuzzleCsvImportFixture::parser(csv)?;
    let error = parser
        .next_row(|| false)
        .err()
        .ok_or("Accepted invalid UTF-8")?;
    assert!(matches!(error.issue(), PuzzleCsvImportIssueError::Utf8(_)));
    assert_eq!(error.column(), Some(4));
    assert_eq!(
        error.continuation(),
        PuzzleCsvImportEnumContinuation::NextRow
    );
    assert_eq!(
        parser
            .next_row(|| false)?
            .ok_or("Missing next row")?
            .logical_row(),
        3
    );
    let mut parser = PuzzleCsvImportFixture::parser(b"PuzzleId,FEN,Moves,\xff\n".to_vec())?;
    let header = parser
        .next_row(|| false)
        .err()
        .ok_or("Accepted invalid header UTF-8")?;
    assert_eq!(
        header.continuation(),
        PuzzleCsvImportEnumContinuation::Terminated
    );
    Ok(())
}

#[test]
fn exact_encoded_logical_row_bound_counts_quotes_and_embedded_crlf()
-> Result<(), Box<dyn std::error::Error>> {
    let prefix = format!("{},\"line\r\n", PuzzleCsvImportFixture::row("e7e5 g1f3"));
    let padding = PuzzleCsvImport::<Cursor<Vec<u8>>>::MAX_ROW_BYTES - prefix.len() - 1;
    for extra in [0, 1] {
        let mut csv = b"PuzzleId,FEN,Moves,Extra\r\n".to_vec();
        csv.extend_from_slice(prefix.as_bytes());
        csv.extend(std::iter::repeat_n(b'x', padding + extra));
        csv.extend_from_slice(b"\"\r\n");
        let mut parser = PuzzleCsvImportFixture::parser(csv)?;
        match parser.next_row(|| false) {
            Ok(Some(row)) => {
                assert_eq!(extra, 0);
                assert_eq!(row.source_metadata().fields()[3].value().len(), padding + 6);
            }
            Err(error) => {
                assert_eq!(extra, 1);
                assert!(matches!(
                    error.issue(),
                    PuzzleCsvImportIssueError::RowTooLarge { .. }
                ));
                assert_eq!(
                    error.continuation(),
                    PuzzleCsvImportEnumContinuation::Terminated
                );
            }
            Ok(None) => return Err("Missing row bound outcome".into()),
        }
    }
    Ok(())
}

struct PuzzleCsvImportControlledReader<'a> {
    bytes: Cursor<Vec<u8>>,
    stop_at: usize,
    cancel: Option<&'a AtomicBool>,
}

impl Read for PuzzleCsvImportControlledReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let count = output.len().min(available.len());
        output[..count].copy_from_slice(&available[..count]);
        self.consume(count);
        Ok(count)
    }
}

impl BufRead for PuzzleCsvImportControlledReader<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.bytes.position() >= u64::try_from(self.stop_at).map_err(io::Error::other)? {
            match self.cancel {
                Some(cancel) => cancel.store(true, Ordering::Relaxed),
                None => return Err(io::Error::other("fixed reader failure")),
            }
        }
        let available = self.bytes.fill_buf()?;
        Ok(&available[..available.len().min(1)])
    }

    fn consume(&mut self, amount: usize) {
        self.bytes.consume(amount);
    }
}

#[test]
fn reader_failure_is_terminal_and_preserves_io_cause_without_yielding_a_partial_row()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes = format!(
        "PuzzleId,FEN,Moves\n{}",
        PuzzleCsvImportFixture::row("e7e5 g1f3")
    );
    let reader = PuzzleCsvImportControlledReader {
        bytes: Cursor::new(bytes.into_bytes()),
        stop_at: 30,
        cancel: None,
    };
    let mut parser = PuzzleCsvImport::new(reader, PuzzleCsvImportFixture::dataset()?);
    let error = parser
        .next_row(|| false)
        .err()
        .ok_or("Missing read error")?;
    assert!(
        matches!(error.issue(), PuzzleCsvImportIssueError::Read(source) if source.kind() == io::ErrorKind::Other)
    );
    assert_eq!(
        error.continuation(),
        PuzzleCsvImportEnumContinuation::Terminated
    );
    assert!(parser.next_row(|| false)?.is_none());
    Ok(())
}

#[test]
fn cancellation_before_header_and_during_multiline_row_is_typed_and_terminal()
-> Result<(), Box<dyn std::error::Error>> {
    let mut parser = PuzzleCsvImportFixture::parser(Vec::new())?;
    let error = parser
        .next_row(|| true)
        .err()
        .ok_or("Missing early cancellation")?;
    assert!(matches!(
        error.issue(),
        PuzzleCsvImportIssueError::Cancelled
    ));
    let cancel = AtomicBool::new(false);
    let bytes = format!(
        "PuzzleId,FEN,Moves,Extra\n{},\"line one\nline two\"",
        PuzzleCsvImportFixture::row("e7e5 g1f3")
    );
    let stop_at = bytes.len() - "line two\"".len();
    let reader = PuzzleCsvImportControlledReader {
        bytes: Cursor::new(bytes.into_bytes()),
        stop_at,
        cancel: Some(&cancel),
    };
    let mut parser = PuzzleCsvImport::new(reader, PuzzleCsvImportFixture::dataset()?);
    let error = parser
        .next_row(|| cancel.load(Ordering::Relaxed))
        .err()
        .ok_or("Missing mid-row cancellation")?;
    assert_eq!(error.logical_row(), 2);
    assert!(matches!(
        error.issue(),
        PuzzleCsvImportIssueError::Cancelled
    ));
    assert_eq!(
        error.continuation(),
        PuzzleCsvImportEnumContinuation::Terminated
    );
    assert!(parser.next_row(|| false)?.is_none());
    Ok(())
}

#[test]
fn first_result_does_not_read_later_record_and_one_byte_buffers_are_supported()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes = format!(
        "PuzzleId,FEN,Moves\n{}\nsecond",
        PuzzleCsvImportFixture::row("e7e5 g1f3")
    );
    let stop_at = bytes.len() - "second".len();
    let reader = PuzzleCsvImportControlledReader {
        bytes: Cursor::new(bytes.into_bytes()),
        stop_at,
        cancel: None,
    };
    let mut parser = PuzzleCsvImport::new(reader, PuzzleCsvImportFixture::dataset()?);
    assert!(parser.next_row(|| false)?.is_some());
    let error = parser
        .next_row(|| false)
        .err()
        .ok_or("Expected later read error")?;
    assert_eq!(error.logical_row(), 3);
    assert!(matches!(error.issue(), PuzzleCsvImportIssueError::Read(_)));
    Ok(())
}

#[test]
fn over_limit_legal_solution_is_excluded_before_preview_and_next_row_remains_available()
-> Result<(), Box<dyn std::error::Error>> {
    let csv = format!(
        "PuzzleId,FEN,Moves\n{}\n{}\n",
        PuzzleCsvImportFixture::row(&PuzzleCsvImportFixture::cycle_moves(4_097)),
        PuzzleCsvImportFixture::row("e7e5 g1f3"),
    );
    let mut parser = PuzzleCsvImportFixture::parser(csv.into_bytes())?;
    let over_limit = parser.next_row(|| false);
    let next = parser
        .next_row(|| false)?
        .ok_or("missing following valid row")?;
    assert_eq!(next.logical_row(), 3);
    let error = over_limit
        .err()
        .ok_or("an unpersistable solution was accepted into preview")?;
    assert_eq!(error.field(), Some(PuzzleCsvImportEnumField::Moves));
    assert_eq!(
        error.continuation(),
        PuzzleCsvImportEnumContinuation::NextRow
    );
    assert!(matches!(
        error.issue(),
        PuzzleCsvImportIssueError::Definition(
            chess::puzzle::tree::PuzzleDefinitionTreeError::LineTooLong {
                actual: 4_097,
                limit: 4_096,
                ..
            }
        )
    ));
    Ok(())
}

#[test]
fn the_maximum_supported_solution_can_enter_the_real_persisted_run()
-> Result<(), Box<dyn std::error::Error>> {
    let row = PuzzleCsvImportFixture::one(format!(
        "PuzzleId,FEN,Moves\n{}",
        PuzzleCsvImportFixture::row(&PuzzleCsvImportFixture::cycle_moves(4_096)),
    ))?;
    assert_eq!(
        row.definition()
            .solution()
            .lines()
            .first()
            .ok_or("missing complete solution")?
            .nodes()
            .len(),
        4_096
    );
    let (definition, source_metadata) = row.into_parts();
    assert_eq!(
        source_metadata
            .fields()
            .iter()
            .find(|field| field.header().field() == Some(PuzzleCsvImportEnumField::Moves))
            .ok_or("missing original move metadata")?
            .value()
            .split(' ')
            .count(),
        4_097
    );
    PuzzleCsvImportFixture::persisted(definition)?;
    Ok(())
}

#[test]
fn identity_limit_accepts_a_persistable_multibyte_identifier_at_the_byte_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let id = "é".repeat(512);
    let row = PuzzleCsvImportFixture::one(format!(
        "PuzzleId,FEN,Moves\n{id},{},e7e5 g1f3",
        PuzzleCsvImportFixture::FEN,
    ))?;
    assert_eq!(row.definition().identity().puzzle_id(), id);
    PuzzleCsvImportFixture::persisted(row.into_parts().0)?;
    Ok(())
}

#[test]
fn identity_limit_excludes_an_unpersistable_identifier_and_continues_preview()
-> Result<(), Box<dyn std::error::Error>> {
    let id = "x".repeat(1_025);
    let mut parser = PuzzleCsvImportFixture::parser(
        format!(
            "PuzzleId,FEN,Moves\n{id},{},e7e5 g1f3\n{}\n",
            PuzzleCsvImportFixture::FEN,
            PuzzleCsvImportFixture::row("e7e5 g1f3"),
        )
        .into_bytes(),
    )?;
    let over_limit = parser.next_row(|| false);
    let following = parser
        .next_row(|| false)?
        .ok_or("missing next preview row")?;
    assert_eq!(following.logical_row(), 3);
    let error = over_limit
        .err()
        .ok_or("an unpersistable identifier entered valid preview")?;
    assert_eq!(error.field(), Some(PuzzleCsvImportEnumField::PuzzleId));
    assert_eq!(
        error.continuation(),
        PuzzleCsvImportEnumContinuation::NextRow
    );
    assert!(matches!(
        error.issue(),
        PuzzleCsvImportIssueError::Identity(_)
    ));
    Ok(())
}

#[derive(Default)]
struct PuzzleCsvImportExecutorGate {
    released: Mutex<bool>,
    changed: Condvar,
}

impl PuzzleCsvImportExecutorGate {
    fn poisoned<T>(_poisoned: std::sync::PoisonError<T>) -> io::Error {
        io::Error::other("CSV scan gate poisoned")
    }

    fn wait(&self) -> io::Result<()> {
        let mut released = self.released.lock().map_err(Self::poisoned)?;
        while !*released {
            released = self.changed.wait(released).map_err(Self::poisoned)?;
        }
        Ok(())
    }

    fn release(&self) -> io::Result<()> {
        *self.released.lock().map_err(Self::poisoned)? = true;
        self.changed.notify_all();
        Ok(())
    }
}

struct PuzzleCsvImportExecutorReader {
    bytes: Cursor<Vec<u8>>,
    stop_at: u64,
    entered: Option<mpsc::SyncSender<()>>,
    gate: Arc<PuzzleCsvImportExecutorGate>,
    observer: Arc<Notify>,
}

impl Drop for PuzzleCsvImportExecutorReader {
    fn drop(&mut self) {
        drop(self.entered.take());
        self.observer.notify_one();
    }
}

impl Read for PuzzleCsvImportExecutorReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = self.fill_buf()?.read(output)?;
        self.consume(count);
        Ok(count)
    }
}

impl BufRead for PuzzleCsvImportExecutorReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.bytes.position() == self.stop_at
            && let Some(entered) = self.entered.take()
        {
            let delivered = entered.send(()).map_err(io::Error::other);
            self.observer.notify_one();
            delivered?;
            self.gate.wait()?;
        }
        self.bytes
            .fill_buf()
            .map(|bytes| &bytes[..bytes.len().min(1)])
    }

    fn consume(&mut self, amount: usize) {
        self.bytes.consume(amount);
    }
}

struct PuzzleCsvImportExecutorOutcome {
    row: Result<Option<PuzzleCsvImportRow>, PuzzleCsvImportError>,
    following_row: Result<Option<PuzzleCsvImportRow>, PuzzleCsvImportError>,
}

impl PuzzleCsvImportExecutorOutcome {
    fn assert_cancelled(self) -> Result<(), Box<dyn std::error::Error>> {
        let error = self
            .row
            .err()
            .ok_or("Executor cancellation yielded a CSV row or end of input")?;
        assert_eq!(error.logical_row(), 2);
        assert_eq!(error.column(), Some(4));
        assert_eq!(error.field(), None);
        assert!(matches!(
            error.issue(),
            PuzzleCsvImportIssueError::Cancelled
        ));
        assert_eq!(
            error.continuation(),
            PuzzleCsvImportEnumContinuation::Terminated
        );
        assert!(self.following_row?.is_none());
        Ok(())
    }
}

struct PuzzleCsvImportExecutorFixture {
    executor: CapacityExecutor,
    job: CapacityExecutorJob<PuzzleCsvImportExecutorOutcome>,
    entered: mpsc::Receiver<()>,
    gate: Arc<PuzzleCsvImportExecutorGate>,
    observer: Arc<Notify>,
}

impl PuzzleCsvImportExecutorFixture {
    fn budget() -> Result<CapacityBudget, Box<dyn std::error::Error>> {
        Ok(CapacityBudget::new(
            NonZeroUsize::new(CapacityPhase::MINIMUM_WORKERS)
                .ok_or("Invalid CSV executor fixture capacity")?,
        )?)
    }

    fn new(budget: CapacityBudget) -> Result<Self, Box<dyn std::error::Error>> {
        let prefix = format!(
            "PuzzleId,FEN,Moves,Extra\n{},\"line one\n",
            PuzzleCsvImportFixture::row("e7e5 g1f3")
        );
        let stop_at = u64::try_from(prefix.len())?;
        let csv = format!(
            "{prefix}line two\"\n{},following\n",
            PuzzleCsvImportFixture::row("e7e5 g1f3")
        );
        let (entered_sender, entered) = mpsc::sync_channel(1);
        let gate = Arc::new(PuzzleCsvImportExecutorGate::default());
        let observer = Arc::new(Notify::new());
        let reader = PuzzleCsvImportExecutorReader {
            bytes: Cursor::new(csv.into_bytes()),
            stop_at,
            entered: Some(entered_sender),
            gate: Arc::clone(&gate),
            observer: Arc::clone(&observer),
        };
        let mut parser = PuzzleCsvImport::new(reader, PuzzleCsvImportFixture::dataset()?);
        let executor = CapacityExecutor::new(budget)?;
        let job = executor.submit(move |cancellation| {
            let row = parser.next_row(|| cancellation.is_cancelled());
            let following_row = parser.next_row(|| false);
            PuzzleCsvImportExecutorOutcome { row, following_row }
        })?;
        Ok(Self {
            executor,
            job,
            entered,
            gate,
            observer,
        })
    }

    async fn wait_until_scanning(&self) -> Result<(), mpsc::TryRecvError> {
        self.observer.notified().await;
        self.entered.try_recv()
    }

    fn prepare(phase: &mut CapacityPhase) -> Result<CapacityPhaseGeneration, CapacityPhaseError> {
        let generation = phase.request_preparation()?;
        phase.acknowledge_searches_stopped(generation)?;
        phase.try_enter_preparation(generation)?;
        Ok(generation)
    }

    fn enter_steady(phase: &mut CapacityPhase) -> Result<(), CapacityPhaseError> {
        let generation = Self::prepare(phase)?;
        phase.complete_preparation(generation)
    }
}

#[tokio::test]
async fn executor_job_cancellation_during_csv_scan_is_typed_and_terminal()
-> Result<(), Box<dyn std::error::Error>> {
    let budget = PuzzleCsvImportExecutorFixture::budget()?;
    let fixture = PuzzleCsvImportExecutorFixture::new(budget.clone())?;
    let entered = fixture.wait_until_scanning().await;
    fixture.job.cancel();
    let released = fixture.gate.release();
    let joined = fixture.job.join().await;
    let shutdown = fixture.executor.shutdown().await;
    shutdown?;
    entered?;
    released?;
    joined?.assert_cancelled()?;
    assert_eq!(budget.available(), budget.workers());
    Ok(())
}

#[tokio::test]
async fn capacity_phase_revocation_during_csv_scan_is_typed_and_terminal()
-> Result<(), Box<dyn std::error::Error>> {
    let budget = PuzzleCsvImportExecutorFixture::budget()?;
    let mut phase = CapacityPhase::new(budget.clone())?;
    PuzzleCsvImportExecutorFixture::enter_steady(&mut phase)?;
    let fixture = PuzzleCsvImportExecutorFixture::new(budget.clone())?;
    let entered = fixture.wait_until_scanning().await;
    let preparation_while_scanning = PuzzleCsvImportExecutorFixture::prepare(&mut phase);
    let released = fixture.gate.release();
    let joined = fixture.job.join().await;
    let restored = PuzzleCsvImportExecutorFixture::enter_steady(&mut phase);
    let restored_state = phase.state();
    drop(phase);
    let shutdown = fixture.executor.shutdown().await;
    shutdown?;
    entered?;
    released?;
    restored?;
    assert!(matches!(
        preparation_while_scanning,
        Err(CapacityPhaseError::QuiescencePending)
    ));
    assert_eq!(restored_state, CapacityPhaseEnumState::Steady);
    joined?.assert_cancelled()?;
    assert_eq!(budget.available(), budget.workers());
    Ok(())
}

#[test]
fn csv_header_names_map_to_fields_with_unchanged_spellings()
-> Result<(), Box<dyn std::error::Error>> {
    use chess::puzzle::csv_import::PuzzleCsvImportEnumField;
    let expected = [
        ("PuzzleId", PuzzleCsvImportEnumField::PuzzleId),
        ("FEN", PuzzleCsvImportEnumField::Fen),
        ("Moves", PuzzleCsvImportEnumField::Moves),
        ("Rating", PuzzleCsvImportEnumField::Rating),
        ("RatingDeviation", PuzzleCsvImportEnumField::RatingDeviation),
        ("Popularity", PuzzleCsvImportEnumField::Popularity),
        ("NbPlays", PuzzleCsvImportEnumField::NbPlays),
        ("Themes", PuzzleCsvImportEnumField::Themes),
        ("GameUrl", PuzzleCsvImportEnumField::GameUrl),
        ("OpeningTags", PuzzleCsvImportEnumField::OpeningTags),
        ("DailyDate", PuzzleCsvImportEnumField::DailyDate),
    ];
    for (name, field) in expected {
        assert_eq!(name.parse::<PuzzleCsvImportEnumField>()?, field);
    }
    assert!("fen".parse::<PuzzleCsvImportEnumField>().is_err());
    assert!("Unknown".parse::<PuzzleCsvImportEnumField>().is_err());
    Ok(())
}
