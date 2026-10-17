mod observation;

use std::collections::VecDeque;
use std::sync::Arc;

use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::identity::DesktopRequestId;
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceError, DesktopLibraryServiceOperation,
};
use crate::desktop::messages::{DesktopMessagesEntityId, DesktopRequest};
use crate::game::enums::GameId;
use crate::game::library_organization::GameLibraryOrganizationFolderId;
use crate::game::storage_repository::GameStorageRevision;
use crate::presentation::library::enums::PresentationLibraryEnumLane;
use crate::presentation::library::error::PresentationLibraryError;
use crate::presentation::library::intent::{PresentationLibraryIntent, PresentationLibraryPending};
use crate::storage::library_metadata::{
    StorageLibraryMetadataEnumRequest, StorageLibraryMetadataFolderPage,
    StorageLibraryMetadataFolderRead, StorageLibraryMetadataTagPage,
};
use crate::storage::library_query::{
    StorageLibraryQuery, StorageLibraryQueryCursor, StorageLibraryQueryOptions,
    StorageLibraryQueryPage,
};

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
}

impl PresentationLibraryController {
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
