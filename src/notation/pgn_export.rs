use std::collections::HashMap;
use std::fmt::Display;
use std::io::{self, Write};

use thiserror::Error;

use crate::board::enums::{BoardEnumColor, BoardEnumVariant};
use crate::board::position::BoardPosition;
use crate::clock::duration::ClockDuration;
use crate::game::enums::{GameEnumMarkingColor, GameMoveId};
use crate::game::move_node::GameMoveNode;
use crate::game::session::GameMetadataText;
use crate::game::tree::GameTreeError;
use crate::notation::game::{NotationGame, NotationGameEnumWarning};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NotationPgnExportEnumSelection {
    #[default]
    Full,
    Mainline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NotationPgnExportEnumCommentIssue {
    #[error("Literal recognized annotation directives cannot be represented losslessly")]
    RecognizedDirective,
    #[error("Brace-containing notes with carriage returns cannot use lossless line comments")]
    CarriageReturnInLineComment,
}

#[derive(Debug, Error)]
pub enum NotationPgnExportError {
    #[error("PGN export was cancelled")]
    Cancelled,
    #[error("PGN destination failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Tree(#[from] GameTreeError),
    #[error("The selected mainline tip {node:?} has continuations that full PGN cannot represent")]
    MainlineHasContinuations { node: GameMoveId },
    #[error("Notes at {node:?} cannot be exported: {reason}")]
    UnsupportedComment {
        node: GameMoveId,
        reason: NotationPgnExportEnumCommentIssue,
    },
    #[error("Tag {name} contains a control character that PGN cannot preserve")]
    UnsupportedTagValue { name: String },
}

enum NotationPgnExportEnumTag {
    Event,
    Site,
    Date,
    Round,
    White,
    Black,
    Result,
    WhiteElo,
    BlackElo,
    Opening,
    Variation,
    Eco,
    Annotator,
    TimeControl,
    Variant,
    SetUp,
    Fen,
}

impl NotationPgnExportEnumTag {
    const fn name(&self) -> &'static str {
        match self {
            Self::Event => "Event",
            Self::Site => "Site",
            Self::Date => "Date",
            Self::Round => "Round",
            Self::White => "White",
            Self::Black => "Black",
            Self::Result => "Result",
            Self::WhiteElo => "WhiteElo",
            Self::BlackElo => "BlackElo",
            Self::Opening => "Opening",
            Self::Variation => "Variation",
            Self::Eco => "ECO",
            Self::Annotator => "Annotator",
            Self::TimeControl => "TimeControl",
            Self::Variant => "Variant",
            Self::SetUp => "SetUp",
            Self::Fen => "FEN",
        }
    }
}

#[derive(Clone, Copy)]
enum NotationPgnExportEnumDirective {
    Circle,
    Arrow,
    Clock,
}

impl NotationPgnExportEnumDirective {
    const ALL: [Self; 3] = [Self::Circle, Self::Arrow, Self::Clock];

    const fn opening(self) -> &'static str {
        match self {
            Self::Circle => "[%csl",
            Self::Arrow => "[%cal",
            Self::Clock => "[%clk",
        }
    }

    fn occurs_in(self, notes: &str) -> bool {
        notes.match_indices(self.opening()).any(|(index, opening)| {
            notes[index + opening.len()..]
                .chars()
                .next()
                .is_some_and(|next| next.is_whitespace() || next == ']')
        })
    }
}

enum NotationPgnExportEnumSpacing {
    Adjacent,
    Space,
}

struct NotationPgnExportWriter<'a, W, C> {
    destination: &'a mut W,
    cancellation: C,
    spacing: NotationPgnExportEnumSpacing,
}

impl<'a, W: Write, C: FnMut() -> bool> NotationPgnExportWriter<'a, W, C> {
    const CHUNK_BYTES: usize = 4096;

    fn new(destination: &'a mut W, cancellation: C) -> Self {
        Self {
            destination,
            cancellation,
            spacing: NotationPgnExportEnumSpacing::Adjacent,
        }
    }

