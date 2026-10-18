use bevy_egui::egui::{self, Ui};

use crate::board::enums::{BoardEnumColor, BoardEnumVariant};
use crate::game::enums::{GameEnumMode, GameEnumResult};
use crate::game::library_organization::{
    GameLibraryOrganizationTagId, GameLibraryOrganizationTagSet,
};
use crate::game::session::{GameMetadataDate, GameMetadataOpeningCode, GameMetadataText};
use crate::notation::game_identity::NotationGameIdentityEnumResult;
use crate::presentation::library::error::PresentationLibraryError;
use crate::presentation::theme::components::PresentationThemeComponents;
use crate::presentation::theme::tokens::PresentationThemeColors;
use crate::presentation::theme::typography::PresentationThemeTypography;
use crate::storage::library_prepared::StorageLibraryEnumStartKind;
use crate::storage::library_query::{
    StorageLibraryDateInterval, StorageLibraryEnumFilter, StorageLibraryEnumFolderFilter,
    StorageLibraryEnumSort, StorageLibraryEnumTagFilter, StorageLibraryPlyInterval,
    StorageLibraryQueryOptions,
};

#[derive(Default)]
pub(super) struct PresentationLibraryFilters {
    query: String,
    white: String,
    black: String,
    date_from: String,
    date_to: String,
    opening: String,
    code: String,
    plies_from: String,
    plies_to: String,
    options: StorageLibraryQueryOptions,
}

impl PresentationLibraryFilters {
    const TEXT_CHARACTERS: usize = 256;
    const DATE_CHARACTERS: usize = 10;
    const CODE_CHARACTERS: usize = 3;
    const PLY_CHARACTERS: usize = 6;
    const SEARCH_WIDTH: f32 = 420.0;

