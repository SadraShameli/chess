use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};

use crate::board::enums::BoardEnumVariant;
use crate::board::position::BoardPosition;
use crate::desktop::application::DesktopApplicationWakeProvider;
use crate::desktop::dataset_commands::{DesktopDatasetCommands, DesktopDatasetStarter};
use crate::desktop::file_handle::{DesktopFileDialogProvider, DesktopFileEnumPurpose};
use crate::desktop::native::dialogs_proxy::DesktopNativeDialogsProxy;
use crate::desktop::native::event_loop::DesktopNativeEventLoopWake;
use crate::desktop::native::report_host::{
    DesktopNativeReportHost, DesktopNativeStudyInput, DesktopNativeStudyOutput,
};
use crate::presentation::opening::controller::PresentationOpeningIntent;
use crate::presentation::settings::datasets::PresentationSettingsDatasetsIntent;
use crate::presentation::study::PresentationStudy;
use crate::report::job::ReportJobId;
use crate::storage::connection::StorageConnection;
use crate::storage::dataset_import::{
    StorageDatasetImport, StorageDatasetImportEnumConfirmation, StorageDatasetImportRequest,
};
use crate::storage::dataset_integrity::StorageDatasetIntegrityReport;
use crate::storage::dataset_package::StorageDatasetPackageEnumFormat;
use crate::storage::dataset_registry::{
    StorageDatasetId, StorageDatasetRegistry, StorageDatasetRegistryEnumKind,
};

struct DesktopNativeStudyDriverState {
    inflight: Option<oneshot::Receiver<DesktopNativeStudyOutput>>,
    last_run: Option<Instant>,
    last_board: Option<String>,
    explorer_position: Option<BoardPosition>,
    opening_queue: Vec<PresentationOpeningIntent>,
    dataset_queue: Vec<PresentationSettingsDatasetsIntent>,
    notices: Vec<String>,
}

pub struct DesktopNativeStudyDriver {
    host: Arc<DesktopNativeReportHost>,
    runtime: Handle,
    wake: Arc<DesktopNativeEventLoopWake>,
    dialogs: Arc<DesktopNativeDialogsProxy>,
    reanalysis: mpsc::UnboundedSender<ReportJobId>,
    database: PathBuf,
    starters: Vec<DesktopDatasetStarter>,
    state: Mutex<DesktopNativeStudyDriverState>,
}

impl DesktopNativeStudyDriver {
    pub const REFRESH: Duration = Duration::from_millis(500);

    pub fn new(
        host: Arc<DesktopNativeReportHost>,
        runtime: Handle,
        wake: Arc<DesktopNativeEventLoopWake>,
        dialogs: Arc<DesktopNativeDialogsProxy>,
        reanalysis: mpsc::UnboundedSender<ReportJobId>,
        database: PathBuf,
        starters: Vec<DesktopDatasetStarter>,
    ) -> Self {
        Self {
            host,
            runtime,
            wake,
            dialogs,
            reanalysis,
            database,
            starters,
            state: Mutex::new(DesktopNativeStudyDriverState {
                inflight: None,
                last_run: None,
                last_board: None,
                explorer_position: None,
                opening_queue: Vec::new(),
                dataset_queue: Vec::new(),
                notices: Vec::new(),
            }),
        }
    }