    fn check_cancellation(&mut self) -> Result<(), NotationPgnExportError> {
        if (self.cancellation)() {
            Err(NotationPgnExportError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn raw(&mut self, text: &str) -> Result<(), NotationPgnExportError> {
        let mut bytes = text.as_bytes();
        while !bytes.is_empty() {
            self.check_cancellation()?;
            let chunk = &bytes[..bytes.len().min(Self::CHUNK_BYTES)];
            match self.destination.write(chunk) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero).into()),
                Ok(written) => {
                    if written > chunk.len() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "The destination returned an invalid write length",
                        )
                        .into());
                    }
                    bytes = &bytes[written..];
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn begin_token(&mut self) -> Result<(), NotationPgnExportError> {
        match self.spacing {
            NotationPgnExportEnumSpacing::Adjacent => {}
            NotationPgnExportEnumSpacing::Space => self.raw(" ")?,
        }
        self.spacing = NotationPgnExportEnumSpacing::Space;
        Ok(())
    }

    fn token(&mut self, text: &str) -> Result<(), NotationPgnExportError> {
        self.begin_token()?;
        self.raw(text)
    }

    fn open_variation(&mut self) -> Result<(), NotationPgnExportError> {
        self.token("(")?;
        self.spacing = NotationPgnExportEnumSpacing::Adjacent;
        Ok(())
    }

    fn close_variation(&mut self) -> Result<(), NotationPgnExportError> {
        self.raw(")")?;
        self.spacing = NotationPgnExportEnumSpacing::Space;
        Ok(())
    }

    fn tag(&mut self, name: &str, value: &str) -> Result<(), NotationPgnExportError> {
        if value.chars().any(char::is_control) {
            return Err(NotationPgnExportError::UnsupportedTagValue {
                name: name.to_owned(),
            });
        }
        self.raw("[")?;
        self.raw(name)?;
        self.raw(" \"")?;
        let mut start = 0;
        for (index, escaped) in value.match_indices(['\\', '"']) {
            self.raw(&value[start..index])?;
            self.raw("\\")?;
            self.raw(escaped)?;
            start = index + escaped.len();
        }
        self.raw(&value[start..])?;
        self.raw("\"]\n")
    }

    fn known_tag(
        &mut self,
        tag: NotationPgnExportEnumTag,
        value: &str,
    ) -> Result<(), NotationPgnExportError> {
        self.tag(tag.name(), value)
    }

    fn optional_tag(
        &mut self,
        tag: NotationPgnExportEnumTag,
        value: Option<&GameMetadataText>,
    ) -> Result<(), NotationPgnExportError> {
        if let Some(value) = value {
            self.known_tag(tag, value.as_str())?;
        }
        Ok(())
    }
}

struct NotationPgnExportLine {
    node: GameMoveId,
    mainline_index: Option<usize>,
    starts_line: bool,
    includes_siblings: bool,
}

enum NotationPgnExportEnumFrame {
    Line(NotationPgnExportLine),
    Siblings {
        parent: GameMoveId,
        selected: GameMoveId,
        next_index: usize,
    },
    CloseVariation,
}

pub struct NotationPgnExport<'a> {
    game: &'a NotationGame,
    selection: NotationPgnExportEnumSelection,
}

struct NotationPgnExportTraversal<'a> {
    path: &'a [GameMoveId],
    clocks: &'a HashMap<GameMoveId, ClockDuration>,
}

impl<'a> NotationPgnExport<'a> {
    const MINUTES_PER_HOUR: u64 = 60;
    const SECONDS_PER_MINUTE: u64 = 60;
    const YEAR_WIDTH: usize = 4;
    const DATE_COMPONENT_WIDTH: usize = 2;

    pub const fn new(game: &'a NotationGame) -> Self {
        Self {
            game,
            selection: NotationPgnExportEnumSelection::Full,
        }
    }

    pub const fn with_selection(mut self, selection: NotationPgnExportEnumSelection) -> Self {
        self.selection = selection;
        self
    }

