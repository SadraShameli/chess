use bevy_egui::egui;
use egui_kittest::{Harness, kittest::Queryable};

use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::DesktopLibraryServiceEnumAction;
use crate::desktop::library_service::tests::fixture::DesktopLibraryServiceFixture;
use crate::desktop::library_service::tests::fixture::DesktopLibraryServiceTestResult;
use crate::game::library_organization::GameLibraryOrganizationEnumMutation;
use crate::presentation::library::tests::fixture::PresentationLibraryTestFixture;
use crate::presentation::library::view::{PresentationLibraryEnumEffect, PresentationLibraryPanel};
use crate::storage::library_query::StorageLibraryEnumFilter;

#[tokio::test]
async fn repeated_egui_passes_emit_initial_reads_once_and_show_real_empty_state()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let panel = PresentationLibraryPanel::new(fixture.service.workspace);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 1000.0))
            .build_ui_state(
                |ui,
                 (panel, effects): &mut (
                    PresentationLibraryPanel,
                    Vec<PresentationLibraryEnumEffect>,
                )| {
                    let produced = panel.show(ui);
                    if !produced.is_empty() {
                        ui.ctx().request_discard("Exercise repeated Library passes");
                    }
                    effects.extend(produced);
                },
                (panel, Vec::<PresentationLibraryEnumEffect>::new()),
            );
        harness.run_steps(3);
        let initial = harness.state().1.len();
        harness.run_steps(3);
        let repeated = harness.state().1.len();
        let effects = std::mem::take(&mut harness.state_mut().1);
        for effect in effects {
            let PresentationLibraryEnumEffect::Dispatch(intent) = effect else {
                return Err("initial read opened game".into());
            };
            fixture
                .execute(harness.state_mut().0.controller_mut(), &intent)
                .await??;
        }
        harness.run_steps(3);
        let empty = harness
            .query_all_by_label("No games match these filters")
            .count();
        let options = harness.state().0.controller().options().clone();
        let intent = harness
            .state_mut()
            .0
            .controller_mut()
            .query(DesktopRequestId::new(), options)?
            .ok_or("refresh")?;
        let loading = harness.state().0.controller().is_query_pending();
        harness
            .state_mut()
            .0
            .controller_mut()
            .abandon_intent(&intent)?;
        Ok((initial, repeated, empty, loading))
    }
    .await;
    fixture.close().await?;
    let (initial, repeated, empty, loading) = observation?;
    assert_eq!(initial, 3);
    assert_eq!(repeated, initial);
    assert_eq!(empty, 1);
    assert!(loading);
    Ok(())
}

