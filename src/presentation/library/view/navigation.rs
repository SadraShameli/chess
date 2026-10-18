use bevy_egui::egui::{self, Ui};

use crate::desktop::identity::DesktopRequestId;
use crate::game::library_organization::GameLibraryOrganizationFolderId;
use crate::presentation::library::enums::PresentationLibraryEnumLane;
use crate::presentation::library::error::PresentationLibraryError;
use crate::presentation::library::view::{PresentationLibraryEnumEffect, PresentationLibraryPanel};
use crate::storage::library_metadata::StorageLibraryMetadataEnumRequest;
use crate::storage::library_query::StorageLibraryEnumFolderFilter;
use crate::storage::library_repository::StorageLibraryCatalogRevision;

impl PresentationLibraryPanel {
    pub(super) fn organizations(
        &mut self,
        ui: &mut Ui,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        ui.collapsing("Folder filters", |ui| self.folders(ui, effects));
        ui.collapsing("Tag filters", |ui| self.tags(ui, effects));
    }

    fn folders(&mut self, ui: &mut Ui, effects: &mut Vec<PresentationLibraryEnumEffect>) {
        ui.selectable_value(
            self.filters.folder_filter(),
            StorageLibraryEnumFolderFilter::Any,
            "All folders",
        );
        ui.selectable_value(
            self.filters.folder_filter(),
            StorageLibraryEnumFolderFilter::Unfiled,
            "Unfiled only",
        );
        let pending = self
            .controller
            .is_pending(PresentationLibraryEnumLane::Folders)
            || self
                .controller
                .is_pending(PresentationLibraryEnumLane::Folder);
        if pending {
            ui.spinner();
            ui.label("Loading folders");
        }
        let roots = ui.add_enabled(!pending, egui::Button::new("Browse root folders"));
        if self.frame.clicked(ui, &roots) {
            self.browse(None, None, effects);
        }
        if let Some(id) = self.controller.folder_parent() {
            let detail = self
                .controller
                .folder()
                .filter(|read| read.folder().id() == id);
            if let Some(detail) = detail {
                ui.label(format!(
                    "Browsing {} ({id})",
                    detail.folder().name().as_str()
                ));
                let parent = detail.folder().parent();
                let catalog = detail.catalog_revision();
                let up = ui.add_enabled(!pending, egui::Button::new("Up one folder"));
                if self.frame.clicked(ui, &up) {
                    self.browse(parent, Some(catalog), effects);
                }
            } else {
                ui.label(format!("Folder name and parent not loaded: {id}"));
            }
            ui.selectable_value(
                self.filters.folder_filter(),
                StorageLibraryEnumFolderFilter::Exact(id),
                "Filter this folder only",
            );
            ui.selectable_value(
                self.filters.folder_filter(),
                StorageLibraryEnumFolderFilter::Subtree(id),
                "Filter this folder and descendants",
            );
        }
        let Some(page) = self.controller.folders() else {
            ui.label("Folder metadata is not loaded");
            return;
        };
        if page.entries().is_empty() {
            ui.label("No child folders on this page");
        }
        let catalog = page.catalog_revision();
        let cursor = page.next_cursor().copied();
        let mut browse = None;
        let mut removal = None;
        for folder in page.entries() {
            ui.push_id(folder.id().as_uuid(), |ui| {
                ui.label(format!("{} ({})", folder.name().as_str(), folder.id()));
                ui.selectable_value(
                    self.filters.folder_filter(),
                    StorageLibraryEnumFolderFilter::Exact(folder.id()),
                    "Filter this folder",
                );
                let child = ui.add_enabled(!pending, egui::Button::new("Browse children"));
                if self.frame.clicked(ui, &child) {
                    browse = Some(folder.id());
                }
                let remove = ui.add_enabled(!pending, egui::Button::new("Preview folder removal"));
                if self.frame.clicked(ui, &remove) {
                    removal = Some(folder.id());
                }
            });
        }
        if let Some(id) = browse {
            self.browse(Some(id), Some(catalog), effects);
        }
        if let Some(id) = removal {
            self.request_folder_removal(id, effects);
        }
        let next = ui.add_enabled(
            cursor.is_some() && !pending,
            egui::Button::new("Next folder page"),
        );
        if self.frame.clicked(ui, &next)
            && let Some(cursor) = cursor
        {
            let result = self.controller.metadata(
                DesktopRequestId::new(),
                StorageLibraryMetadataEnumRequest::Folders {
                    parent: self.controller.folder_parent(),
                    cursor: Some(cursor),
                },
            );
            self.collect(result, effects);
        }
        if let StorageLibraryEnumFolderFilter::Exact(id)
        | StorageLibraryEnumFolderFilter::Subtree(id) = self.filters.folder_filter()
        {
            ui.label(format!("Selected folder identity: {id}"));
        }
    }

