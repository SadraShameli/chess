use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::desktop::library_commands::{
    DesktopLibraryCommandsConfirmation, DesktopLibraryCommandsError,
    DesktopLibraryCommandsLifecycleProvider,
};
use crate::game::enums::GameId;
use crate::game::library_organization::{
    GameLibraryOrganizationRevision, GameLibraryOrganizationTimestamp,
};
use crate::game::storage_repository::GameStorageRevision;
use crate::storage::library_repository::StorageLibraryError;
use crate::storage::trash::{
    StorageTrashPlan, StorageTrashPlanOptions, StorageTrashPreviewRequest,
};

struct DesktopLibraryCommandsFixtureLifecycle {
    revision: Option<GameStorageRevision>,
    observations: AtomicUsize,
}

impl DesktopLibraryCommandsLifecycleProvider for DesktopLibraryCommandsFixtureLifecycle {
    type Error = Infallible;

    fn closed_revision(&self, _game: GameId) -> Result<Option<GameStorageRevision>, Self::Error> {
        self.observations.fetch_add(1, Ordering::SeqCst);
        Ok(self.revision)
    }
}

struct DesktopLibraryCommandsFixture;

impl DesktopLibraryCommandsFixture {
    fn plan(
        action: StorageTrashPreviewRequest,
    ) -> Result<StorageTrashPlan, Box<dyn std::error::Error>> {
        Ok(StorageTrashPlan::new(StorageTrashPlanOptions {
            action,
            catalog: "12".parse()?,
            content: match action {
                StorageTrashPreviewRequest::RemoveFolder(_) => None,
                StorageTrashPreviewRequest::MoveToTrash(_)
                | StorageTrashPreviewRequest::PermanentlyDelete(_) => {
                    Some(GameStorageRevision::new(7))
                }
            },
            organization: GameLibraryOrganizationRevision::ZERO,
            folders: u64::from(matches!(
                action,
                StorageTrashPreviewRequest::RemoveFolder(_)
            )),
            games: 1,
        }))
    }
}

#[test]
fn confirmed_game_destruction_refuses_open_or_differently_saved_games()
-> Result<(), Box<dyn std::error::Error>> {
    let id: GameId = "00000000-0000-0000-0000-000000000071".parse()?;
    for revision in [
        None,
        Some(GameStorageRevision::new(6)),
        Some(GameStorageRevision::new(8)),
    ] {
        let lifecycle = DesktopLibraryCommandsFixtureLifecycle {
            revision,
            observations: AtomicUsize::new(0),
        };
        let confirmation = DesktopLibraryCommandsConfirmation::confirm(
            DesktopLibraryCommandsFixture::plan(StorageTrashPreviewRequest::MoveToTrash(id))?,
        );
        assert!(matches!(
            confirmation.prepare(&lifecycle, GameLibraryOrganizationTimestamp::new(100)?),
            Err(DesktopLibraryCommandsError::Storage(error))
                if matches!(*error, StorageLibraryError::LifecycleRequired)
        ));
        assert_eq!(lifecycle.observations.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[test]
fn confirmed_destruction_binds_the_observed_closed_revision_to_the_exact_plan()
-> Result<(), Box<dyn std::error::Error>> {
    let id: GameId = "00000000-0000-0000-0000-000000000071".parse()?;
    let lifecycle = DesktopLibraryCommandsFixtureLifecycle {
        revision: Some(GameStorageRevision::new(7)),
        observations: AtomicUsize::new(0),
    };
    let request = DesktopLibraryCommandsConfirmation::confirm(DesktopLibraryCommandsFixture::plan(
        StorageTrashPreviewRequest::MoveToTrash(id),
    )?)
    .prepare(&lifecycle, GameLibraryOrganizationTimestamp::new(100)?)?;
    let evidence = request
        .closed_game()
        .ok_or("missing observed closed-game evidence")?;
    assert_eq!(evidence.id(), id);
    assert_eq!(evidence.content_revision(), GameStorageRevision::new(7));
    assert_eq!(
        request.plan().content_revision(),
        Some(evidence.content_revision())
    );
    assert_eq!(request.plan().catalog_revision().value(), 12);
    Ok(())
}

#[test]
fn confirmed_folder_removal_does_not_fabricate_a_game_closure()
-> Result<(), Box<dyn std::error::Error>> {
    let folder = "00000000-0000-0000-0000-000000000072".parse()?;
    let lifecycle = DesktopLibraryCommandsFixtureLifecycle {
        revision: None,
        observations: AtomicUsize::new(0),
    };
    let request = DesktopLibraryCommandsConfirmation::confirm(DesktopLibraryCommandsFixture::plan(
        StorageTrashPreviewRequest::RemoveFolder(folder),
    )?)
    .prepare(&lifecycle, GameLibraryOrganizationTimestamp::new(100)?)?;
    assert!(request.closed_game().is_none());
    assert_eq!(lifecycle.observations.load(Ordering::SeqCst), 0);
    Ok(())
}
