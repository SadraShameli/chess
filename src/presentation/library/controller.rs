mod observation;

use std::collections::VecDeque;
use std::sync::Arc;

use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumImportAction,
    DesktopLibraryServiceError, DesktopLibraryServiceExport, DesktopLibraryServiceFile,
    DesktopLibraryServiceOperation,
};
use crate::desktop::messages::{DesktopMessagesEntityId, DesktopRequest};
use crate::game::enums::{GameId, GameRevision};
use crate::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationFolderId,
    GameLibraryOrganizationGameTarget,
};
use crate::game::storage_repository::GameStorageRevision;
use crate::presentation::library::enums::PresentationLibraryEnumLane;
use crate::presentation::library::error::PresentationLibraryError;
use crate::presentation::library::intent::{PresentationLibraryIntent, PresentationLibraryPending};
use crate::storage::import_session::{
    StorageImportSessionConfirmation, StorageImportSessionDetailPage,
    StorageImportSessionDetailRequest, StorageImportSessionEnumChoice,
    StorageImportSessionEnumCommitOutcome, StorageImportSessionPage,
    StorageImportSessionPageRequest, StorageImportSessionSummary,
};
use crate::storage::library_metadata::{
    StorageLibraryMetadataEnumRequest, StorageLibraryMetadataFolderPage,
    StorageLibraryMetadataFolderRead, StorageLibraryMetadataTagPage,
};
use crate::storage::library_query::{
    StorageLibraryQuery, StorageLibraryQueryCursor, StorageLibraryQueryOptions,
    StorageLibraryQueryPage,
};
use crate::storage::trash::{
    StorageTrashConfirmedRequest, StorageTrashPlan, StorageTrashPreviewRequest,
};

#[derive(Default)]
pub struct PresentationLibraryImport {
    pub(crate) summary: Option<Arc<StorageImportSessionSummary>>,
    pub(crate) page: Option<Arc<StorageImportSessionPage>>,
    pub(crate) detail: Option<Arc<StorageImportSessionDetailPage>>,
    pub(crate) confirmation: Option<StorageImportSessionConfirmation>,
    pub(crate) outcome: Option<Arc<StorageImportSessionEnumCommitOutcome>>,
}

impl PresentationLibraryImport {
    pub fn summary(&self) -> Option<&StorageImportSessionSummary> {
        self.summary.as_deref()
    }
    pub fn page(&self) -> Option<&StorageImportSessionPage> {
        self.page.as_deref()
    }
    pub fn detail(&self) -> Option<&StorageImportSessionDetailPage> {
        self.detail.as_deref()
    }
    pub const fn confirmation(&self) -> Option<&StorageImportSessionConfirmation> {
        self.confirmation.as_ref()
    }
    pub fn outcome(&self) -> Option<&StorageImportSessionEnumCommitOutcome> {
        self.outcome.as_deref()
    }
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

pub struct PresentationLibraryController {
    workspace: DesktopMessagesEntityId,
    options: StorageLibraryQueryOptions,
    page: Option<Arc<StorageLibraryQueryPage>>,
    folders: Option<StorageLibraryMetadataFolderPage>,
    tags: Option<StorageLibraryMetadataTagPage>,
    folder: Option<StorageLibraryMetadataFolderRead>,
    parent: Option<GameLibraryOrganizationFolderId>,
    pending: Vec<PresentationLibraryPending>,
    recent: VecDeque<DesktopRequestId>,
    failures: [Option<Arc<DesktopLibraryServiceError>>; PresentationLibraryEnumLane::COUNT],
    needs_resync: bool,
    trash_plan: Option<StorageTrashPlan>,
    import: PresentationLibraryImport,
    export_bytes: Option<u64>,
    open_game: Option<(GameId, GameRevision)>,
    unsaved_games: Vec<GameId>,
}

impl PresentationLibraryController {
    pub const LANES: usize = PresentationLibraryEnumLane::COUNT;
    const RECENT_INPUT_CAPACITY: usize = 256;

