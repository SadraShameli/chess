#[path = "retry_open.rs"]
mod retry_open;

use std::sync::Arc;

use crate::clock::duration::ClockInstant;
use crate::desktop::game_editor::DesktopGameEditorSnapshotEnumState;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumResult, DesktopLibraryServiceError,
};
use crate::game::enums::{GameEnumEngineReadiness, GameEnumPhase};
use crate::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationName,
    GameLibraryOrganizationRequest, GameLibraryOrganizationTimestamp,
};
use crate::presentation::library::controller::PresentationLibraryController;
use crate::presentation::library::error::PresentationLibraryError;
use crate::presentation::library::tests::fixture::PresentationLibraryTestFixture;
use crate::storage::library_metadata::{
    StorageLibraryMetadataEnumRequest, StorageLibraryMetadataEnumResponse,
};
use crate::storage::library_query::{
    StorageLibraryEnumFilter, StorageLibraryEnumSort, StorageLibraryQuery,
    StorageLibraryQueryOptions,
};
use crate::storage::library_repository::StorageLibraryRepository;

#[tokio::test]
async fn real_pages_keep_default_sort_and_exact_continuations() -> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        for id in 100..200 {
            fixture.service.seed(id, true).await?;
        }
        let newest = DesktopLibraryServiceFixture::id(200)?;
        fixture.seed_notation(newest, "1. d4 *").await?;
        let mut controller = PresentationLibraryController::new(fixture.service.workspace);
        let first = controller.query(DesktopRequestId::new(), StorageLibraryQueryOptions::default())?
            .ok_or("missing initial query")?;
        assert_eq!(controller.options().sort, StorageLibraryEnumSort::UpdatedDescending);
        fixture.execute(&mut controller, &first).await??;
        let page = controller.page().ok_or("missing real page")?;
        let ids = page.entries().iter().map(|row| row.id()).collect::<Vec<_>>();
        assert_eq!(ids.first(), Some(&newest));
        let cursor = page.next_cursor().ok_or("missing cursor")?.clone();
        let count = page.entries().len();
        let next = controller.next_page(DesktopRequestId::new())?.ok_or("missing next query")?;
        assert!(matches!(next.operation().action(), DesktopLibraryServiceEnumAction::Query { cursor: Some(actual), .. } if *actual == cursor));
        fixture.execute(&mut controller, &next).await??;
        let second = controller.page().ok_or("missing second page")?;
        Ok((count, second.entries().len(), second.next_cursor().is_none(), ids.contains(&second.entries()[0].id())))
    }.await;
    fixture.close().await?;
    let (first, second, ended, duplicate) = observation?;
    assert_eq!(first, StorageLibraryQuery::PAGE_SIZE);
    assert_eq!(second, 1);
    assert!(ended);
    assert!(!duplicate);
    Ok(())
}

#[tokio::test]
async fn stale_cursor_requires_explicit_first_page_resync() -> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        for id in 300..=400 {
            fixture.service.seed(id, true).await?;
        }
        let mut controller = PresentationLibraryController::new(fixture.service.workspace);
        let initial = controller
            .query(
                DesktopRequestId::new(),
                StorageLibraryQueryOptions::default(),
            )?
            .ok_or("initial")?;
        fixture.execute(&mut controller, &initial).await??;
        fixture.service.seed(401, true).await?;
        let next = controller
            .next_page(DesktopRequestId::new())?
            .ok_or("next")?;
        let refused = fixture.execute(&mut controller, &next).await?.is_err();
        let needs_resync = controller.needs_resync();
        let stale_hidden = controller.page().is_none();
        let resync = controller
            .refresh(DesktopRequestId::new())?
            .ok_or("resync")?;
        let reset = matches!(
            resync.operation().action(),
            DesktopLibraryServiceEnumAction::Query { cursor: None, .. }
        );
        fixture.execute(&mut controller, &resync).await??;
        Ok((
            refused,
            needs_resync,
            stale_hidden,
            reset,
            controller.page().is_some(),
        ))
    }
    .await;
    fixture.close().await?;
    let (refused, needs_resync, hidden, reset, recovered) = observation?;
    assert!(refused && needs_resync && hidden && reset && recovered);
    Ok(())
}