    pub fn write<W: Write, C: FnMut() -> bool>(
        &self,
        destination: &mut W,
        cancellation: C,
    ) -> Result<(), NotationPgnExportError> {
        let mut writer = NotationPgnExportWriter::new(destination, cancellation);
        writer.check_cancellation()?;
        let tree = self.game.tree();
        let path = tree.path_to(self.game.mainline_tip())?;
        if self.selection == NotationPgnExportEnumSelection::Full
            && !tree.node(self.game.mainline_tip())?.children().is_empty()
        {
            return Err(NotationPgnExportError::MainlineHasContinuations {
                node: self.game.mainline_tip(),
            });
        }
        let mut clocks = HashMap::with_capacity(self.game.clocks().len());
        for clock in self.game.clocks() {
            writer.check_cancellation()?;
            clocks.insert(clock.node(), clock.remaining());
        }
        self.headers(&mut writer)?;
        writer.raw("\n")?;
        self.annotations(tree.node(tree.root())?, &clocks, &mut writer)?;
        let traversal = NotationPgnExportTraversal {
            path: &path,
            clocks: &clocks,
        };
        let mut pending = Vec::new();
        if let Some(node) = path.get(1) {
            pending.push(NotationPgnExportEnumFrame::Line(NotationPgnExportLine {
                node: *node,
                mainline_index: Some(1),
                starts_line: true,
                includes_siblings: true,
            }));
        }
        while let Some(frame) = pending.pop() {
            writer.check_cancellation()?;
            match frame {
                NotationPgnExportEnumFrame::Line(line) => {
                    self.line(line, &traversal, &mut pending, &mut writer)?
                }
                NotationPgnExportEnumFrame::Siblings {
                    parent,
                    selected,
                    next_index,
                } => {
                    let children = tree.node(parent)?.children();
                    if let Some((index, node)) = children
                        .iter()
                        .enumerate()
                        .skip(next_index)
                        .find(|(_, node)| **node != selected)
                    {
                        pending.push(NotationPgnExportEnumFrame::Siblings {
                            parent,
                            selected,
                            next_index: index + 1,
                        });
                        pending.push(NotationPgnExportEnumFrame::CloseVariation);
                        pending.push(NotationPgnExportEnumFrame::Line(NotationPgnExportLine {
                            node: *node,
                            mainline_index: None,
                            starts_line: true,
                            includes_siblings: false,
                        }));
                        writer.open_variation()?;
                    }
                }
                NotationPgnExportEnumFrame::CloseVariation => writer.close_variation()?,
            }
        }
        writer.token(self.game.result().pgn_token())?;
        writer.raw("\n")
    }

    fn headers<W: Write, C: FnMut() -> bool>(
        &self,
        writer: &mut NotationPgnExportWriter<'_, W, C>,
    ) -> Result<(), NotationPgnExportError> {
        let known = self.game.metadata().known();
        writer.optional_tag(NotationPgnExportEnumTag::Event, known.event.as_ref())?;
        writer.optional_tag(NotationPgnExportEnumTag::Site, known.site.as_ref())?;
        if let Some(date) = known.date {
            writer.known_tag(
                NotationPgnExportEnumTag::Date,
                &format!(
                    "{}.{}.{}",
                    Self::date_component(date.year(), Self::YEAR_WIDTH),
                    Self::date_component(date.month(), Self::DATE_COMPONENT_WIDTH),
                    Self::date_component(date.day(), Self::DATE_COMPONENT_WIDTH)
                ),
            )?;
        }
        writer.optional_tag(NotationPgnExportEnumTag::Round, known.round.as_ref())?;
        writer.optional_tag(NotationPgnExportEnumTag::White, known.white.as_ref())?;
        writer.optional_tag(NotationPgnExportEnumTag::Black, known.black.as_ref())?;
        if let Some(result) = self.game.result_header() {
            writer.known_tag(NotationPgnExportEnumTag::Result, result.pgn_token())?;
        }
        self.rating(BoardEnumColor::White, writer)?;
        self.rating(BoardEnumColor::Black, writer)?;
        writer.optional_tag(NotationPgnExportEnumTag::Opening, known.opening.as_ref())?;
        writer.optional_tag(
            NotationPgnExportEnumTag::Variation,
            known.variation.as_ref(),
        )?;
        if let Some(code) = &known.eco {
            writer.known_tag(NotationPgnExportEnumTag::Eco, code.as_str())?;
        }
        writer.optional_tag(
            NotationPgnExportEnumTag::Annotator,
            known.annotator.as_ref(),
        )?;
        writer.optional_tag(
            NotationPgnExportEnumTag::TimeControl,
            self.game.time_control(),
        )?;
        let position = self.game.start().position();
        let variant = match position.variant() {
            BoardEnumVariant::Standard => "Standard",
            BoardEnumVariant::Chess960 => "Chess960",
        };
        writer.known_tag(NotationPgnExportEnumTag::Variant, variant)?;
        if position.variant() == BoardEnumVariant::Chess960
            || position != &BoardPosition::standard()
        {
            writer.known_tag(NotationPgnExportEnumTag::SetUp, "1")?;
            writer.known_tag(
                NotationPgnExportEnumTag::Fen,
                self.game.start().initial_fen(),
            )?;
        }
        for extension in self.game.metadata().extensions() {
            writer.tag(extension.name(), extension.value().as_str())?;
        }
        Ok(())
    }

