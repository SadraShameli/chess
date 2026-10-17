use serde::{Deserialize, Serialize};

use crate::board::enums::BoardEnumColor;
use crate::board::start::BoardStart;
use crate::capacity::executor::CapacityExecutorCancellation;
use crate::game::enums::{GameEnumResult, GameId, GameMoveId};
use crate::game::session::GameMetadataText;
use crate::game::tree::GameTree;
use crate::notation::game::{
    NotationGame, NotationGameClock, NotationGameEnumWarning, NotationGameParts,
};
use crate::notation::game_identity::NotationGameIdentityEnumResult;
use crate::storage::game_document::StorageGameDocumentError;
use crate::storage::game_document::records::{
    StorageGameDocumentAnnotations, StorageGameDocumentDuration, StorageGameDocumentMetadata,
    StorageGameDocumentMoves, StorageGameDocumentText,
};
use crate::storage::library_record::{StorageLibraryEnumContent, StorageLibraryRecord};

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StorageGameDocumentEnumNotationResult {
    Unfinished,
    Completed(GameEnumResult),
}

impl From<NotationGameIdentityEnumResult> for StorageGameDocumentEnumNotationResult {
    fn from(result: NotationGameIdentityEnumResult) -> Self {
        match result {
            NotationGameIdentityEnumResult::Unfinished => Self::Unfinished,
            NotationGameIdentityEnumResult::Completed(result) => Self::Completed(result),
        }
    }
}

impl From<StorageGameDocumentEnumNotationResult> for NotationGameIdentityEnumResult {
    fn from(result: StorageGameDocumentEnumNotationResult) -> Self {
        match result {
            StorageGameDocumentEnumNotationResult::Unfinished => Self::Unfinished,
            StorageGameDocumentEnumNotationResult::Completed(result) => Self::Completed(result),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageGameDocumentNotationClock {
    node: GameMoveId,
    remaining_nanos: StorageGameDocumentDuration,
}

impl From<NotationGameClock> for StorageGameDocumentNotationClock {
    fn from(clock: NotationGameClock) -> Self {
        Self {
            node: clock.node(),
            remaining_nanos: clock.remaining().into(),
        }
    }
}

impl From<StorageGameDocumentNotationClock> for NotationGameClock {
    fn from(clock: StorageGameDocumentNotationClock) -> Self {
        Self::new(clock.node, clock.remaining_nanos.into())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StorageGameDocumentEnumNotationWarning {
    UnknownDirective {
        node: GameMoveId,
        directive: StorageGameDocumentText,
    },
    UnavailableRating {
        color: BoardEnumColor,
        value: StorageGameDocumentText,
    },
}

impl StorageGameDocumentEnumNotationWarning {
    fn capture(warning: &NotationGameEnumWarning) -> Result<Self, StorageGameDocumentError> {
        Ok(match warning {
            NotationGameEnumWarning::UnknownDirective { node, directive } => {
                Self::UnknownDirective {
                    node: *node,
                    directive: StorageGameDocumentText::copy(directive)?,
                }
            }
            NotationGameEnumWarning::UnavailableRating { color, value } => {
                Self::UnavailableRating {
                    color: *color,
                    value: StorageGameDocumentText::copy(value.as_str())?,
                }
            }
        })
    }

    fn restore(self) -> Result<NotationGameEnumWarning, StorageGameDocumentError> {
        Ok(match self {
            Self::UnknownDirective { node, directive } => {
                NotationGameEnumWarning::UnknownDirective {
                    node,
                    directive: directive.0,
                }
            }
            Self::UnavailableRating { color, value } => {
                NotationGameEnumWarning::UnavailableRating {
                    color,
                    value: GameMetadataText::new(value.0)?,
                }
            }
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StorageGameDocumentNotation {
    id: GameId,
    root: GameMoveId,
    start: BoardStart,
    root_annotations: StorageGameDocumentAnnotations,
    moves: StorageGameDocumentMoves,
    mainline_tip: GameMoveId,
    metadata: StorageGameDocumentMetadata,
    result: StorageGameDocumentEnumNotationResult,
    result_header: Option<StorageGameDocumentEnumNotationResult>,
    raw_time_control: Option<StorageGameDocumentText>,
    clocks: Vec<StorageGameDocumentNotationClock>,
    warnings: Vec<StorageGameDocumentEnumNotationWarning>,
}

impl StorageGameDocumentNotation {
    pub(super) fn capture(
        id: GameId,
        game: &NotationGame,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<Self, StorageGameDocumentError> {
        StorageGameDocumentError::check_cancellation(cancellation)?;
        let root = game.tree().node(game.tree().root())?;
        if root.clock().is_some() || root.before_clock().is_some() {
            return Err(StorageGameDocumentError::NotationRuntimeClock);
        }
        let moves = StorageGameDocumentMoves::capture(game.tree(), None, cancellation)?;
        let mut warnings = Vec::with_capacity(game.warnings().len());
        for warning in game.warnings() {
            StorageGameDocumentError::check_cancellation(cancellation)?;
            warnings.push(StorageGameDocumentEnumNotationWarning::capture(warning)?);
        }
        Ok(Self {
            id,
            root: root.id(),
            start: game.start().clone(),
            root_annotations: StorageGameDocumentAnnotations::capture(root.annotations())?,
            moves,
            mainline_tip: game.mainline_tip(),
            metadata: StorageGameDocumentMetadata::capture(game.metadata())?,
            result: game.result().into(),
            result_header: game.result_header().map(Into::into),
            raw_time_control: game
                .time_control()
                .map(|text| StorageGameDocumentText::copy(text.as_str()))
                .transpose()?,
            clocks: game
                .clocks()
                .iter()
                .copied()
                .map(StorageGameDocumentNotationClock::from)
                .collect(),
            warnings,
        })
    }

    pub(super) fn restore(
        self,
        cancellation: Option<&CapacityExecutorCancellation>,
    ) -> Result<StorageLibraryRecord, StorageGameDocumentError> {
        StorageGameDocumentError::check_cancellation(cancellation)?;
        let mut tree = GameTree::new(self.root, self.start.position().clone(), None);
        tree.set_annotations(self.root, self.root_annotations.restore()?)?;
        let tree = self.moves.restore(tree, None, cancellation)?;
        let mut warnings = Vec::with_capacity(self.warnings.len());
        for warning in self.warnings {
            StorageGameDocumentError::check_cancellation(cancellation)?;
            warnings.push(warning.restore()?);
        }
        let game = NotationGame::new(NotationGameParts {
            start: self.start,
            tree,
            mainline_tip: self.mainline_tip,
            metadata: self.metadata.restore()?,
            result: self.result.into(),
            result_header: self.result_header.map(Into::into),
            time_control: self
                .raw_time_control
                .map(|text| GameMetadataText::new(text.0))
                .transpose()?,
            clocks: self
                .clocks
                .into_iter()
                .map(NotationGameClock::from)
                .collect(),
            warnings,
        })?;
        Ok(StorageLibraryRecord::new(
            self.id,
            StorageLibraryEnumContent::Notation(Box::new(game)),
        )?)
    }
}