#[tokio::test]
async fn native_player_filter_and_open_emit_once_with_truthful_notation_labels()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        fixture.seed_notation(DesktopLibraryServiceFixture::id(1100)?, "[White \"Ada\"]\n[Black \"Grace\"]\n\n1. e4 *").await?;
        let mut panel = PresentationLibraryPanel::new(fixture.service.workspace);
        let query = panel.controller_mut().refresh(DesktopRequestId::new())?.ok_or("query")?;
        fixture.execute(panel.controller_mut(), &query).await??;
        let mut harness = Harness::builder().with_size(egui::vec2(1280.0, 1000.0)).build_ui_state(
            |ui, (panel, effects): &mut (PresentationLibraryPanel, Vec<PresentationLibraryEnumEffect>)| {
                let produced = panel.show(ui);
                if !produced.is_empty() {
                    ui.ctx().request_discard("Exercise repeated Library inputs");
                }
                effects.extend(produced);
            }, (panel, Vec::<PresentationLibraryEnumEffect>::new())
        );
        let initial_metadata = std::mem::take(&mut harness.state_mut().1);
        for effect in initial_metadata {
            let PresentationLibraryEnumEffect::Dispatch(intent) = effect else { return Err("metadata opened a game".into()); };
            fixture.execute(harness.state_mut().0.controller_mut(), &intent).await??;
        }
        harness.run_steps(3);
        let unavailable = harness.query_all_by_label_contains("Opponent mode unavailable").count();
        let fake_played = harness.query_all_by_label_contains("Local two-player").count();
        harness.get_by_label("Search players").focus();
        harness.run_steps(1);
        harness.get_by_label("Search players").type_text("Ada");
        harness.run_steps(2);
        harness.get_by_label("Apply filters").click();
        harness.run_steps(3);
        let filtered_effects = std::mem::take(&mut harness.state_mut().1);
        let query_count = filtered_effects.len();
        let mut matched = false;
        for effect in filtered_effects {
            let PresentationLibraryEnumEffect::Dispatch(intent) = effect else { return Err("filter opened game".into()); };
            matched = matches!(intent.operation().action(), DesktopLibraryServiceEnumAction::Query { options, cursor: None } if matches!(&options.either_player, StorageLibraryEnumFilter::Value(player) if player.as_str() == "Ada"));
            fixture.execute(harness.state_mut().0.controller_mut(), &intent).await??;
        }
        harness.run_steps(3);
        harness.get_by_label("Open saved game").click();
        harness.run_steps(3);
        let open_count = harness.state().1.len();
        let Some(PresentationLibraryEnumEffect::OpenSaved(intent)) = harness.state().1.first() else { return Err("missing guarded Open intent".into()); };
        let unadmitted = harness.state().0.controller().retained_request(intent).is_none();
        let exact_game = matches!(intent.operation().action(), DesktopLibraryServiceEnumAction::Open { game, .. } if *game == DesktopLibraryServiceFixture::id(1100)?);
        Ok((unavailable, fake_played, query_count, matched, open_count, unadmitted, exact_game))
    }.await;
    fixture.close().await?;
    let (unavailable, fake_played, queries, matched, opens, unadmitted, exact_game) = observation?;
    assert_eq!(unavailable, 1);
    assert_eq!(fake_played, 0);
    assert_eq!(queries, 1);
    assert_eq!(opens, 1);
    assert!(matched && unadmitted && exact_game);
    Ok(())
}

#[tokio::test]
async fn favorite_button_emits_one_exact_organization_mutation_and_toggles_off_after_refresh()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let game = DesktopLibraryServiceFixture::id(1200)?;
        fixture.seed_notation(game, "[White \"Ada\"]\n\n1. e4 *").await?;
        let mut panel = PresentationLibraryPanel::new(fixture.service.workspace);
        let query = panel.controller_mut().refresh(DesktopRequestId::new())?.ok_or("query")?;
        fixture.execute(panel.controller_mut(), &query).await??;
        let mut harness = Harness::builder().with_size(egui::vec2(1280.0, 1000.0)).build_ui_state(
            |ui, (panel, effects): &mut (PresentationLibraryPanel, Vec<PresentationLibraryEnumEffect>)| {
                let produced = panel.show(ui);
                if !produced.is_empty() {
                    ui.ctx().request_discard("Exercise favorite input");
                }
                effects.extend(produced);
            }, (panel, Vec::<PresentationLibraryEnumEffect>::new())
        );
        let initial = std::mem::take(&mut harness.state_mut().1);
        for effect in initial {
            let PresentationLibraryEnumEffect::Dispatch(intent) = effect else { return Err("metadata opened a game".into()); };
            fixture.execute(harness.state_mut().0.controller_mut(), &intent).await??;
        }
        harness.run_steps(3);
        harness.get_by_label("Add to Favorites").click();
        harness.run_steps(3);
        let effects = std::mem::take(&mut harness.state_mut().1);
        let count = effects.len();
        let mut exact = false;
        for effect in effects {
            let PresentationLibraryEnumEffect::Dispatch(intent) = effect else { return Err("favorite opened a game".into()); };
            exact = matches!(intent.operation().action(), DesktopLibraryServiceEnumAction::Organize(GameLibraryOrganizationEnumMutation::SetFavorite(target, true)) if target.id() == game);
            fixture.execute(harness.state_mut().0.controller_mut(), &intent).await??;
        }
        let resync = harness.state().0.controller().needs_resync();
        Ok((count, exact, resync))
    }.await;
    fixture.close().await?;
    assert_eq!(observation?, (1, true, true));
    Ok(())
}
