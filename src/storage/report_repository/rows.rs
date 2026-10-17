use diesel::prelude::*;

use crate::storage::schema::{report_jobs, report_results};

#[derive(Insertable)]
#[diesel(table_name = report_jobs)]
pub(super) struct StorageReportNewJob<'a> {
    pub(super) job_id: &'a str,
    pub(super) game_id: &'a str,
    pub(super) source_revision: &'a str,
    pub(super) line_digest: &'a [u8],
    pub(super) queue_ordinal: i32,
    pub(super) state: &'a str,
    pub(super) freshness: &'a str,
    pub(super) position_count: i32,
    pub(super) configuration: &'a str,
    pub(super) source: &'a str,
}

#[derive(Insertable)]
#[diesel(table_name = report_results)]
pub(super) struct StorageReportNewResult<'a> {
    pub(super) job_id: &'a str,
    pub(super) position_index: i32,
    pub(super) body: &'a str,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = report_jobs)]
pub(super) struct StorageReportJobRow {
    pub(super) job_id: String,
    pub(super) game_id: String,
    pub(super) source_revision: String,
    pub(super) line_digest: Vec<u8>,
    pub(super) queue_ordinal: i32,
    pub(super) state: String,
    pub(super) freshness: String,
    pub(super) position_count: i32,
    pub(super) configuration: String,
    pub(super) source: String,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = report_results)]
pub(super) struct StorageReportResultRow {
    pub(super) position_index: i32,
    pub(super) body: String,
}
