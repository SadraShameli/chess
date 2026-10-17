use crate::desktop::annotation_commands::{
    DesktopAnnotationCommands, DesktopAnnotationCommandsEnumAction,
    DesktopAnnotationCommandsEnumAdmission,
};
use crate::desktop::annotation_owner::{
    DesktopAnnotationOwner, DesktopAnnotationOwnerError, DesktopAnnotationOwnerResources,
};
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::storage::library_repository::{StorageLibraryRepository, StorageLibrarySourceMemory};
use std::sync::Arc;

impl DesktopLibraryServiceFixture {
    async fn owner_for_checkpoint_test(
        &self,
    ) -> DesktopLibraryServiceTestResult<(DesktopAnnotationOwner, StorageLibrarySourceMemory)> {
        let revision = self.seed(100, true).await?;
        let recovered = StorageLibraryRepository::new(self.store.client(), &self.executor)
            .load(DesktopLibraryServiceFixture::id(100)?)
            .await?
            .ok_or("missing notation")?;
        let (record, memory) = recovered.into_retained_record();
        let opening = self.receipt()?;
        let mut owner = DesktopAnnotationOwner::open(
            record,
            revision,
            opening.clone(),
            DesktopAnnotationOwnerResources {
                executor: Arc::clone(&self.executor),
                client: self.store.client(),
                wall_time: Arc::<
                    crate::desktop::library_service::tests::fixture::DesktopLibraryServiceTestWall,
                >::clone(&self.wall),
            },
        )?;
        owner.observe_compute(&opening).await?;
        Ok((owner, memory))
    }
}

#[tokio::test]
async fn accepted_annotation_save_continuation_keeps_its_cancelled_original_receipt()
-> DesktopLibraryServiceTestResult {
    let fixture = DesktopLibraryServiceFixture::new().await?;
    let (mut owner, memory) = fixture.owner_for_checkpoint_test().await?;
    let application = fixture.receipt()?;
    owner.submit(DesktopAnnotationCommands::new(
        application.clone(),
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: "accepted before cancellation".to_owned(),
            nags: Vec::new(),
        },
    ))?;
    owner.observe_compute(&application).await?;
    assert!(owner.status().is_dirty());
    application.cancellation().request();
    let result = owner.submit(DesktopAnnotationCommands::new(
        application.clone(),
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::Checkpoint,
    ));
    let admitted = matches!(
        &result,
        Ok(DesktopAnnotationCommandsEnumAdmission::SavePending)
    );
    let refusal = result.err();
    let mut repeated = None;
    if admitted {
        owner.observe_save(&application).await?;
        owner.observe_save(&application).await?;
        repeated = owner
            .submit(DesktopAnnotationCommands::new(
                application.clone(),
                owner.snapshot()?.context(),
                DesktopAnnotationCommandsEnumAction::Checkpoint,
            ))
            .err();
    }
    let dirty = owner.status().is_dirty();
    let calls = fixture.wall.calls.load(std::sync::atomic::Ordering::SeqCst);
    drop((owner, memory));
    fixture.close().await?;
    assert!(admitted, "accepted continuation was refused: {refusal:?}");
    assert!(!dirty);
    assert_eq!(calls, 1);
    assert!(matches!(
        repeated,
        Some(DesktopAnnotationOwnerError::Cancelled)
    ));
    Ok(())
}

#[tokio::test]
async fn cancelled_foreign_checkpoint_cannot_continue_another_accepted_edit()
-> DesktopLibraryServiceTestResult {
    let fixture = DesktopLibraryServiceFixture::new().await?;
    let (mut owner, memory) = fixture.owner_for_checkpoint_test().await?;
    let accepted = fixture.receipt()?;
    owner.submit(DesktopAnnotationCommands::new(
        accepted.clone(),
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
            notes: "owned by the accepted receipt".to_owned(),
            nags: Vec::new(),
        },
    ))?;
    owner.observe_compute(&accepted).await?;
    let foreign = fixture.receipt()?;
    foreign.cancellation().request();
    let refusal = owner
        .submit(DesktopAnnotationCommands::new(
            foreign,
            owner.snapshot()?.context(),
            DesktopAnnotationCommandsEnumAction::Checkpoint,
        ))
        .err();
    let pending = owner.submit(DesktopAnnotationCommands::new(
        accepted.clone(),
        owner.snapshot()?.context(),
        DesktopAnnotationCommandsEnumAction::Checkpoint,
    ))?;
    owner.observe_save(&accepted).await?;
    owner.observe_save(&accepted).await?;
    let dirty = owner.status().is_dirty();
    let notes = owner.snapshot()?.annotations().notes().to_owned();
    drop((owner, memory));
    fixture.close().await?;
    assert!(matches!(
        refusal,
        Some(DesktopAnnotationOwnerError::Cancelled)
    ));
    assert!(matches!(
        pending,
        DesktopAnnotationCommandsEnumAdmission::SavePending
    ));
    assert!(!dirty);
    assert_eq!(notes, "owned by the accepted receipt");
    Ok(())
}
