use bevy_egui::egui::{self, Ui};
use strum::IntoEnumIterator;

use crate::desktop::preferences::{
    DesktopPreferences, DesktopPreferencesEnumAid, DesktopPreferencesEnumAudioCategory,
    DesktopPreferencesEnumMotion, DesktopPreferencesEnumNotation, DesktopPreferencesEnumPatch,
    DesktopPreferencesEnumSection, DesktopPreferencesEnumShortcutAction,
    DesktopPreferencesEnumTheme, DesktopPreferencesPercent,
};
use crate::desktop::preferences_reset::DesktopPreferencesEnumResetScope;
use crate::desktop::update_commands::DesktopUpdateOperation;
use crate::presentation::action::shortcuts::{
    PresentationShortcutEnumPlatform, PresentationShortcuts,
};
use crate::presentation::settings::controller::{
    PresentationSettingsController, PresentationSettingsEnumPrompt, PresentationSettingsOutcome,
};
use crate::presentation::settings::update::{
    PresentationSettingsUpdate, PresentationSettingsUpdateEnumAction,
};
use crate::presentation::settings::update_install::PresentationSettingsUpdateInstall;
use crate::update::state::UpdateEnumPhase;

pub struct PresentationSettingsView;

struct PresentationSettingsEngineRow<'l> {
    label: &'l str,
    current: Option<u32>,
    minimum: u32,
    maximum: u32,
}

impl PresentationSettingsView {
    pub fn show(
        ui: &mut Ui,
        controller: &mut PresentationSettingsController,
    ) -> PresentationSettingsOutcome {
        let mut outcome = controller.poll();
        ui.heading("Settings");
        ui.label("Appearance applies immediately. Everything else applies after Save.");
        if let Some(notice) = controller.notice() {
            ui.label(notice.to_owned());
        }
        for section in DesktopPreferencesEnumSection::iter() {
            ui.separator();
            ui.horizontal(|ui| {
                ui.heading(section.label());
                if ui.button("Reset section").clicked() {
                    controller.begin_reset(DesktopPreferencesEnumResetScope::Section(section));
                }
            });
            let step = Self::section(ui, controller, section);
            outcome.appearance_changed |= step.appearance_changed;
        }
        ui.separator();
        Self::updates(ui, controller);
        ui.separator();
        Self::exchange(ui, controller);
        ui.separator();
        outcome.close |= Self::actions(ui, controller).close;
        outcome.close |= Self::prompt(ui, controller).close;
        outcome
    }

    fn section(
        ui: &mut Ui,
        controller: &mut PresentationSettingsController,
        section: DesktopPreferencesEnumSection,
    ) -> PresentationSettingsOutcome {
        let mut outcome = PresentationSettingsOutcome::default();
        let pending = controller.draft().pending();
        match section {
            DesktopPreferencesEnumSection::Appearance => {
                let mut theme = controller.theme();
                for choice in DesktopPreferencesEnumTheme::iter() {
                    ui.radio_value(&mut theme, choice, choice.label());
                }
                if theme != controller.theme() {
                    let step = controller.edit(DesktopPreferencesEnumPatch::Theme(theme));
                    outcome.appearance_changed |= step.appearance_changed;
                }
                let mut motion = controller.motion();
                for choice in DesktopPreferencesEnumMotion::iter() {
                    ui.radio_value(&mut motion, choice, choice.label());
                }
                if motion != controller.motion() {
                    let step = controller.edit(DesktopPreferencesEnumPatch::Motion(motion));
                    outcome.appearance_changed |= step.appearance_changed;
                }
                ui.label("Move notation");
                let mut notation = pending.notation();
                for choice in DesktopPreferencesEnumNotation::iter() {
                    ui.radio_value(&mut notation, choice, choice.label());
                }
                if notation != pending.notation() {
                    let step = controller.edit(DesktopPreferencesEnumPatch::Notation(notation));
                    outcome.appearance_changed |= step.appearance_changed;
                }
            }
            DesktopPreferencesEnumSection::BoardAids => {
                for aid in DesktopPreferencesEnumAid::iter() {
                    let mut enabled = pending.aid(aid);
                    if ui.checkbox(&mut enabled, aid.label()).changed() {
                        controller.edit(DesktopPreferencesEnumPatch::Aid(aid, enabled));
                    }
                }
            }
            DesktopPreferencesEnumSection::Audio => Self::audio(ui, controller, pending),
            DesktopPreferencesEnumSection::Shortcuts => Self::shortcuts(ui, controller, pending),
            DesktopPreferencesEnumSection::Engine => Self::engine(ui, controller, pending),
        }
        outcome
    }