    fn rating<W: Write, C: FnMut() -> bool>(
        &self,
        color: BoardEnumColor,
        writer: &mut NotationPgnExportWriter<'_, W, C>,
    ) -> Result<(), NotationPgnExportError> {
        let known = self.game.metadata().known();
        let (tag, rating) = match color {
            BoardEnumColor::White => (NotationPgnExportEnumTag::WhiteElo, known.white_elo),
            BoardEnumColor::Black => (NotationPgnExportEnumTag::BlackElo, known.black_elo),
        };
        if let Some(rating) = rating {
            return writer.known_tag(tag, &rating.to_string());
        }
        for warning in self.game.warnings() {
            match warning {
                NotationGameEnumWarning::UnknownDirective { .. } => {}
                NotationGameEnumWarning::UnavailableRating {
                    color: warning_color,
                    value,
                } => {
                    if *warning_color == color {
                        return writer.known_tag(tag, value.as_str());
                    }
                }
            }
        }
        Ok(())
    }

    fn date_component<T: Display>(value: Option<T>, width: usize) -> String {
        match value {
            Some(value) => format!("{value:0width$}"),
            None => "?".repeat(width),
        }
    }

    fn line<W: Write, C: FnMut() -> bool>(
        &self,
        line: NotationPgnExportLine,
        traversal: &NotationPgnExportTraversal<'_>,
        pending: &mut Vec<NotationPgnExportEnumFrame>,
        writer: &mut NotationPgnExportWriter<'_, W, C>,
    ) -> Result<(), NotationPgnExportError> {
        let tree = self.game.tree();
        let node = tree.node(line.node)?;
        let parent = tree.node(node.parent().ok_or(GameTreeError::BrokenRelationship)?)?;
        match parent.position().turn() {
            BoardEnumColor::White => {
                writer.token(&format!("{}.", parent.position().fullmoves()))?
            }
            BoardEnumColor::Black => {
                if line.starts_line {
                    writer.token(&format!("{}...", parent.position().fullmoves()))?;
                }
            }
        }
        writer.token(node.san().ok_or(GameTreeError::BrokenRelationship)?)?;
        let annotated = self.annotations(node, traversal.clocks, writer)?;
        let includes_siblings = self.selection == NotationPgnExportEnumSelection::Full
            && line.includes_siblings
            && parent.children().len() > 1;
        let next = match line.mainline_index {
            Some(index) => traversal
                .path
                .get(index + 1)
                .copied()
                .map(|node| (node, Some(index + 1))),
            None => node.children().first().copied().map(|node| (node, None)),
        };
        if let Some((node, mainline_index)) = next {
            pending.push(NotationPgnExportEnumFrame::Line(NotationPgnExportLine {
                node,
                mainline_index,
                starts_line: annotated || includes_siblings,
                includes_siblings: true,
            }));
        }
        if includes_siblings {
            pending.push(NotationPgnExportEnumFrame::Siblings {
                parent: parent.id(),
                selected: node.id(),
                next_index: 0,
            });
        }
        Ok(())
    }

