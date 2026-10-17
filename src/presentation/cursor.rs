use crate::game::enums::GameMoveId;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PresentationStudyCursor {
    selected: Option<GameMoveId>,
}

impl PresentationStudyCursor {
    pub const fn new() -> Self {
        Self { selected: None }
    }

    pub const fn selected(self) -> Option<GameMoveId> {
        self.selected
    }

    pub const fn select(&mut self, node: GameMoveId) {
        self.selected = Some(node);
    }

    pub const fn clear(&mut self) {
        self.selected = None;
    }
}
