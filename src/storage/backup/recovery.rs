use std::fs;
use std::path::Path;

use crate::storage::backup::error::BackupError;
use crate::storage::backup::journal::{BackupRestoreEnumPhase, BackupRestoreJournal};
use crate::storage::backup::restore::BackupRestorer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupRecoveryEnumOutcome {
    NothingToRecover,
    RolledBack,
    Completed,
}

pub struct BackupRecovery;

impl BackupRecovery {
    pub fn recover_blocking(
        data_directory: &Path,
        database: &Path,
    ) -> Result<BackupRecoveryEnumOutcome, BackupError> {
        let Some(journal) = BackupRestoreJournal::read_blocking(data_directory)? else {
            return Ok(BackupRecoveryEnumOutcome::NothingToRecover);
        };
        let staged = BackupRestoreJournal::staged_path(data_directory, journal.restore_id());
        let aside = BackupRestoreJournal::aside_path(database, journal.restore_id());
        let live_present = database.exists();
        let staged_present = staged.exists();
        let aside_present = aside.exists();
        let outcome = match (journal.phase(), live_present, aside_present, staged_present) {
            (BackupRestoreEnumPhase::Pending, true, false, true) => {
                BackupRestorer::install_blocking(data_directory, database, &journal)?;
                return Ok(BackupRecoveryEnumOutcome::Completed);
            }
            (BackupRestoreEnumPhase::Pending, true, false, false) => {
                BackupRecoveryEnumOutcome::RolledBack
            }
            (BackupRestoreEnumPhase::Validated, true, false, _)
            | (BackupRestoreEnumPhase::Installing, true, false, true) => {
                Self::remove_if_present(&staged)?;
                BackupRecoveryEnumOutcome::RolledBack
            }
            (BackupRestoreEnumPhase::Installing, false, true, true) => {
                fs::rename(&staged, database)?;
                fs::remove_file(&aside)?;
                BackupRecoveryEnumOutcome::Completed
            }
            (BackupRestoreEnumPhase::Installing, true, true, false) => {
                fs::remove_file(&aside)?;
                BackupRecoveryEnumOutcome::Completed
            }
            (BackupRestoreEnumPhase::Installing, true, false, false) => {
                BackupRecoveryEnumOutcome::Completed
            }
            (BackupRestoreEnumPhase::Installing, false, true, false) => {
                fs::rename(&aside, database)?;
                BackupRecoveryEnumOutcome::RolledBack
            }
            _ => return Err(BackupError::JournalInconsistent),
        };
        BackupRestoreJournal::sync_directory(data_directory)?;
        BackupRestoreJournal::remove_blocking(data_directory)?;
        Ok(outcome)
    }

    fn remove_if_present(path: &Path) -> Result<(), BackupError> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}
