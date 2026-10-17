mod budget;
mod lexer;

use std::collections::{HashMap, HashSet};
use std::io::BufRead;

use thiserror::Error;

use crate::board::enums::{BoardEnumColor, BoardEnumVariant};
use crate::board::r#move::BoardMoveError;
use crate::board::position::{BoardPosition, BoardPositionError};
use crate::board::start::BoardStart;
use crate::clock::duration::ClockDuration;
use crate::game::enums::{GameEnumMarkingColor, GameEnumResult, GameIdentityError, GameMoveId};
use crate::game::move_node::{
    GameMoveNodeAnnotationError, GameMoveNodeAnnotations, GameMoveNodeMarking, GameMoveNodeNag,
};
use crate::game::session::{
    GameMetadata, GameMetadataDate, GameMetadataError, GameMetadataExtension,
    GameMetadataKnownTags, GameMetadataOpeningCode, GameMetadataText,
};
use crate::game::tree::{GameTree, GameTreeError};
use crate::notation::game::{
    NotationGame, NotationGameClock, NotationGameEnumWarning, NotationGameError, NotationGameParts,
};
use crate::notation::game_identity::NotationGameIdentityEnumResult;
use crate::notation::pgn_import::budget::{NotationPgnImportBudget, NotationPgnImportBudgetPolicy};
use crate::notation::pgn_import::lexer::{
    NotationPgnImportLexer, NotationPgnImportLexerEnumToken, NotationPgnImportLexerToken,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotationPgnImportLocation {
    byte_offset: u64,
    line: u64,
    column: u64,
}

impl NotationPgnImportLocation {
    pub const fn byte_offset(self) -> u64 {
        self.byte_offset
    }
    pub const fn line(self) -> u64 {
        self.line
    }
    pub const fn column(self) -> u64 {
        self.column
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotationPgnImportEnumRecovery {
    NextGame,
    Terminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NotationPgnImportEnumLimit {
    #[error("The PGN input exceeds its byte limit")]
    InputBytes,
    #[error("The PGN input exceeds its encountered-game limit")]
    Games,
    #[error("The PGN input exceeds its total-ply limit")]
    TotalPlies,
    #[error("The PGN game exceeds its ply limit")]
    GamePlies,
    #[error("The PGN game exceeds its variation-depth limit")]
    VariationDepth,
    #[error("The PGN text field or assembled note exceeds its byte limit")]
    TextBytes,
    #[error("The PGN retained and pending decoded data exceeds its byte limit")]
    DecodedBytes,
    #[error("A PGN accounting value exceeds its numeric range")]
    Arithmetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NotationPgnImportEnumSyntax {
    #[error("Invalid PGN tag syntax")]
    InvalidTag,
    #[error("Invalid PGN tag escape")]
    InvalidEscape,
    #[error("The PGN tag is not closed")]
    UnterminatedTag,
    #[error("The PGN brace comment is not closed")]
    UnterminatedComment,
    #[error("Unexpected PGN delimiter or token")]
    UnexpectedToken,
    #[error("Unexpected variation delimiter")]
    UnexpectedVariation,
    #[error("An empty variation cannot be retained")]
    EmptyVariation,
    #[error("The variation is not closed")]
    UnclosedVariation,
    #[error("Every PGN game requires a movetext result marker")]
    MissingResult,
    #[error("A result marker cannot end an open variation")]
    ResultInVariation,
    #[error("The Result header disagrees with the movetext result")]
    ResultMismatch,
    #[error("The Result tag value is invalid")]
    InvalidResult,
    #[error("A PGN tag occurs more than once")]
    DuplicateTag,
    #[error("Invalid PGN annotation glyph")]
    InvalidNag,
    #[error("The PGN move number differs from its position")]
    InvalidMoveNumber,
    #[error("Invalid or conflicting supported PGN annotation directive")]
    InvalidDirective,
    #[error("The PGN variant is unsupported")]
    UnsupportedVariant,
    #[error("The PGN setup and FEN tags do not describe an explicit valid start")]
    InvalidStart,
    #[error("PGN annotation text contains a NUL character")]
    InvalidText,
}

#[derive(Debug, Error)]
pub enum NotationPgnImportReason {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Utf8(#[from] std::str::Utf8Error),
    #[error(transparent)]
    Limit(#[from] NotationPgnImportEnumLimit),
    #[error(transparent)]
    Syntax(#[from] NotationPgnImportEnumSyntax),
    #[error(transparent)]
    Move(#[from] BoardMoveError),
    #[error(transparent)]
    Position(#[from] BoardPositionError),
    #[error(transparent)]
    Metadata(#[from] GameMetadataError),
    #[error(transparent)]
    Tree(#[from] GameTreeError),
    #[error(transparent)]
    Annotation(#[from] GameMoveNodeAnnotationError),
    #[error(transparent)]
    Game(#[from] NotationGameError),
    #[error(transparent)]
    MoveIdentity(#[from] GameIdentityError),
    #[error("{primary}; PGN recovery failed at byte {byte_offset}: {recovery}", byte_offset = .location.byte_offset)]
    Recovery {
        #[source]
        primary: Box<NotationPgnImportReason>,
        recovery: Box<NotationPgnImportReason>,
        location: NotationPgnImportLocation,
    },
    #[error("Repeated PGN continuation occurrences cannot be represented without coalescing")]
    UnsupportedOccurrence,
    #[error("PGN import was cancelled")]
    Cancelled,
    #[error("The PGN stream has already terminated")]
    Terminated,
}

impl NotationPgnImportReason {
    pub fn is_cancelled(&self) -> bool {
        match self {
            Self::Cancelled => true,
            Self::Recovery {
                primary, recovery, ..
            } => primary.is_cancelled() || recovery.is_cancelled(),
            Self::Io(_)
            | Self::Utf8(_)
            | Self::Limit(_)
            | Self::Syntax(_)
            | Self::Move(_)
            | Self::Position(_)
            | Self::Metadata(_)
            | Self::Tree(_)
            | Self::Annotation(_)
            | Self::Game(_)
            | Self::MoveIdentity(_)
            | Self::UnsupportedOccurrence
            | Self::Terminated => false,
        }
    }
}

#[derive(Debug, Error)]
#[error("PGN game {game_ordinal} at byte {byte_offset}: {reason}", byte_offset = .location.byte_offset)]
pub struct NotationPgnImportError {
    game_ordinal: u64,
    location: NotationPgnImportLocation,
    recovery: NotationPgnImportEnumRecovery,
    #[source]
    reason: NotationPgnImportReason,
}

impl NotationPgnImportError {
    pub const fn game_ordinal(&self) -> u64 {
        self.game_ordinal
    }
    pub const fn location(&self) -> NotationPgnImportLocation {
        self.location
    }
    pub const fn recovery(&self) -> NotationPgnImportEnumRecovery {
        self.recovery
    }
    pub fn reason(&self) -> &NotationPgnImportReason {
        &self.reason
    }
}

struct NotationPgnImportResult;

impl NotationPgnImportResult {
    fn parse(value: &str) -> Option<NotationGameIdentityEnumResult> {
        [
            NotationGameIdentityEnumResult::Unfinished,
            NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(
                BoardEnumColor::White,
            )),
            NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(
                BoardEnumColor::Black,
            )),
            NotationGameIdentityEnumResult::Completed(GameEnumResult::Draw),
        ]
        .into_iter()
        .find(|result| result.pgn_token() == value)
    }
}

#[derive(Default)]
struct NotationPgnImportStaging {
    headers: NotationPgnImportHeaders,
    game: Option<NotationPgnImportGame>,
    depth: u64,
}

pub struct NotationPgnImport<R, I> {
    lexer: NotationPgnImportLexer<R>,
    move_ids: I,
    is_terminated: bool,
}

impl<R: BufRead, I: FnMut() -> Result<GameMoveId, GameIdentityError>> NotationPgnImport<R, I> {
    pub fn new(input: R, move_ids: I) -> Self {
        Self::with_policy(input, move_ids, NotationPgnImportBudgetPolicy::production())
    }

    fn with_policy(input: R, move_ids: I, policy: NotationPgnImportBudgetPolicy) -> Self {
        Self {
            lexer: NotationPgnImportLexer::new(input, policy),
            move_ids,
            is_terminated: false,
        }
    }

    fn error(
        &self,
        location: NotationPgnImportLocation,
        reason: NotationPgnImportReason,
    ) -> NotationPgnImportError {
        NotationPgnImportError {
            game_ordinal: self.lexer.budget.game_ordinal().max(1),
            location,
            recovery: NotationPgnImportEnumRecovery::Terminated,
            reason,
        }
    }

    fn fatal(&mut self, reason: NotationPgnImportReason) -> NotationPgnImportError {
        self.is_terminated = true;
        self.error(self.lexer.location(), reason)
    }

    pub fn next_game<C: Fn() -> bool>(
        &mut self,
        cancellation: &C,
    ) -> Result<Option<NotationGame>, NotationPgnImportError> {
        if self.is_terminated {
            return Err(self.fatal(NotationPgnImportReason::Terminated));
        }
        match self.lexer.has_next(cancellation) {
            Ok(false) => return Ok(None),
            Ok(true) => (),
            Err(reason) => return Err(self.fatal(reason)),
        }
        if let Err(reason) = self.lexer.budget.begin_game() {
            return Err(self.fatal(reason.into()));
        }
        let mut staging = NotationPgnImportStaging::default();
        loop {
            let token = match self.lexer.next(cancellation) {
                Ok(Some(token)) => token,
                Ok(None) => {
                    let syntax = if staging.depth == 0 {
                        NotationPgnImportEnumSyntax::MissingResult
                    } else {
                        NotationPgnImportEnumSyntax::UnclosedVariation
                    };
                    return Err(self.fatal(syntax.into()));
                }
                Err(reason) => {
                    self.is_terminated = true;
                    return Err(self.error(self.lexer.token_location(), reason));
                }
            };
            let location = token.location;
            let is_boundary = matches!(token.kind, NotationPgnImportLexerEnumToken::Result(_))
                && staging.depth == 0;
            match self.apply_token(token, &mut staging, cancellation) {
                Ok(Some(completed)) => return Ok(Some(completed)),
                Ok(None) => (),
                Err(reason) => {
                    let mut error = self.error(location, reason);
                    if matches!(
                        error.reason,
                        NotationPgnImportReason::Limit(_)
                            | NotationPgnImportReason::MoveIdentity(_)
                            | NotationPgnImportReason::Cancelled
                            | NotationPgnImportReason::Syntax(
                                NotationPgnImportEnumSyntax::UnexpectedVariation
                            )
                    ) {
                        self.is_terminated = true;
                        return Err(error);
                    }
                    if is_boundary {
                        error.recovery = NotationPgnImportEnumRecovery::NextGame;
                        return Err(error);
                    }
                    match self.drain(staging.depth, cancellation) {
                        Ok(true) => error.recovery = NotationPgnImportEnumRecovery::NextGame,
                        Ok(false) => self.is_terminated = true,
                        Err(recovery) => {
                            self.is_terminated = true;
                            error.reason = NotationPgnImportReason::Recovery {
                                primary: Box::new(error.reason),
                                recovery: Box::new(recovery),
                                location: self.lexer.location(),
                            };
                        }
                    }
                    return Err(error);
                }
            }
        }
    }

    fn apply_token<C: Fn() -> bool>(
        &mut self,
        token: NotationPgnImportLexerToken,
        staging: &mut NotationPgnImportStaging,
        cancellation: &C,
    ) -> Result<Option<NotationGame>, NotationPgnImportReason> {
        let NotationPgnImportStaging {
            headers,
            game,
            depth,
        } = staging;
        let kind = match token.kind {
            NotationPgnImportLexerEnumToken::Tag { name, value } => {
                if game.is_some() {
                    return Err(NotationPgnImportEnumSyntax::UnexpectedToken.into());
                }
                headers.accept(name, value, &mut self.lexer.budget)?;
                return Ok(None);
            }
            kind @ (NotationPgnImportLexerEnumToken::Move(_)
            | NotationPgnImportLexerEnumToken::MoveNumber { .. }
            | NotationPgnImportLexerEnumToken::Nag(_)
            | NotationPgnImportLexerEnumToken::Comment(_)
            | NotationPgnImportLexerEnumToken::BeginVariation
            | NotationPgnImportLexerEnumToken::EndVariation
            | NotationPgnImportLexerEnumToken::Result(_)) => kind,
        };
        if game.is_none() {
            let root = (self.move_ids)()?;
            let start = headers.start()?;
            let bytes = u64::try_from(start.initial_fen().len())
                .map_err(|_error| NotationPgnImportEnumLimit::Arithmetic)?;
            self.lexer.budget.retain(bytes)?;
            *game = Some(NotationPgnImportGame::new(start, root));
        }
        let current = game
            .as_mut()
            .ok_or(NotationPgnImportEnumSyntax::InvalidStart)?;
        match kind {
            NotationPgnImportLexerEnumToken::Tag { .. } => {
                Err(NotationPgnImportEnumSyntax::UnexpectedToken.into())
            }
            NotationPgnImportLexerEnumToken::Move(san) => {
                current.movement(&san, &mut self.move_ids, &mut self.lexer.budget)?;
                Ok(None)
            }
            NotationPgnImportLexerEnumToken::MoveNumber { number, color } => {
                let position = current.tree.node(current.cursor)?.position();
                if position.fullmoves() != number || position.turn() != color {
                    return Err(NotationPgnImportEnumSyntax::InvalidMoveNumber.into());
                }
                Ok(None)
            }
            NotationPgnImportLexerEnumToken::Nag(nag) => {
                current.nag(nag, &mut self.lexer.budget)?;
                Ok(None)
            }
            NotationPgnImportLexerEnumToken::Comment(text) => {
                current.comment(&text, &mut self.lexer.budget, cancellation)?;
                Ok(None)
            }
            NotationPgnImportLexerEnumToken::BeginVariation => {
                *depth = depth
                    .checked_add(1)
                    .ok_or(NotationPgnImportEnumLimit::Arithmetic)?;
                self.lexer.budget.depth(*depth)?;
                current.begin_variation()?;
                Ok(None)
            }
            NotationPgnImportLexerEnumToken::EndVariation => {
                *depth = depth
                    .checked_sub(1)
                    .ok_or(NotationPgnImportEnumSyntax::UnexpectedVariation)?;
                current.end_variation()?;
                Ok(None)
            }
            NotationPgnImportLexerEnumToken::Result(result) => {
                if *depth != 0 {
                    return Err(NotationPgnImportEnumSyntax::ResultInVariation.into());
                }
                if headers.result_header.is_some_and(|header| header != result) {
                    return Err(NotationPgnImportEnumSyntax::ResultMismatch.into());
                }
                let finished = game
                    .take()
                    .ok_or(NotationPgnImportEnumSyntax::InvalidStart)?;
                Ok(Some(finished.finish(
                    std::mem::take(headers),
                    result,
                    cancellation,
                )?))
            }
        }
    }

    fn drain<C: Fn() -> bool>(
        &mut self,
        mut depth: u64,
        cancellation: &C,
    ) -> Result<bool, NotationPgnImportReason> {
        while let Some(token) = self.lexer.next(cancellation)? {
            match token.kind {
                NotationPgnImportLexerEnumToken::BeginVariation => {
                    depth = depth
                        .checked_add(1)
                        .ok_or(NotationPgnImportEnumLimit::Arithmetic)?;
                    self.lexer.budget.depth(depth)?;
                }
                NotationPgnImportLexerEnumToken::EndVariation => {
                    let Some(parent) = depth.checked_sub(1) else {
                        return Ok(false);
                    };
                    depth = parent;
                }
                NotationPgnImportLexerEnumToken::Result(_) if depth == 0 => return Ok(true),
                NotationPgnImportLexerEnumToken::Tag { .. }
                | NotationPgnImportLexerEnumToken::Move(_)
                | NotationPgnImportLexerEnumToken::MoveNumber { .. }
                | NotationPgnImportLexerEnumToken::Nag(_)
                | NotationPgnImportLexerEnumToken::Comment(_)
                | NotationPgnImportLexerEnumToken::Result(_) => (),
            }
        }
        Ok(false)
    }
}

#[derive(Default)]
struct NotationPgnImportHeaders {
    known: GameMetadataKnownTags,
    extensions: Vec<GameMetadataExtension>,
    names: HashSet<String>,
    result_header: Option<NotationGameIdentityEnumResult>,
    fen: Option<String>,
    setup: Option<String>,
    variant: Option<BoardEnumVariant>,
    time_control: Option<GameMetadataText>,
    warnings: Vec<NotationGameEnumWarning>,
}

impl NotationPgnImportHeaders {
    fn accept(
        &mut self,
        name: String,
        value: String,
        budget: &mut NotationPgnImportBudget,
    ) -> Result<(), NotationPgnImportReason> {
        let name_bytes = u64::try_from(name.capacity())
            .map_err(|_error| NotationPgnImportEnumLimit::Arithmetic)?;
        let name_clone_bytes =
            u64::try_from(name.len()).map_err(|_error| NotationPgnImportEnumLimit::Arithmetic)?;
        let value_bytes = u64::try_from(value.capacity())
            .map_err(|_error| NotationPgnImportEnumLimit::Arithmetic)?;
        budget.retain(
            name_bytes
                .checked_add(name_clone_bytes)
                .and_then(|bytes| bytes.checked_add(value_bytes))
                .ok_or(NotationPgnImportEnumLimit::Arithmetic)?,
        )?;
        if !self.names.insert(name.clone()) {
            return Err(NotationPgnImportEnumSyntax::DuplicateTag.into());
        }
        match name.as_str() {
            "Event" => self.known.event = Some(GameMetadataText::new(value)?),
            "Site" => self.known.site = Some(GameMetadataText::new(value)?),
            "Date" => self.known.date = Some(Self::date(&value)?),
            "Round" => self.known.round = Some(GameMetadataText::new(value)?),
            "White" => self.known.white = Some(GameMetadataText::new(value)?),
            "Black" => self.known.black = Some(GameMetadataText::new(value)?),
            "WhiteElo" => {
                self.known.white_elo = self.rating(BoardEnumColor::White, value, budget)?
            }
            "BlackElo" => {
                self.known.black_elo = self.rating(BoardEnumColor::Black, value, budget)?
            }
            "Opening" => self.known.opening = Some(GameMetadataText::new(value)?),
            "Variation" => self.known.variation = Some(GameMetadataText::new(value)?),
            "ECO" => self.known.eco = Some(GameMetadataOpeningCode::new(value)?),
            "Annotator" => self.known.annotator = Some(GameMetadataText::new(value)?),
            "Result" => {
                self.result_header = Some(
                    NotationPgnImportResult::parse(&value)
                        .ok_or(NotationPgnImportEnumSyntax::InvalidResult)?,
                )
            }
            "FEN" => self.fen = Some(value),
            "SetUp" => self.setup = Some(value),
            "Variant" => {
                self.variant = Some(match value.as_str() {
                    "Standard" => BoardEnumVariant::Standard,
                    "Chess960" | "Fischerandom" | "FischerRandom" | "DFRC" => {
                        BoardEnumVariant::Chess960
                    }
                    _ => return Err(NotationPgnImportEnumSyntax::UnsupportedVariant.into()),
                })
            }
            "TimeControl" => self.time_control = Some(GameMetadataText::new(value)?),
            _ => self.extensions.push(GameMetadataExtension::new(
                name,
                GameMetadataText::new(value)?,
            )?),
        }
        Ok(())
    }

    fn rating(
        &mut self,
        color: BoardEnumColor,
        value: String,
        budget: &mut NotationPgnImportBudget,
    ) -> Result<Option<u32>, NotationPgnImportReason> {
        if let Some(rating) = NotationGameEnumWarning::known_rating(&value) {
            return Ok(Some(rating));
        }
        budget.reserve_retained_entry(&mut self.warnings)?;
        self.warnings
            .push(NotationGameEnumWarning::UnavailableRating {
                color,
                value: GameMetadataText::new(value)?,
            });
        Ok(None)
    }

    fn date(value: &str) -> Result<GameMetadataDate, GameMetadataError> {
        let mut fields = value.split('.');
        let year = Self::date_part(fields.next(), 4)?;
        let month = Self::date_part(fields.next(), 2)?
            .map(u8::try_from)
            .transpose()
            .map_err(|_error| GameMetadataError::InvalidDate)?;
        let day = Self::date_part(fields.next(), 2)?
            .map(u8::try_from)
            .transpose()
            .map_err(|_error| GameMetadataError::InvalidDate)?;
        if fields.next().is_some() {
            return Err(GameMetadataError::InvalidDate);
        }
        GameMetadataDate::new(year, month, day)
    }

    fn date_part(value: Option<&str>, width: usize) -> Result<Option<u16>, GameMetadataError> {
        let value = value.ok_or(GameMetadataError::InvalidDate)?;
        if value.len() != width {
            return Err(GameMetadataError::InvalidDate);
        }
        if value.bytes().all(|byte| byte == b'?') {
            return Ok(None);
        }
        if !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(GameMetadataError::InvalidDate);
        }
        value
            .parse()
            .map(Some)
            .map_err(|_error| GameMetadataError::InvalidDate)
    }

    fn start(&self) -> Result<BoardStart, NotationPgnImportReason> {
        let variant = self.variant.unwrap_or(BoardEnumVariant::Standard);
        match (self.setup.as_deref(), self.fen.as_deref()) {
            (Some("1"), Some(fen)) => {
                Ok(BoardStart::custom(BoardPosition::from_fen(fen, variant)?))
            }
            (None | Some("0"), None) if variant == BoardEnumVariant::Standard => {
                Ok(BoardStart::standard())
            }
            _ => Err(NotationPgnImportEnumSyntax::InvalidStart.into()),
        }
    }
}

struct NotationPgnImportVariation {
    resume: GameMoveId,
    base: GameMoveId,
}

struct NotationPgnImportNodeAnnotations {
    node: GameMoveId,
    notes: String,
    nags: Vec<GameMoveNodeNag>,
    markings: Vec<GameMoveNodeMarking>,
    clock: Option<ClockDuration>,
    has_comment: bool,
}

impl NotationPgnImportNodeAnnotations {
    const NANOS_PER_SECOND: u64 = 1_000_000_000;
    const SECONDS_PER_MINUTE: u64 = 60;
    const MINUTES_PER_HOUR: u64 = 60;

    fn new(node: GameMoveId) -> Self {
        Self {
            node,
            notes: String::new(),
            nags: Vec::new(),
            markings: Vec::new(),
            clock: None,
            has_comment: false,
        }
    }

    fn append_notes(
        &mut self,
        text: &str,
        budget: &mut NotationPgnImportBudget,
    ) -> Result<(), NotationPgnImportReason> {
        budget.reserve_retained_text(&mut self.notes, text.len())?;
        self.notes.push_str(text);
        Ok(())
    }

    fn clock_value(value: &str) -> Result<ClockDuration, NotationPgnImportReason> {
        let mut fields = value.split(':');
        let hours = Self::decimal(
            fields
                .next()
                .ok_or(NotationPgnImportEnumSyntax::InvalidDirective)?,
        )?;
        let minutes = fields
            .next()
            .ok_or(NotationPgnImportEnumSyntax::InvalidDirective)?;
        let seconds = fields
            .next()
            .ok_or(NotationPgnImportEnumSyntax::InvalidDirective)?;
        if fields.next().is_some() || minutes.len() != 2 {
            return Err(NotationPgnImportEnumSyntax::InvalidDirective.into());
        }
        let minutes = Self::decimal(minutes)?;
        let (whole, fraction) = seconds
            .split_once('.')
            .map_or((seconds, None), |(whole, fraction)| (whole, Some(fraction)));
        if whole.len() != 2 {
            return Err(NotationPgnImportEnumSyntax::InvalidDirective.into());
        }
        let seconds = Self::decimal(whole)?;
        if minutes >= Self::MINUTES_PER_HOUR || seconds >= Self::SECONDS_PER_MINUTE {
            return Err(NotationPgnImportEnumSyntax::InvalidDirective.into());
        }
        let nanos = match fraction {
            None => 0,
            Some(value) if (1..=9).contains(&value.len()) => {
                let digits = u32::try_from(value.len())
                    .map_err(|_error| NotationPgnImportEnumSyntax::InvalidDirective)?;
                Self::decimal(value)?
                    .checked_mul(10_u64.pow(9 - digits))
                    .ok_or(NotationPgnImportEnumSyntax::InvalidDirective)?
            }
            Some(_) => return Err(NotationPgnImportEnumSyntax::InvalidDirective.into()),
        };
        let duration = hours
            .checked_mul(Self::MINUTES_PER_HOUR)
            .and_then(|value| value.checked_add(minutes))
            .and_then(|value| value.checked_mul(Self::SECONDS_PER_MINUTE))
            .and_then(|value| value.checked_add(seconds))
            .and_then(|value| value.checked_mul(Self::NANOS_PER_SECOND))
            .and_then(|value| value.checked_add(nanos))
            .ok_or(NotationPgnImportEnumSyntax::InvalidDirective)?;
        Ok(ClockDuration::from_nanos(duration))
    }

    fn decimal(value: &str) -> Result<u64, NotationPgnImportEnumSyntax> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(NotationPgnImportEnumSyntax::InvalidDirective);
        }
        value
            .parse()
            .map_err(|_error| NotationPgnImportEnumSyntax::InvalidDirective)
    }

    fn marking(
        value: &str,
        is_arrow: bool,
    ) -> Result<GameMoveNodeMarking, NotationPgnImportReason> {
        if !value.is_ascii() || value.len() != if is_arrow { 5 } else { 3 } {
            return Err(NotationPgnImportEnumSyntax::InvalidDirective.into());
        }
        let color = match value.as_bytes()[0] {
            b'G' => GameEnumMarkingColor::Green,
            b'R' => GameEnumMarkingColor::Red,
            b'B' => GameEnumMarkingColor::Blue,
            b'Y' => GameEnumMarkingColor::Yellow,
            _ => return Err(NotationPgnImportEnumSyntax::InvalidDirective.into()),
        };
        let origin = value[1..3]
            .parse()
            .map_err(|_error| NotationPgnImportEnumSyntax::InvalidDirective)?;
        if is_arrow {
            let destination = value[3..5]
                .parse()
                .map_err(|_error| NotationPgnImportEnumSyntax::InvalidDirective)?;
            Ok(GameMoveNodeMarking::arrow(color, origin, destination)?)
        } else {
            Ok(GameMoveNodeMarking::circle(color, origin))
        }
    }
}

struct NotationPgnImportGame {
    start: BoardStart,
    tree: GameTree,
    cursor: GameMoveId,
    mainline_tip: GameMoveId,
    variations: Vec<NotationPgnImportVariation>,
    annotations: Vec<NotationPgnImportNodeAnnotations>,
    annotation_indices: HashMap<GameMoveId, usize>,
    warnings: Vec<NotationGameEnumWarning>,
}

impl NotationPgnImportGame {
    fn new(start: BoardStart, root: GameMoveId) -> Self {
        let tree = GameTree::new(root, start.position().clone(), None);
        Self {
            start,
            tree,
            cursor: root,
            mainline_tip: root,
            variations: Vec::new(),
            annotations: vec![NotationPgnImportNodeAnnotations::new(root)],
            annotation_indices: HashMap::from([(root, 0)]),
            warnings: Vec::new(),
        }
    }

    fn movement<I: FnMut() -> Result<GameMoveId, GameIdentityError>>(
        &mut self,
        san: &str,
        ids: &mut I,
        budget: &mut NotationPgnImportBudget,
    ) -> Result<(), NotationPgnImportReason> {
        let parent = self.tree.node(self.cursor)?;
        let movement = parent.position().parse_san(san)?;
        for child in parent.children() {
            if self.tree.node(*child)?.movement() == Some(movement) {
                return Err(NotationPgnImportReason::UnsupportedOccurrence);
            }
        }
        budget.retain(
            u64::try_from(san.len()).map_err(|_error| NotationPgnImportEnumLimit::Arithmetic)?,
        )?;
        let id = ids()?;
        self.cursor = self.tree.append(self.cursor, id, movement, None)?;
        self.annotation_indices
            .insert(self.cursor, self.annotations.len());
        self.annotations
            .push(NotationPgnImportNodeAnnotations::new(self.cursor));
        if self.variations.is_empty() {
            self.mainline_tip = self.cursor;
        }
        Ok(())
    }

    fn begin_variation(&mut self) -> Result<(), NotationPgnImportReason> {
        let base = self
            .tree
            .node(self.cursor)?
            .parent()
            .ok_or(NotationPgnImportEnumSyntax::UnexpectedVariation)?;
        self.variations.push(NotationPgnImportVariation {
            resume: self.cursor,
            base,
        });
        self.cursor = base;
        Ok(())
    }

    fn end_variation(&mut self) -> Result<(), NotationPgnImportReason> {
        let variation = self
            .variations
            .pop()
            .ok_or(NotationPgnImportEnumSyntax::UnexpectedVariation)?;
        if variation.base == self.cursor {
            return Err(NotationPgnImportEnumSyntax::EmptyVariation.into());
        }
        self.cursor = variation.resume;
        Ok(())
    }

    fn annotation_index(&self) -> Result<usize, NotationPgnImportReason> {
        self.annotation_indices
            .get(&self.cursor)
            .copied()
            .ok_or_else(|| GameTreeError::UnknownNode.into())
    }

    fn nag(
        &mut self,
        value: u8,
        budget: &mut NotationPgnImportBudget,
    ) -> Result<(), NotationPgnImportReason> {
        let index = self.annotation_index()?;
        budget.reserve_retained_entry(&mut self.annotations[index].nags)?;
        self.annotations[index]
            .nags
            .push(GameMoveNodeNag::new(value));
        Ok(())
    }

    fn comment<C: Fn() -> bool>(
        &mut self,
        text: &str,
        budget: &mut NotationPgnImportBudget,
        cancellation: &C,
    ) -> Result<(), NotationPgnImportReason> {
        let index = self.annotation_index()?;
        let annotations = &mut self.annotations[index];
        if annotations.has_comment {
            annotations.append_notes("\n", budget)?;
        }
        annotations.has_comment = true;
        let mut rest = text;
        while let Some(opener) = rest.find("[%") {
            if cancellation() {
                return Err(NotationPgnImportReason::Cancelled);
            }
            annotations.append_notes(&rest[..opener], budget)?;
            rest = &rest[opener..];
            let bytes = rest.as_bytes();
            let end = bytes
                .iter()
                .enumerate()
                .skip(2)
                .find_map(|(index, byte)| {
                    if *byte == b']' {
                        Some(index + 1)
                    } else if *byte == b'[' && bytes.get(index + 1) == Some(&b'%') {
                        Some(index)
                    } else {
                        None
                    }
                })
                .unwrap_or(rest.len());
            let directive = &rest[..end];
            let is_closed = directive.ends_with(']');
            let body = if is_closed {
                &directive[2..directive.len() - 1]
            } else {
                &directive[2..]
            };
            let (name, value) = body
                .split_once(char::is_whitespace)
                .map_or((body, ""), |(name, value)| (name, value.trim()));
            match (is_closed, name) {
                (true, "clk") => {
                    let clock = NotationPgnImportNodeAnnotations::clock_value(value)?;
                    if annotations.clock.is_some_and(|existing| existing != clock) {
                        return Err(NotationPgnImportEnumSyntax::InvalidDirective.into());
                    }
                    annotations.clock = Some(clock);
                }
                (true, "csl" | "cal") => {
                    for value in value.split(',') {
                        if cancellation() {
                            return Err(NotationPgnImportReason::Cancelled);
                        }
                        let marking =
                            NotationPgnImportNodeAnnotations::marking(value.trim(), name == "cal")?;
                        budget.reserve_retained_entry(&mut annotations.markings)?;
                        annotations.markings.push(marking);
                    }
                }
                _ => {
                    annotations.append_notes(directive, budget)?;
                    budget.retain(
                        u64::try_from(directive.len())
                            .map_err(|_error| NotationPgnImportEnumLimit::Arithmetic)?,
                    )?;
                    budget.reserve_retained_entry(&mut self.warnings)?;
                    self.warnings
                        .push(NotationGameEnumWarning::UnknownDirective {
                            node: self.cursor,
                            directive: directive.to_owned(),
                        });
                }
            }
            rest = &rest[end..];
        }
        annotations.append_notes(rest, budget)
    }

    fn finish<C: Fn() -> bool>(
        mut self,
        headers: NotationPgnImportHeaders,
        result: NotationGameIdentityEnumResult,
        cancellation: &C,
    ) -> Result<NotationGame, NotationPgnImportReason> {
        let mut clocks = Vec::new();
        for annotation in self.annotations {
            if cancellation() {
                return Err(NotationPgnImportReason::Cancelled);
            }
            if let Some(remaining) = annotation.clock {
                clocks.push(NotationGameClock::new(annotation.node, remaining));
            }
            self.tree.set_annotations(
                annotation.node,
                GameMoveNodeAnnotations::new(
                    annotation.notes,
                    annotation.nags,
                    annotation.markings,
                )?,
            )?;
        }
        let mut warnings = headers.warnings;
        warnings.append(&mut self.warnings);
        Ok(NotationGame::new(NotationGameParts {
            start: self.start,
            tree: self.tree,
            mainline_tip: self.mainline_tip,
            metadata: GameMetadata::new(headers.known, headers.extensions)?,
            result,
            result_header: headers.result_header,
            time_control: headers.time_control,
            clocks,
            warnings,
        })?)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io::{BufRead, BufReader, Cursor, Read};

    use crate::board::r#move::BoardMoveError;
    use crate::game::enums::{GameIdentityError, GameMoveId};
    use crate::notation::pgn_import::budget::NotationPgnImportBudgetPolicy;
    use crate::notation::pgn_import::{
        NotationPgnImport, NotationPgnImportEnumLimit, NotationPgnImportEnumRecovery,
        NotationPgnImportLocation, NotationPgnImportReason,
    };

    struct NotationPgnImportFailingReader;

    impl Read for NotationPgnImportFailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }

    struct NotationPgnImportPolicyFixture;

    impl NotationPgnImportPolicyFixture {
        fn ids() -> impl FnMut() -> Result<GameMoveId, GameIdentityError> {
            let mut next = 0_u64;
            move || {
                next += 1;
                format!("00000000-0000-4000-8000-{next:012x}").parse()
            }
        }

        fn failure(
            input: &[u8],
            policy: NotationPgnImportBudgetPolicy,
            expected: NotationPgnImportEnumLimit,
        ) {
            let mut reader =
                NotationPgnImport::with_policy(Cursor::new(input), Self::ids(), policy);
            let error = reader.next_game(&|| false).err();
            assert!(
                matches!(error.as_ref().map(|error| error.reason()), Some(NotationPgnImportReason::Limit(actual)) if *actual == expected)
            );
        }

        fn recovery_failure<R: BufRead>(
            input: R,
            policy: NotationPgnImportBudgetPolicy,
            expected_location: NotationPgnImportLocation,
        ) -> Result<NotationPgnImportReason, Box<dyn Error>> {
            let mut reader = NotationPgnImport::with_policy(input, Self::ids(), policy);
            assert!(matches!(reader.next_game(&|| false), Ok(Some(_))));
            let error = reader
                .next_game(&|| false)
                .err()
                .ok_or("Expected the second game's recovery to fail")?;
            assert_eq!(error.game_ordinal(), 2);
            assert_eq!(
                error.location(),
                NotationPgnImportLocation {
                    byte_offset: 3,
                    line: 2,
                    column: 1,
                }
            );
            assert_eq!(error.recovery(), NotationPgnImportEnumRecovery::Terminated);
            assert!(!error.reason().is_cancelled());
            assert!(matches!(
                reader
                    .next_game(&|| false)
                    .as_ref()
                    .map_err(|error| error.reason()),
                Err(NotationPgnImportReason::Terminated)
            ));
            let NotationPgnImportReason::Recovery {
                primary,
                recovery,
                location,
            } = error.reason
            else {
                return Err("Expected both typed causes to be retained".into());
            };
            assert!(matches!(
                *primary,
                NotationPgnImportReason::Move(BoardMoveError::SanSyntax(_))
            ));
            assert_eq!(location, expected_location);
            Ok(*recovery)
        }
    }

    #[test]
    fn input_limit_reads_actual_bytes_and_does_not_masquerade_as_eof() {
        let policy = NotationPgnImportBudgetPolicy {
            input_bytes: 2,
            ..NotationPgnImportBudgetPolicy::production()
        };
        let mut exact = NotationPgnImport::with_policy(
            Cursor::new(b"* "),
            NotationPgnImportPolicyFixture::ids(),
            policy,
        );
        assert!(matches!(exact.next_game(&|| false), Ok(Some(_))));
        assert!(matches!(exact.next_game(&|| false), Ok(None)));
        let mut excess = NotationPgnImport::with_policy(
            Cursor::new(b"*  "),
            NotationPgnImportPolicyFixture::ids(),
            policy,
        );
        assert!(matches!(excess.next_game(&|| false), Ok(Some(_))));
        assert!(matches!(
            excess
                .next_game(&|| false)
                .as_ref()
                .map_err(|error| error.reason()),
            Err(NotationPgnImportReason::Limit(
                NotationPgnImportEnumLimit::InputBytes
            ))
        ));
    }

    #[test]
    fn excluded_games_and_plies_stay_in_stream_totals() {
        let policy = NotationPgnImportBudgetPolicy {
            total_plies: 2,
            ..NotationPgnImportBudgetPolicy::production()
        };
        let mut reader = NotationPgnImport::with_policy(
            Cursor::new(b"1. e4 bogus * 1. d4 *"),
            NotationPgnImportPolicyFixture::ids(),
            policy,
        );
        assert!(reader.next_game(&|| false).is_err());
        assert!(matches!(
            reader
                .next_game(&|| false)
                .as_ref()
                .map_err(|error| error.reason()),
            Err(NotationPgnImportReason::Limit(
                NotationPgnImportEnumLimit::TotalPlies
            ))
        ));
        let policy = NotationPgnImportBudgetPolicy {
            games: 1,
            ..NotationPgnImportBudgetPolicy::production()
        };
        let mut reader = NotationPgnImport::with_policy(
            Cursor::new(b"bogus * *"),
            NotationPgnImportPolicyFixture::ids(),
            policy,
        );
        assert!(reader.next_game(&|| false).is_err());
        assert!(matches!(
            reader
                .next_game(&|| false)
                .as_ref()
                .map_err(|error| error.reason()),
            Err(NotationPgnImportReason::Limit(
                NotationPgnImportEnumLimit::Games
            ))
        ));
    }

    #[test]
    fn branch_plies_text_and_decoded_growth_are_bounded() {
        NotationPgnImportPolicyFixture::failure(
            b"1. e4 (1. d4) *",
            NotationPgnImportBudgetPolicy {
                game_plies: 1,
                ..NotationPgnImportBudgetPolicy::production()
            },
            NotationPgnImportEnumLimit::GamePlies,
        );
        NotationPgnImportPolicyFixture::failure(
            b"1. e4 {ab}{cd} *",
            NotationPgnImportBudgetPolicy {
                text_bytes: 4,
                ..NotationPgnImportBudgetPolicy::production()
            },
            NotationPgnImportEnumLimit::TextBytes,
        );
        NotationPgnImportPolicyFixture::failure(
            b"{abc} *",
            NotationPgnImportBudgetPolicy {
                decoded_bytes: 2,
                ..NotationPgnImportBudgetPolicy::production()
            },
            NotationPgnImportEnumLimit::DecodedBytes,
        );
        NotationPgnImportPolicyFixture::failure(
            b"1. e4 (1. d4 (1. c4)) *",
            NotationPgnImportBudgetPolicy {
                variation_depth: 1,
                ..NotationPgnImportBudgetPolicy::production()
            },
            NotationPgnImportEnumLimit::VariationDepth,
        );
    }

    #[test]
    fn cancellation_during_error_drain_is_observed() -> Result<(), Box<dyn Error>> {
        let checks = std::cell::Cell::new(0_u32);
        let cancel = || {
            checks.set(checks.get() + 1);
            checks.get() > 50
        };
        let input = format!("bogus {{{}}} *", "x".repeat(1000));
        let mut reader = NotationPgnImport::new(
            Cursor::new(input.as_bytes()),
            NotationPgnImportPolicyFixture::ids(),
        );
        let error = reader
            .next_game(&cancel)
            .err()
            .ok_or("Expected cancellation during error recovery")?;
        assert!(error.reason().is_cancelled());
        assert_eq!(error.game_ordinal(), 1);
        assert_eq!(error.location().byte_offset(), 0);
        assert_eq!(error.recovery(), NotationPgnImportEnumRecovery::Terminated);
        let NotationPgnImportReason::Recovery {
            primary,
            recovery,
            location,
        } = error.reason()
        else {
            return Err("Expected the cancelled recovery and original move failure".into());
        };
        assert!(matches!(
            primary.as_ref(),
            NotationPgnImportReason::Move(BoardMoveError::SanSyntax(_))
        ));
        assert!(matches!(
            recovery.as_ref(),
            NotationPgnImportReason::Cancelled
        ));
        assert!(location.byte_offset() > error.location().byte_offset());
        assert!(location.byte_offset() < u64::try_from(input.len())?);
        Ok(())
    }

    #[test]
    fn failed_error_drain_retains_typed_io_failure() -> Result<(), Box<dyn Error>> {
        let input =
            BufReader::new(Cursor::new(b"* \nbogus {").chain(NotationPgnImportFailingReader));
        let recovery = NotationPgnImportPolicyFixture::recovery_failure(
            input,
            NotationPgnImportBudgetPolicy::production(),
            NotationPgnImportLocation {
                byte_offset: 10,
                line: 2,
                column: 8,
            },
        )?;
        assert!(matches!(
            recovery,
            NotationPgnImportReason::Io(error) if error.kind() == std::io::ErrorKind::BrokenPipe
        ));
        Ok(())
    }

    #[test]
    fn failed_error_drain_retains_typed_utf8_failure() -> Result<(), Box<dyn Error>> {
        let recovery = NotationPgnImportPolicyFixture::recovery_failure(
            Cursor::new(b"* \nbogus {\xff} *"),
            NotationPgnImportBudgetPolicy::production(),
            NotationPgnImportLocation {
                byte_offset: 12,
                line: 2,
                column: 10,
            },
        )?;
        assert!(matches!(
            recovery,
            NotationPgnImportReason::Utf8(error) if error.valid_up_to() == 0
        ));
        Ok(())
    }

    #[test]
    fn failed_error_drain_retains_typed_limit_failure() -> Result<(), Box<dyn Error>> {
        let recovery = NotationPgnImportPolicyFixture::recovery_failure(
            Cursor::new(b"* \nbogus {text} *"),
            NotationPgnImportBudgetPolicy {
                input_bytes: 11,
                ..NotationPgnImportBudgetPolicy::production()
            },
            NotationPgnImportLocation {
                byte_offset: 11,
                line: 2,
                column: 9,
            },
        )?;
        assert!(matches!(
            recovery,
            NotationPgnImportReason::Limit(NotationPgnImportEnumLimit::InputBytes)
        ));
        Ok(())
    }
}
