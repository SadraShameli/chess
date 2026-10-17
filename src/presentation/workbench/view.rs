use std::collections::BTreeMap;

use bevy_egui::egui::{self, Ui};

use crate::desktop::error::DesktopFailure;
use crate::desktop::identity::DesktopRequestId;
use crate::engine::enums::{EngineEnumOption, EngineOptionEnumDescriptor};
use crate::engine::options::EngineOptionsEnumValue;
use crate::engine::package::manifest::EnginePackageManifest;
use crate::presentation::theme::tokens::PresentationThemeTokens;
use crate::presentation::workbench::engine_information::{
    PresentationWorkbenchEngineInformation, PresentationWorkbenchEngineInformationEnumState,
};
use crate::presentation::workbench::configuration::{
    PresentationWorkbenchConfiguration, PresentationWorkbenchConfigurationError,
};
use crate::presentation::workbench::controller::{
    PresentationWorkbenchController, PresentationWorkbenchControllerContext,
    PresentationWorkbenchControllerEnumAdmission, PresentationWorkbenchControllerError,
    PresentationWorkbenchControllerIntent,
};
use crate::presentation::workbench::snapshot::{
    PresentationWorkbenchJobSnapshot, PresentationWorkbenchSnapshot,
    PresentationWorkbenchTablebaseEnumState,
};
use crate::workbench::enums::{WorkbenchEnumJobState, WorkbenchEnumOperation};
use crate::workbench::job::WorkbenchJobDeadline;
use crate::workbench::parameters::{WorkbenchBenchParameters, WorkbenchPerftDepth};

pub struct PresentationWorkbenchView {
    configuration: PresentationWorkbenchConfiguration,
    operation: WorkbenchEnumOperation,
    numbers: BTreeMap<EngineEnumOption, String>,
    minimal: Option<bool>,
    context: Option<PresentationWorkbenchControllerContext>,
    submitted_frame: Option<u64>,
    error: Option<PresentationWorkbenchControllerError>,
    tablebase_selection_failure: Option<DesktopFailure>,
    engine_retry: bool,
    recovered_paused: bool,
}

impl Default for PresentationWorkbenchView {
    fn default() -> Self {
        Self {
            configuration: PresentationWorkbenchConfiguration::default(),
            operation: WorkbenchEnumOperation::Inspector,
            numbers: BTreeMap::new(),
            minimal: None,
            context: None,
            submitted_frame: None,
            error: None,
            tablebase_selection_failure: None,
            engine_retry: false,
            recovered_paused: false,
        }
    }
}

impl PresentationWorkbenchView {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_recovered_paused(&mut self, recovered_paused: bool) {
        self.recovered_paused = recovered_paused;
    }

    pub fn take_engine_retry(&mut self) -> bool {
        std::mem::take(&mut self.engine_retry)
    }

    pub fn set_tablebase_selection_failure(&mut self, failure: Option<DesktopFailure>) {
        self.tablebase_selection_failure = failure;
    }

    pub const fn configuration(&self) -> &PresentationWorkbenchConfiguration {
        &self.configuration
    }
    pub fn configuration_mut(&mut self) -> &mut PresentationWorkbenchConfiguration {
        &mut self.configuration
    }
    pub fn set_operation(&mut self, operation: WorkbenchEnumOperation) {
        self.operation = operation;
    }

