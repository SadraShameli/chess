use diesel::SqliteConnection;
use diesel::prelude::*;

use crate::storage::backup::error::BackupError;
use crate::storage::backup::manifest::BackupEnumCategory;
use crate::storage::schema::backup_state;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupState {
    first_user_data_ms: Option<u64>,
    last_automatic_ms: Option<u64>,
    warning: Option<String>,
}

impl BackupState {
    pub const fn new(
        first_user_data_ms: Option<u64>,
        last_automatic_ms: Option<u64>,
        warning: Option<String>,
    ) -> Self {
        Self {
            first_user_data_ms,
            last_automatic_ms,
            warning,
        }
    }

    pub const fn first_user_data_ms(&self) -> Option<u64> {
        self.first_user_data_ms
    }

    pub const fn last_automatic_ms(&self) -> Option<u64> {
        self.last_automatic_ms
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupEnumDue {
    NoUserData,
    NotYet { due_ms: u64 },
    Due { overdue_ms: u64 },
}

pub struct BackupScheduler {
    attempted_this_session: bool,
}

impl Default for BackupScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl BackupScheduler {
    pub const INTERVAL_MS: u64 = 24 * 60 * 60 * 1000;
    pub const MAX_WARNING_BYTES: usize = 2048;

    pub const fn new() -> Self {
        Self {
            attempted_this_session: false,
        }
    }

    pub fn due(state: &BackupState, now_ms: u64) -> BackupEnumDue {
        let anchor = state.last_automatic_ms.or(state.first_user_data_ms);
        let Some(anchor) = anchor else {
            return BackupEnumDue::NoUserData;
        };
        let due_ms = anchor.saturating_add(Self::INTERVAL_MS);
        if now_ms >= due_ms {
            BackupEnumDue::Due {
                overdue_ms: now_ms - due_ms,
            }
        } else {
            BackupEnumDue::NotYet { due_ms }
        }
    }

    pub fn take_attempt(&mut self, state: &BackupState, now_ms: u64) -> bool {
        if self.attempted_this_session
            || !matches!(Self::due(state, now_ms), BackupEnumDue::Due { .. })
        {
            return false;
        }
        self.attempted_this_session = true;
        true
    }

    pub fn finish_attempt(&mut self, succeeded: bool) {
        if succeeded {
            self.attempted_this_session = false;
        }
    }

    pub fn load_blocking(connection: &mut SqliteConnection) -> Result<BackupState, BackupError> {
        let (first, last, warning) = backup_state::table
            .select((
                backup_state::first_user_data_ms,
                backup_state::last_automatic_ms,
                backup_state::automatic_warning,
            ))
            .first::<(Option<String>, Option<String>, Option<String>)>(connection)?;
        Ok(BackupState::new(
            first.map(|value| value.parse::<u64>()).transpose()?,
            last.map(|value| value.parse::<u64>()).transpose()?,
            warning,
        ))
    }

    pub fn record_user_data_blocking(
        connection: &mut SqliteConnection,
        now_ms: u64,
    ) -> Result<(), BackupError> {
        diesel::update(backup_state::table.filter(backup_state::first_user_data_ms.is_null()))
            .set(backup_state::first_user_data_ms.eq(Some(now_ms.to_string())))
            .execute(connection)?;
        Ok(())
    }

    pub fn record_success_blocking(
        connection: &mut SqliteConnection,
        category: BackupEnumCategory,
        now_ms: u64,
    ) -> Result<(), BackupError> {
        match category {
            BackupEnumCategory::Automatic => {
                diesel::update(backup_state::table)
                    .set((
                        backup_state::last_automatic_ms.eq(Some(now_ms.to_string())),
                        backup_state::automatic_warning.eq(None::<String>),
                    ))
                    .execute(connection)?;
            }
            BackupEnumCategory::Manual => {
                diesel::update(backup_state::table)
                    .set(backup_state::automatic_warning.eq(None::<String>))
                    .execute(connection)?;
            }
            BackupEnumCategory::PreRestore | BackupEnumCategory::PreUpdate => {}
        }
        Ok(())
    }

    pub fn record_failure_blocking(
        connection: &mut SqliteConnection,
        message: &str,
    ) -> Result<(), BackupError> {
        let mut end = message.len().min(Self::MAX_WARNING_BYTES);
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        diesel::update(backup_state::table)
            .set(backup_state::automatic_warning.eq(Some(message[..end].to_owned())))
            .execute(connection)?;
        Ok(())
    }
}
