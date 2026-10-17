use std::sync::Arc;

use crate::desktop::preferences::{
    DesktopPreferences, DesktopPreferencesChord, DesktopPreferencesDraft,
    DesktopPreferencesEnumMotion, DesktopPreferencesEnumPatch,
    DesktopPreferencesEnumShortcutAction, DesktopPreferencesEnumTheme,
};
use crate::desktop::preferences_import::{
    DesktopPreferencesCodec, DesktopPreferencesImportPreview,
};
use crate::desktop::preferences_reset::{
    DesktopPreferencesEnumResetScope, DesktopPreferencesResetPreview,
};
use crate::desktop::preferences_store::{DesktopPreferencesLoad, DesktopPreferencesStore};
use crate::desktop::update_commands::{DesktopUpdateOperation, DesktopUpdateSnapshot};
use crate::presentation::action::shortcuts::PresentationShortcutEnumPlatform;
use crate::presentation::settings::shortcuts::PresentationSettingsShortcuts;

pub struct PresentationSettingsConnection {
    pub store: Arc<DesktopPreferencesStore>,
    pub loaded: DesktopPreferencesLoad,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentationSettingsEnumPrompt {
    None,
    Leave,
    Reset(DesktopPreferencesResetPreview),
    Import(DesktopPreferencesImportPreview),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresentationSettingsOutcome {
    pub appearance_changed: bool,
    pub close: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PresentationSettingsShortcutEdit {
    pub command: bool,
    pub key: String,
}

pub struct PresentationSettingsController {
    draft: DesktopPreferencesDraft,
    effective: DesktopPreferences,
    store: Option<Arc<DesktopPreferencesStore>>,
    saving: bool,
    prompt: PresentationSettingsEnumPrompt,
    notice: Option<String>,
    exchange: String,
    shortcut_edits: [PresentationSettingsShortcutEdit; 4],
    update: Option<DesktopUpdateSnapshot>,
    update_intent: Option<DesktopUpdateOperation>,
    update_credential: String,
    update_persist: bool,
    update_install_prompt: bool,
    update_install_confirmed: bool,
    update_install_requested: bool,
    update_frames: u32,
}

impl PresentationSettingsController {
    pub fn new(connection: Option<PresentationSettingsConnection>) -> Self {
        let (store, loaded) = match connection {
            Some(connection) => (Some(connection.store), connection.loaded),
            None => (None, DesktopPreferencesLoad::defaults()),
        };
        let draft = DesktopPreferencesDraft::new(loaded.preferences(), loaded.revision());
        Self {
            effective: loaded.preferences(),
            notice: loaded.warning().map(str::to_owned),
            draft,
            store,
            saving: false,
            prompt: PresentationSettingsEnumPrompt::None,
            exchange: String::new(),
            shortcut_edits: Default::default(),
            update: None,
            update_intent: None,
            update_credential: String::new(),
            update_persist: true,
            update_install_prompt: false,
            update_install_confirmed: false,
            update_install_requested: false,
            update_frames: 0,
        }
    }

    pub const fn effective(&self) -> &DesktopPreferences {
        &self.effective
    }

    pub const fn draft(&self) -> &DesktopPreferencesDraft {
        &self.draft
    }

    pub const fn prompt(&self) -> &PresentationSettingsEnumPrompt {
        &self.prompt
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub const fn is_saving(&self) -> bool {
        self.saving
    }

    pub fn can_save(&self) -> bool {
        self.store.is_some() && !self.saving && self.draft.status().has_pending_changes()
    }

    pub const fn exchange_mut(&mut self) -> &mut String {
        &mut self.exchange
    }

    pub const fn shortcut_edit_mut(
        &mut self,
        action: DesktopPreferencesEnumShortcutAction,
    ) -> &mut PresentationSettingsShortcutEdit {
        &mut self.shortcut_edits[action.slot()]
    }

    pub fn accept_update(&mut self, snapshot: DesktopUpdateSnapshot) {
        self.update = Some(snapshot);
    }

    pub const fn update(&self) -> Option<&DesktopUpdateSnapshot> {
        self.update.as_ref()
    }

    pub fn request_update(&mut self, operation: DesktopUpdateOperation) {
        self.update_intent = Some(operation);
    }

    pub fn request_install(&mut self) {
        self.update_install_requested = true;
    }

    pub fn take_install_request(&mut self) -> bool {
        std::mem::take(&mut self.update_install_requested)
    }

    pub fn take_update_intent(&mut self) -> Option<DesktopUpdateOperation> {
        self.update_intent.take()
    }

    pub fn update_poll_due(&mut self) -> bool {
        self.update_frames = self.update_frames.wrapping_add(1);
        (self.update.is_none() && self.update_intent.is_none())
            || self.update_frames.is_multiple_of(30)
    }

    pub const fn update_credential_mut(&mut self) -> &mut String {
        &mut self.update_credential
    }

    pub const fn update_persist_mut(&mut self) -> &mut bool {
        &mut self.update_persist
    }

    pub const fn update_install_prompt(&self) -> bool {
        self.update_install_prompt
    }

    pub fn set_update_install_prompt(&mut self, open: bool) {
        self.update_install_prompt = open;
        self.update_install_confirmed = false;
    }

    pub const fn update_install_confirmed_mut(&mut self) -> &mut bool {
        &mut self.update_install_confirmed
    }

    pub fn submit_update_credential(&mut self, persist: bool) {
        let text = std::mem::take(&mut self.update_credential);
        match crate::update::credentials::UpdateCredential::new(text) {
            Ok(credential) => {
                self.update_intent = Some(DesktopUpdateOperation::EnterCredential {
                    credential,
                    persist,
                });
            }
            Err(error) => self.notice = Some(error.to_string()),
        }
    }

    pub fn engine_note(&self) -> Option<&'static str> {
        let pending = self.draft.pending();
        let saved = self.draft.saved();
        let differs = pending.engine_threads() != saved.engine_threads()
            || pending.engine_hash_mib() != saved.engine_hash_mib()
            || pending.engine_move_overhead_ms() != saved.engine_move_overhead_ms();
        differs.then_some(
            "Engine changes apply after Save, at the next safe boundary. Running games, runs and reports keep their configuration.",
        )
    }

    fn refresh_effective(&mut self) -> bool {
        let pending = self.draft.pending();
        let mut effective = self.draft.saved();
        effective.apply(DesktopPreferencesEnumPatch::Theme(pending.theme()));
        effective.apply(DesktopPreferencesEnumPatch::Motion(pending.motion()));
        effective.apply(DesktopPreferencesEnumPatch::Notation(pending.notation()));
        let changed = effective != self.effective;
        self.effective = effective;
        changed
    }

    pub fn edit(&mut self, patch: DesktopPreferencesEnumPatch) -> PresentationSettingsOutcome {
        self.draft.edit(patch);
        PresentationSettingsOutcome {
            appearance_changed: self.refresh_effective(),
            close: false,
        }
    }

    pub fn theme(&self) -> DesktopPreferencesEnumTheme {
        self.draft.pending().theme()
    }

    pub fn motion(&self) -> DesktopPreferencesEnumMotion {
        self.draft.pending().motion()
    }

    pub fn save(&mut self) {
        let Some(store) = &self.store else {
            self.notice = Some("Saving settings is unavailable".to_owned());
            return;
        };
        if self.saving {
            return;
        }
        store.save(self.draft.status().saved_revision(), self.draft.pending());
        self.saving = true;
        self.notice = None;
    }

    pub fn poll(&mut self) -> PresentationSettingsOutcome {
        let mut outcome = PresentationSettingsOutcome::default();
        let Some(store) = &self.store else {
            return outcome;
        };
        if !self.saving {
            return outcome;
        }
        let Some(result) = store.take_outcome() else {
            return outcome;
        };
        self.saving = false;
        match result {
            Ok(revision) => match self.draft.commit(revision) {
                Ok(()) => {
                    self.notice = Some("Settings saved".to_owned());
                    outcome.appearance_changed = self.refresh_effective();
                    if self.prompt == PresentationSettingsEnumPrompt::Leave {
                        self.prompt = PresentationSettingsEnumPrompt::None;
                        outcome.close = true;
                    }
                }
                Err(error) => self.notice = Some(error.to_string()),
            },
            Err(error) => {
                self.notice = Some(format!("Settings were not saved: {error}"));
                if self.prompt == PresentationSettingsEnumPrompt::Leave {
                    self.prompt = PresentationSettingsEnumPrompt::None;
                }
            }
        }
        outcome
    }

    pub fn discard(&mut self) -> PresentationSettingsOutcome {
        self.draft.discard();
        PresentationSettingsOutcome {
            appearance_changed: self.refresh_effective(),
            close: false,
        }
    }

    pub fn request_close(&mut self) -> bool {
        if self.draft.status().has_pending_changes() {
            self.prompt = PresentationSettingsEnumPrompt::Leave;
            false
        } else {
            true
        }
    }

    pub fn leave_discarding(&mut self) -> PresentationSettingsOutcome {
        let mut outcome = self.discard();
        self.prompt = PresentationSettingsEnumPrompt::None;
        outcome.close = true;
        outcome
    }

    pub fn leave_saving(&mut self) {
        self.save();
    }

    pub fn stay(&mut self) {
        self.prompt = PresentationSettingsEnumPrompt::None;
    }

    pub fn begin_reset(&mut self, scope: DesktopPreferencesEnumResetScope) {
        let preview = DesktopPreferencesResetPreview::new(self.draft.pending(), scope);
        if preview.changes().is_empty() {
            self.notice = Some("Nothing to reset".to_owned());
        } else {
            self.prompt = PresentationSettingsEnumPrompt::Reset(preview);
        }
    }

    pub fn export(&mut self) {
        match DesktopPreferencesCodec::encode(self.draft.pending()) {
            Ok(text) => self.exchange = text,
            Err(error) => self.notice = Some(error.to_string()),
        }
    }

    pub fn begin_import(&mut self) {
        match DesktopPreferencesImportPreview::new(self.draft.pending(), self.exchange.as_bytes()) {
            Ok(preview) if preview.is_empty() => {
                self.notice = Some("The document changes nothing".to_owned());
            }
            Ok(preview) => self.prompt = PresentationSettingsEnumPrompt::Import(preview),
            Err(error) => self.notice = Some(format!("The settings document was refused: {error}")),
        }
    }

    pub fn confirm_prompt(&mut self) -> PresentationSettingsOutcome {
        let prompt = std::mem::replace(&mut self.prompt, PresentationSettingsEnumPrompt::None);
        match prompt {
            PresentationSettingsEnumPrompt::Reset(preview) => {
                if preview.confirm().apply(&mut self.draft).is_none() {
                    self.notice = Some("The settings changed after the preview".to_owned());
                }
            }
            PresentationSettingsEnumPrompt::Import(preview) => {
                if let Err(error) = preview.confirm().apply(&mut self.draft) {
                    self.notice = Some(error.to_string());
                }
            }
            PresentationSettingsEnumPrompt::Leave | PresentationSettingsEnumPrompt::None => {}
        }
        PresentationSettingsOutcome {
            appearance_changed: self.refresh_effective(),
            close: false,
        }
    }

    pub fn apply_shortcut(&mut self, action: DesktopPreferencesEnumShortcutAction) {
        let edit = self.shortcut_edits[action.slot()].clone();
        let Some(key) = edit.key.chars().next() else {
            self.notice = Some("Enter a key".to_owned());
            return;
        };
        let chord = match DesktopPreferencesChord::new(edit.command, false, false, key) {
            Ok(chord) => chord,
            Err(error) => {
                self.notice = Some(error.to_string());
                return;
            }
        };
        match PresentationSettingsShortcuts::assign(
            &mut self.draft,
            PresentationShortcutEnumPlatform::current(),
            action,
            chord,
        ) {
            Ok(_) => self.notice = None,
            Err(error) => self.notice = Some(error.to_string()),
        }
    }

    pub fn reset_shortcut(&mut self, action: DesktopPreferencesEnumShortcutAction) {
        PresentationSettingsShortcuts::reset(&mut self.draft, action);
    }
}
