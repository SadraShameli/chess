#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopFocusEnumWindow {
    Main,
    Puzzle,
}

impl DesktopFocusEnumWindow {
    const fn index(self) -> usize {
        match self {
            Self::Main => 0,
            Self::Puzzle => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopFocusEnumChange {
    AppGained,
    AppLost,
}

#[derive(Debug, Default)]
pub struct DesktopFocus {
    focused: [bool; 2],
}

impl DesktopFocus {
    pub fn app_focused(&self) -> bool {
        self.focused.iter().any(|focused| *focused)
    }

    pub fn puzzle_paused_by_focus(&self) -> bool {
        !self.focused[DesktopFocusEnumWindow::Puzzle.index()]
    }

    pub fn window_changed(
        &mut self,
        window: DesktopFocusEnumWindow,
        focused: bool,
    ) -> Option<DesktopFocusEnumChange> {
        let before = self.app_focused();
        self.focused[window.index()] = focused;
        match (before, self.app_focused()) {
            (false, true) => Some(DesktopFocusEnumChange::AppGained),
            (true, false) => Some(DesktopFocusEnumChange::AppLost),
            _ => None,
        }
    }
}