    pub fn drive(&self, study: &mut PresentationStudy, board: Option<(String, BoardEnumVariant)>) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(receiver) = &mut state.inflight {
            match receiver.try_recv() {
                Ok(output) => {
                    state.inflight = None;
                    let pending = DesktopNativeReportHost::apply(output, study);
                    for id in pending.reanalysis {
                        if self.reanalysis.send(id).is_err() {
                            state.notices.push(
                                "Reanalysis is unavailable without the report engine".to_owned(),
                            );
                        }
                    }
                    state.opening_queue.extend(pending.opening);
                    state.dataset_queue.extend(pending.datasets);
                }
                Err(oneshot::error::TryRecvError::Empty) => return,
                Err(oneshot::error::TryRecvError::Closed) => {
                    state.inflight = None;
                    state
                        .notices
                        .push("The study update stopped before it finished".to_owned());
                }
            }
        }
        state.opening_queue.extend(study.take_opening_intents());
        state.dataset_queue.extend(study.take_dataset_intents());
        let report_intents = study.take_report_intents();
        let (fen, variant) = board.unzip();
        let board_position = match (&fen, variant) {
            (Some(fen), Some(variant)) => BoardPosition::from_fen(fen, variant).ok(),
            _ => None,
        }
        .unwrap_or_else(BoardPosition::standard);
        let mut moved = false;
        if state.last_board != fen {
            state.last_board = fen;
            state.explorer_position = None;
            moved = true;
        }
        for intent in std::mem::take(&mut state.opening_queue) {
            match intent {
                PresentationOpeningIntent::FollowContinuation { movement, .. } => {
                    let base = state
                        .explorer_position
                        .clone()
                        .unwrap_or_else(|| board_position.clone());
                    match base.play(movement) {
                        Ok(next) => {
                            state.explorer_position = Some(next);
                            moved = true;
                        }
                        Err(error) => state
                            .notices
                            .push(format!("The continuation could not be followed: {error}")),
                    }
                }
                PresentationOpeningIntent::SamplePage(_) => state
                    .notices
                    .push("Paging through sample games is not available yet".to_owned()),
                PresentationOpeningIntent::OpenSample(_) => state.notices.push(
                    "Opening a sample game from the explorer is not available yet".to_owned(),
                ),
            }
        }
        let now = Instant::now();
        let due = state
            .last_run
            .is_none_or(|last| now.duration_since(last) >= Self::REFRESH);
        if !(due || moved || !report_intents.is_empty() || !state.dataset_queue.is_empty()) {
            return;
        }
        let position = state.explorer_position.clone().unwrap_or(board_position);
        let input = DesktopNativeStudyInput {
            report_intents,
            current_report: study.report_mut().view().map(|view| view.id),
            position,
            starters: self.starters.clone(),
            integrity: StorageDatasetIntegrityReport::default(),
        };
        let datasets = std::mem::take(&mut state.dataset_queue);
        let notices = std::mem::take(&mut state.notices);
        let (sender, receiver) = oneshot::channel();
        let host = Arc::clone(&self.host);
        let dialogs = Arc::clone(&self.dialogs);
        let wake = Arc::clone(&self.wake);
        let database = self.database.clone();
        self.runtime.spawn(async move {
            let mut notices = notices;
            for intent in datasets {
                match Self::run_dataset(&dialogs, &database, intent).await {
                    Ok(Some(message)) => notices.push(message),
                    Ok(None) => {}
                    Err(message) => notices.push(message),
                }
            }
            let mut output = host.process(input).await;
            output.failures.extend(notices);
            if sender.send(output).is_err() {
                host.record_failure("The study update was dropped".to_owned());
            }
            if let Err(error) = wake.wake() {
                host.record_failure(format!("The window could not be woken: {error}"));
            }
        });
        state.inflight = Some(receiver);
        state.last_run = Some(now);
    }

    async fn run_dataset(
        dialogs: &DesktopNativeDialogsProxy,
        database: &Path,
        intent: PresentationSettingsDatasetsIntent,
    ) -> Result<Option<String>, String> {
        match intent {
            PresentationSettingsDatasetsIntent::Delete(id) => {
                let database = database.to_path_buf();
                tokio::task::spawn_blocking(move || Self::delete(&database, id))
                    .await
                    .map_err(|error| error.to_string())?
                    .map(|()| Some("The data collection was deleted".to_owned()))
            }
            PresentationSettingsDatasetsIntent::Import(kind) => {
                Self::import(dialogs, database, kind, None).await
            }
            PresentationSettingsDatasetsIntent::Replace(id) => {
                let listing = database.to_path_buf();
                let kind = tokio::task::spawn_blocking(move || Self::kind_of(&listing, id))
                    .await
                    .map_err(|error| error.to_string())??;
                Self::import(dialogs, database, kind, Some(id)).await
            }
        }
    }

    fn delete(database: &Path, id: StorageDatasetId) -> Result<(), String> {
        let mut connection =
            StorageConnection::open(database).map_err(|error| error.to_string())?;
        DesktopDatasetCommands::delete(
            connection.diesel(),
            id,
            StorageDatasetImportEnumConfirmation::Confirmed,
        )
        .map_err(|error| error.to_string())
    }

    fn kind_of(
        database: &Path,
        id: StorageDatasetId,
    ) -> Result<StorageDatasetRegistryEnumKind, String> {
        let mut connection =
            StorageConnection::open(database).map_err(|error| error.to_string())?;
        StorageDatasetRegistry::list(connection.diesel())
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.kind)
            .ok_or_else(|| "The data collection no longer exists".to_owned())
    }

    async fn import(
        dialogs: &DesktopNativeDialogsProxy,
        database: &Path,
        kind: StorageDatasetRegistryEnumKind,
        replaces: Option<StorageDatasetId>,
    ) -> Result<Option<String>, String> {
        let Some(path) = dialogs
            .select(DesktopFileEnumPurpose::ReadFile)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|error| error.to_string())?;
        if metadata.len() > StorageDatasetImport::MAX_BYTES {
            return Err("The selected file is larger than the import limit".to_owned());
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|error| error.to_string())?;
        let format = match kind {
            StorageDatasetRegistryEnumKind::Corpus => StorageDatasetPackageEnumFormat::Pgn,
            StorageDatasetRegistryEnumKind::Book => StorageDatasetPackageEnumFormat::Polyglot,
            StorageDatasetRegistryEnumKind::Puzzles => StorageDatasetPackageEnumFormat::PuzzleCsv,
        };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("imported")
            .to_owned();
        let request = StorageDatasetImportRequest {
            kind,
            name: name.clone(),
            version: format!("import-{stamp}"),
            license: "Imported by the user".to_owned(),
            format,
            location: path.to_string_lossy().into_owned(),
        };
        let database = database.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let preview = DesktopDatasetCommands::preview_import(
                request,
                &bytes,
                StorageDatasetImport::MAX_BYTES,
                replaces,
                || false,
            )
            .map_err(|error| error.to_string())?;
            let mut connection =
                StorageConnection::open(&database).map_err(|error| error.to_string())?;
            DesktopDatasetCommands::apply_import(
                connection.diesel(),
                &preview,
                StorageDatasetImportEnumConfirmation::Confirmed,
            )
            .map_err(|error| error.to_string())?;
            Ok(Some(format!(
                "Imported {name}: {} records accepted, {} rejected",
                preview.accepted(),
                preview.rejected()
            )))
        })
        .await
        .map_err(|error| error.to_string())?
    }
}
