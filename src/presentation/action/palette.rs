use crate::presentation::action::registry::{
    PresentationActionContext, PresentationActionEnumAvailability, PresentationActionEnumWorkspace,
    PresentationActionRegistry,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationActionPaletteEntry {
    action: PresentationActionEnumWorkspace,
    availability: PresentationActionEnumAvailability,
}

impl PresentationActionPaletteEntry {
    pub const fn action(self) -> PresentationActionEnumWorkspace {
        self.action
    }

    pub const fn availability(self) -> PresentationActionEnumAvailability {
        self.availability
    }

    pub const fn requires_confirmation(self) -> bool {
        self.action.requires_confirmation()
    }
}

pub struct PresentationActionPalette;

impl PresentationActionPalette {
    pub fn entries(
        query: &str,
        context: PresentationActionContext,
    ) -> Vec<PresentationActionPaletteEntry> {
        let needle = query.trim().to_lowercase();
        PresentationActionRegistry::workspace_actions()
            .iter()
            .copied()
            .filter(|action| action.label().to_lowercase().contains(&needle))
            .map(|action| PresentationActionPaletteEntry {
                action,
                availability: action.availability(context),
            })
            .collect()
    }

    pub fn activate(
        action: PresentationActionEnumWorkspace,
        context: PresentationActionContext,
    ) -> Result<PresentationActionPaletteEntry, &'static str> {
        match action.availability(context) {
            PresentationActionEnumAvailability::Available => Ok(PresentationActionPaletteEntry {
                action,
                availability: PresentationActionEnumAvailability::Available,
            }),
            PresentationActionEnumAvailability::Unavailable(reason) => Err(reason),
        }
    }
}