    pub fn new(workspace: DesktopMessagesEntityId) -> Self {
        Self {
            workspace,
            options: StorageLibraryQueryOptions::default(),
            page: None,
            folders: None,
            tags: None,
            folder: None,
            parent: None,
            pending: Vec::with_capacity(PresentationLibraryEnumLane::COUNT),
            recent: VecDeque::with_capacity(Self::RECENT_INPUT_CAPACITY),
            failures: std::array::from_fn(|_| None),
            needs_resync: false,
            trash_plan: None,
            import: PresentationLibraryImport::default(),
            export_bytes: None,
            open_game: None,
            unsaved_games: Vec::new(),
        }
    }

    pub const fn options(&self) -> &StorageLibraryQueryOptions {
        &self.options
    }

    pub fn page(&self) -> Option<&StorageLibraryQueryPage> {
        self.page.as_deref()
    }

    pub const fn folders(&self) -> Option<&StorageLibraryMetadataFolderPage> {
        self.folders.as_ref()
    }

    pub const fn tags(&self) -> Option<&StorageLibraryMetadataTagPage> {
        self.tags.as_ref()
    }

    pub const fn folder(&self) -> Option<&StorageLibraryMetadataFolderRead> {
        self.folder.as_ref()
    }

    pub const fn folder_parent(&self) -> Option<GameLibraryOrganizationFolderId> {
        self.parent
    }

    pub const fn needs_resync(&self) -> bool {
        self.needs_resync
    }

    pub fn is_query_pending(&self) -> bool {
        self.is_pending(PresentationLibraryEnumLane::Query)
    }

    pub fn is_open_pending(&self) -> bool {
        self.is_pending(PresentationLibraryEnumLane::Open)
    }

    pub fn query(
        &mut self,
        input: DesktopRequestId,
        options: StorageLibraryQueryOptions,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        self.begin_query(input, options, None)
    }

    pub fn refresh(
        &mut self,
        input: DesktopRequestId,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        self.query(input, self.options.clone())
    }

    pub fn next_page(
        &mut self,
        input: DesktopRequestId,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        if self.has_input(input) {
            return Ok(None);
        }
        if self.needs_resync {
            return Err(PresentationLibraryError::ResyncRequired);
        }
        let cursor = self
            .page()
            .and_then(StorageLibraryQueryPage::next_cursor)
            .ok_or(PresentationLibraryError::EndOfPages)?
            .clone();
        self.begin_query(input, self.options.clone(), Some(cursor))
    }

    fn begin_query(
        &mut self,
        input: DesktopRequestId,
        options: StorageLibraryQueryOptions,
        cursor: Option<StorageLibraryQueryCursor>,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        if self.has_input(input) {
            return Ok(None);
        }
        let query = StorageLibraryQuery::new(options.clone())?;
        if let Some(cursor) = &cursor {
            query.with_cursor(cursor.clone())?;
        }
        let intent = self.begin(
            input,
            DesktopLibraryServiceEnumAction::Query {
                options: options.clone(),
                cursor,
            },
        )?;
        if intent.is_some() {
            self.options = options;
            self.page = None;
            self.needs_resync = false;
        }
        Ok(intent)
    }

    pub fn metadata(
        &mut self,
        input: DesktopRequestId,
        request: StorageLibraryMetadataEnumRequest,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        let intent = self.begin(input, DesktopLibraryServiceEnumAction::Metadata(request))?;
        if intent.is_some() {
            match request {
                StorageLibraryMetadataEnumRequest::Folders { parent, .. } => {
                    self.folders = None;
                    self.parent = parent;
                    if self
                        .folder
                        .as_ref()
                        .is_some_and(|read| Some(read.folder().id()) != parent)
                    {
                        self.folder = None;
                    }
                }
                StorageLibraryMetadataEnumRequest::Tags { .. } => self.tags = None,
                StorageLibraryMetadataEnumRequest::Folder { .. } => self.folder = None,
            }
        }
        Ok(intent)
    }

