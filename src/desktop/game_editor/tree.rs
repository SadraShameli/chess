use crate::desktop::game_editor::{
    DesktopGameEditor, DesktopGameEditorContext, DesktopGameEditorError,
};
use crate::game::enums::GameMoveId;
use crate::game::move_node::GameMoveNode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopGameEditorTreeCursor {
    context: DesktopGameEditorContext,
    next_index: usize,
    next_node: GameMoveId,
}

impl DesktopGameEditorTreeCursor {
    fn start_index(self, editor: &DesktopGameEditor<'_>) -> Result<usize, DesktopGameEditorError> {
        editor.context.check_expected(self.context)?;
        let next = editor.tree().nodes().nth(self.next_index);
        if next.map(GameMoveNode::id) != Some(self.next_node) {
            return Err(DesktopGameEditorError::InvalidCursor);
        }
        Ok(self.next_index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopGameEditorTreeNode {
    id: GameMoveId,
    parent: Option<GameMoveId>,
    san: Option<String>,
    child_count: usize,
    first_child: Option<GameMoveId>,
    has_notes: bool,
    has_nags: bool,
    has_markings: bool,
}

impl DesktopGameEditorTreeNode {
    fn capture(node: &GameMoveNode) -> Self {
        let annotations = node.annotations();
        Self {
            id: node.id(),
            parent: node.parent(),
            san: node.san().map(str::to_owned),
            child_count: node.children().len(),
            first_child: node.children().first().copied(),
            has_notes: !annotations.notes().is_empty(),
            has_nags: !annotations.nags().is_empty(),
            has_markings: !annotations.markings().is_empty(),
        }
    }

    pub const fn id(&self) -> GameMoveId {
        self.id
    }

    pub const fn parent(&self) -> Option<GameMoveId> {
        self.parent
    }

    pub fn san(&self) -> Option<&str> {
        self.san.as_deref()
    }

    pub const fn child_count(&self) -> usize {
        self.child_count
    }

    pub const fn first_child(&self) -> Option<GameMoveId> {
        self.first_child
    }

    pub const fn has_notes(&self) -> bool {
        self.has_notes
    }

    pub const fn has_nags(&self) -> bool {
        self.has_nags
    }

    pub const fn has_markings(&self) -> bool {
        self.has_markings
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopGameEditorTreePage {
    context: DesktopGameEditorContext,
    nodes: Vec<DesktopGameEditorTreeNode>,
    next: Option<DesktopGameEditorTreeCursor>,
}

impl DesktopGameEditorTreePage {
    pub const MAX_ROWS: usize = 100;

    pub(super) fn capture(
        editor: &DesktopGameEditor<'_>,
        expected: DesktopGameEditorContext,
        cursor: Option<DesktopGameEditorTreeCursor>,
    ) -> Result<Self, DesktopGameEditorError> {
        editor.context.check_expected(expected)?;
        let start = match cursor {
            Some(cursor) => cursor.start_index(editor)?,
            None => 0,
        };
        let mut remaining = editor.tree().nodes().skip(start);
        let nodes: Vec<_> = remaining
            .by_ref()
            .take(Self::MAX_ROWS)
            .map(DesktopGameEditorTreeNode::capture)
            .collect();
        let next_index = Self::next_index(start, nodes.len())?;
        let next = remaining.next().map(|node| DesktopGameEditorTreeCursor {
            context: editor.context,
            next_index,
            next_node: node.id(),
        });
        Ok(Self {
            context: editor.context,
            nodes,
            next,
        })
    }

    fn next_index(start: usize, count: usize) -> Result<usize, DesktopGameEditorError> {
        start
            .checked_add(count)
            .ok_or(DesktopGameEditorError::PagePositionOverflow)
    }

    pub const fn context(&self) -> DesktopGameEditorContext {
        self.context
    }

    pub fn nodes(&self) -> &[DesktopGameEditorTreeNode] {
        &self.nodes
    }

    pub const fn next_cursor(&self) -> Option<DesktopGameEditorTreeCursor> {
        self.next
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopGameEditorTreeChildrenCursor {
    context: DesktopGameEditorContext,
    parent: GameMoveId,
    next_index: usize,
    next_node: GameMoveId,
}

impl DesktopGameEditorTreeChildrenCursor {
    fn start_index(
        self,
        editor: &DesktopGameEditor<'_>,
        parent: GameMoveId,
        children: &[GameMoveId],
    ) -> Result<usize, DesktopGameEditorError> {
        editor.context.check_expected(self.context)?;
        if self.parent != parent || children.get(self.next_index) != Some(&self.next_node) {
            return Err(DesktopGameEditorError::InvalidCursor);
        }
        Ok(self.next_index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopGameEditorTreeChildrenPage {
    context: DesktopGameEditorContext,
    parent: GameMoveId,
    nodes: Vec<DesktopGameEditorTreeNode>,
    next: Option<DesktopGameEditorTreeChildrenCursor>,
}

impl DesktopGameEditorTreeChildrenPage {
    pub(super) fn capture(
        editor: &DesktopGameEditor<'_>,
        expected: DesktopGameEditorContext,
        parent: GameMoveId,
        cursor: Option<DesktopGameEditorTreeChildrenCursor>,
    ) -> Result<Self, DesktopGameEditorError> {
        editor.context.check_expected(expected)?;
        let tree = editor.tree();
        let children = tree.node(parent)?.children();
        let start = match cursor {
            Some(cursor) => cursor.start_index(editor, parent, children)?,
            None => 0,
        };
        let nodes = children
            .iter()
            .skip(start)
            .take(DesktopGameEditorTreePage::MAX_ROWS)
            .map(|id| tree.node(*id).map(DesktopGameEditorTreeNode::capture))
            .collect::<Result<Vec<_>, _>>()?;
        let next_index = DesktopGameEditorTreePage::next_index(start, nodes.len())?;
        let next = children
            .get(next_index)
            .map(|&id| DesktopGameEditorTreeChildrenCursor {
                context: editor.context,
                parent,
                next_index,
                next_node: id,
            });
        Ok(Self {
            context: editor.context,
            parent,
            nodes,
            next,
        })
    }

    pub const fn context(&self) -> DesktopGameEditorContext {
        self.context
    }

    pub const fn parent(&self) -> GameMoveId {
        self.parent
    }

    pub fn nodes(&self) -> &[DesktopGameEditorTreeNode] {
        &self.nodes
    }

    pub const fn next_cursor(&self) -> Option<DesktopGameEditorTreeChildrenCursor> {
        self.next
    }
}