#[tokio::test]
async fn exact_request_and_receipt_survive_guard_and_reject_replacement()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let mut played = DesktopLibraryServiceFixture::played(500)?;
        played.start(
            played.revision(),
            ClockInstant::from_nanos(50),
            GameEnumEngineReadiness::NotRequired,
            &crate::desktop::library_service::tests::fixture::DesktopLibraryServiceTestProof,
        )?;
        fixture.service.seed_played(&played).await?;
        fixture.service.seed(501, true).await?;
        let mut controller = PresentationLibraryController::new(fixture.service.workspace);
        let initial = controller
            .refresh(DesktopRequestId::new())?
            .ok_or("initial")?;
        fixture.execute(&mut controller, &initial).await??;
        let game = DesktopLibraryServiceFixture::id(500)?;
        let input = DesktopRequestId::new();
        let open = controller.open_saved(input, game)?.ok_or("open")?;
        assert!(controller.open_saved(input, game)?.is_none());
        let request = fixture.request(&open)?;
        let replacement = fixture.request(&open)?;
        assert_ne!(open.input_id(), request.request_id());
        controller.attach_request(&open, request.clone())?;
        let replacement_rejected = matches!(
            controller.attach_request(&open, replacement),
            Err(PresentationLibraryError::ForeignRequest)
        );
        let held_operation = open.operation().clone();
        let refresh = controller
            .refresh(DesktopRequestId::new())?
            .ok_or("refresh")?;
        fixture.execute(&mut controller, &refresh).await??;
        let receipt = fixture.application.port().try_submit(&request)?;
        controller.attach_receipt(&open, receipt.clone())?;
        let work = fixture.application.try_next()?.ok_or("open work")?;
        fixture.application.begin_commit(&work)?;
        let mut service_receipt = fixture.service.port.try_submit(
            crate::desktop::library_service::DesktopLibraryServiceRequest::new(
                receipt.clone(),
                held_operation.clone(),
            ),
        )?;
        let result = fixture.service.drive(&mut service_receipt).await?;
        let wrong_operation = refresh.operation();
        let wrong_rejected = matches!(
            controller.observe_result(&receipt, wrong_operation, &result),
            Err(PresentationLibraryError::UnexpectedReply)
        );
        controller.observe_result(&receipt, &held_operation, &result)?;
        let DesktopLibraryServiceEnumResult::Opened(publication) = result else {
            return Err("missing opening".into());
        };
        let paused = matches!(
            publication.state(),
            DesktopGameEditorSnapshotEnumState::Played {
                phase: GameEnumPhase::Interrupted,
                ..
            }
        );
        let exact = publication.context().game() == game;
        Ok((replacement_rejected, wrong_rejected, paused, exact))
    }
    .await;
    fixture.close().await?;
    let (replacement_rejected, wrong_rejected, paused, exact) = observation?;
    assert!(replacement_rejected && wrong_rejected && paused && exact);
    Ok(())
}