    pub fn open_saved(
        &mut self,
        input: DesktopRequestId,
        game: GameId,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        if self.has_input(input) {
            return Ok(None);
        }
        let storage_revision = self
            .page()
            .and_then(|page| page.entries().iter().find(|entry| entry.id() == game))
            .ok_or(PresentationLibraryError::MissingGame)?
            .content_revision();
        self.open_saved_revision(input, game, storage_revision)
    }

    pub fn open_saved_revision(
        &mut self,
        input: DesktopRequestId,
        game: GameId,
        storage_revision: GameStorageRevision,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        if self.has_input(input) {
            return Ok(None);
        }
        self.begin(
            input,
            DesktopLibraryServiceEnumAction::Open {
                game,
                storage_revision,
            },
        )
    }

    pub fn organize(
        &mut self,
        input: DesktopRequestId,
        mutation: GameLibraryOrganizationEnumMutation,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        self.begin(input, DesktopLibraryServiceEnumAction::Organize(mutation))
    }

    pub fn preview_trash(
        &mut self,
        input: DesktopRequestId,
        request: StorageTrashPreviewRequest,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        self.begin(
            input,
            DesktopLibraryServiceEnumAction::PreviewTrash(request),
        )
    }

    pub fn apply_trash(
        &mut self,
        input: DesktopRequestId,
        request: StorageTrashConfirmedRequest,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        self.begin(input, DesktopLibraryServiceEnumAction::ApplyTrash(request))
    }

    pub fn restore(
        &mut self,
        input: DesktopRequestId,
        target: GameLibraryOrganizationGameTarget,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        self.begin(input, DesktopLibraryServiceEnumAction::Restore(target))
    }

    pub fn remove_folder(
        &mut self,
        input: DesktopRequestId,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        if self.has_input(input) {
            return Ok(None);
        }
        let plan = self
            .trash_plan
            .clone()
            .filter(|plan| matches!(plan.action(), StorageTrashPreviewRequest::RemoveFolder(_)))
            .ok_or(PresentationLibraryError::MissingPlan)?;
        self.begin(input, DesktopLibraryServiceEnumAction::RemoveFolder(plan))
    }

    pub fn confirm_game_trash(
        &mut self,
        input: DesktopRequestId,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        if self.has_input(input) {
            return Ok(None);
        }
        let plan = self
            .trash_plan
            .clone()
            .filter(|plan| {
                matches!(
                    plan.action(),
                    StorageTrashPreviewRequest::MoveToTrash(_)
                        | StorageTrashPreviewRequest::PermanentlyDelete(_)
                )
            })
            .ok_or(PresentationLibraryError::MissingPlan)?;
        if let StorageTrashPreviewRequest::MoveToTrash(game)
        | StorageTrashPreviewRequest::PermanentlyDelete(game) = plan.action()
            && self.unsaved_games.contains(&game)
        {
            return Err(PresentationLibraryError::UnsavedGame(game));
        }
        let open_revision = match plan.action() {
            StorageTrashPreviewRequest::MoveToTrash(game)
            | StorageTrashPreviewRequest::PermanentlyDelete(game) => self
                .open_game
                .filter(|(open, _revision)| *open == game)
                .map(|(_open, revision)| revision),
            StorageTrashPreviewRequest::RemoveFolder(_) => None,
        };
        self.begin(
            input,
            DesktopLibraryServiceEnumAction::ConfirmGameTrash {
                plan,
                open_revision,
            },
        )
    }

    pub fn observe_open_game(&mut self, open: Option<(GameId, GameRevision)>) {
        self.open_game = open;
    }

    pub fn observe_unsaved_games(&mut self, games: Vec<GameId>) {
        self.unsaved_games = games;
    }

    pub const fn trash_plan(&self) -> Option<&StorageTrashPlan> {
        self.trash_plan.as_ref()
    }

    pub fn dismiss_trash_plan(&mut self) {
        self.trash_plan = None;
    }

    pub fn import_prepare(
        &mut self,
        input: DesktopRequestId,
        file: DesktopLibraryServiceFile,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        self.begin(
            input,
            DesktopLibraryServiceEnumAction::Import(
                DesktopLibraryServiceEnumImportAction::Prepare(file),
            ),
        )
    }

