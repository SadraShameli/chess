use bevy_egui::egui::{self, Align, Layout, Margin, RichText, Ui};
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
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
    PresentationThemeEnumTone,
};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::presentation::theme::typography::PresentationThemeTypography;
use crate::update::state::UpdateEnumPhase;

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumIter, strum::IntoStaticStr)]
pub enum PresentationSettingsEnumPage {
    #[strum(serialize = "Appearance")]
    Appearance,
    #[strum(serialize = "Board aids")]
    BoardAids,
    #[strum(serialize = "Audio")]
    Audio,
    #[strum(serialize = "Shortcuts")]
    Shortcuts,
    #[strum(serialize = "Engine")]
    Engine,
    #[strum(serialize = "Updates")]
    Updates,
    #[strum(serialize = "Import and export")]
    Exchange,
    #[strum(serialize = "Backups and restore")]
    Backups,
    #[strum(serialize = "Study and data")]
    Study,
}

impl PresentationSettingsEnumPage {
    pub fn label(self) -> &'static str {
        self.into()
    }

    pub const fn of_section(section: DesktopPreferencesEnumSection) -> Self {
        match section {
            DesktopPreferencesEnumSection::Appearance => Self::Appearance,
            DesktopPreferencesEnumSection::BoardAids => Self::BoardAids,
            DesktopPreferencesEnumSection::Audio => Self::Audio,
            DesktopPreferencesEnumSection::Shortcuts => Self::Shortcuts,
            DesktopPreferencesEnumSection::Engine => Self::Engine,
        }
    }

    fn salt(self) -> egui::Id {
        egui::Id::new(("chess.settings.page", self.label()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresentationSettingsNavigation {
    current: Option<PresentationSettingsEnumPage>,
    target: Option<PresentationSettingsEnumPage>,
}

impl PresentationSettingsNavigation {
    pub const NAVIGATION_WIDTH: f32 = 232.0;
    pub const CONTENT_WIDTH: f32 = 760.0;

    pub fn show(&mut self, ui: &mut Ui, update_badge: Option<&str>) {
        ui.spacing_mut().item_spacing.y = 2.0;
        egui::Frame::new()
            .inner_margin(Margin {
                left: 6,
                right: 6,
                top: 0,
                bottom: 12,
            })
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Settings")
                        .size(PresentationThemeTypography::INPUT_POINTS)
                        .strong(),
                );
            });
        let current = self
            .current
            .unwrap_or(PresentationSettingsEnumPage::Appearance);
        for page in PresentationSettingsEnumPage::iter() {
            let meta = match page {
                PresentationSettingsEnumPage::Updates => update_badge,
                PresentationSettingsEnumPage::Appearance
                | PresentationSettingsEnumPage::BoardAids
                | PresentationSettingsEnumPage::Audio
                | PresentationSettingsEnumPage::Shortcuts
                | PresentationSettingsEnumPage::Engine
                | PresentationSettingsEnumPage::Exchange
                | PresentationSettingsEnumPage::Backups
                | PresentationSettingsEnumPage::Study => None,
            };
            if PresentationThemeComponents::list_row(ui, page.label(), meta, page == current)
                .clicked()
            {
                self.target = Some(page);
                self.current = Some(page);
            }
        }
    }

    pub fn heading(&mut self, ui: &mut Ui, page: PresentationSettingsEnumPage) {
        let response = ui
            .push_id(page.salt(), |ui| {
                ui.label(
                    RichText::new(page.label())
                        .size(PresentationThemeTypography::SECTION_POINTS)
                        .strong(),
                )
            })
            .inner;
        if self.target == Some(page) {
            response.scroll_to_me(Some(Align::TOP));
            self.target = None;
        }
    }

    pub fn page(
        &mut self,
        ui: &mut Ui,
        page: PresentationSettingsEnumPage,
        content: impl FnOnce(&mut Ui),
    ) {
        ui.add_space(PresentationThemeTokens::SPACE_6);
        self.heading(ui, page);
        ui.add_space(PresentationThemeTokens::SPACE_3);
        content(ui);
    }
}

pub struct PresentationSettingsView;

struct PresentationSettingsEngineRow<'l> {
    label: &'l str,
    current: Option<u32>,
    minimum: u32,
    maximum: u32,
}

