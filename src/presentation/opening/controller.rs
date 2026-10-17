use crate::board::r#move::BoardMove;
use crate::desktop::opening_commands::DesktopOpeningExplorerView;
use crate::opening::book::OpeningBookStudy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationOpeningEnumTab {
    Explorer,
    Book,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationOpeningEnumOrigin {
    Explorer,
    Book,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationOpeningIntent {
    FollowContinuation {
        origin: PresentationOpeningEnumOrigin,
        movement: BoardMove,
    },
    SamplePage(usize),
    OpenSample(u64),
}

pub struct PresentationOpeningController {
    tab: PresentationOpeningEnumTab,
    explorer: Option<DesktopOpeningExplorerView>,
    book: Option<OpeningBookStudy>,
}

impl Default for PresentationOpeningController {
    fn default() -> Self {
        Self::new()
    }
}

impl PresentationOpeningController {
    pub const fn new() -> Self {
        Self {
            tab: PresentationOpeningEnumTab::Explorer,
            explorer: None,
            book: None,
        }
    }

    pub const fn tab(&self) -> PresentationOpeningEnumTab {
        self.tab
    }

    pub const fn select_tab(&mut self, tab: PresentationOpeningEnumTab) {
        self.tab = tab;
    }

    pub fn observe_explorer(&mut self, view: DesktopOpeningExplorerView) {
        self.explorer = Some(view);
    }

    pub fn observe_book(&mut self, study: OpeningBookStudy) {
        self.book = Some(study);
    }

    pub const fn explorer(&self) -> Option<&DesktopOpeningExplorerView> {
        self.explorer.as_ref()
    }

    pub const fn book(&self) -> Option<&OpeningBookStudy> {
        self.book.as_ref()
    }
}