    pub fn import_page(
        &mut self,
        input: DesktopRequestId,
        request: StorageImportSessionPageRequest,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        self.begin(
            input,
            DesktopLibraryServiceEnumAction::Import(DesktopLibraryServiceEnumImportAction::Page(
                request,
            )),
        )
    }

    pub fn import_detail(
        &mut self,
        input: DesktopRequestId,
        request: StorageImportSessionDetailRequest,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        self.begin(
            input,
            DesktopLibraryServiceEnumAction::Import(DesktopLibraryServiceEnumImportAction::Detail(
                request,
            )),
        )
    }

    pub fn import_choose(
        &mut self,
        input: DesktopRequestId,
        ordinal: u64,
        choice: StorageImportSessionEnumChoice,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        self.begin(
            input,
            DesktopLibraryServiceEnumAction::Import(
                DesktopLibraryServiceEnumImportAction::Choose { ordinal, choice },
            ),
        )
    }

    pub fn import_freeze(
        &mut self,
        input: DesktopRequestId,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        self.begin(
            input,
            DesktopLibraryServiceEnumAction::Import(DesktopLibraryServiceEnumImportAction::Freeze),
        )
    }

    pub fn import_commit(
        &mut self,
        input: DesktopRequestId,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        if self.has_input(input) {
            return Ok(None);
        }
        let confirmation = self
            .import
            .confirmation
            .clone()
            .ok_or(PresentationLibraryError::MissingConfirmation)?;
        self.begin(
            input,
            DesktopLibraryServiceEnumAction::Import(DesktopLibraryServiceEnumImportAction::Commit(
                confirmation,
            )),
        )
    }

    pub fn import_cancel(
        &mut self,
        input: DesktopRequestId,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        self.begin(
            input,
            DesktopLibraryServiceEnumAction::Import(DesktopLibraryServiceEnumImportAction::Cancel),
        )
    }

    pub fn export_game(
        &mut self,
        input: DesktopRequestId,
        export: DesktopLibraryServiceExport,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        self.begin(input, DesktopLibraryServiceEnumAction::Export(export))
    }

    pub const fn import(&self) -> &PresentationLibraryImport {
        &self.import
    }

    pub const fn export_bytes(&self) -> Option<u64> {
        self.export_bytes
    }

    pub fn can_retry_open(&self) -> bool {
        self.pending.iter().any(|pending| pending.lane == PresentationLibraryEnumLane::Open && pending.certified_open().is_some())
            && self.failures[PresentationLibraryEnumLane::Open.index()].as_ref().is_some_and(|error| matches!(error.disposition(), crate::desktop::library_service::DesktopLibraryServiceEnumDisposition::Retained { expected: Some(_), .. }))
    }

    pub fn retry_open(
        &mut self,
        input: DesktopRequestId,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        if self.has_input(input) {
            return Ok(None);
        }
        if !self.can_retry_open() {
            return Err(PresentationLibraryError::Busy);
        }
        let index = self
            .pending
            .iter()
            .position(|pending| pending.lane == PresentationLibraryEnumLane::Open)
            .ok_or(PresentationLibraryError::UnexpectedReply)?;
        let expected = self.pending[index]
            .certified_open()
            .ok_or(PresentationLibraryError::ForeignContinuation)?;
        let intent = PresentationLibraryIntent::new(input, DesktopLibraryServiceOperation::new(self.workspace, DesktopLibraryServiceEnumAction::Editor { expected, action: crate::desktop::annotation_commands::DesktopAnnotationCommandsEnumAction::RetrySave }));
        let previous = self.pending[index].intent.input_id();
        self.pending[index].begin_open_retry(intent.clone())?;
        self.remember_input(previous);
        self.failures[PresentationLibraryEnumLane::Open.index()] = None;
        Ok(Some(intent))
    }