    const fn results() -> [(NotationGameIdentityEnumResult, &'static str); 4] {
        [
            (NotationGameIdentityEnumResult::Unfinished, "Unfinished"),
            (
                NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(
                    BoardEnumColor::White,
                )),
                "White won",
            ),
            (
                NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(
                    BoardEnumColor::Black,
                )),
                "Black won",
            ),
            (
                NotationGameIdentityEnumResult::Completed(GameEnumResult::Draw),
                "Draw",
            ),
        ]
    }

    const fn sorts() -> [(StorageLibraryEnumSort, &'static str); 6] {
        [
            (
                StorageLibraryEnumSort::UpdatedDescending,
                "Recently modified",
            ),
            (StorageLibraryEnumSort::UpdatedAscending, "Oldest modified"),
            (StorageLibraryEnumSort::DateDescending, "Newest game date"),
            (StorageLibraryEnumSort::DateAscending, "Oldest game date"),
            (StorageLibraryEnumSort::PliesDescending, "Longest game"),
            (StorageLibraryEnumSort::PliesAscending, "Shortest game"),
        ]
    }

    pub(super) fn search(&mut self, ui: &mut Ui) -> bool {
        ui.horizontal(|ui| {
            let field_label = PresentationThemeComponents::field_label(ui, "Search players");
            ui.set_max_width(ui.available_width().min(Self::SEARCH_WIDTH));
            let response = PresentationThemeComponents::search_field(
                ui,
                &mut self.query,
                "Search library",
                "/",
            )
            .labelled_by(field_label.id);
            if ui.input(|input| input.key_pressed(egui::Key::Slash)) && !response.has_focus() {
                response.request_focus();
            }
            response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
        })
        .inner
    }

    pub(super) fn quick(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        ui.menu_button("Result", |ui| {
            changed |= ui
                .selectable_value(
                    &mut self.options.result,
                    StorageLibraryEnumFilter::Any,
                    "Any result",
                )
                .clicked();
            for (value, label) in Self::results() {
                changed |= ui
                    .selectable_value(
                        &mut self.options.result,
                        StorageLibraryEnumFilter::Value(value),
                        label,
                    )
                    .clicked();
            }
        });
        ui.menu_button("Opening", |ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.opening)
                    .id_salt("library-quick-opening")
                    .char_limit(Self::TEXT_CHARACTERS)
                    .hint_text("Opening name contains"),
            );
            let submitted =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if submitted || ui.button("Apply opening filter").clicked() {
                changed = true;
                ui.close();
            }
        });
        ui.menu_button("Date", |ui| {
            for (sort, label) in Self::sorts() {
                if matches!(
                    sort,
                    StorageLibraryEnumSort::DateDescending | StorageLibraryEnumSort::DateAscending
                ) {
                    changed |= ui
                        .selectable_value(&mut self.options.sort, sort, label)
                        .clicked();
                }
            }
        });
        changed
    }

    pub(super) fn chips(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            if let StorageLibraryEnumFilter::Value(result) = &self.options.result {
                let label = Self::results()
                    .into_iter()
                    .find(|(value, _)| value == result)
                    .map_or("Unavailable selection", |(_, label)| label);
                if Self::chip(ui, &format!("Result: {label}")) {
                    self.options.result = StorageLibraryEnumFilter::Any;
                    changed = true;
                }
            }
            if self.options.favorite == StorageLibraryEnumFilter::Value(true)
                && Self::chip(ui, "Favorites only")
            {
                self.options.favorite = StorageLibraryEnumFilter::Any;
                changed = true;
            }
            if self.options.folder != StorageLibraryEnumFolderFilter::Any
                && Self::chip(ui, "Folder filter")
            {
                self.options.folder = StorageLibraryEnumFolderFilter::Any;
                changed = true;
            }
            let tags = self.tags().len();
            if tags > 0 && Self::chip(ui, &format!("Tags: {tags}")) {
                self.options.tags = StorageLibraryEnumTagFilter::Any;
                changed = true;
            }
            if !self.opening.trim().is_empty()
                && Self::chip(ui, &format!("Opening: {}", self.opening.trim()))
            {
                self.opening.clear();
                changed = true;
            }
        });
        changed
    }

    fn chip(ui: &mut Ui, text: &str) -> bool {
        let colors = PresentationThemeColors::of(ui.ctx());
        let response = ui.add(
            egui::Button::new(
                egui::RichText::new(format!("{text}  \u{d7}"))
                    .size(PresentationThemeTypography::CAPTION_POINTS)
                    .color(colors.accent),
            )
            .fill(colors.accent_soft)
            .stroke(egui::Stroke::NONE)
            .corner_radius(egui::CornerRadius::same(u8::MAX)),
        );
        response.clicked()
    }

    pub(super) fn show_all_games(&mut self) {
        self.options.favorite = StorageLibraryEnumFilter::Any;
        self.options.folder = StorageLibraryEnumFolderFilter::Any;
        self.options.tags = StorageLibraryEnumTagFilter::Any;
    }

    pub(super) fn show_favorites(&mut self) {
        self.show_all_games();
        self.options.favorite = StorageLibraryEnumFilter::Value(true);
    }

    pub(super) fn show(&mut self, ui: &mut Ui) {
        ui.collapsing("Game filters", |ui| {
            Self::text(ui, "White player", &mut self.white, Self::TEXT_CHARACTERS);
            Self::text(ui, "Black player", &mut self.black, Self::TEXT_CHARACTERS);
            Self::text(
                ui,
                "Date from (YYYY-MM-DD)",
                &mut self.date_from,
                Self::DATE_CHARACTERS,
            );
            Self::text(
                ui,
                "Date to (YYYY-MM-DD)",
                &mut self.date_to,
                Self::DATE_CHARACTERS,
            );
            ui.label("Partial saved dates may match. Matching rows identify incomplete dates.");
            Self::filter(ui, "Result", &mut self.options.result, &Self::results());
            Self::filter(
                ui,
                "Rules variant",
                &mut self.options.rules_variant,
                &[
                    (BoardEnumVariant::Standard, "Standard"),
                    (BoardEnumVariant::Chess960, "Chess960"),
                ],
            );
            Self::filter(
                ui,
                "Starting position",
                &mut self.options.start_kind,
                &[
                    (StorageLibraryEnumStartKind::Standard, "Standard start"),
                    (StorageLibraryEnumStartKind::Chess960, "Chess960 start"),
                    (StorageLibraryEnumStartKind::Dfrc, "DFRC start"),
                    (StorageLibraryEnumStartKind::Custom, "Custom position"),
                ],
            );
            Self::filter(
                ui,
                "Opponent mode",
                &mut self.options.mode,
                &[
                    (GameEnumMode::LocalTwoPlayer, "Local two-player"),
                    (GameEnumMode::HumanVersusEngine, "Human versus Reckless"),
                ],
            );
            Self::filter(
                ui,
                "Favorites",
                &mut self.options.favorite,
                &[(true, "Favorites only"), (false, "Without favorite")],
            );
            Self::text(ui, "Opening name", &mut self.opening, Self::TEXT_CHARACTERS);
            Self::text(
                ui,
                "Opening code (A00 to E99)",
                &mut self.code,
                Self::CODE_CHARACTERS,
            );
            Self::text(
                ui,
                "Length from (half-moves)",
                &mut self.plies_from,
                Self::PLY_CHARACTERS,
            );
            Self::text(
                ui,
                "Length to (half-moves)",
                &mut self.plies_to,
                Self::PLY_CHARACTERS,
            );
        });
        let sorts = Self::sorts();
        let label = sorts
            .iter()
            .find(|(sort, _)| *sort == self.options.sort)
            .map_or("Sort unavailable", |(_, label)| *label);
        egui::ComboBox::from_label("Sort")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for (sort, label) in sorts {
                    ui.selectable_value(&mut self.options.sort, sort, label);
                }
            });
    }

    fn text(ui: &mut Ui, label: &str, value: &mut String, limit: usize) {
        ui.horizontal(|ui| {
            let field_label = ui.label(label);
            ui.add(
                egui::TextEdit::singleline(value)
                    .id_salt(label)
                    .char_limit(limit)
                    .hint_text(label),
            )
            .labelled_by(field_label.id);
        });
    }

    fn filter<T: Clone + PartialEq>(
        ui: &mut Ui,
        label: &str,
        selected: &mut StorageLibraryEnumFilter<T>,
        values: &[(T, &str)],
    ) {
        let text = match selected {
            StorageLibraryEnumFilter::Any => "Any",
            StorageLibraryEnumFilter::Value(selected) => values
                .iter()
                .find(|(value, _)| value == selected)
                .map_or("Unavailable selection", |(_, label)| *label),
        };
        egui::ComboBox::from_label(label)
            .selected_text(text)
            .show_ui(ui, |ui| {
                ui.selectable_value(selected, StorageLibraryEnumFilter::Any, "Any");
                for (value, label) in values {
                    ui.selectable_value(
                        selected,
                        StorageLibraryEnumFilter::Value(value.clone()),
                        *label,
                    );
                }
            });
    }

    pub(super) fn options(&self) -> Result<StorageLibraryQueryOptions, PresentationLibraryError> {
        let mut options = self.options.clone();
        options.either_player = Self::text_filter(&self.query)?;
        options.white = Self::text_filter(&self.white)?;
        options.black = Self::text_filter(&self.black)?;
        options.opening_name = Self::text_filter(&self.opening)?;
        options.opening_code = if self.code.trim().is_empty() {
            StorageLibraryEnumFilter::Any
        } else {
            StorageLibraryEnumFilter::Value(GameMetadataOpeningCode::new(
                self.code.trim().to_ascii_uppercase(),
            )?)
        };
        options.date = if self.date_from.trim().is_empty() && self.date_to.trim().is_empty() {
            StorageLibraryEnumFilter::Any
        } else {
            StorageLibraryEnumFilter::Value(StorageLibraryDateInterval::new(
                Self::date(&self.date_from)?,
                Self::date(&self.date_to)?,
            )?)
        };
        options.plies = if self.plies_from.trim().is_empty() && self.plies_to.trim().is_empty() {
            StorageLibraryEnumFilter::Any
        } else if self.plies_from.trim().is_empty() || self.plies_to.trim().is_empty() {
            return Err(PresentationLibraryError::PlyRange);
        } else {
            StorageLibraryEnumFilter::Value(StorageLibraryPlyInterval::new(
                self.plies_from.trim().parse()?,
                self.plies_to.trim().parse()?,
            )?)
        };
        Ok(options)
    }

    fn text_filter(
        value: &str,
    ) -> Result<StorageLibraryEnumFilter<GameMetadataText>, PresentationLibraryError> {
        if value.trim().is_empty() {
            Ok(StorageLibraryEnumFilter::Any)
        } else {
            Ok(StorageLibraryEnumFilter::Value(GameMetadataText::new(
                value.trim().to_owned(),
            )?))
        }
    }

    fn date(value: &str) -> Result<GameMetadataDate, PresentationLibraryError> {
        let mut parts = value.trim().split('-');
        let (Some(year), Some(month), Some(day), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(PresentationLibraryError::DateRange);
        };
        if year.len() != 4 || month.len() != 2 || day.len() != 2 {
            return Err(PresentationLibraryError::DateRange);
        }
        Ok(GameMetadataDate::new(
            Some(year.parse()?),
            Some(month.parse()?),
            Some(day.parse()?),
        )?)
    }

    pub(super) fn folder_filter(&mut self) -> &mut StorageLibraryEnumFolderFilter {
        &mut self.options.folder
    }

    pub(super) fn tags(&self) -> &[GameLibraryOrganizationTagId] {
        match &self.options.tags {
            StorageLibraryEnumTagFilter::Any => &[],
            StorageLibraryEnumTagFilter::AnyOf(tags) | StorageLibraryEnumTagFilter::AllOf(tags) => {
                tags.ids()
            }
        }
    }

    pub(super) fn toggle_tag(
        &mut self,
        id: GameLibraryOrganizationTagId,
    ) -> Result<(), PresentationLibraryError> {
        let mut ids = self.tags().to_vec();
        if let Some(index) = ids.iter().position(|selected| *selected == id) {
            ids.remove(index);
        } else {
            ids.push(id);
        }
        let all = matches!(self.options.tags, StorageLibraryEnumTagFilter::AllOf(_));
        self.set_tags(ids, all)
    }

    fn set_tags(
        &mut self,
        ids: Vec<GameLibraryOrganizationTagId>,
        all: bool,
    ) -> Result<(), PresentationLibraryError> {
        self.options.tags = if ids.is_empty() {
            StorageLibraryEnumTagFilter::Any
        } else {
            let tags = GameLibraryOrganizationTagSet::new(ids)?;
            if all {
                StorageLibraryEnumTagFilter::AllOf(tags)
            } else {
                StorageLibraryEnumTagFilter::AnyOf(tags)
            }
        };
        Ok(())
    }

    pub(super) fn show_tag_match(&mut self, ui: &mut Ui) -> Result<(), PresentationLibraryError> {
        let mut all = matches!(self.options.tags, StorageLibraryEnumTagFilter::AllOf(_));
        if ui.checkbox(&mut all, "Require all selected tags").changed() {
            self.set_tags(self.tags().to_vec(), all)?;
        }
        Ok(())
    }
}