    fn browse(
        &mut self,
        parent: Option<GameLibraryOrganizationFolderId>,
        catalog: Option<StorageLibraryCatalogRevision>,
        effects: &mut Vec<PresentationLibraryEnumEffect>,
    ) {
        if self
            .controller
            .is_pending(PresentationLibraryEnumLane::Folders)
            || self
                .controller
                .is_pending(PresentationLibraryEnumLane::Folder)
        {
            self.input_error = Some(PresentationLibraryError::Busy);
            return;
        }
        let result = self.controller.metadata(
            DesktopRequestId::new(),
            StorageLibraryMetadataEnumRequest::Folders {
                parent,
                cursor: None,
            },
        );
        self.collect(result, effects);
        if let (Some(id), Some(catalog)) = (parent, catalog) {
            let result = self.controller.metadata(
                DesktopRequestId::new(),
                StorageLibraryMetadataEnumRequest::Folder { id, catalog },
            );
            self.collect(result, effects);
        }
    }

    fn tags(&mut self, ui: &mut Ui, effects: &mut Vec<PresentationLibraryEnumEffect>) {
        let pending = self
            .controller
            .is_pending(PresentationLibraryEnumLane::Tags);
        if pending {
            ui.spinner();
            ui.label("Loading tags");
        }
        let reload = ui.add_enabled(!pending, egui::Button::new("Reload tags from first page"));
        if self.frame.clicked(ui, &reload) {
            let result = self.controller.metadata(
                DesktopRequestId::new(),
                StorageLibraryMetadataEnumRequest::Tags { cursor: None },
            );
            self.collect(result, effects);
        }
        if let Err(error) = self.filters.show_tag_match(ui) {
            self.input_error = Some(error);
        }
        if let Some(page) = self.controller.tags() {
            let cursor = page.next_cursor().copied();
            for tag in page.entries() {
                let selected = self.filters.tags().contains(&tag.id());
                let toggle = ui
                    .selectable_label(selected, format!("{} ({})", tag.name().as_str(), tag.id()));
                if self.frame.clicked(ui, &toggle)
                    && let Err(error) = self.filters.toggle_tag(tag.id())
                {
                    self.input_error = Some(error);
                }
            }
            if page.entries().is_empty() {
                ui.label("No tags on this page");
            }
            let next = ui.add_enabled(
                cursor.is_some() && !pending,
                egui::Button::new("Next tag page"),
            );
            if self.frame.clicked(ui, &next)
                && let Some(cursor) = cursor
            {
                let result = self.controller.metadata(
                    DesktopRequestId::new(),
                    StorageLibraryMetadataEnumRequest::Tags {
                        cursor: Some(cursor),
                    },
                );
                self.collect(result, effects);
            }
        } else {
            ui.label("Tag metadata is not loaded");
        }
        let mut remove = None;
        for id in self.filters.tags() {
            let name = self
                .controller
                .tags()
                .and_then(|page| page.entries().iter().find(|tag| tag.id() == *id))
                .map(|tag| tag.name().as_str());
            ui.push_id(("selected-tag", id.as_uuid()), |ui| {
                if let Some(name) = name {
                    ui.label(format!("Selected tag: {name} ({id})"));
                } else {
                    ui.label(format!("Selected tag name not loaded: {id}"));
                }
                let deselect = ui.button("Remove tag filter");
                if self.frame.clicked(ui, &deselect) {
                    remove = Some(*id);
                }
            });
        }
        if let Some(id) = remove
            && let Err(error) = self.filters.toggle_tag(id)
        {
            self.input_error = Some(error);
        }
    }
}
