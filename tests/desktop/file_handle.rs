use std::path::PathBuf;

use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::ClockInstant;
use chess::desktop::file_handle::{
    DesktopFileDialogProvider, DesktopFileEnumPurpose, DesktopFileHandleOwner,
    DesktopFileHandleRegistry, DesktopFileHandleRequest,
};

struct DesktopFileDialogFixture(PathBuf);

impl DesktopFileDialogProvider for DesktopFileDialogFixture {
    async fn select(
        &self,
        _purpose: DesktopFileEnumPurpose,
    ) -> Result<Option<PathBuf>, chess::desktop::file_handle::DesktopFileHandleError> {
        Ok(Some(self.0.clone()))
    }
}

#[tokio::test]
async fn native_selections_are_owned_single_use_and_expire()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("selected.pgn");
    std::fs::write(&path, "original")?;
    let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
    let owner = DesktopFileHandleOwner::new();
    let stranger = DesktopFileHandleOwner::new();
    let now = ClockInstant::from_nanos(0);
    let mut registry = DesktopFileHandleRegistry::new();
    let provider = DesktopFileDialogFixture(path);
    let handle = registry
        .select(
            &provider,
            DesktopFileHandleRequest::new(owner, DesktopFileEnumPurpose::ReadFile, now),
            &executor,
        )
        .await?
        .ok_or("fixture selection cancelled")?;
    assert!(
        registry
            .consume(handle, stranger, DesktopFileEnumPurpose::ReadFile, now)
            .is_err()
    );
    assert!(
        registry
            .consume(handle, owner, DesktopFileEnumPurpose::WriteFile, now)
            .is_err()
    );
    let selection = registry.consume(handle, owner, DesktopFileEnumPurpose::ReadFile, now)?;
    assert!(
        registry
            .consume(handle, owner, DesktopFileEnumPurpose::ReadFile, now)
            .is_err()
    );
    assert_eq!(selection.into_source()?.metadata()?.len(), 8);
    let expired = registry
        .select(
            &provider,
            DesktopFileHandleRequest::new(owner, DesktopFileEnumPurpose::ReadFile, now),
            &executor,
        )
        .await?
        .ok_or("fixture selection cancelled")?;
    assert!(
        registry
            .consume(
                expired,
                owner,
                DesktopFileEnumPurpose::ReadFile,
                ClockInstant::from_millis(600_000)?
            )
            .is_err()
    );
    executor.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn source_replacement_and_window_revocation_refuse_selected_handles()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("selected.pgn");
    std::fs::write(&path, "original")?;
    let executor = CapacityExecutor::new(CapacityBudget::discover()?)?;
    let owner = DesktopFileHandleOwner::new();
    let now = ClockInstant::from_nanos(0);
    let mut registry = DesktopFileHandleRegistry::new();
    let provider = DesktopFileDialogFixture(path.clone());
    let handle = registry
        .select(
            &provider,
            DesktopFileHandleRequest::new(owner, DesktopFileEnumPurpose::ReadFile, now),
            &executor,
        )
        .await?
        .ok_or("fixture selection cancelled")?;
    let selected = registry.consume(handle, owner, DesktopFileEnumPurpose::ReadFile, now)?;
    std::fs::rename(&path, directory.path().join("old.pgn"))?;
    std::fs::write(&path, "replaced")?;
    assert!(selected.into_source().is_err());
    let handle = registry
        .select(
            &provider,
            DesktopFileHandleRequest::new(owner, DesktopFileEnumPurpose::ReadFile, now),
            &executor,
        )
        .await?
        .ok_or("fixture selection cancelled")?;
    registry.revoke_owner(owner);
    assert!(
        registry
            .consume(handle, owner, DesktopFileEnumPurpose::ReadFile, now)
            .is_err()
    );
    assert!(
        serde_json::from_str::<chess::desktop::file_handle::DesktopFileHandleId>(
            "\"00000000-0000-0000-0000-000000000000\""
        )
        .is_err()
    );
    executor.shutdown().await?;
    Ok(())
}
