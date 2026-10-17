use crate::game::enums::{GameId, GameMoveId, GameRevision};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameAnnotationsEnumChange {
    Unchanged,
    Changed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameAnnotationsEnumVariationRequest {
    Promote { tip: GameMoveId },
    Delete { root: GameMoveId },
}

#[derive(Debug, PartialEq, Eq)]
enum GameAnnotationsEnumVariationProof {
    Promote {
        tip: GameMoveId,
        path: Vec<GameMoveId>,
        children: Vec<Vec<GameMoveId>>,
        content_fingerprint: [u8; 32],
    },
    Delete {
        root: GameMoveId,
        parent: GameMoveId,
        siblings: Vec<GameMoveId>,
        nodes: Vec<GameMoveId>,
        replacement_tip: GameMoveId,
        content_fingerprint: [u8; 32],
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct GameAnnotationsVariationPreview {
    previous_tip: GameMoveId,
    previous_path: Vec<GameMoveId>,
    proof: GameAnnotationsEnumVariationProof,
}

impl GameAnnotationsVariationPreview {
    pub(crate) fn promotion(
        previous_tip: GameMoveId,
        previous_path: Vec<GameMoveId>,
        scope: GameAnnotationsPromotionScope,
    ) -> Self {
        Self {
            previous_tip,
            previous_path,
            proof: GameAnnotationsEnumVariationProof::Promote {
                tip: scope.tip,
                path: scope.path,
                children: scope.children,
                content_fingerprint: scope.content_fingerprint,
            },
        }
    }

    pub(crate) fn deletion(
        previous_tip: GameMoveId,
        previous_path: Vec<GameMoveId>,
        scope: GameAnnotationsDeletionScope,
    ) -> Self {
        Self {
            previous_tip,
            previous_path,
            proof: GameAnnotationsEnumVariationProof::Delete {
                root: scope.root,
                parent: scope.parent,
                siblings: scope.siblings,
                nodes: scope.nodes,
                replacement_tip: scope.replacement_tip,
                content_fingerprint: scope.content_fingerprint,
            },
        }
    }

    pub const fn request(&self) -> GameAnnotationsEnumVariationRequest {
        match &self.proof {
            GameAnnotationsEnumVariationProof::Promote { tip, .. } => {
                GameAnnotationsEnumVariationRequest::Promote { tip: *tip }
            }
            GameAnnotationsEnumVariationProof::Delete { root, .. } => {
                GameAnnotationsEnumVariationRequest::Delete { root: *root }
            }
        }
    }

    pub const fn previous_tip(&self) -> GameMoveId {
        self.previous_tip
    }

    pub fn previous_path(&self) -> &[GameMoveId] {
        &self.previous_path
    }

    pub fn selected_path(&self) -> Option<&[GameMoveId]> {
        match &self.proof {
            GameAnnotationsEnumVariationProof::Promote { path, .. } => Some(path),
            GameAnnotationsEnumVariationProof::Delete { .. } => None,
        }
    }

    pub fn deleted_nodes(&self) -> &[GameMoveId] {
        match &self.proof {
            GameAnnotationsEnumVariationProof::Promote { .. } => &[],
            GameAnnotationsEnumVariationProof::Delete { nodes, .. } => nodes,
        }
    }

    pub const fn parent(&self) -> Option<GameMoveId> {
        match &self.proof {
            GameAnnotationsEnumVariationProof::Promote { .. } => None,
            GameAnnotationsEnumVariationProof::Delete { parent, .. } => Some(*parent),
        }
    }

    pub const fn resulting_tip(&self) -> GameMoveId {
        match &self.proof {
            GameAnnotationsEnumVariationProof::Promote { tip, .. } => *tip,
            GameAnnotationsEnumVariationProof::Delete {
                replacement_tip, ..
            } => *replacement_tip,
        }
    }

    pub fn confirm(self) -> GameAnnotationsConfirmedVariation {
        GameAnnotationsConfirmedVariation { preview: self }
    }
}

pub(crate) struct GameAnnotationsPromotionScope {
    pub tip: GameMoveId,
    pub path: Vec<GameMoveId>,
    pub children: Vec<Vec<GameMoveId>>,
    pub content_fingerprint: [u8; 32],
}

pub(crate) struct GameAnnotationsDeletionScope {
    pub root: GameMoveId,
    pub parent: GameMoveId,
    pub siblings: Vec<GameMoveId>,
    pub nodes: Vec<GameMoveId>,
    pub replacement_tip: GameMoveId,
    pub content_fingerprint: [u8; 32],
}

#[derive(Debug)]
pub struct GameAnnotationsConfirmedVariation {
    preview: GameAnnotationsVariationPreview,
}

impl GameAnnotationsConfirmedVariation {
    pub(crate) fn preview(&self) -> &GameAnnotationsVariationPreview {
        &self.preview
    }
}

#[derive(Debug)]
pub struct GameAnnotationsSessionPreview {
    game: GameId,
    revision: GameRevision,
    variation: GameAnnotationsVariationPreview,
}

impl GameAnnotationsSessionPreview {
    pub(crate) fn new(
        game: GameId,
        revision: GameRevision,
        variation: GameAnnotationsVariationPreview,
    ) -> Self {
        Self {
            game,
            revision,
            variation,
        }
    }

    pub const fn game(&self) -> GameId {
        self.game
    }

    pub const fn revision(&self) -> GameRevision {
        self.revision
    }

    pub const fn variation(&self) -> &GameAnnotationsVariationPreview {
        &self.variation
    }

    pub fn confirm(self) -> GameAnnotationsSessionConfirmation {
        GameAnnotationsSessionConfirmation {
            game: self.game,
            revision: self.revision,
            variation: self.variation.confirm(),
        }
    }
}

#[derive(Debug)]
pub struct GameAnnotationsSessionConfirmation {
    game: GameId,
    revision: GameRevision,
    variation: GameAnnotationsConfirmedVariation,
}

impl GameAnnotationsSessionConfirmation {
    pub(crate) const fn game(&self) -> GameId {
        self.game
    }

    pub(crate) const fn revision(&self) -> GameRevision {
        self.revision
    }

    pub(crate) const fn variation(&self) -> &GameAnnotationsConfirmedVariation {
        &self.variation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameAnnotationsVariationApplied {
    change: GameAnnotationsEnumChange,
    mainline_tip: GameMoveId,
    view: GameMoveId,
}

impl GameAnnotationsVariationApplied {
    pub(crate) const fn new(
        change: GameAnnotationsEnumChange,
        mainline_tip: GameMoveId,
        view: GameMoveId,
    ) -> Self {
        Self {
            change,
            mainline_tip,
            view,
        }
    }

    pub const fn change(self) -> GameAnnotationsEnumChange {
        self.change
    }

    pub const fn mainline_tip(self) -> GameMoveId {
        self.mainline_tip
    }

    pub const fn view(self) -> GameMoveId {
        self.view
    }
}