    pub fn attach_request(
        &mut self,
        intent: &PresentationLibraryIntent,
        request: DesktopRequest,
    ) -> Result<(), PresentationLibraryError> {
        self.pending_intent_mut(intent)?.attach_request(request)
    }

    pub fn attach_receipt(
        &mut self,
        intent: &PresentationLibraryIntent,
        receipt: DesktopApplicationReceipt,
    ) -> Result<(), PresentationLibraryError> {
        self.pending_intent_mut(intent)?.attach_receipt(receipt)
    }

    pub fn retained_request(&self, intent: &PresentationLibraryIntent) -> Option<&DesktopRequest> {
        self.pending
            .iter()
            .find_map(|pending| pending.retained_request(intent))
    }

    pub fn retained_receipt(
        &self,
        intent: &PresentationLibraryIntent,
    ) -> Option<&DesktopApplicationReceipt> {
        self.pending
            .iter()
            .find_map(|pending| pending.retained_receipt(intent))
    }

    pub fn abandon_intent(
        &mut self,
        intent: &PresentationLibraryIntent,
    ) -> Result<(), PresentationLibraryError> {
        let index = self
            .pending
            .iter()
            .position(|pending| pending.intent == *intent)
            .ok_or(PresentationLibraryError::UnexpectedReply)?;
        if self.pending[index].request().is_some() || self.pending[index].certified_open().is_some()
        {
            return Err(PresentationLibraryError::AlreadyBound);
        }
        self.finish(index);
        Ok(())
    }

    fn begin(
        &mut self,
        input: DesktopRequestId,
        action: DesktopLibraryServiceEnumAction,
    ) -> Result<Option<PresentationLibraryIntent>, PresentationLibraryError> {
        if self.has_input(input) {
            return Ok(None);
        }
        let operation = DesktopLibraryServiceOperation::new(self.workspace, action);
        let lane = PresentationLibraryEnumLane::operation(&operation)?;
        if self.is_pending(lane) {
            return Err(PresentationLibraryError::Busy);
        }
        let intent = PresentationLibraryIntent::new(input, operation);
        self.failures[lane.index()] = None;
        self.pending
            .push(PresentationLibraryPending::new(intent.clone(), lane));
        Ok(Some(intent))
    }

    fn has_input(&self, input: DesktopRequestId) -> bool {
        self.recent.contains(&input)
            || self.pending.iter().any(|pending| {
                pending.intent.input_id() == input || pending.original_input() == Some(input)
            })
    }

    fn finish(&mut self, index: usize) {
        let pending = self.pending.remove(index);
        if let Some(input) = pending.original_input() {
            self.remember_input(input);
        }
        self.remember_input(pending.intent.input_id());
    }

    fn remember_input(&mut self, input: DesktopRequestId) {
        if self.recent.contains(&input) {
            return;
        }
        if self.recent.len() == Self::RECENT_INPUT_CAPACITY {
            self.recent.pop_front();
        }
        self.recent.push_back(input);
    }

    fn pending_intent_mut(
        &mut self,
        intent: &PresentationLibraryIntent,
    ) -> Result<&mut PresentationLibraryPending, PresentationLibraryError> {
        self.pending
            .iter_mut()
            .find(|pending| pending.intent == *intent)
            .ok_or(PresentationLibraryError::UnexpectedReply)
    }

    fn bound(
        &self,
        receipt: &DesktopApplicationReceipt,
        expected: &DesktopLibraryServiceOperation,
    ) -> Result<usize, PresentationLibraryError> {
        let index = self
            .pending
            .iter()
            .position(|pending| pending.intent.operation() == expected)
            .ok_or(PresentationLibraryError::UnexpectedReply)?;
        self.pending[index].check(receipt)?;
        Ok(index)
    }

    pub(crate) fn is_pending(&self, lane: PresentationLibraryEnumLane) -> bool {
        self.pending.iter().any(|pending| pending.lane == lane)
    }

    pub(crate) fn failure(
        &self,
        lane: PresentationLibraryEnumLane,
    ) -> Option<&DesktopLibraryServiceError> {
        self.failures[lane.index()].as_deref()
    }
}