    fn audio(
        ui: &mut Ui,
        controller: &mut PresentationSettingsController,
        pending: DesktopPreferences,
    ) {
        let mut master = pending.master_volume().value();
        if ui
            .add(
                egui::Slider::new(&mut master, 0..=DesktopPreferencesPercent::MAX)
                    .text("Master volume"),
            )
            .changed()
            && let Ok(volume) = DesktopPreferencesPercent::new(master)
        {
            controller.edit(DesktopPreferencesEnumPatch::MasterVolume(volume));
        }
        for category in DesktopPreferencesEnumAudioCategory::iter() {
            let entry = pending.category(category);
            let mut enabled = entry.enabled();
            if ui.checkbox(&mut enabled, category.label()).changed() {
                controller.edit(DesktopPreferencesEnumPatch::CategoryEnabled(
                    category, enabled,
                ));
            }
            let mut level = entry.volume().value();
            if ui
                .add(
                    egui::Slider::new(&mut level, 0..=DesktopPreferencesPercent::MAX)
                        .text(format!("{} volume", category.label())),
                )
                .changed()
                && let Ok(volume) = DesktopPreferencesPercent::new(level)
            {
                controller.edit(DesktopPreferencesEnumPatch::CategoryVolume(
                    category, volume,
                ));
            }
        }
    }

    fn shortcuts(
        ui: &mut Ui,
        controller: &mut PresentationSettingsController,
        pending: DesktopPreferences,
    ) {
        let platform = PresentationShortcutEnumPlatform::current();
        for action in DesktopPreferencesEnumShortcutAction::iter() {
            let label = PresentationShortcuts::label(
                platform,
                PresentationShortcuts::chord(pending, action),
            );
            ui.horizontal(|ui| {
                ui.label(format!("{}: {label}", action.label()));
                let edit = controller.shortcut_edit_mut(action);
                ui.checkbox(&mut edit.command, "Command");
                ui.add(egui::TextEdit::singleline(&mut edit.key).desired_width(24.0));
                if ui.button("Apply").clicked() {
                    controller.apply_shortcut(action);
                }
                if ui.button("Default").clicked() {
                    controller.reset_shortcut(action);
                }
            });
        }
    }

    fn engine(
        ui: &mut Ui,
        controller: &mut PresentationSettingsController,
        pending: DesktopPreferences,
    ) {
        Self::engine_row(
            ui,
            PresentationSettingsEngineRow {
                label: "Threads",
                current: pending.engine_threads(),
                minimum: 1,
                maximum: DesktopPreferencesEnumPatch::MAX_ENGINE_THREADS,
            },
            DesktopPreferencesEnumPatch::engine_threads,
            controller,
        );
        Self::engine_row(
            ui,
            PresentationSettingsEngineRow {
                label: "Hash (MiB)",
                current: pending.engine_hash_mib(),
                minimum: 1,
                maximum: DesktopPreferencesEnumPatch::MAX_ENGINE_HASH_MIB,
            },
            DesktopPreferencesEnumPatch::engine_hash,
            controller,
        );
        Self::engine_row(
            ui,
            PresentationSettingsEngineRow {
                label: "Move overhead (ms)",
                current: pending.engine_move_overhead_ms(),
                minimum: 0,
                maximum: DesktopPreferencesEnumPatch::MAX_ENGINE_OVERHEAD_MS,
            },
            DesktopPreferencesEnumPatch::engine_move_overhead,
            controller,
        );
        if let Some(note) = controller.engine_note() {
            ui.label(note);
        }
    }

    fn engine_row(
        ui: &mut Ui,
        row: PresentationSettingsEngineRow<'_>,
        patch: impl Fn(
            Option<u32>,
        ) -> Result<
            DesktopPreferencesEnumPatch,
            crate::desktop::preferences::DesktopPreferencesError,
        >,
        controller: &mut PresentationSettingsController,
    ) {
        let PresentationSettingsEngineRow {
            label,
            current,
            minimum,
            maximum,
        } = row;
        ui.horizontal(|ui| {
            let mut custom = current.is_some();
            let mut value = current.unwrap_or(minimum.max(1));
            let toggled = ui.checkbox(&mut custom, label).changed();
            let edited = custom
                && ui
                    .add(egui::DragValue::new(&mut value).range(minimum..=maximum))
                    .changed();
            if toggled || edited {
                let requested = custom.then_some(value);
                match patch(requested) {
                    Ok(change) => {
                        controller.edit(change);
                    }
                    Err(error) => controller.reject(&error),
                }
            }
            if !custom {
                ui.label("Engine default");
            }
        });
    }

