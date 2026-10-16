use chess::storage::atomic_file::{StorageAtomicFileStage, StorageAtomicFileTarget};

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
