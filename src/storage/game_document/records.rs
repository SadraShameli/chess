use std::fmt::{Formatter, Result as FormatResult};

use serde::de::{Error as DeserializeError, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::board::r#move::BoardMove;
use crate::board::position::BoardPosition;
use crate::board::square::BoardSquare;
use crate::capacity::executor::CapacityExecutorCancellation;
use crate::clock::duration::ClockDuration;
use crate::clock::state::ClockStateSnapshot;
use crate::clock::time_control::ClockTimeControl;
use crate::game::enums::GameMoveId;
use crate::game::move_node::{GameMoveNodeAnnotations, GameMoveNodeMarking, GameMoveNodeNag};
use crate::game::session::GameSessionRestoreError;
use crate::game::session::{
    GameMetadata, GameMetadataDate, GameMetadataError, GameMetadataExtension,
    GameMetadataKnownTags, GameMetadataOpeningCode, GameMetadataText,
};
use crate::game::tree::{GameTree, GameTreeCandidateClock, GameTreeError};
use crate::storage::game_document::{
    StorageGameDocument, StorageGameDocumentClock, StorageGameDocumentError,
};

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct StorageGameDocumentDuration(u64);

impl From<ClockDuration> for StorageGameDocumentDuration {
    fn from(duration: ClockDuration) -> Self {
        Self(duration.as_nanos())
    }
}

impl From<StorageGameDocumentDuration> for ClockDuration {
    fn from(duration: StorageGameDocumentDuration) -> Self {
        Self::from_nanos(duration.0)
    }
}

#[derive(Serialize)]
#[serde(transparent)]
pub(super) struct StorageGameDocumentText(pub(super) String);

impl StorageGameDocumentText {
    pub(super) fn copy(value: &str) -> Result<Self, StorageGameDocumentError> {
        Self::validate_length(value)?;
        Ok(Self(value.to_owned()))
    }

    fn validate_length(value: &str) -> Result<(), StorageGameDocumentError> {
        if value.len() > StorageGameDocument::MAX_TEXT_BYTES {
            return Err(StorageGameDocumentError::TextTooLarge);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for StorageGameDocumentText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl TryFrom<String> for StorageGameDocumentText {
    type Error = StorageGameDocumentError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::validate_length(&value)?;
        Ok(Self(value))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentMarking {
    color: crate::game::enums::GameEnumMarkingColor,
    origin: BoardSquare,
    destination: Option<BoardSquare>,
}

impl From<GameMoveNodeMarking> for StorageGameDocumentMarking {
    fn from(marking: GameMoveNodeMarking) -> Self {
        Self {
            color: marking.color(),
            origin: marking.origin(),
            destination: marking.destination(),
        }
    }
}

impl StorageGameDocumentMarking {
    fn into_marking(self) -> Result<GameMoveNodeMarking, StorageGameDocumentError> {
        Ok(match self.destination {
            Some(destination) => GameMoveNodeMarking::arrow(self.color, self.origin, destination)?,
            None => GameMoveNodeMarking::circle(self.color, self.origin),
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StorageGameDocumentAnnotations {
    notes: StorageGameDocumentText,
    nags: Vec<u8>,
    markings: Vec<StorageGameDocumentMarking>,
}

impl StorageGameDocumentAnnotations {
    pub(super) fn capture(
        annotations: &GameMoveNodeAnnotations,
    ) -> Result<Self, StorageGameDocumentError> {
        Ok(Self {
            notes: StorageGameDocumentText::copy(annotations.notes())?,
            nags: annotations.nags().iter().map(|nag| nag.value()).collect(),
            markings: annotations
                .markings()
                .iter()
                .copied()
                .map(StorageGameDocumentMarking::from)
                .collect(),
        })
    }

    pub(super) fn restore(self) -> Result<GameMoveNodeAnnotations, StorageGameDocumentError> {
        Ok(GameMoveNodeAnnotations::new(
            self.notes.0,
            self.nags.into_iter().map(GameMoveNodeNag::new).collect(),
            self.markings
                .into_iter()
                .map(StorageGameDocumentMarking::into_marking)
                .collect::<Result<_, _>>()?,
        )?)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StorageGameDocumentMove {
    id: GameMoveId,
    parent: GameMoveId,
    uci: String,
    annotations: StorageGameDocumentAnnotations,
    #[serde(skip_serializing_if = "Option::is_none")]
    before_clock: Option<StorageGameDocumentClock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    clock: Option<StorageGameDocumentClock>,
}

pub(super) struct StorageGameDocumentMoveSource<'a> {
    pub(super) id: GameMoveId,
    pub(super) parent: GameMoveId,
    pub(super) movement: BoardMove,
    pub(super) annotations: &'a GameMoveNodeAnnotations,
    pub(super) before_clock: Option<&'a ClockStateSnapshot>,
    pub(super) clock: Option<&'a ClockStateSnapshot>,
}

impl StorageGameDocumentMove {
    pub(super) fn capture(
        source: StorageGameDocumentMoveSource<'_>,
        initial: &BoardPosition,
        control: Option<&ClockTimeControl>,
    ) -> Result<Self, StorageGameDocumentError> {
        Ok(Self {
            id: source.id,
            parent: source.parent,
            uci: source.movement.uci(initial.variant()),
            annotations: StorageGameDocumentAnnotations::capture(source.annotations)?,
            before_clock: Self::capture_clock(source.before_clock, control)?,
            clock: Self::capture_clock(source.clock, control)?,
        })
    }

    fn capture_clock(
        clock: Option<&ClockStateSnapshot>,
        control: Option<&ClockTimeControl>,
    ) -> Result<Option<StorageGameDocumentClock>, StorageGameDocumentError> {
        match (clock, control) {
            (Some(clock), Some(control)) => {
                Ok(Some(StorageGameDocumentClock::capture(clock, control)?))
            }
            (Some(_), None) => Err(StorageGameDocumentError::NotationRuntimeClock),
            (None, _) => Ok(None),
        }
    }

    fn restore_clock(
        clock: Option<StorageGameDocumentClock>,
        control: Option<&ClockTimeControl>,
    ) -> Result<Option<ClockStateSnapshot>, StorageGameDocumentError> {
        match (clock, control) {
            (Some(clock), Some(control)) => Ok(Some(clock.restore(control)?)),
            (Some(_), None) => Err(StorageGameDocumentError::NotationRuntimeClock),
            (None, _) => Ok(None),
        }
    }

    fn restore(
        self,
        tree: &mut GameTree,
        control: Option<&ClockTimeControl>,
    ) -> Result<(), StorageGameDocumentError> {
        let movement = tree.node(self.parent)?.position().parse_uci(&self.uci)?;
        let before = Self::restore_clock(self.before_clock, control)?;
        let after = Self::restore_clock(self.clock, control)?;
        let clock = match (before, after) {
            (Some(before), Some(after)) => Some(GameTreeCandidateClock { before, after }),
            (None, None) => None,
            (Some(_), None) | (None, Some(_)) => {
                return Err(StorageGameDocumentError::Session(
                    GameSessionRestoreError::UnpairedClocks.into(),
                ));
            }
        };
        tree.restore_move(self.parent, self.id, movement, clock)?;
        tree.set_annotations(self.id, self.annotations.restore()?)?;
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(transparent)]
pub(super) struct StorageGameDocumentMoves(pub(super) Vec<StorageGameDocumentMove>);

impl StorageGameDocumentMoves {
    pub(super) fn capture(
        tree: &GameTree,
        control: Option<&ClockTimeControl>,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<Self, StorageGameDocumentError> {
        if tree.len().saturating_sub(1) > StorageGameDocument::MAX_MOVES {
            return Err(StorageGameDocumentError::TooManyMoves);
        }
        let mut moves = Vec::with_capacity(tree.len().saturating_sub(1));
        let initial = tree.node(tree.root())?.position();
        for node in tree.nodes() {
            StorageGameDocumentError::check_cancellation(cancellation)?;
            if let (Some(parent), Some(movement)) = (node.parent(), node.movement()) {
                moves.push(StorageGameDocumentMove::capture(
                    StorageGameDocumentMoveSource {
                        id: node.id(),
                        parent,
                        movement,
                        annotations: node.annotations(),
                        before_clock: node.before_clock(),
                        clock: node.clock(),
                    },
                    initial,
                    control,
                )?);
            }
        }
        Ok(Self(moves))
    }

    pub(super) fn restore(
        self,
        mut tree: GameTree,
        control: Option<&ClockTimeControl>,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<GameTree, StorageGameDocumentError> {
        for movement in self.0 {
            StorageGameDocumentError::check_cancellation(cancellation)?;
            if tree.contains(movement.id) {
                return Err(GameTreeError::DuplicateIdentity.into());
            }
            movement.restore(&mut tree, control)?;
        }
        Ok(tree)
    }
}

struct StorageGameDocumentMovesVisitor;

impl<'de> Visitor<'de> for StorageGameDocumentMovesVisitor {
    type Value = StorageGameDocumentMoves;
    fn expecting(&self, formatter: &mut Formatter<'_>) -> FormatResult {
        formatter.write_str("a bounded game move-node sequence")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        if sequence
            .size_hint()
            .is_some_and(|length| length > StorageGameDocument::MAX_MOVES)
        {
            return Err(A::Error::custom(StorageGameDocumentError::TooManyMoves));
        }
        let mut moves = Vec::new();
        while moves.len() < StorageGameDocument::MAX_MOVES {
            match sequence.next_element()? {
                Some(movement) => moves.push(movement),
                None => return Ok(StorageGameDocumentMoves(moves)),
            }
        }
        if sequence.next_element::<IgnoredAny>()?.is_some() {
            return Err(A::Error::custom(StorageGameDocumentError::TooManyMoves));
        }
        Ok(StorageGameDocumentMoves(moves))
    }
}

impl<'de> Deserialize<'de> for StorageGameDocumentMoves {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_seq(StorageGameDocumentMovesVisitor)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentDate {
    year: Option<u16>,
    month: Option<u8>,
    day: Option<u8>,
}

impl From<GameMetadataDate> for StorageGameDocumentDate {
    fn from(date: GameMetadataDate) -> Self {
        Self {
            year: date.year(),
            month: date.month(),
            day: date.day(),
        }
    }
}

impl StorageGameDocumentDate {
    fn restore(self) -> Result<GameMetadataDate, StorageGameDocumentError> {
        Ok(GameMetadataDate::new(self.year, self.month, self.day)?)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentExtension {
    name: StorageGameDocumentText,
    value: StorageGameDocumentText,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StorageGameDocumentMetadata {
    event: Option<StorageGameDocumentText>,
    site: Option<StorageGameDocumentText>,
    date: Option<StorageGameDocumentDate>,
    round: Option<StorageGameDocumentText>,
    white: Option<StorageGameDocumentText>,
    black: Option<StorageGameDocumentText>,
    white_elo: Option<u32>,
    black_elo: Option<u32>,
    opening: Option<StorageGameDocumentText>,
    variation: Option<StorageGameDocumentText>,
    eco: Option<StorageGameDocumentText>,
    annotator: Option<StorageGameDocumentText>,
    extensions: Vec<StorageGameDocumentExtension>,
}

impl StorageGameDocumentMetadata {
    fn text(
        value: &Option<GameMetadataText>,
    ) -> Result<Option<StorageGameDocumentText>, StorageGameDocumentError> {
        value
            .as_ref()
            .map(|text| StorageGameDocumentText::copy(text.as_str()))
            .transpose()
    }
    fn restored_text(
        value: Option<StorageGameDocumentText>,
    ) -> Result<Option<GameMetadataText>, StorageGameDocumentError> {
        value
            .map(|text| GameMetadataText::new(text.0).map_err(StorageGameDocumentError::Metadata))
            .transpose()
    }
    pub(super) fn capture(metadata: &GameMetadata) -> Result<Self, StorageGameDocumentError> {
        let known = metadata.known();
        Ok(Self {
            event: Self::text(&known.event)?,
            site: Self::text(&known.site)?,
            date: known.date.map(StorageGameDocumentDate::from),
            round: Self::text(&known.round)?,
            white: Self::text(&known.white)?,
            black: Self::text(&known.black)?,
            white_elo: known.white_elo,
            black_elo: known.black_elo,
            opening: Self::text(&known.opening)?,
            variation: Self::text(&known.variation)?,
            eco: known
                .eco
                .as_ref()
                .map(|code| StorageGameDocumentText::copy(code.as_str()))
                .transpose()?,
            annotator: Self::text(&known.annotator)?,
            extensions: metadata
                .extensions()
                .iter()
                .map(|extension| {
                    Ok(StorageGameDocumentExtension {
                        name: StorageGameDocumentText::copy(extension.name())?,
                        value: StorageGameDocumentText::copy(extension.value().as_str())?,
                    })
                })
                .collect::<Result<_, StorageGameDocumentError>>()?,
        })
    }
    pub(super) fn restore(self) -> Result<GameMetadata, StorageGameDocumentError> {
        let known = GameMetadataKnownTags {
            event: Self::restored_text(self.event)?,
            site: Self::restored_text(self.site)?,
            date: self
                .date
                .map(StorageGameDocumentDate::restore)
                .transpose()?,
            round: Self::restored_text(self.round)?,
            white: Self::restored_text(self.white)?,
            black: Self::restored_text(self.black)?,
            white_elo: self.white_elo,
            black_elo: self.black_elo,
            opening: Self::restored_text(self.opening)?,
            variation: Self::restored_text(self.variation)?,
            eco: self
                .eco
                .map(|code| GameMetadataOpeningCode::new(code.0))
                .transpose()?,
            annotator: Self::restored_text(self.annotator)?,
        };
        let extensions = self
            .extensions
            .into_iter()
            .map(|extension| {
                GameMetadataExtension::new(
                    extension.name.0,
                    GameMetadataText::new(extension.value.0)?,
                )
            })
            .collect::<Result<_, GameMetadataError>>()?;
        Ok(GameMetadata::new(known, extensions)?)
    }
}