    fn updates(ui: &mut Ui, controller: &mut PresentationSettingsController) {
        ui.heading("Updates");
        if controller.update_poll_due() {
            controller.request_update(DesktopUpdateOperation::Snapshot);
        }
        let Some(snapshot) = controller.update().cloned() else {
            ui.label("Update status is loading.");
            return;
        };
        if let Some(reason) = snapshot.disabled_reason() {
            ui.label(format!(
                "Updates are unavailable: {reason}. Offline chess is unaffected."
            ));
            return;
        }
        if let Some(message) = snapshot.message() {
            ui.label(message.to_owned());
        }
        let chosen = PresentationSettingsUpdate::show(ui, snapshot.state(), snapshot.credential());
        match chosen {
            Some(PresentationSettingsUpdateEnumAction::Check) => {
                controller.request_update(DesktopUpdateOperation::Check);
            }
            Some(PresentationSettingsUpdateEnumAction::Approve) => {
                controller.request_update(DesktopUpdateOperation::Approve);
            }
            Some(PresentationSettingsUpdateEnumAction::Cancel) => {
                controller.request_update(DesktopUpdateOperation::Cancel);
            }
            Some(PresentationSettingsUpdateEnumAction::Download) => {
                controller.request_update(DesktopUpdateOperation::RequestDownload);
            }
            Some(PresentationSettingsUpdateEnumAction::Retry) => {
                controller.request_update(if snapshot.state().release().is_some() {
                    DesktopUpdateOperation::RequestDownload
                } else {
                    DesktopUpdateOperation::Check
                });
            }
            Some(PresentationSettingsUpdateEnumAction::Install) => {
                controller.set_update_install_prompt(true);
            }
            Some(
                PresentationSettingsUpdateEnumAction::EnterCredential
                | PresentationSettingsUpdateEnumAction::UseSessionCredential,
            )
            | None => {}
        }
        if matches!(
            snapshot.state().phase(),
            UpdateEnumPhase::CredentialsMissing | UpdateEnumPhase::CredentialsRejected
        ) {
            ui.add(egui::TextEdit::singleline(controller.update_credential_mut()).password(true));
            ui.checkbox(controller.update_persist_mut(), "Keep in secure storage");
            if ui.button("Save credential").clicked() {
                let persist = *controller.update_persist_mut();
                controller.submit_update_credential(persist);
            }
            if chosen == Some(PresentationSettingsUpdateEnumAction::UseSessionCredential) {
                controller.submit_update_credential(false);
            }
        }
        if controller.update_install_prompt() {
            let mut confirmed = *controller.update_install_confirmed_mut();
            let install = PresentationSettingsUpdateInstall::show_confirmation(ui, &mut confirmed);
            *controller.update_install_confirmed_mut() = confirmed;
            if install {
                controller.request_install();
                controller.set_update_install_prompt(false);
            }
        }
    }

    fn exchange(ui: &mut Ui, controller: &mut PresentationSettingsController) {
        ui.label("Export or import settings (no machine paths are included).");
        ui.text_edit_multiline(controller.exchange_mut());
        ui.horizontal(|ui| {
            if ui.button("Export").clicked() {
                controller.export();
            }
            if ui.button("Preview import").clicked() {
                controller.begin_import();
            }
        });
    }

    fn actions(
        ui: &mut Ui,
        controller: &mut PresentationSettingsController,
    ) -> PresentationSettingsOutcome {
        let mut outcome = PresentationSettingsOutcome::default();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(controller.can_save(), egui::Button::new("Save"))
                .clicked()
            {
                controller.save();
            }
            if ui
                .add_enabled(
                    controller.draft().status().has_pending_changes(),
                    egui::Button::new("Discard"),
                )
                .clicked()
            {
                let step = controller.discard();
                outcome.appearance_changed |= step.appearance_changed;
            }
            if ui.button("Reset all").clicked() {
                controller.begin_reset(DesktopPreferencesEnumResetScope::All);
            }
        });
        if controller.is_saving() {
            ui.label("Saving");
        }
        outcome
    }

    fn prompt(
        ui: &mut Ui,
        controller: &mut PresentationSettingsController,
    ) -> PresentationSettingsOutcome {
        let mut outcome = PresentationSettingsOutcome::default();
        match controller.prompt().clone() {
            PresentationSettingsEnumPrompt::None => {}
            PresentationSettingsEnumPrompt::Leave => {
                ui.separator();
                ui.label("Settings have unsaved changes.");
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        controller.leave_saving();
                    }
                    if ui.button("Discard").clicked() {
                        let step = controller.leave_discarding();
                        outcome.appearance_changed |= step.appearance_changed;
                        outcome.close = true;
                    }
                    if ui.button("Stay").clicked() {
                        controller.stay();
                    }
                });
            }
            PresentationSettingsEnumPrompt::Reset(preview) => {
                ui.separator();
                ui.label(format!(
                    "Reset {} setting(s) to defaults? Games, history and datasets are not changed.",
                    preview.changes().len()
                ));
                outcome.appearance_changed |= Self::confirm_buttons(ui, controller);
            }
            PresentationSettingsEnumPrompt::Import(preview) => {
                ui.separator();
                ui.label(format!(
                    "Import changes {} setting(s); other settings and destination locations stay.",
                    preview.changes().len()
                ));
                outcome.appearance_changed |= Self::confirm_buttons(ui, controller);
            }
        }
        outcome
    }

    fn confirm_buttons(ui: &mut Ui, controller: &mut PresentationSettingsController) -> bool {
        let mut appearance = false;
        ui.horizontal(|ui| {
            if ui.button("Confirm").clicked() {
                appearance = controller.confirm_prompt().appearance_changed;
            }
            if ui.button("Cancel").clicked() {
                controller.stay();
            }
        });
        appearance
    }
}