#[tokio::test]
async fn canonical_metadata_pages_remain_bounded_and_navigate_nested_folders()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let repository = StorageLibraryRepository::new(
            fixture.service.store.client(),
            &fixture.service.executor,
        );
        let folder = uuid::Uuid::from_u128(600).to_string().parse()?;
        let parent_receipt = repository
            .organize(GameLibraryOrganizationRequest::new(
                GameLibraryOrganizationTimestamp::new(10)?,
                GameLibraryOrganizationEnumMutation::CreateFolder(
                    folder,
                    None,
                    GameLibraryOrganizationName::new("Studies".into())?,
                ),
            )?)
            .await?;
        let child = uuid::Uuid::from_u128(601).to_string().parse()?;
        repository
            .organize(GameLibraryOrganizationRequest::new(
                GameLibraryOrganizationTimestamp::new(10)?,
                GameLibraryOrganizationEnumMutation::CreateFolder(
                    child,
                    Some(
                        crate::game::library_organization::GameLibraryOrganizationFolderTarget::new(
                            folder,
                            parent_receipt.revision(),
                        ),
                    ),
                    GameLibraryOrganizationName::new("Replies".into())?,
                ),
            )?)
            .await?;
        for id in 700..=800 {
            repository
                .organize(GameLibraryOrganizationRequest::new(
                    GameLibraryOrganizationTimestamp::new(10)?,
                    GameLibraryOrganizationEnumMutation::CreateTag(
                        uuid::Uuid::from_u128(id).to_string().parse()?,
                        GameLibraryOrganizationName::new(format!("Tag {id}"))?,
                    ),
                )?)
                .await?;
        }
        let mut controller = PresentationLibraryController::new(fixture.service.workspace);
        let tags = controller
            .metadata(
                DesktopRequestId::new(),
                StorageLibraryMetadataEnumRequest::Tags { cursor: None },
            )?
            .ok_or("tags")?;
        fixture.execute(&mut controller, &tags).await??;
        let tag_page = controller.tags().ok_or("tag page")?;
        let first = tag_page.entries().len();
        let cursor = *tag_page.next_cursor().ok_or("tag cursor")?;
        let next = controller
            .metadata(
                DesktopRequestId::new(),
                StorageLibraryMetadataEnumRequest::Tags {
                    cursor: Some(cursor),
                },
            )?
            .ok_or("next tags")?;
        fixture.execute(&mut controller, &next).await??;
        let second = controller.tags().ok_or("second tags")?.entries().len();
        let folders = controller
            .metadata(
                DesktopRequestId::new(),
                StorageLibraryMetadataEnumRequest::Folders {
                    parent: None,
                    cursor: None,
                },
            )?
            .ok_or("folders")?;
        fixture.execute(&mut controller, &folders).await??;
        let page = controller.folders().ok_or("folder page")?;
        let catalog = page.catalog_revision();
        let folder_name = page
            .entries()
            .first()
            .ok_or("folder")?
            .name()
            .as_str()
            .to_owned();
        let detail = controller
            .metadata(
                DesktopRequestId::new(),
                StorageLibraryMetadataEnumRequest::Folder {
                    id: folder,
                    catalog,
                },
            )?
            .ok_or("detail")?;
        let result = fixture.execute(&mut controller, &detail).await??;
        assert!(matches!(
            result,
            DesktopLibraryServiceEnumResult::Metadata(StorageLibraryMetadataEnumResponse::Folder(
                _
            ))
        ));
        let children = controller
            .metadata(
                DesktopRequestId::new(),
                StorageLibraryMetadataEnumRequest::Folders {
                    parent: Some(folder),
                    cursor: None,
                },
            )?
            .ok_or("children")?;
        fixture.execute(&mut controller, &children).await??;
        let child_page = controller.folders().ok_or("children page")?;
        assert_eq!(
            child_page.entries().first().ok_or("nested child")?.id(),
            child
        );
        let catalog = child_page.catalog_revision();
        let child_detail = controller
            .metadata(
                DesktopRequestId::new(),
                StorageLibraryMetadataEnumRequest::Folder { id: child, catalog },
            )?
            .ok_or("child detail")?;
        fixture.execute(&mut controller, &child_detail).await??;
        let grandchildren = controller
            .metadata(
                DesktopRequestId::new(),
                StorageLibraryMetadataEnumRequest::Folders {
                    parent: Some(child),
                    cursor: None,
                },
            )?
            .ok_or("grandchildren")?;
        fixture.execute(&mut controller, &grandchildren).await??;
        Ok((
            first,
            second,
            folder_name,
            controller.folder_parent(),
            controller
                .folders()
                .ok_or("children page")?
                .entries()
                .is_empty(),
        ))
    }
    .await;
    fixture.close().await?;
    let (first, second, name, parent, empty_children) = observation?;
    assert_eq!(first, StorageLibraryMetadataEnumRequest::PAGE_SIZE);
    assert_eq!(second, 1);
    assert_eq!(name, "Studies");
    assert!(parent.is_some() && empty_children);
    Ok(())
}

#[tokio::test]
async fn recent_input_ledger_evicts_completed_inputs_without_lifetime_lockout()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let mut controller = PresentationLibraryController::new(fixture.service.workspace);
        for _ in 0..300 {
            let input = DesktopRequestId::new();
            let intent = controller.refresh(input)?.ok_or("fresh input refused")?;
            assert!(controller.refresh(input)?.is_none());
            controller.abandon_intent(&intent)?;
            assert!(controller.refresh(input)?.is_none());
        }
        let intent = controller
            .refresh(DesktopRequestId::new())?
            .ok_or("ledger permanently saturated")?;
        fixture.execute(&mut controller, &intent).await??;
        Ok(controller
            .page()
            .is_some_and(|page| page.entries().is_empty()))
    }
    .await;
    fixture.close().await?;
    assert!(observation?);
    Ok(())
}

