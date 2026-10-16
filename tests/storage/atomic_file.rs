use chess::storage::atomic_file::{
    StorageAtomicFileError, StorageAtomicFileStage, StorageAtomicFileTarget,
};

#[test]
fn atomic_staging_preserves_old_data_until_commit_and_refuses_stale_targets()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("game.pgn");
    std::fs::write(&path, "original")?;
    let target = StorageAtomicFileTarget::capture(path.clone())?;
    let mut stage = StorageAtomicFileStage::new(target)?;
    stage.write(b"replacement")?;
    assert_eq!(std::fs::read(&path)?, b"original");
    drop(stage);
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    let target = StorageAtomicFileTarget::capture(path.clone())?;
    let mut stage = StorageAtomicFileStage::new(target)?;
    stage.write(b"replacement")?;
    stage.commit()?;
    assert_eq!(std::fs::read(&path)?, b"replacement");
    let stale = StorageAtomicFileTarget::capture(path.clone())?;
    let mut stage = StorageAtomicFileStage::new(stale)?;
    stage.write(b"must not overwrite")?;
    std::fs::write(&path, "external change")?;
    assert!(stage.commit().is_err());
    assert_eq!(std::fs::read(&path)?, b"external change");
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    Ok(())
}

#[test]
fn a_new_destination_never_silently_overwrites_an_appearing_file()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("new.pgn");
    let target = StorageAtomicFileTarget::capture(path.clone())?;
    let mut stage = StorageAtomicFileStage::new(target)?;
    stage.write(b"new")?;
    std::fs::write(&path, "unexpected")?;
    assert!(stage.commit().is_err());
    assert_eq!(std::fs::read(&path)?, b"unexpected");
    Ok(())
}

#[test]
fn publication_guard_cancels_after_staging_without_replacing_either_target_kind()
-> Result<(), Box<dyn std::error::Error>> {
    for existing in [false, true] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("diagnostic.json");
        if existing {
            std::fs::write(&path, b"original")?;
        }
        let target = StorageAtomicFileTarget::capture(path.clone())?;
        let mut stage = StorageAtomicFileStage::new(target)?;
        stage.write(b"frozen export")?;
        let mut observed = false;
        let result = stage.commit_if(|| {
            observed = true;
            true
        });
        assert!(observed);
        assert!(matches!(result, Err(StorageAtomicFileError::Cancelled)));
        assert_eq!(path.exists(), existing);
        if existing {
            assert_eq!(std::fs::read(&path)?, b"original");
        }
        assert_eq!(
            std::fs::read_dir(directory.path())?.count(),
            usize::from(existing)
        );
    }
    Ok(())
}

#[test]
fn publication_guard_runs_only_after_target_validation_and_preserves_its_source_error()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("diagnostic.json");
    let mut stage = StorageAtomicFileStage::new(StorageAtomicFileTarget::capture(path.clone())?)?;
    stage.write(b"frozen export")?;
    std::fs::write(&path, b"appeared after selection")?;
    let mut observed = false;
    let result = stage.commit_if(|| {
        observed = true;
        true
    });
    assert!(!observed);
    assert!(matches!(
        result,
        Err(StorageAtomicFileError::StaleSelection)
    ));
    assert_eq!(std::fs::read(&path)?, b"appeared after selection");
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    Ok(())
}

#[test]
fn publication_errors_never_turn_into_cancellation_or_a_successful_byte_count()
-> Result<(), Box<dyn std::error::Error>> {
    use std::error::Error;

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("diagnostic.json");
    let mut stage = StorageAtomicFileStage::new(StorageAtomicFileTarget::capture(path.clone())?)?;
    stage.write(b"frozen export")?;
    let mut interference = Ok(());
    let result = stage.commit_if(|| {
        interference = std::fs::create_dir(&path);
        false
    });
    interference?;
    let error = result.err().ok_or("Publication failure returned success")?;
    assert!(matches!(error, StorageAtomicFileError::Io(_)));
    assert!(error.source().is_some());
    assert!(path.is_dir());
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    Ok(())
}
