use crate::desktop::preferences::{
    DesktopPreferences, DesktopPreferencesDraft, DesktopPreferencesEnumPatch,
    DesktopPreferencesEnumSection, DesktopPreferencesStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopPreferencesEnumResetScope {
    Section(DesktopPreferencesEnumSection),
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPreferencesResetPreview {
    base: DesktopPreferences,
    changes: Vec<DesktopPreferencesEnumPatch>,
}

impl DesktopPreferencesResetPreview {
    pub fn new(current: DesktopPreferences, scope: DesktopPreferencesEnumResetScope) -> Self {
        let defaults = DesktopPreferences::default();
        let candidates = match scope {
            DesktopPreferencesEnumResetScope::Section(section) => defaults.section_patches(section),
            DesktopPreferencesEnumResetScope::All => defaults.patches(),
        };
        let mut scratch = current;
        let changes = candidates
            .into_iter()
            .filter(|patch| scratch.apply(*patch))
            .collect();
        Self {
            base: current,
            changes,
        }
    }

    pub fn changes(&self) -> &[DesktopPreferencesEnumPatch] {
        &self.changes
    }

    pub fn confirm(self) -> DesktopPreferencesConfirmedReset {
        DesktopPreferencesConfirmedReset(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPreferencesConfirmedReset(DesktopPreferencesResetPreview);

impl DesktopPreferencesConfirmedReset {
    pub fn apply(self, draft: &mut DesktopPreferencesDraft) -> Option<DesktopPreferencesStatus> {
        if draft.pending() != self.0.base {
            return None;
        }
        for patch in self.0.changes {
            draft.edit(patch);
        }
        Some(draft.status())
    }
}
