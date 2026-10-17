use crate::clock::duration::ClockDuration;
use crate::desktop::dto::DesktopBoardSnapshot;
use crate::desktop::game_editor::{
    DesktopGameEditor, DesktopGameEditorContext, DesktopGameEditorEnumInput, DesktopGameEditorError,
};
use crate::game::enums::{GameEnumPhase, GameMoveId};
use crate::game::move_node::GameMoveNodeAnnotations;
use crate::game::outcome::GameOutcome;
use crate::game::session::{GameMetadata, GameMetadataText};
use crate::notation::game_identity::NotationGameIdentityEnumResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopGameEditorSnapshotEnumState {
    Played {
        live: GameMoveId,
        phase: GameEnumPhase,
        outcome: Option<GameOutcome>,
    },
    Notation {
        result: NotationGameIdentityEnumResult,
        result_header: Option<NotationGameIdentityEnumResult>,
        time_control: Option<GameMetadataText>,
        selected_clock_annotation: Option<ClockDuration>,
    },
}

#[derive(Debug, Clone)]
pub struct DesktopGameEditorSnapshot {
    context: DesktopGameEditorContext,
    root: GameMoveId,
    mainline_tip: GameMoveId,
    board: DesktopBoardSnapshot,
    annotations: GameMoveNodeAnnotations,
    metadata: GameMetadata,
    state: DesktopGameEditorSnapshotEnumState,
}

impl DesktopGameEditorSnapshot {
    pub(super) fn capture(
        editor: &DesktopGameEditor<'_>,
        expected: DesktopGameEditorContext,
    ) -> Result<Self, DesktopGameEditorError> {
        editor.context.check_expected(expected)?;
        let tree = editor.tree();
        let selected = tree.node(editor.context.selected())?;
        let (mainline_tip, state) = match editor.input {
            DesktopGameEditorEnumInput::Played(session) => (
                session.study_mainline_tip(),
                DesktopGameEditorSnapshotEnumState::Played {
                    live: session.live(),
                    phase: session.phase(),
                    outcome: session.outcome(),
                },
            ),
            DesktopGameEditorEnumInput::Notation(game) => (
                game.mainline_tip(),
                DesktopGameEditorSnapshotEnumState::Notation {
                    result: game.result(),
                    result_header: game.result_header(),
                    time_control: game.time_control().cloned(),
                    selected_clock_annotation: game.clock(selected.id()),
                },
            ),
        };
        Ok(Self {
            context: editor.context,
            root: tree.root(),
            mainline_tip,
            board: DesktopBoardSnapshot::from_position(selected.position()),
            annotations: selected.annotations().clone(),
            metadata: editor.input.metadata().clone(),
            state,
        })
    }

    pub const fn context(&self) -> DesktopGameEditorContext {
        self.context
    }

    pub const fn root(&self) -> GameMoveId {
        self.root
    }

    pub const fn mainline_tip(&self) -> GameMoveId {
        self.mainline_tip
    }

    pub const fn board(&self) -> &DesktopBoardSnapshot {
        &self.board
    }

    pub const fn annotations(&self) -> &GameMoveNodeAnnotations {
        &self.annotations
    }

    pub const fn metadata(&self) -> &GameMetadata {
        &self.metadata
    }

    pub const fn state(&self) -> &DesktopGameEditorSnapshotEnumState {
        &self.state
    }
}