    pub fn set_number(
        &mut self,
        option: EngineEnumOption,
        input: String,
    ) -> Result<(), PresentationWorkbenchConfigurationError> {
        if !matches!(
            option.typed_descriptor(),
            EngineOptionEnumDescriptor::Spin(_)
        ) {
            return Err(PresentationWorkbenchConfigurationError::NonNumeric(option));
        }
        self.numbers.insert(option, input);
        Ok(())
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        controller: &mut PresentationWorkbenchController,
    ) -> Option<PresentationWorkbenchControllerIntent> {
        self.synchronize(controller.context());
        let intent = egui::ScrollArea::vertical()
            .id_salt("workbench.panel")
            .show(ui, |ui| {
                ui.heading("Engine Workbench");
                self.show_admission(ui, controller);
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                }
                let mut intent = self.show_options(ui, controller);
                ui.separator();
                if let Some(job) = controller.job() {
                    Self::show_job(ui, job);
                } else {
                    ui.label("No diagnostic job yet. Select an operation and explicitly Run.");
                }
                if intent.is_none() {
                    intent = self.show_job_actions(ui, controller);
                }
                ui.separator();
                if intent.is_none() {
                    intent = self.show_diagnostics(ui, controller);
                }
                ui.separator();
                Self::show_unsupported(ui);
                intent
            })
            .inner;
        if intent.is_some() {
            self.submitted_frame = Some(ui.ctx().cumulative_frame_nr());
        }
        intent
    }

    fn synchronize(&mut self, context: PresentationWorkbenchControllerContext) {
        if self.context.is_some_and(|previous| {
            previous.generation() != context.generation()
                || previous.selection() != context.selection()
        }) {
            self.numbers.clear();
            self.minimal = None;
            self.error = None;
        }
        self.context = Some(context);
    }

    fn show_admission(&mut self, ui: &mut Ui, controller: &PresentationWorkbenchController) {
        if let Some(failure) = controller.failure() {
            ui.colored_label(ui.visuals().error_fg_color, failure.message());
        }
        if controller.exported_job().is_some() {
            ui.label("Diagnostic export completed by the backend.");
        }
        if controller.engine().is_none() {
            match controller.failure() {
                None => {
                    ui.label("Loading engine identity and capabilities...");
                }
                Some(failure) => self.show_information(
                    ui,
                    &PresentationWorkbenchEngineInformation::unavailable(
                        PresentationWorkbenchEngineInformationEnumState::Diagnostic(
                            failure.message().to_owned(),
                        ),
                    ),
                ),
            }
            return;
        }
        if controller.is_pending() {
            ui.label("Pending backend acknowledgment. Changes are not applied until confirmed.");
        }
        match controller.admission() {
            PresentationWorkbenchControllerEnumAdmission::Unavailable => {
                ui.label("Engine unavailable: restore the selected package and retry setup.");
            }
            PresentationWorkbenchControllerEnumAdmission::Busy(reason) => {
                ui.label(format!(
                    "Engine busy: {reason:?}. Maintenance and diagnostics wait for an idle engine."
                ));
            }
            PresentationWorkbenchControllerEnumAdmission::Idle { available_workers } => {
                ui.label(format!("Available shared CPU capacity: {available_workers} workers. Backend admission is required."));
            }
        }
    }

    fn show_options(
        &mut self,
        ui: &mut Ui,
        controller: &mut PresentationWorkbenchController,
    ) -> Option<PresentationWorkbenchControllerIntent> {
        let engine = controller.engine()?;
        ui.strong("Advanced engine configuration");
        self.show_package(ui, engine);
        ui.small("Save each edited option explicitly. CPU and memory admission is rechecked by the backend at an idle configuration barrier.");
        let mut intent = None;
        for option in EngineEnumOption::ALL {
            ui.push_id(option.name(), |ui| {
                ui.group(|ui| {
                    if intent.is_some() {
                        ui.disable();
                    }
                    if let Some(next) = self.show_option(ui, controller, option) {
                        intent = Some(next);
                    }
                });
            });
        }
        intent
    }

    fn show_package(&mut self, ui: &mut Ui, engine: &PresentationWorkbenchSnapshot) {
        let state = if self.recovered_paused {
            PresentationWorkbenchEngineInformationEnumState::RecoveredPaused
        } else {
            PresentationWorkbenchEngineInformationEnumState::Ready
        };
        let information =
            PresentationWorkbenchEngineInformation::from_package(engine.package(), state);
        self.show_information(ui, &information);
    }

    fn show_information(
        &mut self,
        ui: &mut Ui,
        information: &PresentationWorkbenchEngineInformation,
    ) {
        if information.show(ui).is_some() {
            self.engine_retry = true;
        }
    }

    fn show_option(
        &mut self,
        ui: &mut Ui,
        controller: &mut PresentationWorkbenchController,
        option: EngineEnumOption,
    ) -> Option<PresentationWorkbenchControllerIntent> {
        let engine = controller.engine()?;
        let descriptor = engine.capabilities().descriptor(option)?;
        let enabled = controller.can_configure() && self.can_submit(ui);
        match descriptor {
            EngineOptionEnumDescriptor::Spin(range) => {
                ui.strong(option.name());
                let unit = PresentationWorkbenchConfiguration::unit(option);
                ui.small(format!(
                    "Default: {} {unit}; range {}..{} {unit}",
                    range.default_value(),
                    range.minimum(),
                    range.maximum()
                ));
                if option == EngineEnumOption::MultiPv {
                    ui.small(
                        "Each search additionally caps candidate count to its legal root moves.",
                    );
                }
                let applied = engine.value(option);
                ui.label(format!(
                    "Applied: {}",
                    applied
                        .as_ref()
                        .map(Self::value_text)
                        .unwrap_or_else(|| "unavailable until acknowledged".to_owned())
                ));
                let draft = self.numbers.entry(option).or_insert_with(|| {
                    applied
                        .as_ref()
                        .map(Self::value_text)
                        .unwrap_or_else(|| range.default_value().to_string())
                });
                Self::field(ui, &format!("{} value", option.name()), draft);
                let setting = PresentationWorkbenchConfiguration::number(
                    option,
                    draft,
                    engine.capabilities(),
                );
                if let Err(error) = &setting {
                    ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                }
                if Self::button(
                    ui,
                    enabled && setting.is_ok(),
                    &format!("Save {}", option.name()),
                ) && let Ok(setting) = setting
                {
                    let result =
                        controller.configure(setting.value().clone(), DesktopRequestId::new());
                    return self.accept(result);
                }
            }
            EngineOptionEnumDescriptor::Check(default) => match option {
                EngineEnumOption::Minimal => {
                    ui.strong(option.name());
                    ui.small(format!(
                        "Default: {default}. Minimal suppresses intermediate search output."
                    ));
                    ui.label(format!(
                        "Applied: {}",
                        engine
                            .value(option)
                            .as_ref()
                            .map(Self::value_text)
                            .unwrap_or_else(|| "unavailable".to_owned())
                    ));
                    let value = self.minimal.get_or_insert_with(|| {
                        engine
                            .applied()
                            .map_or(default, |applied| applied.is_minimal())
                    });
                    ui.checkbox(value, "Minimal final-report output");
                    let value = *value;
                    if Self::button(ui, enabled, "Save Minimal") {
                        let result = controller.configure(
                            EngineOptionsEnumValue::Minimal(value),
                            DesktopRequestId::new(),
                        );
                        return self.accept(result);
                    }
                }
                EngineEnumOption::Chess960 => {
                    ui.strong(option.name());
                    ui.small(format!(
                        "Engine default: {default}. Derived from the selected game's variant."
                    ));
                    if let Some(position) = controller.position() {
                        ui.label(format!(
                            "Current game variant: {:?}",
                            position.current().variant()
                        ));
                    } else {
                        ui.label("Current game variant: unavailable. Choose through New Game.");
                    }
                    ui.small("This panel cannot change a live board's variant.");
                }
                EngineEnumOption::Hash
                | EngineEnumOption::Threads
                | EngineEnumOption::MoveOverhead
                | EngineEnumOption::ClearHash
                | EngineEnumOption::MultiPv
                | EngineEnumOption::SyzygyPath => {
                    ui.label("Unavailable: incompatible advertised option descriptor.");
                }
            },
            EngineOptionEnumDescriptor::Button => {
                ui.small(
                    "Clear the transposition table only while idle. This action has no value.",
                );
                if Self::button(ui, enabled, option.name()) {
                    let result = controller
                        .configure(EngineOptionsEnumValue::ClearHash, DesktopRequestId::new());
                    return self.accept(result);
                }
            }
            EngineOptionEnumDescriptor::String => {
                return self.show_tablebases(ui, controller, enabled);
            }
        }
        None
    }

    fn show_tablebases(
        &mut self,
        ui: &mut Ui,
        controller: &mut PresentationWorkbenchController,
        enabled: bool,
    ) -> Option<PresentationWorkbenchControllerIntent> {
        let tablebases = controller.engine()?.tablebases();
        ui.strong(EngineEnumOption::SyzygyPath.name());
        ui.small("Default: empty, optional local Syzygy tables.");
        match tablebases.state() {
            PresentationWorkbenchTablebaseEnumState::Unconfigured => {
                ui.label("Tablebases: unconfigured. Ordinary chess remains available.");
            }
            PresentationWorkbenchTablebaseEnumState::Configured => {
                ui.label("Tablebases: directories prepared, engine loading not confirmed.");
            }
            PresentationWorkbenchTablebaseEnumState::Loaded(pieces) => {
                ui.label(format!("Tablebases: loaded, up to {pieces} pieces."));
            }
            PresentationWorkbenchTablebaseEnumState::Unavailable => {
                ui.label("Tablebases: unavailable. Check readable .rtbw/.rtbz files and directory access, then configure again.");
            }
            PresentationWorkbenchTablebaseEnumState::Failed(message) => {
                ui.colored_label(ui.visuals().error_fg_color, message.as_ref());
            }
        }
        for directory in tablebases.directories() {
            ui.label(directory.display().to_string());
        }
        if let Some(alias) = tablebases.alias() {
            ui.small(format!("Managed engine alias: {}", alias.display()));
        }
        ui.small("The backend prepares whitespace-free file links for paths containing spaces. Grant read/traversal access; Unix aliases are private (0700). Windows links need Developer Mode or link privilege. Choose a suitable writable alias parent if preparation fails. Files are never silently copied.");
        let is_prepared = !matches!(
            tablebases.state(),
            PresentationWorkbenchTablebaseEnumState::Unconfigured
        );
        if let Some(failure) = &self.tablebase_selection_failure {
            ui.label(failure.message());
        }
        if Self::button(
            ui,
            enabled && self.tablebase_selection_failure.is_none(),
            "Select tablebase directories",
        ) {
            let result = controller.select_tablebases(DesktopRequestId::new());
            return self.accept(result);
        }
        if Self::button(ui, enabled && is_prepared, "Configure selected tablebases") {
            let result = controller.configure_tablebases(DesktopRequestId::new());
            return self.accept(result);
        }
        None
    }

    fn show_diagnostics(
        &mut self,
        ui: &mut Ui,
        controller: &mut PresentationWorkbenchController,
    ) -> Option<PresentationWorkbenchControllerIntent> {
        ui.strong("Diagnostic operations");
        ui.small("Diagnostics run in isolated backend children, outside the UI and live engine. They require exclusive admission and share the application CPU budget.");
        ui.horizontal_wrapped(|ui| {
            for operation in WorkbenchEnumOperation::ALL {
                ui.radio_value(
                    &mut self.operation,
                    operation,
                    PresentationWorkbenchConfiguration::operation_label(operation),
                );
            }
        });
        match self.operation {
            WorkbenchEnumOperation::Inspector => {
                ui.small("Inspect the selected validated position and bounded engine output.");
            }
            WorkbenchEnumOperation::RawEvaluation => {
                ui.small("Raw White-perspective static NNUE integer. This is separate from search centipawns.");
            }
            WorkbenchEnumOperation::Compiler => {
                ui.small("Read compiler, target and target-feature diagnostics for the selected package.");
            }
            WorkbenchEnumOperation::Perft => {
                ui.small(format!(
                    "Default depth {}; range {}..{} plies. Root divide is included.",
                    WorkbenchPerftDepth::default().value(),
                    WorkbenchPerftDepth::MINIMUM,
                    WorkbenchPerftDepth::MAXIMUM
                ));
                Self::field(
                    ui,
                    "Perft depth (plies)",
                    &mut self.configuration.perft_depth,
                );
            }
            WorkbenchEnumOperation::Bench => {
                ui.small(format!("Built-in {}-position corpus. Empty fields use upstream defaults, independent of gameplay resource settings.", WorkbenchBenchParameters::POSITIONS));
                if let Some(engine) = controller.engine() {
                    match WorkbenchBenchParameters::new(engine.capabilities(), None, None, None) {
                        Ok(defaults) => {
                            ui.small(format!(
                                "Defaults: {} MiB, {} thread(s), depth {}",
                                defaults.hash_mib(),
                                defaults.threads(),
                                defaults.depth()
                            ));
                        }
                        Err(error) => {
                            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                        }
                    }
                }
                Self::field(
                    ui,
                    "Benchmark hash (MiB, optional)",
                    &mut self.configuration.bench_hash,
                );
                Self::field(
                    ui,
                    "Benchmark threads (optional)",
                    &mut self.configuration.bench_threads,
                );
                Self::field(
                    ui,
                    "Benchmark depth (plies, optional)",
                    &mut self.configuration.bench_depth,
                );
            }
        }
        Self::field(ui, "Deadline (ms)", &mut self.configuration.deadline_millis);
        ui.small(format!(
            "Default deadline {} ms; maximum {} ms. Cancellation waits for backend child cleanup.",
            WorkbenchJobDeadline::default().duration().as_millis(),
            WorkbenchJobDeadline::MAXIMUM.as_millis()
        ));
        let engine = controller.engine()?;
        let parameters = self.configuration.parameters(
            self.operation,
            engine.capabilities(),
            controller.position(),
        );
        let deadline = self.configuration.deadline();
        if let Err(error) = &parameters {
            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
        }
        if let Err(error) = &deadline {
            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
        }
        let admission = parameters
            .as_ref()
            .ok()
            .map(|parameters| controller.check_parameters(parameters));
        if let Some(Err(error)) = &admission {
            ui.label(error.to_string());
        }
        let enabled = self.can_submit(ui) && deadline.is_ok() && matches!(admission, Some(Ok(())));
        if Self::button(ui, enabled, "Run diagnostic")
            && let (Ok(parameters), Ok(deadline)) = (parameters, deadline)
        {
            let result = controller.start(parameters, deadline, DesktopRequestId::new());
            return self.accept(result);
        }
        None
    }

    fn show_job(ui: &mut Ui, job: &PresentationWorkbenchJobSnapshot) {
        ui.strong(job.title());
        ui.label(format!("Job: {}", job.id().value()));
        ui.label(format!("State: {}", Self::state_label(job.state())));
        ui.label(job.parameters());
        ui.small(format!(
            "Job package: {} | {:?} | {}",
            job.package().target().triple(),
            job.package().profile(),
            job.package().digest()
        ));
        ui.label(format!(
            "Progress: {} / {}",
            job.progress().completed(),
            job.progress().total()
        ));
        ui.label(format!(
            "Deadline budget: {} ms; elapsed at last backend update: {} ms",
            job.timeout().duration().as_millis(),
            job.elapsed().as_millis()
        ));
        if !job.is_terminal() {
            if let Some(remaining) = job.remaining() {
                ui.label(format!(
                    "Deadline remaining at last backend update: {} ms",
                    remaining.as_millis()
                ));
            } else {
                ui.label("Deadline reached. Waiting for backend cancellation and child cleanup.");
            }
        }
        if let Some(cancellation) = job.cancellation() {
            ui.label(format!("Cancellation reason: {:?}", cancellation.reason()));
            if !job.rows().is_empty() {
                ui.small("A result arrived during cleanup. The cancellation outcome still applies to this job.");
            }
        }
        if let Some(failure) = job.failure() {
            ui.colored_label(ui.visuals().error_fg_color, failure);
        }
        if job.rows().is_empty() {
            ui.label(if job.is_terminal() {
                "No completed result payload was reported."
            } else {
                "Waiting for a diagnostic result."
            });
        }
        egui::Grid::new(("workbench.result", job.id()))
            .striped(true)
            .show(ui, |ui| {
                for row in job.rows() {
                    ui.label(row.label());
                    ui.label(row.value());
                    ui.end_row();
                }
            });
        if job.omitted_rows() > 0 {
            ui.label(format!("{} result rows omitted from this bounded view; export the backend result for the full payload.", job.omitted_rows()));
        }
        if !job.diagnostics().is_empty() || job.omitted_lines() > 0 {
            egui::CollapsingHeader::new("Bounded diagnostic output")
                .id_salt(job.id())
                .show(ui, |ui| {
                    if job.omitted_lines() > 0 {
                        ui.label(format!(
                            "{} earlier output lines omitted or evicted.",
                            job.omitted_lines()
                        ));
                    }
                    for line in job.diagnostics() {
                        ui.monospace(line.as_ref());
                    }
                });
        }
    }

    fn show_job_actions(
        &mut self,
        ui: &mut Ui,
        controller: &mut PresentationWorkbenchController,
    ) -> Option<PresentationWorkbenchControllerIntent> {
        let job = controller.job()?.id();
        if Self::button(
            ui,
            controller.can_cancel() && self.can_submit(ui),
            "Cancel diagnostic job",
        ) {
            let result = controller.cancel(job, DesktopRequestId::new());
            return self.accept(result);
        }
        if Self::button(
            ui,
            controller.can_export() && self.can_submit(ui),
            "Export diagnostic result",
        ) {
            let result = controller.export(job, DesktopRequestId::new());
            return self.accept(result);
        }
        None
    }

    fn show_unsupported(ui: &mut Ui) {
        egui::CollapsingHeader::new("Unavailable engine features at this pin").show(ui, |ui| {
            ui.label("Reckless v0.9.0 does not implement ponder, strength/Elo/Skill Level controls, searchmoves, mate search, combined search limits, WDL probabilities, UCI_ShowWDL or UCI_AnalyseMode.");
            ui.label("Runtime EvalFile replacement, engine-native opening books/Polyglot, SyzygyProbeDepth, SyzygyProbeLimit and Syzygy50MoveRule are absent from this option registry. Opening study and clocks belong to the application.");
            ui.label("NUMA requires a separately built Linux profile and runtime evidence. It is not a UCI toggle. SPSA is unavailable in this pin pending the controlled generated-function compile check (DEV-08).");
            ui.hyperlink_to("Pinned parser and advertised options", format!("https://github.com/codedeliveryservice/Reckless/blob/{}/src/uci.rs", EnginePackageManifest::COMMIT));
            ui.hyperlink_to("Pinned build and feature definitions", format!("https://github.com/codedeliveryservice/Reckless/blob/{}/Cargo.toml", EnginePackageManifest::COMMIT));
        });
    }

    fn field(ui: &mut Ui, label: &str, value: &mut String) {
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(label);
            ui.add(
                egui::TextEdit::singleline(value)
                    .id_salt(label.id)
                    .desired_width(PresentationThemeTokens::PANEL_WIDTH)
                    .char_limit(32),
            )
            .labelled_by(label.id);
        });
    }

    fn button(ui: &mut Ui, enabled: bool, label: &str) -> bool {
        ui.add_enabled(
            enabled,
            egui::Button::new(label).min_size(egui::vec2(0.0, PresentationThemeTokens::MIN_TARGET)),
        )
        .clicked()
    }

    fn can_submit(&self, ui: &Ui) -> bool {
        self.submitted_frame != Some(ui.ctx().cumulative_frame_nr())
    }

    fn accept(
        &mut self,
        result: Result<
            Option<PresentationWorkbenchControllerIntent>,
            PresentationWorkbenchControllerError,
        >,
    ) -> Option<PresentationWorkbenchControllerIntent> {
        match result {
            Ok(intent) => {
                self.error = None;
                intent
            }
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }

    fn value_text(value: &EngineOptionsEnumValue) -> String {
        match value {
            EngineOptionsEnumValue::Hash(value)
            | EngineOptionsEnumValue::Threads(value)
            | EngineOptionsEnumValue::MultiPv(value) => value.to_string(),
            EngineOptionsEnumValue::MoveOverhead(value) => value.as_millis().to_string(),
            EngineOptionsEnumValue::Minimal(value) => value.to_string(),
            EngineOptionsEnumValue::Chess960(value) => format!("{value:?}"),
            EngineOptionsEnumValue::ClearHash => "Button action".to_owned(),
            EngineOptionsEnumValue::SyzygyPath(value) => value.as_str().to_owned(),
        }
    }

    const fn state_label(state: WorkbenchEnumJobState) -> &'static str {
        match state {
            WorkbenchEnumJobState::Queued => "Queued for admission",
            WorkbenchEnumJobState::Admitted => "Admitted, waiting to start",
            WorkbenchEnumJobState::Running => "Running",
            WorkbenchEnumJobState::Cancelling => "Cancelling, waiting for child cleanup",
            WorkbenchEnumJobState::Succeeded => "Completed",
            WorkbenchEnumJobState::Failed => "Failed",
            WorkbenchEnumJobState::Cancelled => "Cancelled",
            WorkbenchEnumJobState::TimedOut => "Deadline expired",
        }
    }
}
