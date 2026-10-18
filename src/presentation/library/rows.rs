use bevy_egui::egui::Ui;

use crate::board::enums::BoardEnumVariant;
use crate::game::enums::GameEnumMode;
use crate::game::session::GameMetadataDate;
use crate::presentation::library::controller::PresentationLibraryController;
use crate::presentation::theme::icon::{PresentationThemeEnumIcon, PresentationThemeIcon};
use crate::presentation::theme::tokens::PresentationThemeColors;
use crate::storage::library_prepared::StorageLibraryEnumStartKind;
use crate::storage::library_query::StorageLibraryQueryEntry;

pub(super) struct PresentationLibraryRows;

impl PresentationLibraryRows {
    pub(super) fn show(
        ui: &mut Ui,
        row: &StorageLibraryQueryEntry,
        controller: &PresentationLibraryController,
    ) {
        ui.horizontal(|ui| {
            if row.organization().is_favorite() {
                let colors = PresentationThemeColors::of(ui.ctx());
                PresentationThemeIcon::show(
                    ui,
                    PresentationThemeEnumIcon::Star,
                    14.0,
                    colors.accent,
                );
            }
            ui.strong(format!("{} vs {}", Self::white(row), Self::black(row)));
        });
        ui.label(format!(
            "{} | {} | {} half-moves",
            Self::date(row.date()),
            row.result().pgn_token(),
            row.plies()
        ));
        ui.label(format!(
            "{} | {} | {}",
            Self::variant(row.rules_variant()),
            Self::start(row.start_kind()),
            Self::mode(row.mode())
        ));
        ui.label(format!(
            "Opening: {} | Code: {}",
            Self::opening_name(row),
            Self::opening_code(row)
        ));
        match row.modified() {
            Some(modified) => {
                ui.label(format!(
                    "Modified: {} ms since Unix epoch",
                    modified.value()
                ));
            }
            None => {
                ui.label("Modified time unavailable");
            }
        }
        if row.is_partial_date_match() {
            ui.label("Possible date match: saved date is incomplete");
        }
        if row.organization().is_favorite() {
            ui.label("Favorite");
        }
        match row.organization().folder() {
            None => {
                ui.label("Unfiled");
            }
            Some(id) => {
                let catalog = controller.page().map(|page| page.catalog_revision());
                let name = controller
                    .folders()
                    .filter(|page| Some(page.catalog_revision()) == catalog)
                    .and_then(|page| page.entries().iter().find(|folder| folder.id() == id))
                    .map(|folder| folder.name().as_str())
                    .or_else(|| {
                        controller
                            .folder()
                            .filter(|read| {
                                Some(read.catalog_revision()) == catalog && read.folder().id() == id
                            })
                            .map(|read| read.folder().name().as_str())
                    });
                if let Some(name) = name {
                    ui.label(format!("Folder: {name} ({id})"));
                } else {
                    ui.label(format!("Folder name not loaded: {id}"));
                }
            }
        }
        for id in row.organization().tags().ids() {
            let name = controller
                .tags()
                .filter(|page| {
                    controller
                        .page()
                        .is_some_and(|query| query.catalog_revision() == page.catalog_revision())
                })
                .and_then(|page| page.entries().iter().find(|tag| tag.id() == *id))
                .map(|tag| tag.name().as_str());
            if let Some(name) = name {
                ui.label(format!("Tag: {name} ({id})"));
            } else {
                ui.label(format!("Tag name not loaded: {id}"));
            }
        }
        ui.small(format!(
            "Saved game {} | Content revision {}",
            row.id().as_uuid(),
            row.content_revision().value()
        ));
    }

    fn white(row: &StorageLibraryQueryEntry) -> &str {
        row.white()
            .map_or("Unknown White player", |name| name.as_str())
    }

    fn black(row: &StorageLibraryQueryEntry) -> &str {
        row.black()
            .map_or("Unknown Black player", |name| name.as_str())
    }

    fn opening_name(row: &StorageLibraryQueryEntry) -> &str {
        row.opening()
            .map_or("Unavailable", |opening| opening.as_str())
    }

    fn opening_code(row: &StorageLibraryQueryEntry) -> &str {
        row.opening_code()
            .map_or("Unavailable", |code| code.as_str())
    }

    pub(super) fn players(row: &StorageLibraryQueryEntry) -> String {
        format!("{} \u{2013} {}", Self::white(row), Self::black(row))
    }

    pub(super) fn opening(row: &StorageLibraryQueryEntry) -> String {
        match (row.opening_code(), row.opening()) {
            (Some(code), Some(name)) => format!("{}  {}", code.as_str(), name.as_str()),
            (None, Some(name)) => name.as_str().to_owned(),
            (Some(code), None) => code.as_str().to_owned(),
            (None, None) => "No opening".to_owned(),
        }
    }

    pub(super) fn year(row: &StorageLibraryQueryEntry) -> String {
        Self::date(row.date())
    }

    pub(super) fn summary(row: &StorageLibraryQueryEntry) -> String {
        format!(
            "{} \u{b7} {} \u{b7} {} half-moves",
            Self::date(row.date()),
            row.result().pgn_token(),
            row.plies()
        )
    }

    fn date(date: Option<GameMetadataDate>) -> String {
        match date {
            None => "Date unavailable".to_owned(),
            Some(date) => format!(
                "{}.{}.{}",
                date.year()
                    .map_or_else(|| "????".into(), |year| format!("{year:04}")),
                date.month()
                    .map_or_else(|| "??".into(), |month| format!("{month:02}")),
                date.day()
                    .map_or_else(|| "??".into(), |day| format!("{day:02}"))
            ),
        }
    }

    fn variant(variant: BoardEnumVariant) -> &'static str {
        match variant {
            BoardEnumVariant::Standard => "Standard rules",
            BoardEnumVariant::Chess960 => "Chess960 rules",
        }
    }

    fn start(start: StorageLibraryEnumStartKind) -> &'static str {
        match start {
            StorageLibraryEnumStartKind::Standard => "Standard start",
            StorageLibraryEnumStartKind::Chess960 => "Chess960 start",
            StorageLibraryEnumStartKind::Dfrc => "DFRC start",
            StorageLibraryEnumStartKind::Custom => "Custom position",
        }
    }

    fn mode(mode: Option<GameEnumMode>) -> &'static str {
        match mode {
            Some(GameEnumMode::LocalTwoPlayer) => "Local two-player",
            Some(GameEnumMode::HumanVersusEngine) => "Human versus Reckless",
            None => "Opponent mode unavailable",
        }
    }
}