impl PresentationSettingsView {
    pub fn card<R>(ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> R {
        PresentationThemeComponents::card(ui)
            .inner_margin(Margin::ZERO)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                content(ui)
            })
            .inner
    }

    pub fn row<R>(
        ui: &mut Ui,
        title: &str,
        description: Option<&str>,
        control: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        egui::Frame::new()
            .inner_margin(Margin::symmetric(16, 14))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(
                    PresentationThemeTokens::SPACE_4,
                    PresentationThemeTokens::SPACE_1,
                );
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_max_width((ui.available_width() * 0.5).max(160.0));
                        ui.label(RichText::new(title).strong());
                        if let Some(description) = description {
                            PresentationThemeComponents::caption(ui, description);
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), control)
                        .inner
                })
                .inner
            })
            .inner
    }

    pub fn block<R>(ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> R {
        egui::Frame::new()
            .inner_margin(Margin::symmetric(16, 14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::vec2(
                    PresentationThemeTokens::SPACE_2,
                    PresentationThemeTokens::SPACE_2,
                );
                content(ui)
            })
            .inner
    }

    pub fn show(
        ui: &mut Ui,
        controller: &mut PresentationSettingsController,
        navigation: &mut PresentationSettingsNavigation,
        display: impl FnOnce(&mut Ui),
    ) -> PresentationSettingsOutcome {
        let mut outcome = controller.poll();
        outcome.close |= Self::actions(ui, controller).close;
        ui.add_space(PresentationThemeTokens::SPACE_2);
        PresentationThemeComponents::caption(
            ui,
            "Appearance applies immediately. Everything else applies after Save.",
        );
        if let Some(notice) = controller.notice() {
            PresentationThemeComponents::pill(
                ui,
                notice.to_owned(),
                PresentationThemeEnumTone::Accent,
            );
        }
        let prompt = Self::prompt(ui, controller);
        outcome.close |= prompt.close;
        outcome.appearance_changed |= prompt.appearance_changed;
        let mut display = Some(display);
        for section in DesktopPreferencesEnumSection::iter() {
            navigation.page(
                ui,
                PresentationSettingsEnumPage::of_section(section),
                |ui| {
                    let step = Self::card(ui, |ui| {
                        let step = Self::section(ui, controller, section);
                        if section == DesktopPreferencesEnumSection::BoardAids
                            && let Some(display) = display.take()
                        {
                            PresentationThemeComponents::divider(ui);
                            display(ui);
                        }
                        PresentationThemeComponents::divider(ui);
                        Self::block(ui, |ui| {
                            if PresentationThemeEnumButton::Ghost
                                .show_sized(ui, "Reset section", PresentationThemeEnumSize::Small)
                                .clicked()
                            {
                                controller.begin_reset(DesktopPreferencesEnumResetScope::Section(
                                    section,
                                ));
                            }
                        });
                        step
                    });
                    outcome.appearance_changed |= step.appearance_changed;
                },
            );
        }
        navigation.page(ui, PresentationSettingsEnumPage::Updates, |ui| {
            Self::card(ui, |ui| Self::block(ui, |ui| Self::updates(ui, controller)));
        });
        navigation.page(ui, PresentationSettingsEnumPage::Exchange, |ui| {
            Self::card(ui, |ui| {
                Self::block(ui, |ui| Self::exchange(ui, controller))
            });
        });
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
                let theme = Self::row(ui, "Theme", None, |ui| {
                    let mut theme = controller.theme();
                    let options: Vec<(DesktopPreferencesEnumTheme, &str)> =
                        DesktopPreferencesEnumTheme::iter()
                            .map(|choice| (choice, choice.label()))
                            .collect();
                    PresentationThemeComponents::segmented(ui, &mut theme, &options, false)
                        .then_some(theme)
                });
                if let Some(theme) = theme {
                    let step = controller.edit(DesktopPreferencesEnumPatch::Theme(theme));
                    outcome.appearance_changed |= step.appearance_changed;
                }
                PresentationThemeComponents::divider(ui);
                let motion = Self::row(
                    ui,
                    "Motion",
                    Some("Instant piece moves, no camera drift, no panel slides."),
                    |ui| {
                        let mut motion = controller.motion();
                        let options: Vec<(DesktopPreferencesEnumMotion, &str)> =
                            DesktopPreferencesEnumMotion::iter()
                                .map(|choice| (choice, choice.label()))
                                .collect();
                        PresentationThemeComponents::segmented(ui, &mut motion, &options, false)
                            .then_some(motion)
                    },
                );
                if let Some(motion) = motion {
                    let step = controller.edit(DesktopPreferencesEnumPatch::Motion(motion));
                    outcome.appearance_changed |= step.appearance_changed;
                }
                PresentationThemeComponents::divider(ui);
                let notation = Self::row(
                    ui,
                    "Move notation",
                    Some("Figurine or letter piece names."),
                    |ui| {
                        let mut notation = pending.notation();
                        let options: Vec<(DesktopPreferencesEnumNotation, &str)> =
                            DesktopPreferencesEnumNotation::iter()
                                .map(|choice| (choice, choice.label()))
                                .collect();
                        PresentationThemeComponents::segmented(ui, &mut notation, &options, false)
                            .then_some(notation)
                    },
                );
                if let Some(notation) = notation {
                    let step = controller.edit(DesktopPreferencesEnumPatch::Notation(notation));
                    outcome.appearance_changed |= step.appearance_changed;
                }
            }
            DesktopPreferencesEnumSection::BoardAids => {
                for (index, aid) in DesktopPreferencesEnumAid::iter().enumerate() {
                    if index > 0 {
                        PresentationThemeComponents::divider(ui);
                    }
                    Self::block(ui, |ui| {
                        let mut enabled = pending.aid(aid);
                        if ui.checkbox(&mut enabled, aid.label()).changed() {
                            controller.edit(DesktopPreferencesEnumPatch::Aid(aid, enabled));
                        }
                    });
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
        Self::block(ui, |ui| {
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
        });
        for category in DesktopPreferencesEnumAudioCategory::iter() {
            PresentationThemeComponents::divider(ui);
            Self::block(ui, |ui| {
                let entry = pending.category(category);
                ui.horizontal(|ui| {
                    let mut enabled = entry.enabled();
                    if ui.checkbox(&mut enabled, category.label()).changed() {
                        controller.edit(DesktopPreferencesEnumPatch::CategoryEnabled(
                            category, enabled,
                        ));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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
                    });
                });
            });
        }
    }

    fn shortcuts(
        ui: &mut Ui,
        controller: &mut PresentationSettingsController,
        pending: DesktopPreferences,
    ) {
        let platform = PresentationShortcutEnumPlatform::current();
        for (index, action) in DesktopPreferencesEnumShortcutAction::iter().enumerate() {
            if index > 0 {
                PresentationThemeComponents::divider(ui);
            }
            let label = PresentationShortcuts::label(
                platform,
                PresentationShortcuts::chord(pending, action),
            );
            egui::Frame::new()
                .inner_margin(Margin::symmetric(16, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}: {label}", action.label()));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if PresentationThemeEnumButton::Ghost
                                .show_sized(ui, "Default", PresentationThemeEnumSize::Small)
                                .clicked()
                            {
                                controller.reset_shortcut(action);
                            }
                            if PresentationThemeEnumButton::Secondary
                                .show_sized(ui, "Apply", PresentationThemeEnumSize::Small)
                                .clicked()
                            {
                                controller.apply_shortcut(action);
                            }
                            let edit = controller.shortcut_edit_mut(action);
                            ui.add(
                                egui::TextEdit::singleline(&mut edit.key)
                                    .desired_width(PresentationThemeTokens::CONTROL_SM),
                            );
                            ui.checkbox(&mut edit.command, "Command");
                            PresentationThemeComponents::kbd(ui, &label);
                        });
                    });
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
        PresentationThemeComponents::divider(ui);
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
        PresentationThemeComponents::divider(ui);
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
            PresentationThemeComponents::divider(ui);
            Self::block(ui, |ui| {
                PresentationThemeComponents::caption(ui, note);
            });
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
        Self::block(ui, |ui| {
            ui.horizontal(|ui| {
                let mut custom = current.is_some();
                let mut value = current.unwrap_or(minimum.max(1));
                let toggled = ui.checkbox(&mut custom, label).changed();
                let edited = ui
                    .with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if custom {
                            ui.add(egui::DragValue::new(&mut value).range(minimum..=maximum))
                                .changed()
                        } else {
                            PresentationThemeComponents::caption(ui, "Engine default");
                            false
                        }
                    })
                    .inner;
                if toggled || edited {
                    let requested = custom.then_some(value);
                    match patch(requested) {
                        Ok(change) => {
                            controller.edit(change);
                        }
                        Err(error) => controller.reject(&error),
                    }
                }
            });
        });
    }

    fn updates(ui: &mut Ui, controller: &mut PresentationSettingsController) {
        if controller.update_poll_due() {
            controller.request_update(DesktopUpdateOperation::Snapshot);
        }
        let Some(snapshot) = controller.update().cloned() else {
            PresentationThemeComponents::caption(ui, "Update status is loading.");
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
            PresentationThemeComponents::divider(ui);
            ui.add(
                egui::TextEdit::singleline(controller.update_credential_mut())
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            ui.checkbox(controller.update_persist_mut(), "Keep in secure storage");
            if PresentationThemeEnumButton::Primary
                .show(ui, "Save credential")
                .clicked()
            {
                let persist = *controller.update_persist_mut();
                controller.submit_update_credential(persist);
            }
            if chosen == Some(PresentationSettingsUpdateEnumAction::UseSessionCredential) {
                controller.submit_update_credential(false);
            }
        }
        if controller.update_install_prompt() {
            PresentationThemeComponents::divider(ui);
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
        PresentationThemeComponents::field_label(
            ui,
            "Export or import settings (no machine paths are included).",
        );
        let colors = PresentationThemeColors::of(ui.ctx());
        ui.add(
            egui::TextEdit::multiline(controller.exchange_mut())
                .background_color(colors.bg)
                .code_editor()
                .desired_rows(4)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            if PresentationThemeEnumButton::Secondary
                .show_sized(ui, "Export", PresentationThemeEnumSize::Small)
                .clicked()
            {
                controller.export();
            }
            if PresentationThemeEnumButton::Secondary
                .show_sized(ui, "Preview import", PresentationThemeEnumSize::Small)
                .clicked()
            {
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
            let pending = controller.draft().status().has_pending_changes();
            if pending {
                PresentationThemeComponents::pill(
                    ui,
                    "Unsaved",
                    PresentationThemeEnumTone::Unsaved,
                );
            }
            if controller.is_saving() {
                ui.label("Saving");
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let save = PresentationThemeEnumButton::Primary.widget(
                    ui,
                    "Save",
                    PresentationThemeEnumSize::Medium,
                );
                if ui.add_enabled(controller.can_save(), save).clicked() {
                    controller.save();
                }
                let discard = PresentationThemeEnumButton::Secondary.widget(
                    ui,
                    "Discard",
                    PresentationThemeEnumSize::Medium,
                );
                if ui.add_enabled(pending, discard).clicked() {
                    let step = controller.discard();
                    outcome.appearance_changed |= step.appearance_changed;
                }
                if PresentationThemeEnumButton::Ghost
                    .show(ui, "Reset all")
                    .clicked()
                {
                    controller.begin_reset(DesktopPreferencesEnumResetScope::All);
                }
            });
        });
        outcome
    }

    fn prompt(
        ui: &mut Ui,
        controller: &mut PresentationSettingsController,
    ) -> PresentationSettingsOutcome {
        let mut outcome = PresentationSettingsOutcome::default();
        let message = match controller.prompt().clone() {
            PresentationSettingsEnumPrompt::None => return outcome,
            PresentationSettingsEnumPrompt::Leave => None,
            PresentationSettingsEnumPrompt::Reset(preview) => Some(format!(
                "Reset {} setting(s) to defaults? Games, history and datasets are not changed.",
                preview.changes().len()
            )),
            PresentationSettingsEnumPrompt::Import(preview) => Some(format!(
                "Import changes {} setting(s); other settings and destination locations stay.",
                preview.changes().len()
            )),
        };
        ui.add_space(PresentationThemeTokens::SPACE_3);
        let colors = PresentationThemeColors::of(ui.ctx());
        PresentationThemeComponents::card(ui)
            .fill(colors.surface_2)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                match message {
                    None => {
                        ui.label("Settings have unsaved changes.");
                        ui.horizontal(|ui| {
                            if PresentationThemeEnumButton::Primary
                                .show(ui, "Save")
                                .clicked()
                            {
                                controller.leave_saving();
                            }
                            if PresentationThemeEnumButton::Ghost
                                .show(ui, "Discard")
                                .clicked()
                            {
                                let step = controller.leave_discarding();
                                outcome.appearance_changed |= step.appearance_changed;
                                outcome.close = true;
                            }
                            if PresentationThemeEnumButton::Ghost
                                .show(ui, "Stay")
                                .clicked()
                            {
                                controller.stay();
                            }
                        });
                    }
                    Some(message) => {
                        ui.label(message);
                        outcome.appearance_changed |= Self::confirm_buttons(ui, controller);
                    }
                }
            });
        outcome
    }

    fn confirm_buttons(ui: &mut Ui, controller: &mut PresentationSettingsController) -> bool {
        let mut appearance = false;
        ui.horizontal(|ui| {
            if PresentationThemeEnumButton::Primary
                .show(ui, "Confirm")
                .clicked()
            {
                appearance = controller.confirm_prompt().appearance_changed;
            }
            if PresentationThemeEnumButton::Ghost
                .show(ui, "Cancel")
                .clicked()
            {
                controller.stay();
            }
        });
        appearance
    }
}