#[tokio::test]
async fn foreign_receipt_cannot_publish_a_real_repository_page() -> DesktopLibraryServiceTestResult
{
    let fixture = PresentationLibraryTestFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let mut controller = PresentationLibraryController::new(fixture.service.workspace);
        let intent = controller
            .refresh(DesktopRequestId::new())?
            .ok_or("query")?;
        let (receipt, _work) = fixture.admit(&mut controller, &intent)?;
        let foreign_request = fixture.request(&intent)?;
        let foreign = fixture.application.port().try_submit(&foreign_request)?;
        let page = StorageLibraryRepository::new(
            fixture.service.store.client(),
            &fixture.service.executor,
        )
        .query(StorageLibraryQuery::default())
        .await?;
        let result = DesktopLibraryServiceEnumResult::Query(Arc::new(page));
        let rejected = matches!(
            controller.observe_result(&foreign, intent.operation(), &result),
            Err(PresentationLibraryError::ForeignReceipt)
        );
        controller.observe_result(&receipt, intent.operation(), &result)?;
        Ok(rejected && controller.page().is_some())
    }
    .await;
    fixture.close().await?;
    assert!(observation?);
    Ok(())
}

#[tokio::test]
async fn retained_open_failure_keeps_exact_original_and_rejects_foreign_continuation()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        fixture.service.seed(900, true).await?;
        let mut controller = PresentationLibraryController::new(fixture.service.workspace);
        let initial = controller
            .refresh(DesktopRequestId::new())?
            .ok_or("query")?;
        fixture.execute(&mut controller, &initial).await??;
        let open = controller
            .open_saved(
                DesktopRequestId::new(),
                DesktopLibraryServiceFixture::id(900)?,
            )?
            .ok_or("open")?;
        let (receipt, _work) = fixture.admit(&mut controller, &open)?;
        let request = controller
            .retained_request(&open)
            .ok_or("retained request")?
            .request_id();
        let foreign = Arc::new(DesktopLibraryServiceError::Continuation {
            request: DesktopRequestId::new(),
            expected: None,
            source: Arc::new(DesktopLibraryServiceError::Busy),
        });
        let foreign_rejected = matches!(
            controller.observe_error(&receipt, open.operation(), foreign),
            Err(PresentationLibraryError::ForeignContinuation)
        );
        let retained = Arc::new(DesktopLibraryServiceError::Continuation {
            request,
            expected: None,
            source: Arc::new(DesktopLibraryServiceError::Busy),
        });
        controller.observe_error(&receipt, open.operation(), retained)?;
        assert!(!controller.can_retry_open());
        assert!(controller.retry_open(DesktopRequestId::new()).is_err());
        let replacement_refused = matches!(
            controller.open_saved(
                DesktopRequestId::new(),
                DesktopLibraryServiceFixture::id(900)?
            ),
            Err(PresentationLibraryError::Busy)
        );
        let same_request = controller
            .retained_request(&open)
            .is_some_and(|original| original.request_id() == request);
        let same_receipt = controller
            .retained_receipt(&open)
            .is_some_and(|original| original.shares_outcome(&receipt));
        let cannot_abandon = matches!(
            controller.abandon_intent(&open),
            Err(PresentationLibraryError::AlreadyBound)
        );
        Ok((
            foreign_rejected,
            replacement_refused,
            same_request,
            same_receipt,
            cannot_abandon,
            controller.is_open_pending(),
        ))
    }
    .await;
    fixture.close().await?;
    let (foreign, replacement, request, receipt, abandon, pending) = observation?;
    assert!(foreign && replacement && request && receipt && abandon && pending);
    Ok(())
}

