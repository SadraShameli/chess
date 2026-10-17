use std::num::NonZeroUsize;

use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::desktop::native::report_host::{DesktopNativeReportHost, DesktopNativeStudyInput};
use chess::presentation::report::controller::PresentationReportIntent;
use chess::presentation::study::PresentationStudy;
use chess::report::job::ReportJobId;
use chess::storage::dataset_integrity::StorageDatasetIntegrityReport;
use chess::storage::document_store::StorageDocumentStore;
use tempfile::TempDir;
use uuid::Uuid;

type DesktopNativeStudyPumpResult = Result<(), Box<dyn std::error::Error>>;

fn input(intents: Vec<PresentationReportIntent>) -> DesktopNativeStudyInput {
    DesktopNativeStudyInput {
        report_intents: intents,
        current_report: None,
        position: BoardPosition::standard(),
        starters: Vec::new(),
        integrity: StorageDatasetIntegrityReport::default(),
    }
}

#[tokio::test]
async fn failed_report_intents_become_visible_failures_while_the_other_panels_still_refresh()
-> DesktopNativeStudyPumpResult {
    let directory = TempDir::new()?;
    let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("empty budget")?)?;
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget).await?;
    let host = DesktopNativeReportHost::new(store.client());
    let id = ReportJobId::try_from(Uuid::from_u128(5))?;

    let output = host
        .process(input(vec![
            PresentationReportIntent::Cancel(id),
            PresentationReportIntent::ConfirmReplacement(id),
        ]))
        .await;
    assert!(
        output
            .failures
            .iter()
            .any(|failure| failure.contains("could not be cancelled"))
    );
    assert!(
        output
            .failures
            .iter()
            .any(|failure| failure.contains("could not be replaced"))
    );
    assert!(!output.replacement_resolved);
    assert!(output.explorer.is_some());
    assert!(output.rows.as_ref().is_some_and(Vec::is_empty));
    assert!(output.report.is_none());

    let mut study = PresentationStudy::new();
    let pending = DesktopNativeReportHost::apply(output, &mut study);
    assert_eq!(pending.failures.len(), 2);
    assert_eq!(study.report_mut().failures().len(), 2);
    assert!(pending.reanalysis.is_empty());

    let discarded = host
        .process(input(vec![PresentationReportIntent::DiscardReplacement(
            id,
        )]))
        .await;
    assert!(discarded.replacement_resolved);
    assert!(discarded.failures.is_empty());
    let pending = DesktopNativeReportHost::apply(discarded, &mut study);
    assert!(pending.failures.is_empty());
    assert!(study.report_mut().failures().is_empty());
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn reanalysis_requests_are_returned_for_routing_and_host_failures_are_drained_once()
-> DesktopNativeStudyPumpResult {
    let directory = TempDir::new()?;
    let budget = CapacityBudget::new(NonZeroUsize::new(2).ok_or("empty budget")?)?;
    let store =
        StorageDocumentStore::open(directory.path().join("library.sqlite3"), budget).await?;
    let host = DesktopNativeReportHost::new(store.client());
    let id = ReportJobId::try_from(Uuid::from_u128(6))?;
    host.record_failure("The report engine stopped".to_owned());
    let output = host
        .process(input(vec![PresentationReportIntent::PreviewReanalysis(id)]))
        .await;
    assert_eq!(output.reanalysis, vec![id]);
    assert_eq!(
        output.failures,
        vec!["The report engine stopped".to_owned()]
    );
    let again = host.process(input(Vec::new())).await;
    assert!(again.failures.is_empty());
    store.close().await?;
    Ok(())
}
