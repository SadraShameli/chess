mod service;

use bevy_egui::egui::{self, Ui};

use crate::board::r#move::BoardMove;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumPending,
    DesktopAnnotationCommandsEnumResult, DesktopAnnotationCommandsEnumScope,
    DesktopAnnotationCommandsScopePage, DesktopAnnotationCommandsVariationPreview,
};
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::error::DesktopFailure;
use crate::desktop::game_editor::{
    DesktopGameEditorContext, DesktopGameEditorPublication, DesktopGameEditorSnapshotEnumState,
    DesktopGameEditorTreeChildrenPage, DesktopGameEditorTreeNode, DesktopGameEditorTreePage,
};
use crate::desktop::identity::DesktopRequestId;
use crate::game::annotations::GameAnnotationsEnumVariationRequest;
use crate::game::enums::GameMoveId;
use crate::presentation::analysis::editor_intent::{
    PresentationAnalysisEditorError, PresentationAnalysisEditorHistory,
    PresentationAnalysisEditorIntent, PresentationAnalysisEditorModal,
    PresentationAnalysisEditorPending,
};
use crate::presentation::board::figurines::PresentationBoardFigurines;
use crate::presentation::theme::components::{
    PresentationThemeComponents, PresentationThemeEnumButton, PresentationThemeEnumSize,
    PresentationThemeEnumTone,
};
use crate::presentation::theme::tokens::{PresentationThemeColors, PresentationThemeTokens};
use crate::presentation::theme::typography::PresentationThemeTypography;

#[derive(Debug, Clone)]
pub enum PresentationAnalysisVariationTreeEnumEffect {
    Command(PresentationAnalysisEditorIntent),
    Navigate {
        request_id: DesktopRequestId,
        expected: DesktopGameEditorContext,
        selected: GameMoveId,
    },
}

struct PresentationAnalysisVariationTreeRowEffects<'a> {
    action: &'a mut Option<DesktopAnnotationCommandsEnumAction>,
    navigate: &'a mut Option<GameMoveId>,
    invoker: &'a mut Option<egui::Id>,
}

#[derive(Default)]
pub struct PresentationAnalysisVariationTree {
    publication: Option<DesktopGameEditorPublication>,
    source_pending: Option<DesktopAnnotationCommandsEnumPending>,
    pending: Option<PresentationAnalysisEditorPending>,
    tree: Option<DesktopGameEditorTreePage>,
    children: Option<DesktopGameEditorTreeChildrenPage>,
    preview: Option<DesktopAnnotationCommandsVariationPreview>,
    previous_path: Option<DesktopAnnotationCommandsScopePage>,
    selected_path: Option<DesktopAnnotationCommandsScopePage>,
    deleted_nodes: Option<DesktopAnnotationCommandsScopePage>,
    history: PresentationAnalysisEditorHistory,
    modal: PresentationAnalysisEditorModal,
    feedback: Option<String>,
}

impl PresentationAnalysisVariationTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn context(&self) -> Option<DesktopGameEditorContext> {
        self.publication
            .as_ref()
            .map(DesktopGameEditorPublication::context)
    }

    pub const fn tree_page(&self) -> Option<&DesktopGameEditorTreePage> {
        self.tree.as_ref()
    }

    pub const fn children_page(&self) -> Option<&DesktopGameEditorTreeChildrenPage> {
        self.children.as_ref()
    }

    pub const fn preview(&self) -> Option<&DesktopAnnotationCommandsVariationPreview> {
        self.preview.as_ref()
    }

    pub fn scope_page(&self) -> Option<&DesktopAnnotationCommandsScopePage> {
        self.deleted_nodes
            .as_ref()
            .or(self.selected_path.as_ref())
            .or(self.previous_path.as_ref())
    }

    pub fn pending_intent(&self) -> Option<&PresentationAnalysisEditorIntent> {
        self.pending
            .as_ref()
            .map(PresentationAnalysisEditorPending::active_intent)
    }

    pub fn synchronize(
        &mut self,
        publication: &DesktopGameEditorPublication,
        pending: Option<DesktopAnnotationCommandsEnumPending>,
    ) -> bool {
        if let Some(request) = &mut self.pending {
            request.set_service_pending(None);
        }
        self.source_pending = pending;
        if self.context() == Some(publication.context()) {
            return false;
        }
        if self.preview.is_some() {
            self.feedback =
                Some("The game changed. Preview the variation again before confirming.".to_owned());
        }
        self.publication = Some(publication.clone());
        self.tree = None;
        self.children = None;
        self.cancel_preview();
        true
    }

    pub fn request_tree(
        &mut self,
        request_id: DesktopRequestId,
        next: bool,
    ) -> Result<Option<PresentationAnalysisEditorIntent>, PresentationAnalysisEditorError> {
        if self.history.contains(request_id) {
            return Ok(None);
        }
        let expected = self.ready()?;
        let cursor = if next {
            Some(
                self.tree
                    .as_ref()
                    .and_then(DesktopGameEditorTreePage::next_cursor)
                    .ok_or(PresentationAnalysisEditorError::EndOfPages)?,
            )
        } else {
            None
        };
        self.submit(
            request_id,
            expected,
            DesktopAnnotationCommandsEnumAction::TreePage { cursor },
        )
    }

    pub fn request_children(
        &mut self,
        request_id: DesktopRequestId,
        parent: GameMoveId,
        next: bool,
    ) -> Result<Option<PresentationAnalysisEditorIntent>, PresentationAnalysisEditorError> {
        if self.history.contains(request_id) {
            return Ok(None);
        }
        let expected = self.ready()?;
        let cursor = if next {
            let page = self
                .children
                .as_ref()
                .ok_or(PresentationAnalysisEditorError::EndOfPages)?;
            if page.context() != expected || page.parent() != parent {
                return Err(PresentationAnalysisEditorError::StalePage);
            }
            Some(
                page.next_cursor()
                    .ok_or(PresentationAnalysisEditorError::EndOfPages)?,
            )
        } else {
            None
        };
        self.submit(
            request_id,
            expected,
            DesktopAnnotationCommandsEnumAction::ChildrenPage { parent, cursor },
        )
    }

    pub fn preview_variation(
        &mut self,
        request_id: DesktopRequestId,
        request: GameAnnotationsEnumVariationRequest,
    ) -> Result<Option<PresentationAnalysisEditorIntent>, PresentationAnalysisEditorError> {
        if self.history.contains(request_id) {
            return Ok(None);
        }
        let expected = self.ready()?;
        self.cancel_preview();
        self.submit(
            request_id,
            expected,
            DesktopAnnotationCommandsEnumAction::PreviewVariation(request),
        )
    }

    pub fn request_scope(
        &mut self,
        request_id: DesktopRequestId,
        scope: DesktopAnnotationCommandsEnumScope,
        next: bool,
    ) -> Result<Option<PresentationAnalysisEditorIntent>, PresentationAnalysisEditorError> {
        if self.history.contains(request_id) {
            return Ok(None);
        }
        let expected = self.ready()?;
        let preview = self.checked_preview(expected)?;
        let token = preview.token();
        let offset = if next {
            self.page_for_scope(scope)
                .and_then(DesktopAnnotationCommandsScopePage::next)
                .ok_or(PresentationAnalysisEditorError::EndOfPages)?
        } else {
            0
        };
        self.submit(
            request_id,
            expected,
            DesktopAnnotationCommandsEnumAction::VariationScope {
                token,
                scope,
                offset,
            },
        )
    }

    pub fn confirm_variation(
        &mut self,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationAnalysisEditorIntent>, PresentationAnalysisEditorError> {
        if self.history.contains(request_id) {
            return Ok(None);
        }
        let expected = self.ready()?;
        let token = self.checked_preview(expected)?.token();
        self.submit(
            request_id,
            expected,
            DesktopAnnotationCommandsEnumAction::ConfirmVariation { token },
        )
    }

    pub fn append_study_move(
        &mut self,
        request_id: DesktopRequestId,
        expected: DesktopGameEditorContext,
        node: GameMoveId,
        movement: BoardMove,
    ) -> Result<Option<PresentationAnalysisEditorIntent>, PresentationAnalysisEditorError> {
        if self.history.contains(request_id) {
            return Ok(None);
        }
        if self.ready()? != expected {
            return Err(PresentationAnalysisEditorError::Conflict);
        }
        self.cancel_preview();
        self.submit(
            request_id,
            expected,
            DesktopAnnotationCommandsEnumAction::AppendStudyBranch { node, movement },
        )
    }

    pub fn cancel_preview(&mut self) {
        self.preview = None;
        self.previous_path = None;
        self.selected_path = None;
        self.deleted_nodes = None;
    }

    pub fn attach_receipt(
        &mut self,
        intent: &PresentationAnalysisEditorIntent,
        receipt: DesktopApplicationReceipt,
    ) -> Result<(), PresentationAnalysisEditorError> {
        self.pending
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?
            .attach(intent, receipt)
    }

    pub fn observe_result(
        &mut self,
        receipt: &DesktopApplicationReceipt,
        expected: DesktopGameEditorContext,
        result: &DesktopAnnotationCommandsEnumResult,
    ) -> Result<(), PresentationAnalysisEditorError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        pending.check(receipt, expected)?;
        if self.context() != Some(expected) {
            if !matches!(
                result,
                DesktopAnnotationCommandsEnumResult::PendingProjection { .. }
            ) {
                self.finish_pending();
            }
            return Err(PresentationAnalysisEditorError::StalePage);
        }
        match (pending.intent().action(), result) {
            (
                DesktopAnnotationCommandsEnumAction::TreePage { .. },
                DesktopAnnotationCommandsEnumResult::Tree(page),
            ) => {
                if page.context() != expected
                    || page.nodes().len() > DesktopGameEditorTreePage::MAX_ROWS
                {
                    return Err(PresentationAnalysisEditorError::StalePage);
                }
                self.tree = Some(page.clone());
            }
            (
                DesktopAnnotationCommandsEnumAction::ChildrenPage { parent, .. },
                DesktopAnnotationCommandsEnumResult::Children(page),
            ) => {
                if page.context() != expected
                    || page.parent() != *parent
                    || page.nodes().len() > DesktopGameEditorTreePage::MAX_ROWS
                    || page
                        .nodes()
                        .iter()
                        .any(|node| node.parent() != Some(*parent))
                {
                    return Err(PresentationAnalysisEditorError::StalePage);
                }
                self.children = Some(page.clone());
            }
            (
                DesktopAnnotationCommandsEnumAction::PreviewVariation(request),
                DesktopAnnotationCommandsEnumResult::Variation(preview),
            ) => {
                if preview.context() != expected
                    || preview.request() != *request
                    || preview.token() != receipt.request_id()
                {
                    return Err(PresentationAnalysisEditorError::UnexpectedReply);
                }
                self.preview = Some(preview.clone());
                self.modal.refocus();
            }
            (
                DesktopAnnotationCommandsEnumAction::VariationScope {
                    token,
                    scope,
                    offset,
                },
                DesktopAnnotationCommandsEnumResult::Scope(page),
            ) => {
                let preview = self.checked_preview(expected)?;
                let count = Self::scope_count(preview, *scope);
                let end = offset
                    .checked_add(page.nodes().len())
                    .ok_or(PresentationAnalysisEditorError::StalePage)?;
                let next = (end < count).then_some(end);
                if page.context() != expected
                    || page.token() != *token
                    || page.token() != preview.token()
                    || page.scope() != *scope
                    || page.total() != count
                    || page.nodes().len()
                        != count
                            .saturating_sub(*offset)
                            .min(DesktopAnnotationCommandsScopePage::MAX_ROWS)
                    || end > count
                    || page.next() != next
                {
                    return Err(PresentationAnalysisEditorError::StalePage);
                }
                match scope {
                    DesktopAnnotationCommandsEnumScope::PreviousPath => {
                        self.previous_path = Some(page.clone())
                    }
                    DesktopAnnotationCommandsEnumScope::SelectedPath => {
                        self.selected_path = Some(page.clone())
                    }
                    DesktopAnnotationCommandsEnumScope::DeletedNodes => {
                        self.deleted_nodes = Some(page.clone())
                    }
                }
            }
            (
                DesktopAnnotationCommandsEnumAction::AppendStudyBranch { .. }
                | DesktopAnnotationCommandsEnumAction::ConfirmVariation { .. },
                DesktopAnnotationCommandsEnumResult::Edited { snapshot, .. },
            ) => {
                if snapshot.context().game() != expected.game()
                    || snapshot.context().epoch() != expected.epoch()
                {
                    return Err(PresentationAnalysisEditorError::UnexpectedReply);
                }
                let publication = snapshot.clone();
                self.finish_pending();
                self.synchronize(&publication, None);
                self.feedback =
                    Some("Variation updated. Save the study to keep your changes.".to_owned());
                return Ok(());
            }
            (_, DesktopAnnotationCommandsEnumResult::PendingProjection { .. }) => {
                self.feedback = Some("Finishing the change. Please wait.".to_owned());
                return Ok(());
            }
            _ => return Err(PresentationAnalysisEditorError::WrongResult),
        }
        self.finish_pending();
        self.feedback = None;
        Ok(())
    }

    pub fn observe_failure(
        &mut self,
        receipt: &DesktopApplicationReceipt,
        expected: DesktopGameEditorContext,
        failure: &DesktopFailure,
    ) -> Result<(), PresentationAnalysisEditorError> {
        let pending = self
            .pending
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        pending.check(receipt, expected)?;
        pending.fail(failure);
        self.feedback = Some(failure.message().to_owned());
        self.cancel_preview();
        Ok(())
    }

    pub fn dismiss_failure(&mut self) -> Result<(), PresentationAnalysisEditorError> {
        let pending = self
            .pending
            .as_mut()
            .ok_or(PresentationAnalysisEditorError::UnexpectedReply)?;
        if !pending.has_failure() {
            return Err(PresentationAnalysisEditorError::Busy);
        }
        self.feedback = None;
        Ok(())
    }

    pub fn failure(&self) -> Option<&DesktopFailure> {
        self.pending
            .as_ref()
            .and_then(PresentationAnalysisEditorPending::failure)
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        request_id: DesktopRequestId,
    ) -> Result<Option<PresentationAnalysisVariationTreeEnumEffect>, PresentationAnalysisEditorError>
    {
        ui.set_min_width(PresentationThemeTokens::PANEL_WIDTH);
        PresentationThemeComponents::eyebrow(ui, "Variation tree");
        ui.heading("Moves and Variations");
        let Some(publication) = &self.publication else {
            ui.label("Open a game to browse its moves.");
            return Ok(None);
        };
        match publication.state() {
            DesktopGameEditorSnapshotEnumState::Played { .. } => {
                ui.label("Game review");
            }
            DesktopGameEditorSnapshotEnumState::Notation { .. } => {
                ui.label("Recorded game or study");
            }
        }
        if let Some(feedback) = &self.feedback {
            ui.label(feedback);
        }
        if self.source_pending.is_some() {
            ui.label("Updating this game. Move controls will return when it is ready.");
        }
        if self.pending.is_some() {
            ui.label("Waiting for the requested change.");
        }
        let can_act = self.source_pending.is_none() && self.pending.is_none();
        let expected = publication.context();
        let mut action = None;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(can_act, egui::Button::new("Load Moves"))
                .clicked()
            {
                action = Some(DesktopAnnotationCommandsEnumAction::TreePage { cursor: None });
            }
            if let Some(cursor) = self
                .tree
                .as_ref()
                .and_then(DesktopGameEditorTreePage::next_cursor)
                && ui
                    .add_enabled(can_act, egui::Button::new("Next Moves"))
                    .clicked()
            {
                action = Some(DesktopAnnotationCommandsEnumAction::TreePage {
                    cursor: Some(cursor),
                });
            }
        });
        let mut navigate = None;
        let mut invoker = None;
        if let Some(page) = &self.tree {
            if page.nodes().is_empty() {
                ui.label("This page has no moves.");
            }
            egui::ScrollArea::vertical()
                .id_salt("chess.analysis.tree")
                .max_height(320.0)
                .show(ui, |ui| {
                    for node in page.nodes() {
                        Self::show_node(
                            ui,
                            node,
                            expected,
                            can_act,
                            PresentationAnalysisVariationTreeRowEffects {
                                action: &mut action,
                                navigate: &mut navigate,
                                invoker: &mut invoker,
                            },
                        );
                    }
                });
        } else if self.pending.is_none() {
            ui.label("Load moves to browse this game.");
        }
        if let Some(page) = &self.children {
            PresentationThemeComponents::divider(ui);
            PresentationThemeComponents::subhead(ui, "Continuations in saved order");
            if page.nodes().is_empty() {
                ui.label("There are no continuations from this position.");
            }
            egui::ScrollArea::vertical()
                .id_salt("chess.analysis.children")
                .max_height(220.0)
                .show(ui, |ui| {
                    for node in page.nodes() {
                        Self::show_node(
                            ui,
                            node,
                            expected,
                            can_act,
                            PresentationAnalysisVariationTreeRowEffects {
                                action: &mut action,
                                navigate: &mut navigate,
                                invoker: &mut invoker,
                            },
                        );
                    }
                });
            if let Some(cursor) = page.next_cursor()
                && ui
                    .add_enabled(can_act, egui::Button::new("Next Continuations"))
                    .clicked()
            {
                action = Some(DesktopAnnotationCommandsEnumAction::ChildrenPage {
                    parent: page.parent(),
                    cursor: Some(cursor),
                });
            }
        }
        PresentationThemeComponents::caption(ui, "Use the board to add a study move.");
        if self
            .pending
            .as_ref()
            .is_some_and(PresentationAnalysisEditorPending::has_failure)
            && PresentationThemeEnumButton::Secondary
                .show(ui, "Retry Action")
                .clicked()
            && !ui.is_sizing_pass()
        {
            return self
                .retry_pending(request_id)
                .map(|intent| intent.map(PresentationAnalysisVariationTreeEnumEffect::Command));
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.failure().is_some())
            && PresentationThemeEnumButton::Ghost
                .show(ui, "Dismiss Error")
                .clicked()
            && !ui.is_sizing_pass()
        {
            self.dismiss_failure()?;
        }
        let mut cancelled = false;
        let mut modal_focus = std::mem::take(&mut self.modal);
        if let Some(preview) = &self.preview {
            modal_focus.capture_invoker(ui.ctx());
            let modal = PresentationThemeComponents::modal(ui.ctx(), egui::Id::new("chess.analysis.variation_confirmation"))
                .show(ui.ctx(), |ui| {
                    PresentationAnalysisEditorModal::label(ui, "Variation Review");
                    ui.heading(match preview.request() {
                        GameAnnotationsEnumVariationRequest::Promote { .. } => {
                            "Promote this variation?"
                        }
                        GameAnnotationsEnumVariationRequest::Delete { .. } => {
                            "Delete this variation?"
                        }
                    });
                    ui.label(format!(
                        "Previous main line ends at {}.",
                        self.node_label(preview.previous_tip())
                    ));
                    ui.label(format!(
                        "Resulting main line ends at {}.",
                        self.node_label(preview.resulting_tip())
                    ));
                    ui.label(format!(
                        "This removes {} positions, including descendants.",
                        preview.deleted_count()
                    ));
                    if let GameAnnotationsEnumVariationRequest::Delete { root } = preview.request() {
                        ui.label(format!("The branch starts at {}.", self.node_label(root)));
                    }
                    if matches!(
                        preview.request(),
                        GameAnnotationsEnumVariationRequest::Promote { .. }
                    ) {
                        ui.label("The previous main line remains an alternative variation.");
                    }
                    let mut focusable = Vec::new();
                    ui.label("Load move names to read positions on another page. Select a position to review it on the board.");
                    let names = ui.add_enabled(can_act, egui::Button::new("Load Move Names"));
                    if names.enabled() { focusable.push(names.id); }
                    if names.clicked() { action = Some(DesktopAnnotationCommandsEnumAction::TreePage { cursor: None }); }
                    if let Some(cursor) = self.tree.as_ref().and_then(DesktopGameEditorTreePage::next_cursor) {
                        let names = ui.add_enabled(can_act, egui::Button::new("Next Move Names"));
                        if names.enabled() { focusable.push(names.id); }
                        if names.clicked() { action = Some(DesktopAnnotationCommandsEnumAction::TreePage { cursor: Some(cursor) }); }
                    }
                    for scope in [
                        DesktopAnnotationCommandsEnumScope::PreviousPath,
                        DesktopAnnotationCommandsEnumScope::SelectedPath,
                        DesktopAnnotationCommandsEnumScope::DeletedNodes,
                    ] {
                        let count = Self::scope_count(preview, scope);
                        if count == 0 {
                            continue;
                        }
                        ui.label(format!("{}: {} positions", Self::scope_label(scope), count));
                        if let Some(page) = self.page_for_scope(scope) {
                            egui::ScrollArea::vertical()
                                .id_salt(("chess.analysis.scope", Self::scope_label(scope)))
                                .max_height(100.0)
                                .show(ui, |ui| {
                                    for node in page.nodes() {
                                        ui.push_id((Self::scope_label(scope), *node), |ui| {
                                            let position = ui.add_enabled(can_act, egui::Button::new(self.node_label(*node)));
                                            if position.enabled() { focusable.push(position.id); }
                                            if position.clicked() { navigate = Some(*node); }
                                        });
                                    }
                                });
                        }
                        let page = self.page_for_scope(scope);
                        let offset = page.and_then(DesktopAnnotationCommandsScopePage::next);
                        let label = if offset.is_some() {
                            format!("Next {}", Self::scope_label(scope))
                        } else {
                            format!("Show {}", Self::scope_label(scope))
                        };
                        let response = ui.add_enabled(can_act, egui::Button::new(label));
                        if response.enabled() {
                            focusable.push(response.id);
                        }
                        if response.clicked() {
                            action = Some(DesktopAnnotationCommandsEnumAction::VariationScope {
                                token: preview.token(),
                                scope,
                                offset: offset.unwrap_or(0),
                            });
                        }
                    }
                    let cancel =
                        ui.add_enabled(self.pending.is_none(), egui::Button::new("Keep Variation"));
                    if cancel.enabled() {
                        focusable.push(cancel.id);
                    }
                    cancelled = cancel.clicked();
                    let confirm =
                        ui.add_enabled(can_act, egui::Button::new("Confirm Variation Change"));
                    if confirm.enabled() {
                        focusable.push(confirm.id);
                    }
                    if confirm.clicked() {
                        action = Some(DesktopAnnotationCommandsEnumAction::ConfirmVariation {
                            token: preview.token(),
                        });
                    }
                    modal_focus.contain(ui, &focusable);
                });
            cancelled |= modal.should_close() && self.pending.is_none();
        }
        self.modal = modal_focus;
        if self.preview.is_none() {
            self.modal.close(ui.ctx());
        }
        if ui.is_sizing_pass() {
            return Ok(None);
        }
        if cancelled {
            self.cancel_preview();
            self.modal.close(ui.ctx());
            return Ok(None);
        }
        if let Some(selected) = navigate {
            if !self.history.record(request_id) {
                return Ok(None);
            }
            return Ok(Some(
                PresentationAnalysisVariationTreeEnumEffect::Navigate {
                    request_id,
                    expected,
                    selected,
                },
            ));
        }
        if let Some(action) = action {
            if let DesktopAnnotationCommandsEnumAction::PreviewVariation(_) = &action {
                self.modal.open(invoker);
            }
            return self
                .submit(request_id, expected, action)
                .map(|intent| intent.map(PresentationAnalysisVariationTreeEnumEffect::Command));
        }
        Ok(None)
    }

    fn show_node(
        ui: &mut Ui,
        node: &DesktopGameEditorTreeNode,
        expected: DesktopGameEditorContext,
        enabled: bool,
        effects: PresentationAnalysisVariationTreeRowEffects<'_>,
    ) {
        ui.push_id(node.id(), |ui| {
            ui.horizontal_wrapped(|ui| {
                let label = PresentationBoardFigurines::display(
                    ui.ctx(),
                    node.san().unwrap_or("Start"),
                    None,
                );
                let colors = PresentationThemeColors::of(ui.ctx());
                let current = node.id() == expected.selected();
                let text = egui::RichText::new(label)
                    .size(PresentationThemeTypography::UI_POINTS)
                    .color(if current { colors.accent } else { colors.text });
                let text = if current { text.strong() } else { text };
                if ui
                    .add_enabled(
                        enabled,
                        egui::Button::selectable(current, text)
                            .fill(if current {
                                colors.accent_soft
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .corner_radius(egui::CornerRadius::same(
                                PresentationThemeTokens::RADIUS_SM,
                            ))
                            .min_size(egui::vec2(0.0, PresentationThemeTokens::CONTROL_SM)),
                    )
                    .clicked()
                {
                    *effects.navigate = Some(node.id());
                }
                PresentationThemeComponents::caption(
                    ui,
                    format!("{} continuations", node.child_count()),
                );
                if node.has_notes() {
                    PresentationThemeComponents::pill(
                        ui,
                        "Notes",
                        PresentationThemeEnumTone::Accent,
                    );
                }
                if node.has_nags() {
                    PresentationThemeComponents::pill(
                        ui,
                        "NAGs",
                        PresentationThemeEnumTone::Neutral,
                    );
                }
                if node.has_markings() {
                    PresentationThemeComponents::pill(
                        ui,
                        "Drawings",
                        PresentationThemeEnumTone::Neutral,
                    );
                }
                if ui
                    .add_enabled(
                        enabled,
                        PresentationThemeEnumButton::Ghost.widget(
                            ui,
                            "Show Continuations",
                            PresentationThemeEnumSize::Small,
                        ),
                    )
                    .clicked()
                {
                    *effects.action = Some(DesktopAnnotationCommandsEnumAction::ChildrenPage {
                        parent: node.id(),
                        cursor: None,
                    });
                }
                for (label, request) in [
                    (
                        "Preview Promotion",
                        GameAnnotationsEnumVariationRequest::Promote { tip: node.id() },
                    ),
                    (
                        "Preview Deletion",
                        GameAnnotationsEnumVariationRequest::Delete { root: node.id() },
                    ),
                ] {
                    let response = ui.add_enabled(
                        enabled && node.parent().is_some(),
                        PresentationThemeEnumButton::Ghost.widget(
                            ui,
                            label,
                            PresentationThemeEnumSize::Small,
                        ),
                    );
                    if response.clicked() {
                        *effects.action = Some(
                            DesktopAnnotationCommandsEnumAction::PreviewVariation(request),
                        );
                        *effects.invoker = Some(response.id);
                    }
                }
            });
        });
    }

    fn ready(&self) -> Result<DesktopGameEditorContext, PresentationAnalysisEditorError> {
        if self.source_pending.is_some() {
            return Err(PresentationAnalysisEditorError::SourcePending);
        }
        if self.pending.is_some() {
            return Err(PresentationAnalysisEditorError::Busy);
        }
        self.context()
            .ok_or(PresentationAnalysisEditorError::NoSelection)
    }

    fn submit(
        &mut self,
        request_id: DesktopRequestId,
        expected: DesktopGameEditorContext,
        action: DesktopAnnotationCommandsEnumAction,
    ) -> Result<Option<PresentationAnalysisEditorIntent>, PresentationAnalysisEditorError> {
        if self.history.contains(request_id) {
            return Ok(None);
        }
        if self.ready()? != expected {
            return Err(PresentationAnalysisEditorError::Conflict);
        }
        if !self.history.begin(request_id)? {
            return Ok(None);
        }
        let intent = PresentationAnalysisEditorIntent::new(request_id, expected, action);
        self.pending = Some(PresentationAnalysisEditorPending::new(intent.clone()));
        Ok(Some(intent))
    }

    fn finish_pending(&mut self) {
        if let Some(pending) = self.pending.take() {
            self.history.complete(pending.intent().request_id());
            if let Some(retry) = pending.retry_request_id() {
                self.history.complete(retry);
            }
        }
    }

    fn checked_preview(
        &self,
        expected: DesktopGameEditorContext,
    ) -> Result<&DesktopAnnotationCommandsVariationPreview, PresentationAnalysisEditorError> {
        let preview = self
            .preview
            .as_ref()
            .ok_or(PresentationAnalysisEditorError::MissingPreview)?;
        if preview.context() != expected {
            return Err(PresentationAnalysisEditorError::MissingPreview);
        }
        Ok(preview)
    }

    fn page_for_scope(
        &self,
        scope: DesktopAnnotationCommandsEnumScope,
    ) -> Option<&DesktopAnnotationCommandsScopePage> {
        match scope {
            DesktopAnnotationCommandsEnumScope::PreviousPath => self.previous_path.as_ref(),
            DesktopAnnotationCommandsEnumScope::SelectedPath => self.selected_path.as_ref(),
            DesktopAnnotationCommandsEnumScope::DeletedNodes => self.deleted_nodes.as_ref(),
        }
    }

    fn scope_count(
        preview: &DesktopAnnotationCommandsVariationPreview,
        scope: DesktopAnnotationCommandsEnumScope,
    ) -> usize {
        match scope {
            DesktopAnnotationCommandsEnumScope::PreviousPath => preview.previous_path_count(),
            DesktopAnnotationCommandsEnumScope::SelectedPath => preview.selected_path_count(),
            DesktopAnnotationCommandsEnumScope::DeletedNodes => preview.deleted_count(),
        }
    }

    const fn scope_label(scope: DesktopAnnotationCommandsEnumScope) -> &'static str {
        match scope {
            DesktopAnnotationCommandsEnumScope::PreviousPath => "Previous Line",
            DesktopAnnotationCommandsEnumScope::SelectedPath => "Resulting Line",
            DesktopAnnotationCommandsEnumScope::DeletedNodes => "Removed Positions",
        }
    }

    fn node_label(&self, id: GameMoveId) -> String {
        if self
            .publication
            .as_ref()
            .is_some_and(|publication| publication.root() == id)
        {
            return "Start".to_owned();
        }
        self.tree
            .as_ref()
            .into_iter()
            .flat_map(|page| page.nodes())
            .chain(
                self.children
                    .as_ref()
                    .into_iter()
                    .flat_map(|page| page.nodes()),
            )
            .find(|node| node.id() == id)
            .map_or_else(
                || "Position on another page".to_owned(),
                |node| node.san().unwrap_or("Start").to_owned(),
            )
    }
}

#[cfg(test)]
#[path = "../../../tests/presentation/analysis/variation_tree.rs"]
mod tests;
