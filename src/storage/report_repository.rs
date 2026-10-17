use std::num::TryFromIntError;

use diesel::SqliteConnection;
use diesel::dsl::max;
use diesel::prelude::*;
use thiserror::Error;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::storage::document_store::{StorageDocumentClient, StorageDocumentError};
use crate::storage::schema::{report_jobs, report_results};

mod rows;

use rows::{
    StorageReportJobRow, StorageReportNewJob, StorageReportNewResult, StorageReportResultRow,
};

#[derive(Debug, Error)]
pub enum StorageReportError {
    #[error(transparent)]
    Document(#[from] StorageDocumentError),
    #[error(transparent)]
    Diesel(#[from] diesel::result::Error),
    #[error("The report database reply channel closed: {0}")]
    Channel(#[from] oneshot::error::RecvError),
    #[error("The report database value exceeds its supported range: {0}")]
    Range(#[from] TryFromIntError),
    #[error("The stored report identifier is invalid: {0}")]
    Identifier(#[from] uuid::Error),
    #[error("The stored report revision is invalid: {0}")]
    Revision(#[from] std::num::ParseIntError),
    #[error("The report job does not exist: {0}")]
    MissingJob(Uuid),
    #[error("The report job already exists: {0}")]
    ExistingJob(Uuid),
    #[error("A report job needs between 1 and {0} positions")]
    PositionCount(u32),
    #[error("A report body or configuration exceeds {0} bytes")]
    BodyTooLarge(usize),
    #[error("Report position {index} is outside the job's {count} positions")]
    PositionIndex { index: u32, count: u32 },
    #[error("Report position {0} already holds a different completed result")]
    ResultConflict(u32),
    #[error("The report job is {0:?} and cannot change")]
    Terminal(StorageReportEnumState),
    #[error("The report job cannot move from {from:?} to {to:?}")]
    Transition {
        from: StorageReportEnumState,
        to: StorageReportEnumState,
    },
    #[error("The report job has {recorded} of {count} positions and cannot complete")]
    Incomplete { recorded: u32, count: u32 },
    #[error("The stored report rows are inconsistent")]
    Corrupt,
    #[error("The report queue ordinal is exhausted")]
    QueueExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageReportEnumState {
    Queued,
    Paused,
    Cancelled,
    Completed,
}

impl StorageReportEnumState {
    const fn sql(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Paused => "paused",
            Self::Cancelled => "cancelled",
            Self::Completed => "completed",
        }
    }

    fn parse(value: &str) -> Result<Self, StorageReportError> {
        match value {
            "queued" => Ok(Self::Queued),
            "paused" => Ok(Self::Paused),
            "cancelled" => Ok(Self::Cancelled),
            "completed" => Ok(Self::Completed),
            _ => Err(StorageReportError::Corrupt),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Completed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageReportEnumFreshness {
    Current,
    Outdated,
}

impl StorageReportEnumFreshness {
    const fn sql(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Outdated => "outdated",
        }
    }

    fn parse(value: &str) -> Result<Self, StorageReportError> {
        match value {
            "current" => Ok(Self::Current),
            "outdated" => Ok(Self::Outdated),
            _ => Err(StorageReportError::Corrupt),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageReportJobRecord {
    job: Uuid,
    game: Uuid,
    revision: u64,
    line_digest: [u8; 32],
    position_count: u32,
    configuration: String,
    source: String,
}

impl StorageReportJobRecord {
    pub const MAX_POSITIONS: u32 = 100_000;
    pub const MAX_BODY_BYTES: usize = 1 << 20;

    pub fn new(
        job: Uuid,
        game: Uuid,
        revision: u64,
        line_digest: [u8; 32],
        position_count: u32,
        configuration: String,
        source: String,
    ) -> Result<Self, StorageReportError> {
        if position_count == 0 || position_count > Self::MAX_POSITIONS {
            return Err(StorageReportError::PositionCount(Self::MAX_POSITIONS));
        }
        if configuration.len() > Self::MAX_BODY_BYTES || source.len() > Self::MAX_BODY_BYTES {
            return Err(StorageReportError::BodyTooLarge(Self::MAX_BODY_BYTES));
        }
        Ok(Self {
            job,
            game,
            revision,
            line_digest,
            position_count,
            configuration,
            source,
        })
    }

    pub const fn job(&self) -> Uuid {
        self.job
    }

    pub const fn game(&self) -> Uuid {
        self.game
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn line_digest(&self) -> [u8; 32] {
        self.line_digest
    }

    pub const fn position_count(&self) -> u32 {
        self.position_count
    }

    pub fn configuration(&self) -> &str {
        &self.configuration
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageReportResultRecord {
    index: u32,
    body: String,
}

impl StorageReportResultRecord {
    pub fn new(index: u32, body: String) -> Result<Self, StorageReportError> {
        if body.len() > StorageReportJobRecord::MAX_BODY_BYTES {
            return Err(StorageReportError::BodyTooLarge(
                StorageReportJobRecord::MAX_BODY_BYTES,
            ));
        }
        Ok(Self { index, body })
    }

    pub const fn index(&self) -> u32 {
        self.index
    }

    pub fn body(&self) -> &str {
        &self.body
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageReportJobSnapshot {
    record: StorageReportJobRecord,
    queue_ordinal: u32,
    state: StorageReportEnumState,
    freshness: StorageReportEnumFreshness,
    results: Vec<StorageReportResultRecord>,
}

impl StorageReportJobSnapshot {
    pub const fn record(&self) -> &StorageReportJobRecord {
        &self.record
    }

    pub const fn queue_ordinal(&self) -> u32 {
        self.queue_ordinal
    }

    pub const fn state(&self) -> StorageReportEnumState {
        self.state
    }

    pub const fn freshness(&self) -> StorageReportEnumFreshness {
        self.freshness
    }

    pub fn results(&self) -> &[StorageReportResultRecord] {
        &self.results
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageReportJobSummary {
    job: Uuid,
    game: Uuid,
    revision: u64,
    queue_ordinal: u32,
    state: StorageReportEnumState,
    freshness: StorageReportEnumFreshness,
    position_count: u32,
    completed: u32,
}

impl StorageReportJobSummary {
    pub const fn job(&self) -> Uuid {
        self.job
    }

    pub const fn game(&self) -> Uuid {
        self.game
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn queue_ordinal(&self) -> u32 {
        self.queue_ordinal
    }

    pub const fn state(&self) -> StorageReportEnumState {
        self.state
    }

    pub const fn freshness(&self) -> StorageReportEnumFreshness {
        self.freshness
    }

    pub const fn position_count(&self) -> u32 {
        self.position_count
    }

    pub const fn completed(&self) -> u32 {
        self.completed
    }
}

#[derive(Debug)]
pub(crate) enum StorageReportEnumRequest {
    Insert(StorageReportJobRecord),
    SaveResult {
        job: Uuid,
        result: StorageReportResultRecord,
    },
    SetState {
        job: Uuid,
        state: StorageReportEnumState,
    },
    MarkOutdated {
        game: Uuid,
        current_digest: [u8; 32],
    },
    Recover,
    Load(Uuid),
    List {
        after: Option<u32>,
    },
    Delete(Uuid),
}

#[derive(Debug)]
pub(crate) enum StorageReportEnumResponse {
    Inserted(u32),
    Saved,
    StateChanged,
    Outdated(u64),
    Recovered(u64),
    Job(Option<Box<StorageReportJobSnapshot>>),
    Page(Vec<StorageReportJobSummary>),
    Deleted(bool),
}

pub(crate) struct StorageReportTransaction;

impl StorageReportTransaction {
    const PAGE_SIZE: i64 = 100;

    pub(crate) fn execute(
        connection: &mut SqliteConnection,
        request: StorageReportEnumRequest,
    ) -> Result<StorageReportEnumResponse, StorageReportError> {
        match request {
            StorageReportEnumRequest::Load(job) => connection.transaction(|connection| {
                Self::load(connection, job).map(StorageReportEnumResponse::Job)
            }),
            StorageReportEnumRequest::List { after } => connection.transaction(|connection| {
                Self::list(connection, after).map(StorageReportEnumResponse::Page)
            }),
            write => connection.immediate_transaction(|connection| match write {
                StorageReportEnumRequest::Insert(record) => {
                    Self::insert(connection, &record).map(StorageReportEnumResponse::Inserted)
                }
                StorageReportEnumRequest::SaveResult { job, result } => {
                    Self::save_result(connection, job, &result)
                        .map(|()| StorageReportEnumResponse::Saved)
                }
                StorageReportEnumRequest::SetState { job, state } => {
                    Self::set_state(connection, job, state)
                        .map(|()| StorageReportEnumResponse::StateChanged)
                }
                StorageReportEnumRequest::MarkOutdated {
                    game,
                    current_digest,
                } => Self::mark_outdated(connection, game, current_digest)
                    .map(StorageReportEnumResponse::Outdated),
                StorageReportEnumRequest::Recover => {
                    Self::recover(connection).map(StorageReportEnumResponse::Recovered)
                }
                StorageReportEnumRequest::Delete(job) => diesel::delete(
                    report_jobs::table.filter(report_jobs::job_id.eq(job.to_string())),
                )
                .execute(connection)
                .map(|count| StorageReportEnumResponse::Deleted(count == 1))
                .map_err(StorageReportError::from),
                StorageReportEnumRequest::Load(_) | StorageReportEnumRequest::List { .. } => {
                    Err(StorageReportError::Corrupt)
                }
            }),
        }
    }

    fn row(
        connection: &mut SqliteConnection,
        job: Uuid,
    ) -> Result<StorageReportJobRow, StorageReportError> {
        report_jobs::table
            .filter(report_jobs::job_id.eq(job.to_string()))
            .select(StorageReportJobRow::as_select())
            .first(connection)
            .optional()?
            .ok_or(StorageReportError::MissingJob(job))
    }

    fn recorded(connection: &mut SqliteConnection, job: Uuid) -> Result<u32, StorageReportError> {
        let count: i64 = report_results::table
            .filter(report_results::job_id.eq(job.to_string()))
            .count()
            .get_result(connection)?;
        Ok(u32::try_from(count)?)
    }

    fn insert(
        connection: &mut SqliteConnection,
        record: &StorageReportJobRecord,
    ) -> Result<u32, StorageReportError> {
        let id = record.job.to_string();
        let existing: i64 = report_jobs::table
            .filter(report_jobs::job_id.eq(&id))
            .count()
            .get_result(connection)?;
        if existing != 0 {
            return Err(StorageReportError::ExistingJob(record.job));
        }
        let last: Option<i32> = report_jobs::table
            .select(max(report_jobs::queue_ordinal))
            .first(connection)?;
        let ordinal = match last {
            Some(last) => last
                .checked_add(1)
                .ok_or(StorageReportError::QueueExhausted)?,
            None => 0,
        };
        let game = record.game.to_string();
        let revision = record.revision.to_string();
        diesel::insert_into(report_jobs::table)
            .values(StorageReportNewJob {
                job_id: &id,
                game_id: &game,
                source_revision: &revision,
                line_digest: record.line_digest.as_slice(),
                queue_ordinal: ordinal,
                state: StorageReportEnumState::Queued.sql(),
                freshness: StorageReportEnumFreshness::Current.sql(),
                position_count: i32::try_from(record.position_count)?,
                configuration: &record.configuration,
                source: &record.source,
            })
            .execute(connection)?;
        Ok(u32::try_from(ordinal)?)
    }

    fn save_result(
        connection: &mut SqliteConnection,
        job: Uuid,
        result: &StorageReportResultRecord,
    ) -> Result<(), StorageReportError> {
        let row = Self::row(connection, job)?;
        let state = StorageReportEnumState::parse(&row.state)?;
        let count = u32::try_from(row.position_count)?;
        if result.index >= count {
            return Err(StorageReportError::PositionIndex {
                index: result.index,
                count,
            });
        }
        let id = job.to_string();
        let index = i32::try_from(result.index)?;
        let stored: Option<String> = report_results::table
            .filter(report_results::job_id.eq(&id))
            .filter(report_results::position_index.eq(index))
            .select(report_results::body)
            .first(connection)
            .optional()?;
        match stored {
            Some(body) if body == result.body => Ok(()),
            Some(_) => Err(StorageReportError::ResultConflict(result.index)),
            None => {
                if state.is_terminal() {
                    return Err(StorageReportError::Terminal(state));
                }
                diesel::insert_into(report_results::table)
                    .values(StorageReportNewResult {
                        job_id: &id,
                        position_index: index,
                        body: &result.body,
                    })
                    .execute(connection)?;
                Ok(())
            }
        }
    }

    fn set_state(
        connection: &mut SqliteConnection,
        job: Uuid,
        to: StorageReportEnumState,
    ) -> Result<(), StorageReportError> {
        let row = Self::row(connection, job)?;
        let from = StorageReportEnumState::parse(&row.state)?;
        if from == to {
            return Ok(());
        }
        if from.is_terminal() {
            return Err(StorageReportError::Terminal(from));
        }
        if to == StorageReportEnumState::Completed {
            let count = u32::try_from(row.position_count)?;
            let recorded = Self::recorded(connection, job)?;
            if recorded != count {
                return Err(StorageReportError::Incomplete { recorded, count });
            }
        }
        if to == StorageReportEnumState::Queued
            && StorageReportEnumFreshness::parse(&row.freshness)?
                == StorageReportEnumFreshness::Outdated
        {
            return Err(StorageReportError::Transition { from, to });
        }
        diesel::update(report_jobs::table.filter(report_jobs::job_id.eq(job.to_string())))
            .set(report_jobs::state.eq(to.sql()))
            .execute(connection)?;
        Ok(())
    }

    fn mark_outdated(
        connection: &mut SqliteConnection,
        game: Uuid,
        current_digest: [u8; 32],
    ) -> Result<u64, StorageReportError> {
        let target = report_jobs::table
            .filter(report_jobs::game_id.eq(game.to_string()))
            .filter(report_jobs::freshness.eq(StorageReportEnumFreshness::Current.sql()))
            .filter(report_jobs::line_digest.ne(current_digest.as_slice()));
        let ids: Vec<String> = target.select(report_jobs::job_id).load(connection)?;
        for id in &ids {
            diesel::update(report_jobs::table.filter(report_jobs::job_id.eq(id)))
                .set(report_jobs::freshness.eq(StorageReportEnumFreshness::Outdated.sql()))
                .execute(connection)?;
            diesel::update(
                report_jobs::table
                    .filter(report_jobs::job_id.eq(id))
                    .filter(report_jobs::state.eq(StorageReportEnumState::Queued.sql())),
            )
            .set(report_jobs::state.eq(StorageReportEnumState::Paused.sql()))
            .execute(connection)?;
        }
        Ok(u64::try_from(ids.len())?)
    }

    fn recover(connection: &mut SqliteConnection) -> Result<u64, StorageReportError> {
        let count = diesel::update(
            report_jobs::table.filter(report_jobs::state.eq(StorageReportEnumState::Queued.sql())),
        )
        .set(report_jobs::state.eq(StorageReportEnumState::Paused.sql()))
        .execute(connection)?;
        Ok(u64::try_from(count)?)
    }

    fn snapshot_record(
        row: &StorageReportJobRow,
    ) -> Result<StorageReportJobRecord, StorageReportError> {
        let line_digest: [u8; 32] = row
            .line_digest
            .as_slice()
            .try_into()
            .map_err(|_error| StorageReportError::Corrupt)?;
        StorageReportJobRecord::new(
            row.job_id.parse()?,
            row.game_id.parse()?,
            row.source_revision.parse()?,
            line_digest,
            u32::try_from(row.position_count)?,
            row.configuration.clone(),
            row.source.clone(),
        )
    }

    fn load(
        connection: &mut SqliteConnection,
        job: Uuid,
    ) -> Result<Option<Box<StorageReportJobSnapshot>>, StorageReportError> {
        let row = match Self::row(connection, job) {
            Ok(row) => row,
            Err(StorageReportError::MissingJob(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        let results: Vec<StorageReportResultRow> = report_results::table
            .filter(report_results::job_id.eq(job.to_string()))
            .order(report_results::position_index.asc())
            .select(StorageReportResultRow::as_select())
            .load(connection)?;
        let mut records = Vec::with_capacity(results.len());
        for result in results {
            records.push(StorageReportResultRecord::new(
                u32::try_from(result.position_index)?,
                result.body,
            )?);
        }
        Ok(Some(Box::new(StorageReportJobSnapshot {
            record: Self::snapshot_record(&row)?,
            queue_ordinal: u32::try_from(row.queue_ordinal)?,
            state: StorageReportEnumState::parse(&row.state)?,
            freshness: StorageReportEnumFreshness::parse(&row.freshness)?,
            results: records,
        })))
    }

    fn list(
        connection: &mut SqliteConnection,
        after: Option<u32>,
    ) -> Result<Vec<StorageReportJobSummary>, StorageReportError> {
        let mut query = report_jobs::table
            .select(StorageReportJobRow::as_select())
            .order(report_jobs::queue_ordinal.asc())
            .limit(Self::PAGE_SIZE)
            .into_boxed();
        if let Some(after) = after {
            query = query.filter(report_jobs::queue_ordinal.gt(i32::try_from(after)?));
        }
        let rows: Vec<StorageReportJobRow> = query.load(connection)?;
        let mut page = Vec::with_capacity(rows.len());
        for row in rows {
            let job: Uuid = row.job_id.parse()?;
            page.push(StorageReportJobSummary {
                job,
                game: row.game_id.parse()?,
                revision: row.source_revision.parse()?,
                queue_ordinal: u32::try_from(row.queue_ordinal)?,
                state: StorageReportEnumState::parse(&row.state)?,
                freshness: StorageReportEnumFreshness::parse(&row.freshness)?,
                position_count: u32::try_from(row.position_count)?,
                completed: Self::recorded(connection, job)?,
            });
        }
        Ok(page)
    }
}

pub struct StorageReportRepository {
    client: StorageDocumentClient,
}

impl StorageReportRepository {
    pub const fn new(client: StorageDocumentClient) -> Self {
        Self { client }
    }

    pub async fn insert(&self, record: StorageReportJobRecord) -> Result<u32, StorageReportError> {
        match self
            .client
            .report(StorageReportEnumRequest::Insert(record))
            .await?
        {
            StorageReportEnumResponse::Inserted(ordinal) => Ok(ordinal),
            _ => Err(StorageReportError::Corrupt),
        }
    }

    pub async fn save_result(
        &self,
        job: Uuid,
        result: StorageReportResultRecord,
    ) -> Result<(), StorageReportError> {
        match self
            .client
            .report(StorageReportEnumRequest::SaveResult { job, result })
            .await?
        {
            StorageReportEnumResponse::Saved => Ok(()),
            _ => Err(StorageReportError::Corrupt),
        }
    }

    pub async fn set_state(
        &self,
        job: Uuid,
        state: StorageReportEnumState,
    ) -> Result<(), StorageReportError> {
        match self
            .client
            .report(StorageReportEnumRequest::SetState { job, state })
            .await?
        {
            StorageReportEnumResponse::StateChanged => Ok(()),
            _ => Err(StorageReportError::Corrupt),
        }
    }

    pub async fn mark_outdated(
        &self,
        game: Uuid,
        current_digest: [u8; 32],
    ) -> Result<u64, StorageReportError> {
        match self
            .client
            .report(StorageReportEnumRequest::MarkOutdated {
                game,
                current_digest,
            })
            .await?
        {
            StorageReportEnumResponse::Outdated(count) => Ok(count),
            _ => Err(StorageReportError::Corrupt),
        }
    }

    pub async fn recover_after_relaunch(&self) -> Result<u64, StorageReportError> {
        match self
            .client
            .report(StorageReportEnumRequest::Recover)
            .await?
        {
            StorageReportEnumResponse::Recovered(count) => Ok(count),
            _ => Err(StorageReportError::Corrupt),
        }
    }

    pub async fn load(
        &self,
        job: Uuid,
    ) -> Result<Option<StorageReportJobSnapshot>, StorageReportError> {
        match self
            .client
            .report(StorageReportEnumRequest::Load(job))
            .await?
        {
            StorageReportEnumResponse::Job(snapshot) => Ok(snapshot.map(|boxed| *boxed)),
            _ => Err(StorageReportError::Corrupt),
        }
    }

    pub async fn list(
        &self,
        after: Option<u32>,
    ) -> Result<Vec<StorageReportJobSummary>, StorageReportError> {
        match self
            .client
            .report(StorageReportEnumRequest::List { after })
            .await?
        {
            StorageReportEnumResponse::Page(page) => Ok(page),
            _ => Err(StorageReportError::Corrupt),
        }
    }

    pub async fn delete(&self, job: Uuid) -> Result<bool, StorageReportError> {
        match self
            .client
            .report(StorageReportEnumRequest::Delete(job))
            .await?
        {
            StorageReportEnumResponse::Deleted(deleted) => Ok(deleted),
            _ => Err(StorageReportError::Corrupt),
        }
    }
}
