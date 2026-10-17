use crate::board::editor::{BoardEditorSessionEnumOperation, BoardEditorSessionEnumSelection};
use crate::board::enums::{BoardEnumColor, BoardEnumPieceKind};
use crate::desktop::editor_commands::DesktopEditorCommandsEnumOperation;
use crate::desktop::editor_commands::tests::fixture::{
    DesktopEditorCommandsFixture, DesktopEditorCommandsTestResult,
};
use crate::presentation::editor::controller::{
    PresentationEditorController, PresentationEditorControllerError,
};

#[tokio::test]
async fn accepted_editor_input_retains_the_real_application_receipt_until_matching_completion()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let original = fixture.snapshot()?;
    let mut controller = PresentationEditorController::new(Some(original.clone()));
    let operation = controller.select_palette(BoardEnumColor::White, BoardEnumPieceKind::Queen)?;
    controller.submit(operation.clone(), &fixture)?;
    let receipt = controller
        .receipt()
        .ok_or("missing actual receipt")?
        .clone();
    let request = receipt.request_id();
    let early = controller.accept(request);
    let duplicate = controller.submit(operation, &fixture);
    let work = fixture.next_work()?;
    let same_receipt = receipt.shares_outcome(work.receipt());
    fixture.apply_work(&work).await?;
    controller.accept(request)?;
    let selection = controller.snapshot().ok_or("missing editor")?.selection();
    let pending = controller.is_pending();
    fixture.close().await?;
    assert!(same_receipt);
    assert!(matches!(
        early,
        Err(PresentationEditorControllerError::StaleResult)
    ));
    assert!(matches!(
        duplicate,
        Err(PresentationEditorControllerError::Busy)
    ));
    assert_eq!(
        selection,
        BoardEditorSessionEnumSelection::Palette {
            color: BoardEnumColor::White,
            kind: BoardEnumPieceKind::Queen
        }
    );
    assert!(!pending);
    Ok(())
}

#[tokio::test]
async fn real_entity_backpressure_retains_the_original_request_for_explicit_retry()
-> DesktopEditorCommandsTestResult {
    let mut fixture = DesktopEditorCommandsFixture::new().await?;
    fixture.create(1, None).await?;
    let original = fixture.snapshot()?;
    let blocker = fixture.admit(&DesktopEditorCommandsEnumOperation::Apply {
        expected: original.context(),
        operation: BoardEditorSessionEnumOperation::Select(BoardEditorSessionEnumSelection::None),
    })?;
    let mut controller = PresentationEditorController::new(Some(original));
    let operation = controller.select_palette(BoardEnumColor::Black, BoardEnumPieceKind::Knight)?;
    let refused = controller.submit(operation, &fixture);
    let retained = controller
        .pending_request()
        .ok_or("busy request lost")?
        .request_id();
    let lacked_receipt = controller.receipt().is_none();
    fixture.apply_work(&blocker).await?;
    controller.retry(&fixture)?;
    let accepted = controller
        .receipt()
        .ok_or("retry lacks receipt")?
        .request_id();
    let work = fixture.next_work()?;
    fixture.apply_work(&work).await?;
    controller.accept(accepted)?;
    fixture.close().await?;
    assert!(matches!(
        refused,
        Err(PresentationEditorControllerError::Application(
            crate::desktop::application::DesktopApplicationError::EntityBusy
        ))
    ));
    assert!(lacked_receipt);
    assert_eq!(accepted, retained);
    Ok(())
}