    fn annotations<W: Write, C: FnMut() -> bool>(
        &self,
        node: &GameMoveNode,
        clocks: &HashMap<GameMoveId, ClockDuration>,
        writer: &mut NotationPgnExportWriter<'_, W, C>,
    ) -> Result<bool, NotationPgnExportError> {
        let annotations = node.annotations();
        let notes = annotations.notes();
        if NotationPgnExportEnumDirective::ALL
            .iter()
            .any(|directive| directive.occurs_in(notes))
        {
            return Err(NotationPgnExportError::UnsupportedComment {
                node: node.id(),
                reason: NotationPgnExportEnumCommentIssue::RecognizedDirective,
            });
        }
        let line_comments = notes.contains(['{', '}']);
        if line_comments && notes.contains('\r') {
            return Err(NotationPgnExportError::UnsupportedComment {
                node: node.id(),
                reason: NotationPgnExportEnumCommentIssue::CarriageReturnInLineComment,
            });
        }
        for nag in annotations.nags() {
            writer.token(&format!("${}", nag.value()))?;
        }
        let clock = clocks.get(&node.id()).copied();
        let has_comment =
            !notes.is_empty() || !annotations.markings().is_empty() || clock.is_some();
        if has_comment {
            writer.begin_token()?;
            if line_comments {
                let mut lines = notes.split('\n').peekable();
                while let Some(line) = lines.next() {
                    writer.raw(";")?;
                    writer.raw(line)?;
                    if lines.peek().is_none() {
                        self.directives(node, clock, writer)?;
                    }
                    writer.raw("\n")?;
                }
                writer.spacing = NotationPgnExportEnumSpacing::Adjacent;
            } else {
                writer.raw("{")?;
                writer.raw(notes)?;
                self.directives(node, clock, writer)?;
                writer.raw("}")?;
            }
        }
        Ok(has_comment || !annotations.nags().is_empty())
    }

    fn directives<W: Write, C: FnMut() -> bool>(
        &self,
        node: &GameMoveNode,
        clock: Option<ClockDuration>,
        writer: &mut NotationPgnExportWriter<'_, W, C>,
    ) -> Result<(), NotationPgnExportError> {
        for marking in node.annotations().markings() {
            let directive = match marking.destination() {
                Some(_) => NotationPgnExportEnumDirective::Arrow,
                None => NotationPgnExportEnumDirective::Circle,
            };
            writer.raw(directive.opening())?;
            writer.raw(" ")?;
            writer.raw(match marking.color() {
                GameEnumMarkingColor::Green => "G",
                GameEnumMarkingColor::Red => "R",
                GameEnumMarkingColor::Blue => "B",
                GameEnumMarkingColor::Yellow => "Y",
            })?;
            writer.raw(&marking.origin().to_string())?;
            if let Some(destination) = marking.destination() {
                writer.raw(&destination.to_string())?;
            }
            writer.raw("]")?;
        }
        if let Some(clock) = clock {
            let duration = clock.as_duration();
            let total_seconds = duration.as_secs();
            let total_minutes = total_seconds / Self::SECONDS_PER_MINUTE;
            let hours = total_minutes / Self::MINUTES_PER_HOUR;
            let minutes = total_minutes % Self::MINUTES_PER_HOUR;
            let seconds = total_seconds % Self::SECONDS_PER_MINUTE;
            writer.raw(NotationPgnExportEnumDirective::Clock.opening())?;
            writer.raw(&format!(" {hours}:{minutes:02}:{seconds:02}"))?;
            if duration.subsec_nanos() > 0 {
                writer.raw(".")?;
                writer.raw(format!("{:09}", duration.subsec_nanos()).trim_end_matches('0'))?;
            }
            writer.raw("]")?;
        }
        Ok(())
    }
}
