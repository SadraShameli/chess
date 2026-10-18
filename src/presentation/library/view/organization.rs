use bevy_egui::egui::Ui;

use crate::desktop::identity::DesktopRequestId;
use crate::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationError,
    GameLibraryOrganizationFolderId, GameLibraryOrganizationFolderTarget,
    GameLibraryOrganizationGameTarget, GameLibraryOrganizationName, GameLibraryOrganizationTagId,
    GameLibraryOrganizationTagSet,
};
use crate::presentation::library::controller::PresentationLibraryController;
use crate::presentation::library::enums::PresentationLibraryEnumLane;
use crate::presentation::library::error::PresentationLibraryError;
use crate::presentation::library::view::{
    PresentationLibraryEnumEffect, PresentationLibraryFrame, PresentationLibraryPanel,
};
use crate::presentation::theme::components::{
    PresentationThemeEnumButton, PresentationThemeEnumSize,
};
use crate::storage::library_query::StorageLibraryQueryEntry;

impl PresentationLibraryPanel {
    pub(super) fn organization_creation(
        &mut self,
        ui: &mut Ui,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        let busy = self
            .controller
            .is_pending(PresentationLibraryEnumLane::Organize);
        ui.horizontal(|ui| {
            let label = if self.controller.folder_parent().is_some() {
                "New folder inside the browsed folder"
            } else {
                "New folder"
            };
            ui.label(label);
            ui.text_edit_singleline(&mut self.new_folder_name);
            let create = ui.add_enabled(
                !busy,
                PresentationThemeEnumButton::Secondary.widget(
                    ui,
                    "Create folder",
                    PresentationThemeEnumSize::Small,
                ),
            );
            if self.frame.clicked(ui, &create) {
                let result = self
                    .folder_parent_target()
                    .and_then(|parent| Self::folder_mutation(&self.new_folder_name, parent))
                    .and_then(|mutation| {
                        self.controller.organize(DesktopRequestId::new(), mutation)
                    });
                if result.is_ok() {
                    self.new_folder_name.clear();
                }
                self.collect(result, effects);
            }
        });
        ui.horizontal(|ui| {
            ui.label("New tag");
            ui.text_edit_singleline(&mut self.new_tag_name);
            let create = ui.add_enabled(
                !busy,
                PresentationThemeEnumButton::Secondary.widget(
                    ui,
                    "Create tag",
                    PresentationThemeEnumSize::Small,
                ),
            );
            if self.frame.clicked(ui, &create) {
                let result = Self::tag_mutation(&self.new_tag_name).and_then(|mutation| {
                    self.controller.organize(DesktopRequestId::new(), mutation)
                });
                if result.is_ok() {
                    self.new_tag_name.clear();
                }
                self.collect(result, effects);
            }
        });
    }

    fn folder_parent_target(
        &self,
    ) -> Result<Option<GameLibraryOrganizationFolderTarget>, PresentationLibraryError> {
        let Some(parent) = self.controller.folder_parent() else {
            return Ok(None);
        };
        let read = self
            .controller
            .folder()
            .filter(|read| read.folder().id() == parent)
            .ok_or(PresentationLibraryError::ParentNotLoaded)?;
        Ok(Some(GameLibraryOrganizationFolderTarget::new(
            parent,
            read.folder().revision(),
        )))
    }

    fn folder_mutation(
        name: &str,
        parent: Option<GameLibraryOrganizationFolderTarget>,
    ) -> Result<GameLibraryOrganizationEnumMutation, PresentationLibraryError> {
        let id: GameLibraryOrganizationFolderId = uuid::Uuid::new_v4().to_string().parse()?;
        Ok(GameLibraryOrganizationEnumMutation::CreateFolder(
            id,
            parent,
            GameLibraryOrganizationName::new(name.to_owned())?,
        ))
    }

    fn tag_mutation(
        name: &str,
    ) -> Result<GameLibraryOrganizationEnumMutation, PresentationLibraryError> {
        let id: GameLibraryOrganizationTagId = uuid::Uuid::new_v4().to_string().parse()?;
        Ok(GameLibraryOrganizationEnumMutation::CreateTag(
            id,
            GameLibraryOrganizationName::new(name.to_owned())?,
        ))
    }

    pub(super) fn row_organization(
        ui: &mut Ui,
        frame: &mut PresentationLibraryFrame,
        controller: &PresentationLibraryController,
        row: &StorageLibraryQueryEntry,
        enabled: bool,
    ) -> Result<Option<GameLibraryOrganizationEnumMutation>, GameLibraryOrganizationError> {
        let catalog = controller.page().map(|page| page.catalog_revision());
        let target =
            GameLibraryOrganizationGameTarget::new(row.id(), row.organization().revision());
        let mut chosen = None;
        let mut failure = None;
        if let Some(folders) = controller
            .folders()
            .filter(|page| Some(page.catalog_revision()) == catalog)
        {
            ui.horizontal_wrapped(|ui| {
                for folder in folders.entries() {
                    let current = row.organization().folder() == Some(folder.id());
                    let label = if current {
                        format!("In folder {}", folder.name().as_str())
                    } else {
                        format!("Move to folder {}", folder.name().as_str())
                    };
                    let button = ui.add_enabled(
                        enabled && !current,
                        PresentationThemeEnumButton::Secondary.widget(
                            ui,
                            label,
                            PresentationThemeEnumSize::Small,
                        ),
                    );
                    if frame.clicked(ui, &button) {
                        chosen = Some(GameLibraryOrganizationEnumMutation::MoveToFolder(
                            target,
                            Some(GameLibraryOrganizationFolderTarget::new(
                                folder.id(),
                                folder.revision(),
                            )),
                        ));
                    }
                }
            });
        }
        if row.organization().folder().is_some() {
            let unfile = ui.add_enabled(
                enabled,
                PresentationThemeEnumButton::Secondary.widget(
                    ui,
                    "Move to Unfiled",
                    PresentationThemeEnumSize::Small,
                ),
            );
            if frame.clicked(ui, &unfile) {
                chosen = Some(GameLibraryOrganizationEnumMutation::MoveToFolder(
                    target, None,
                ));
            }
        }
        if let Some(tags) = controller
            .tags()
            .filter(|page| Some(page.catalog_revision()) == catalog)
        {
            ui.horizontal_wrapped(|ui| {
                for tag in tags.entries() {
                    let held = row.organization().tags().ids().contains(&tag.id());
                    let label = if held {
                        format!("Remove tag {}", tag.name().as_str())
                    } else {
                        format!("Add tag {}", tag.name().as_str())
                    };
                    let button = ui.add_enabled(
                        enabled,
                        PresentationThemeEnumButton::Secondary.widget(
                            ui,
                            label,
                            PresentationThemeEnumSize::Small,
                        ),
                    );
                    if frame.clicked(ui, &button) {
                        let mut ids: Vec<GameLibraryOrganizationTagId> = row
                            .organization()
                            .tags()
                            .ids()
                            .iter()
                            .copied()
                            .filter(|id| *id != tag.id())
                            .collect();
                        if !held {
                            ids.push(tag.id());
                        }
                        match GameLibraryOrganizationTagSet::new(ids) {
                            Ok(set) => {
                                chosen = Some(GameLibraryOrganizationEnumMutation::ReplaceTags(
                                    target, set,
                                ));
                            }
                            Err(error) => failure = Some(error),
                        }
                    }
                }
            });
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(chosen),
        }
    }
}
