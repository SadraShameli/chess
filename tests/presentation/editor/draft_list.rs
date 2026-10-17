use bevy_egui::egui;
use egui_kittest::{Harness, kittest::Queryable};

use crate::board::editor::BoardEditorSessionEnumOperation;
use crate::desktop::editor_commands::tests::fixture::{
    DesktopEditorCommandsFixture, DesktopEditorCommandsTestResult,
};
use crate::desktop::editor_commands::{
    DesktopEditorCommandsEnumOperation, DesktopEditorCommandsEnumResult, DesktopEditorCommandsPage,
};
use crate::presentation::editor::draft_list::{
    PresentationEditorDraftList, PresentationEditorDraftListEnumAction,
};
use crate::presentation::library::positions::PresentationLibraryPositions;

struct PresentationEditorDraftListFixture {
    page: DesktopEditorCommandsPage,
    editor: PresentationEditorDraftList,
    library: PresentationLibraryPositions,
    is_library: bool,
    actions: Vec<PresentationEditorDraftListEnumAction>,
    needs_fonts: bool,
}

impl PresentationEditorDraftListFixture {
    fn show(ui: &mut egui::Ui, state: &mut Self) {
        if state.needs_fonts {
            ui.ctx()
                .set_fonts(crate::presentation::theme::font::PresentationThemeFont::definitions());
            ui.ctx().request_repaint();
            state.needs_fonts = false;
            return;
        }
        let action = if state.is_library {
            state.library.show(ui, state.page.page(), None)
        } else {
            state.editor.show(ui, state.page.page(), None)
        };
        if let Some(action) = action {
            state.actions.push(action);
            ui.ctx()
                .request_discard("Verify saved draft action is emitted once");
        }
    }

    fn harness(
        page: DesktopEditorCommandsPage,
        is_library: bool,
    ) -> DesktopEditorCommandsTestResult<Harness<'static, Self>> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1400.0, 900.0))
            .build_ui_state(
                Self::show,
                Self {
                    page,
                    editor: PresentationEditorDraftList::new(),
                    library: PresentationLibraryPositions::new(),
                    is_library,
                    actions: Vec::new(),
                    needs_fonts: true,
                },
            );
        harness.run_ok().ok_or("draft list did not settle")?;
        Ok(harness)
    }
}

#[tokio::test]
async fn editor_and_library_borrow_the_same_repository_page_and_open_the_same_invalid_draft()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    fixture
        .edit(BoardEditorSessionEnumOperation::Edit(
            crate::board::editor::BoardEditorSessionEnumEdit::Counters {
                halfmoves: -1,
                fullmoves: 0,
            },
        ))
        .await?;
    let id = fixture.snapshot()?.context().draft();
    fixture.save().await?;
    fixture.owner.finish_write().await?;
    let DesktopEditorCommandsEnumResult::List(page) = fixture
        .dispatch(DesktopEditorCommandsEnumOperation::List { after: None })
        .await?
    else {
        return Err("missing list".into());
    };
    fixture.close().await?;
    let mut editor = PresentationEditorDraftListFixture::harness(page.clone(), false)?;
    let mut library = PresentationEditorDraftListFixture::harness(page, true)?;
    let same_page = std::ptr::eq(editor.state().page.page(), library.state().page.page());
    for harness in [&mut editor, &mut library] {
        assert!(harness.query_by_label("Invalid position draft").is_some());
        harness
            .get_by_label(&format!("Open draft {id}"))
            .click_accesskit();
        harness.run_ok().ok_or("open draft did not settle")?;
        harness.run_steps(3);
        assert_eq!(
            harness.state().actions,
            [PresentationEditorDraftListEnumAction::Open(id)]
        );
    }
    assert!(same_page);
    Ok(())
}

#[tokio::test]
async fn delete_needs_confirmation_carries_the_listed_revision_and_keep_cancels()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let id = fixture.snapshot()?.context().draft();
    fixture.save().await?;
    fixture.owner.finish_write().await?;
    let DesktopEditorCommandsEnumResult::List(page) = fixture
        .dispatch(DesktopEditorCommandsEnumOperation::List { after: None })
        .await?
    else {
        return Err("missing list".into());
    };
    let revision = page
        .page()
        .entries()
        .iter()
        .find(|entry| entry.id() == id)
        .ok_or("missing listed draft")?
        .storage_revision();
    fixture.close().await?;
    for is_library in [false, true] {
        let mut harness = PresentationEditorDraftListFixture::harness(page.clone(), is_library)?;
        assert!(harness.query_by_label("Valid position draft").is_some());
        harness
            .get_by_label(&format!("Delete draft {id}"))
            .click_accesskit();
        harness.run_ok().ok_or("delete prompt did not settle")?;
        assert!(harness.state().actions.is_empty());
        harness.get_by_label("Keep Saved Draft").click_accesskit();
        harness.run_ok().ok_or("keep did not settle")?;
        harness.run_steps(3);
        assert!(harness.state().actions.is_empty());
        assert!(harness.query_by_label("Confirm Delete Draft").is_none());
        harness
            .get_by_label(&format!("Delete draft {id}"))
            .click_accesskit();
        harness.run_ok().ok_or("second prompt did not settle")?;
        harness
            .get_by_label("Confirm Delete Draft")
            .click_accesskit();
        harness.run_ok().ok_or("confirm did not settle")?;
        harness.run_steps(3);
        assert_eq!(
            harness.state().actions,
            [PresentationEditorDraftListEnumAction::Delete {
                id,
                storage_revision: revision,
                expected: None,
            }]
        );
    }
    Ok(())
}

#[tokio::test]
async fn refresh_starts_from_the_first_page_and_lists_stay_distinct_from_games()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    fixture.save().await?;
    fixture.owner.finish_write().await?;
    let DesktopEditorCommandsEnumResult::List(page) = fixture
        .dispatch(DesktopEditorCommandsEnumOperation::List { after: None })
        .await?
    else {
        return Err("missing list".into());
    };
    fixture.close().await?;
    for is_library in [false, true] {
        let mut harness = PresentationEditorDraftListFixture::harness(page.clone(), is_library)?;
        assert!(harness.query_by_label("Saved position drafts").is_some());
        assert_eq!(harness.query_by_label("Positions").is_some(), is_library);
        harness
            .get_by_label("Refresh Position Drafts")
            .click_accesskit();
        harness.run_ok().ok_or("refresh did not settle")?;
        harness.run_steps(3);
        assert_eq!(
            harness.state().actions,
            [PresentationEditorDraftListEnumAction::List { after: None }]
        );
    }
    Ok(())
}
