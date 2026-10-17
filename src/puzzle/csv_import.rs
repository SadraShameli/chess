use std::collections::HashSet;
use std::io::{self, BufRead};
use std::num::ParseIntError;
use std::str::{FromStr, Utf8Error};

use thiserror::Error;

use crate::board::enums::BoardEnumVariant;
use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::{BoardPosition, BoardPositionError};
use crate::puzzle::definition::PuzzleDefinition;
use crate::puzzle::identity::{PuzzleDefinitionIdentity, PuzzleDefinitionIdentityError};
use crate::puzzle::metadata::{
    PuzzleDefinitionDifficulty, PuzzleDefinitionMetadata, PuzzleDefinitionMetadataError,
    PuzzleDefinitionTheme,
};
use crate::puzzle::selection::PuzzleSelectionDataset;
use crate::puzzle::tree::PuzzleDefinitionTreeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleCsvImportEnumField {
    PuzzleId,
    Fen,
    Moves,
    Rating,
    RatingDeviation,
    Popularity,
    NbPlays,
    Themes,
    GameUrl,
    OpeningTags,
    DailyDate,
}

impl PuzzleCsvImportEnumField {
    const REQUIRED: [Self; 3] = [Self::PuzzleId, Self::Fen, Self::Moves];

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "PuzzleId" => Some(Self::PuzzleId),
            "FEN" => Some(Self::Fen),
            "Moves" => Some(Self::Moves),
            "Rating" => Some(Self::Rating),
            "RatingDeviation" => Some(Self::RatingDeviation),
            "Popularity" => Some(Self::Popularity),
            "NbPlays" => Some(Self::NbPlays),
            "Themes" => Some(Self::Themes),
            "GameUrl" => Some(Self::GameUrl),
            "OpeningTags" => Some(Self::OpeningTags),
            "DailyDate" => Some(Self::DailyDate),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuzzleCsvImportEnumContinuation {
    NextRow,
    Terminated,
}

#[derive(Debug, Error)]
pub enum PuzzleCsvImportIssueError {
    #[error("Required CSV header {field:?} is missing")]
    MissingHeader { field: PuzzleCsvImportEnumField },
    #[error("CSV header {name:?} is duplicated")]
    DuplicateHeader { name: String },
    #[error("CSV headers must not be empty")]
    EmptyHeader,
    #[error("Expected {expected} fields but received {actual}")]
    Width { expected: usize, actual: usize },
    #[error("A quote is only allowed at the start of a field")]
    UnexpectedQuote,
    #[error("A closing quote must be followed by a delimiter, another quote or end of record")]
    TrailingQuotedData,
    #[error("The quoted field does not close before end of input")]
    UnclosedQuote,
    #[error("An unquoted carriage return must be followed by a newline")]
    BareCarriageReturn,
    #[error("Invalid UTF-8 field: {0}")]
    Utf8(#[source] Utf8Error),
    #[error("The encoded logical CSV row exceeds {limit} bytes")]
    RowTooLarge { limit: usize },
    #[error("The logical CSV row number exceeds the supported range")]
    RowNumberOverflow,
    #[error("CSV input could not be read: {0}")]
    Read(#[source] io::Error),
    #[error("CSV parsing was cancelled")]
    Cancelled,
    #[error("Invalid source puzzle identity: {0}")]
    Identity(#[source] PuzzleDefinitionIdentityError),
    #[error("Integer metadata must contain ASCII decimal digits with only a permitted sign")]
    IntegerSyntax,
    #[error("Integer metadata exceeds its supported range: {0}")]
    Integer(#[source] ParseIntError),
    #[error("Invalid puzzle metadata: {0}")]
    Metadata(#[source] PuzzleDefinitionMetadataError),
    #[error("Invalid puzzle FEN: {0}")]
    Fen(#[source] BoardPositionError),
    #[error("Moves must contain an opponent setup move and at least one player move")]
    TooFewMoves,
    #[error("UCI moves must be separated by one ASCII space without surrounding whitespace")]
    MoveSeparator,
    #[error("UCI move at source ply {ply} is invalid: {source}")]
    Move {
        ply: usize,
        #[source]
        source: BoardMoveError,
    },
    #[error("Invalid puzzle solution: {0}")]
    Definition(#[source] PuzzleDefinitionTreeError),
}

#[derive(Debug, Error)]
#[error("CSV logical row {logical_row}, column {column:?}, field {field:?}: {issue}")]
pub struct PuzzleCsvImportError {
    logical_row: usize,
    column: Option<usize>,
    field: Option<PuzzleCsvImportEnumField>,
    continuation: PuzzleCsvImportEnumContinuation,
    #[source]
    issue: PuzzleCsvImportIssueError,
}

impl PuzzleCsvImportError {
    pub fn logical_row(&self) -> usize {
        self.logical_row
    }

    pub fn column(&self) -> Option<usize> {
        self.column
    }

    pub fn field(&self) -> Option<PuzzleCsvImportEnumField> {
        self.field
    }

    pub fn continuation(&self) -> PuzzleCsvImportEnumContinuation {
        self.continuation
    }

    pub fn issue(&self) -> &PuzzleCsvImportIssueError {
        &self.issue
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleCsvImportHeader {
    name: String,
    field: Option<PuzzleCsvImportEnumField>,
}

impl PuzzleCsvImportHeader {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn field(&self) -> Option<PuzzleCsvImportEnumField> {
        self.field
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleCsvImportExtraField {
    header: PuzzleCsvImportHeader,
    value: String,
}

impl PuzzleCsvImportExtraField {
    pub fn header(&self) -> &PuzzleCsvImportHeader {
        &self.header
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleCsvImportSourceMetadata {
    fields: Vec<PuzzleCsvImportExtraField>,
    rating_deviation: Option<u32>,
    popularity: Option<i32>,
    plays: Option<u64>,
}

impl PuzzleCsvImportSourceMetadata {
    pub fn fields(&self) -> &[PuzzleCsvImportExtraField] {
        &self.fields
    }

    pub fn rating_deviation(&self) -> Option<u32> {
        self.rating_deviation
    }

    pub fn popularity(&self) -> Option<i32> {
        self.popularity
    }

    pub fn plays(&self) -> Option<u64> {
        self.plays
    }

    pub fn game_url(&self) -> Option<&str> {
        self.optional_text(PuzzleCsvImportEnumField::GameUrl)
    }

    pub fn opening_tags(&self) -> Option<&str> {
        self.optional_text(PuzzleCsvImportEnumField::OpeningTags)
    }

    pub fn daily_date(&self) -> Option<&str> {
        self.optional_text(PuzzleCsvImportEnumField::DailyDate)
    }

    fn optional_text(&self, field: PuzzleCsvImportEnumField) -> Option<&str> {
        self.fields
            .iter()
            .find(|entry| entry.header.field == Some(field))
            .map(|entry| entry.value.as_str())
            .filter(|value| !value.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PuzzleCsvImportRow {
    logical_row: usize,
    definition: PuzzleDefinition,
    setup_move: BoardMove,
    source_metadata: PuzzleCsvImportSourceMetadata,
}

impl PuzzleCsvImportRow {
    pub fn logical_row(&self) -> usize {
        self.logical_row
    }

    pub fn definition(&self) -> &PuzzleDefinition {
        &self.definition
    }

    pub fn setup_move(&self) -> BoardMove {
        self.setup_move
    }

    pub fn source_metadata(&self) -> &PuzzleCsvImportSourceMetadata {
        &self.source_metadata
    }

    pub fn into_parts(self) -> (PuzzleDefinition, PuzzleCsvImportSourceMetadata) {
        (self.definition, self.source_metadata)
    }
}

#[derive(Clone, Copy)]
enum PuzzleCsvImportEnumState {
    Ready,
    Finished,
}

#[derive(Clone, Copy)]
enum PuzzleCsvImportEnumQuote {
    FieldStart,
    Unquoted,
    Quoted,
    AfterQuote,
}

#[derive(Default)]
struct PuzzleCsvImportRecord {
    decoded: Vec<u8>,
    ends: Vec<usize>,
    encoded_bytes: usize,
}

impl PuzzleCsvImportRecord {
    fn end_field(&mut self) {
        self.ends.push(self.decoded.len());
    }

    fn into_fields(mut self) -> Result<Vec<String>, (usize, Utf8Error)> {
        self.end_field();
        let mut start = 0;
        self.ends
            .into_iter()
            .enumerate()
            .map(|(index, end)| {
                let value = std::str::from_utf8(&self.decoded[start..end])
                    .map_err(|source| (index + 1, source))?;
                start = end;
                Ok(value.to_owned())
            })
            .collect()
    }
}

pub struct PuzzleCsvImport<R> {
    reader: R,
    dataset: PuzzleSelectionDataset,
    headers: Option<Vec<PuzzleCsvImportHeader>>,
    logical_row: usize,
    state: PuzzleCsvImportEnumState,
}

impl<R: BufRead> PuzzleCsvImport<R> {
    pub const MAX_ROW_BYTES: usize = 1_048_576;

    pub fn new(reader: R, dataset: PuzzleSelectionDataset) -> Self {
        Self {
            reader,
            dataset,
            headers: None,
            logical_row: 0,
            state: PuzzleCsvImportEnumState::Ready,
        }
    }

    pub fn headers(&self) -> Option<&[PuzzleCsvImportHeader]> {
        self.headers.as_deref()
    }

    pub fn next_row(
        &mut self,
        cancellation: impl Fn() -> bool,
    ) -> Result<Option<PuzzleCsvImportRow>, PuzzleCsvImportError> {
        if matches!(self.state, PuzzleCsvImportEnumState::Finished) {
            return Ok(None);
        }
        let outcome = self.read_next_row(&cancellation);
        match &outcome {
            Ok(None) => self.state = PuzzleCsvImportEnumState::Finished,
            Err(error) if error.continuation == PuzzleCsvImportEnumContinuation::Terminated => {
                self.state = PuzzleCsvImportEnumState::Finished;
            }
            Ok(Some(_)) | Err(_) => {}
        }
        outcome
    }

    fn read_next_row(
        &mut self,
        cancellation: &impl Fn() -> bool,
    ) -> Result<Option<PuzzleCsvImportRow>, PuzzleCsvImportError> {
        if self.headers.is_none() {
            let names = self.read_record(cancellation)?.ok_or_else(|| {
                self.error(
                    None,
                    PuzzleCsvImportEnumContinuation::Terminated,
                    PuzzleCsvImportIssueError::MissingHeader {
                        field: PuzzleCsvImportEnumField::PuzzleId,
                    },
                )
            })?;
            self.headers = Some(self.validate_headers(names)?);
        }
        let Some(values) = self.read_record(cancellation)? else {
            return Ok(None);
        };
        self.validate_row(values, cancellation).map(Some)
    }

    fn validate_headers(
        &self,
        names: Vec<String>,
    ) -> Result<Vec<PuzzleCsvImportHeader>, PuzzleCsvImportError> {
        let mut seen = HashSet::new();
        for name in &names {
            let issue = if name.is_empty() {
                Some(PuzzleCsvImportIssueError::EmptyHeader)
            } else if !seen.insert(name) {
                Some(PuzzleCsvImportIssueError::DuplicateHeader { name: name.clone() })
            } else {
                None
            };
            if let Some(issue) = issue {
                return Err(self.error(None, PuzzleCsvImportEnumContinuation::Terminated, issue));
            }
        }
        let headers: Vec<_> = names
            .into_iter()
            .map(|name| PuzzleCsvImportHeader {
                field: PuzzleCsvImportEnumField::from_name(&name),
                name,
            })
            .collect();
        for required in PuzzleCsvImportEnumField::REQUIRED {
            if !headers.iter().any(|header| header.field == Some(required)) {
                return Err(self.error(
                    None,
                    PuzzleCsvImportEnumContinuation::Terminated,
                    PuzzleCsvImportIssueError::MissingHeader { field: required },
                ));
            }
        }
        Ok(headers)
    }

    fn read_record(
        &mut self,
        cancellation: &impl Fn() -> bool,
    ) -> Result<Option<Vec<String>>, PuzzleCsvImportError> {
        self.logical_row = self.logical_row.checked_add(1).ok_or_else(|| {
            self.error(
                None,
                PuzzleCsvImportEnumContinuation::Terminated,
                PuzzleCsvImportIssueError::RowNumberOverflow,
            )
        })?;
        let mut record = PuzzleCsvImportRecord::default();
        let mut quote = PuzzleCsvImportEnumQuote::FieldStart;
        loop {
            let column = Some(record.ends.len() + 1);
            let Some(byte) = self.read_byte(column, cancellation)? else {
                match quote {
                    PuzzleCsvImportEnumQuote::Quoted => {
                        return Err(self.error(
                            column,
                            PuzzleCsvImportEnumContinuation::Terminated,
                            PuzzleCsvImportIssueError::UnclosedQuote,
                        ));
                    }
                    PuzzleCsvImportEnumQuote::FieldStart
                    | PuzzleCsvImportEnumQuote::Unquoted
                    | PuzzleCsvImportEnumQuote::AfterQuote => {}
                }
                if record.encoded_bytes == 0 {
                    return Ok(None);
                }
                break;
            };
            if !matches!(quote, PuzzleCsvImportEnumQuote::Quoted) {
                if byte == b'\n' {
                    break;
                }
                if byte == b'\r' {
                    if self.read_byte(column, cancellation)? != Some(b'\n') {
                        return Err(self.error(
                            column,
                            PuzzleCsvImportEnumContinuation::Terminated,
                            PuzzleCsvImportIssueError::BareCarriageReturn,
                        ));
                    }
                    break;
                }
            }
            if record.encoded_bytes == Self::MAX_ROW_BYTES {
                return Err(self.error(
                    column,
                    PuzzleCsvImportEnumContinuation::Terminated,
                    PuzzleCsvImportIssueError::RowTooLarge {
                        limit: Self::MAX_ROW_BYTES,
                    },
                ));
            }
            record.encoded_bytes += 1;
            match (quote, byte) {
                (PuzzleCsvImportEnumQuote::FieldStart, b'"') => {
                    quote = PuzzleCsvImportEnumQuote::Quoted;
                }
                (PuzzleCsvImportEnumQuote::Quoted, b'"') => {
                    quote = PuzzleCsvImportEnumQuote::AfterQuote;
                }
                (PuzzleCsvImportEnumQuote::AfterQuote, b'"') => {
                    record.decoded.push(byte);
                    quote = PuzzleCsvImportEnumQuote::Quoted;
                }
                (PuzzleCsvImportEnumQuote::Unquoted, b'"') => {
                    return Err(self.error(
                        column,
                        PuzzleCsvImportEnumContinuation::Terminated,
                        PuzzleCsvImportIssueError::UnexpectedQuote,
                    ));
                }
                (
                    PuzzleCsvImportEnumQuote::FieldStart
                    | PuzzleCsvImportEnumQuote::Unquoted
                    | PuzzleCsvImportEnumQuote::AfterQuote,
                    b',',
                ) => {
                    record.end_field();
                    quote = PuzzleCsvImportEnumQuote::FieldStart;
                }
                (PuzzleCsvImportEnumQuote::AfterQuote, _) => {
                    return Err(self.error(
                        column,
                        PuzzleCsvImportEnumContinuation::Terminated,
                        PuzzleCsvImportIssueError::TrailingQuotedData,
                    ));
                }
                (PuzzleCsvImportEnumQuote::Quoted, _) => record.decoded.push(byte),
                (PuzzleCsvImportEnumQuote::FieldStart | PuzzleCsvImportEnumQuote::Unquoted, _) => {
                    record.decoded.push(byte);
                    quote = PuzzleCsvImportEnumQuote::Unquoted;
                }
            }
        }
        record.into_fields().map(Some).map_err(|(column, source)| {
            self.error(
                Some(column),
                self.complete_record_continuation(),
                PuzzleCsvImportIssueError::Utf8(source),
            )
        })
    }

    fn read_byte(
        &mut self,
        column: Option<usize>,
        cancellation: &impl Fn() -> bool,
    ) -> Result<Option<u8>, PuzzleCsvImportError> {
        self.check_cancellation(column, cancellation)?;
        let byte = self
            .reader
            .fill_buf()
            .map(|buffer| buffer.first().copied())
            .map_err(|source| {
                self.error(
                    column,
                    PuzzleCsvImportEnumContinuation::Terminated,
                    PuzzleCsvImportIssueError::Read(source),
                )
            })?;
        self.check_cancellation(column, cancellation)?;
        if byte.is_some() {
            self.reader.consume(1);
        }
        Ok(byte)
    }

    fn validate_row(
        &self,
        values: Vec<String>,
        cancellation: &impl Fn() -> bool,
    ) -> Result<PuzzleCsvImportRow, PuzzleCsvImportError> {
        let headers = self.headers.as_deref().ok_or_else(|| {
            self.error(
                None,
                PuzzleCsvImportEnumContinuation::Terminated,
                PuzzleCsvImportIssueError::MissingHeader {
                    field: PuzzleCsvImportEnumField::PuzzleId,
                },
            )
        })?;
        if values.len() != headers.len() {
            return Err(self.error(
                None,
                PuzzleCsvImportEnumContinuation::NextRow,
                PuzzleCsvImportIssueError::Width {
                    expected: headers.len(),
                    actual: values.len(),
                },
            ));
        }
        let record = PuzzleCsvImportValues { headers, values };
        let identity = PuzzleDefinitionIdentity::new(
            self.dataset.source().to_owned(),
            self.dataset.version().to_owned(),
            record.text(PuzzleCsvImportEnumField::PuzzleId).to_owned(),
        )
        .map_err(|source| {
            self.field_error(
                PuzzleCsvImportEnumField::PuzzleId,
                PuzzleCsvImportIssueError::Identity(source),
            )
        })?;
        let original = BoardPosition::from_fen(
            record.text(PuzzleCsvImportEnumField::Fen),
            BoardEnumVariant::Standard,
        )
        .map_err(|source| {
            self.field_error(
                PuzzleCsvImportEnumField::Fen,
                PuzzleCsvImportIssueError::Fen(source),
            )
        })?;
        let metadata = record
            .metadata()
            .map_err(|(field, issue)| self.field_error(field, issue))?;
        let rating_deviation = record
            .unsigned(PuzzleCsvImportEnumField::RatingDeviation)
            .map_err(|issue| self.field_error(PuzzleCsvImportEnumField::RatingDeviation, issue))?;
        let popularity = record
            .popularity()
            .map_err(|issue| self.field_error(PuzzleCsvImportEnumField::Popularity, issue))?;
        let plays = record
            .unsigned(PuzzleCsvImportEnumField::NbPlays)
            .map_err(|issue| self.field_error(PuzzleCsvImportEnumField::NbPlays, issue))?;
        let (setup_move, starting_position, moves) = self.validate_moves(
            original,
            record.text(PuzzleCsvImportEnumField::Moves),
            cancellation,
        )?;
        let definition = PuzzleDefinition::new(identity, starting_position, vec![moves], metadata)
            .map_err(|source| {
                self.field_error(
                    PuzzleCsvImportEnumField::Moves,
                    PuzzleCsvImportIssueError::Definition(source),
                )
            })?;
        self.check_cancellation(None, cancellation)?;
        let fields = headers
            .iter()
            .cloned()
            .zip(record.values)
            .map(|(header, value)| PuzzleCsvImportExtraField { header, value })
            .collect();
        Ok(PuzzleCsvImportRow {
            logical_row: self.logical_row,
            definition,
            setup_move,
            source_metadata: PuzzleCsvImportSourceMetadata {
                fields,
                rating_deviation,
                popularity,
                plays,
            },
        })
    }

    fn validate_moves(
        &self,
        original: BoardPosition,
        text: &str,
        cancellation: &impl Fn() -> bool,
    ) -> Result<(BoardMove, BoardPosition, Vec<BoardMove>), PuzzleCsvImportError> {
        let issue = |issue| self.field_error(PuzzleCsvImportEnumField::Moves, issue);
        let mut values = text.split(' ');
        if !text.is_empty()
            && values.clone().any(|value| {
                value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace())
            })
        {
            return Err(issue(PuzzleCsvImportIssueError::MoveSeparator));
        }
        if text.is_empty() || !text.contains(' ') {
            return Err(issue(PuzzleCsvImportIssueError::TooFewMoves));
        }
        let setup_text = values
            .next()
            .ok_or_else(|| issue(PuzzleCsvImportIssueError::TooFewMoves))?;
        PuzzleDefinition::validate_solution_lengths(std::iter::once(values.clone().count()))
            .map_err(|source| issue(PuzzleCsvImportIssueError::Definition(source)))?;
        let setup_move = original
            .parse_uci(setup_text)
            .map_err(|source| issue(PuzzleCsvImportIssueError::Move { ply: 0, source }))?;
        let starting_position = original
            .play(setup_move)
            .map_err(|source| issue(PuzzleCsvImportIssueError::Move { ply: 0, source }))?;
        let mut position = starting_position.clone();
        let mut moves = Vec::new();
        for (index, value) in values.enumerate() {
            self.check_cancellation(self.column(PuzzleCsvImportEnumField::Moves), cancellation)?;
            let ply = index + 1;
            let movement = position
                .parse_uci(value)
                .map_err(|source| issue(PuzzleCsvImportIssueError::Move { ply, source }))?;
            position = position
                .play(movement)
                .map_err(|source| issue(PuzzleCsvImportIssueError::Move { ply, source }))?;
            moves.push(movement);
        }
        Ok((setup_move, starting_position, moves))
    }

    fn complete_record_continuation(&self) -> PuzzleCsvImportEnumContinuation {
        if self.headers.is_some() {
            PuzzleCsvImportEnumContinuation::NextRow
        } else {
            PuzzleCsvImportEnumContinuation::Terminated
        }
    }

    fn check_cancellation(
        &self,
        column: Option<usize>,
        cancellation: &impl Fn() -> bool,
    ) -> Result<(), PuzzleCsvImportError> {
        if cancellation() {
            return Err(self.error(
                column,
                PuzzleCsvImportEnumContinuation::Terminated,
                PuzzleCsvImportIssueError::Cancelled,
            ));
        }
        Ok(())
    }

    fn column(&self, field: PuzzleCsvImportEnumField) -> Option<usize> {
        self.headers
            .as_ref()?
            .iter()
            .position(|header| header.field == Some(field))
            .map(|index| index + 1)
    }

    fn field_error(
        &self,
        field: PuzzleCsvImportEnumField,
        issue: PuzzleCsvImportIssueError,
    ) -> PuzzleCsvImportError {
        self.error(
            self.column(field),
            PuzzleCsvImportEnumContinuation::NextRow,
            issue,
        )
    }

    fn error(
        &self,
        column: Option<usize>,
        continuation: PuzzleCsvImportEnumContinuation,
        issue: PuzzleCsvImportIssueError,
    ) -> PuzzleCsvImportError {
        let field = column
            .and_then(|column| self.headers.as_ref()?.get(column - 1))
            .and_then(|header| header.field);
        PuzzleCsvImportError {
            logical_row: self.logical_row,
            column,
            field,
            continuation,
            issue,
        }
    }
}

struct PuzzleCsvImportValues<'a> {
    headers: &'a [PuzzleCsvImportHeader],
    values: Vec<String>,
}

impl PuzzleCsvImportValues<'_> {
    fn text(&self, field: PuzzleCsvImportEnumField) -> &str {
        self.headers
            .iter()
            .zip(&self.values)
            .find(|(header, _)| header.field == Some(field))
            .map_or("", |(_, value)| value.as_str())
    }

    fn unsigned<T: FromStr<Err = ParseIntError>>(
        &self,
        field: PuzzleCsvImportEnumField,
    ) -> Result<Option<T>, PuzzleCsvImportIssueError> {
        let text = self.text(field);
        Self::integer(text, text)
    }

    fn popularity(&self) -> Result<Option<i32>, PuzzleCsvImportIssueError> {
        let text = self.text(PuzzleCsvImportEnumField::Popularity);
        let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
        Self::integer(text, digits)
    }

    fn integer<T: FromStr<Err = ParseIntError>>(
        text: &str,
        digits: &str,
    ) -> Result<Option<T>, PuzzleCsvImportIssueError> {
        if text.is_empty() {
            return Ok(None);
        }
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(PuzzleCsvImportIssueError::IntegerSyntax);
        }
        text.parse()
            .map(Some)
            .map_err(PuzzleCsvImportIssueError::Integer)
    }

    fn metadata(
        &self,
    ) -> Result<PuzzleDefinitionMetadata, (PuzzleCsvImportEnumField, PuzzleCsvImportIssueError)>
    {
        let difficulty = match self.text(PuzzleCsvImportEnumField::Rating) {
            "" => None,
            value => Some(
                value
                    .parse::<PuzzleDefinitionDifficulty>()
                    .map_err(|source| {
                        (
                            PuzzleCsvImportEnumField::Rating,
                            PuzzleCsvImportIssueError::Metadata(source),
                        )
                    })?,
            ),
        };
        let themes = match self.text(PuzzleCsvImportEnumField::Themes) {
            "" => None,
            value => Some(
                value
                    .split(' ')
                    .map(str::parse)
                    .collect::<Result<Vec<PuzzleDefinitionTheme>, _>>()
                    .map_err(|source| {
                        (
                            PuzzleCsvImportEnumField::Themes,
                            PuzzleCsvImportIssueError::Metadata(source),
                        )
                    })?,
            ),
        };
        PuzzleDefinitionMetadata::new(difficulty, themes).map_err(|source| {
            (
                PuzzleCsvImportEnumField::Themes,
                PuzzleCsvImportIssueError::Metadata(source),
            )
        })
    }
}