#[tokio::test]
async fn query_producer_forwards_real_filters_without_fabricating_opponent_metadata()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let annotated = DesktopLibraryServiceFixture::id(1000)?;
        fixture.seed_notation(annotated, "[White \"Ada\"]\n[Black \"Grace\"]\n[Date \"2026.10.17\"]\n[Opening \"King Pawn\"]\n[ECO \"C20\"]\n\n1. e4 e5 *").await?;
        fixture.service.seed(1001, false).await?;
        let mut controller = PresentationLibraryController::new(fixture.service.workspace);
        let options = StorageLibraryQueryOptions {
            either_player: StorageLibraryEnumFilter::Value(crate::game::session::GameMetadataText::new("Ada".into())?),
            opening_code: StorageLibraryEnumFilter::Value(crate::game::session::GameMetadataOpeningCode::new("C20".into())?),
            date: StorageLibraryEnumFilter::Value(crate::storage::library_query::StorageLibraryDateInterval::new(crate::game::session::GameMetadataDate::new(Some(2026), Some(10), Some(17))?, crate::game::session::GameMetadataDate::new(Some(2026), Some(10), Some(17))?)?),
            plies: StorageLibraryEnumFilter::Value(crate::storage::library_query::StorageLibraryPlyInterval::new(2, 2)?),
            ..StorageLibraryQueryOptions::default()
        };
        let intent = controller.query(DesktopRequestId::new(), options.clone())?.ok_or("filtered query")?;
        let exact = matches!(intent.operation().action(), DesktopLibraryServiceEnumAction::Query { options: actual, cursor: None } if *actual == options);
        fixture.execute(&mut controller, &intent).await??;
        let page = controller.page().ok_or("filtered page")?;
        let filtered = page.entries().len() == 1 && page.entries()[0].id() == annotated;
        let unavailable = page.entries()[0].mode().is_none();
        let mut local_options = options;
        local_options.mode = StorageLibraryEnumFilter::Value(crate::game::enums::GameEnumMode::LocalTwoPlayer);
        let local = controller.query(DesktopRequestId::new(), local_options)?.ok_or("local filter")?;
        fixture.execute(&mut controller, &local).await??;
        Ok((exact, filtered, unavailable, controller.page().is_some_and(|page| page.entries().is_empty())))
    }.await;
    fixture.close().await?;
    let (exact, filtered, unavailable, excluded) = observation?;
    assert!(exact && filtered && unavailable && excluded);
    Ok(())
}

#[tokio::test]
async fn known_revision_opens_without_a_loaded_page() -> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        fixture.service.seed(600, true).await?;
        let mut controller = PresentationLibraryController::new(fixture.service.workspace);
        let game = DesktopLibraryServiceFixture::id(600)?;
        let refused = matches!(
            controller.open_saved(DesktopRequestId::new(), game),
            Err(PresentationLibraryError::MissingGame)
        );
        let initial = controller
            .refresh(DesktopRequestId::new())?
            .ok_or("initial")?;
        fixture.execute(&mut controller, &initial).await??;
        let revision = controller
            .page()
            .and_then(|page| page.entries().iter().find(|entry| entry.id() == game))
            .ok_or("entry")?
            .content_revision();
        let mut other = PresentationLibraryController::new(fixture.service.workspace);
        let input = DesktopRequestId::new();
        let open = other
            .open_saved_revision(input, game, revision)?
            .ok_or("open")?;
        let repeated = other.open_saved_revision(input, game, revision)?.is_none();
        let exact = matches!(
            open.operation().action(),
            DesktopLibraryServiceEnumAction::Open { game: opened, storage_revision }
                if *opened == game && *storage_revision == revision
        );
        Ok((refused, repeated, exact, other.is_open_pending()))
    }
    .await;
    fixture.close().await?;
    assert_eq!(observation?, (true, true, true, true));
    Ok(())
}

#[tokio::test]
async fn organization_runs_in_its_own_lane_and_marks_the_page_for_resync()
-> DesktopLibraryServiceTestResult {
    let mut fixture = PresentationLibraryTestFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        fixture.service.seed(700, true).await?;
        let mut controller = PresentationLibraryController::new(fixture.service.workspace);
        let initial = controller
            .refresh(DesktopRequestId::new())?
            .ok_or("initial")?;
        fixture.execute(&mut controller, &initial).await??;
        let request = GameLibraryOrganizationRequest::new(
            GameLibraryOrganizationTimestamp::new(10)?,
            GameLibraryOrganizationEnumMutation::CreateTag(
                uuid::Uuid::from_u128(9).to_string().parse()?,
                GameLibraryOrganizationName::new("Rapid".to_owned())?,
            ),
        )?;
        let input = DesktopRequestId::new();
        let intent = controller
            .organize(input, request.clone())?
            .ok_or("organize")?;
        let repeated = controller.organize(input, request)?.is_none();
        fixture.execute(&mut controller, &intent).await??;
        Ok((repeated, controller.needs_resync(), controller.is_query_pending()))
    }
    .await;
    fixture.close().await?;
    assert_eq!(observation?, (true, true, false));
    Ok(())
}
