use std::path::Path;

use chess::desktop::native::resources::{DesktopNativeResources, DesktopNativeResourcesError};

struct DesktopNativeResourcesFixture;

impl DesktopNativeResourcesFixture {
    fn prepare() -> Result<tempfile::TempDir, std::io::Error> {
        let directory = tempfile::tempdir()?;
        let pieces = directory.path().join("pieces");
        std::fs::create_dir(&pieces)?;
        std::fs::write(
            pieces.join("chess.glb"),
            include_bytes!("../../resources/pieces/chess.glb"),
        )?;
        Ok(directory)
    }
}

#[test]
fn packaged_resources_resolve_absolute_assets_without_requiring_engine_installation()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = DesktopNativeResourcesFixture::prepare()?;
    let resources = DesktopNativeResources::from_directory(directory.path())?;
    assert_eq!(resources.root(), directory.path().canonicalize()?);
    assert_eq!(
        resources.pieces(),
        directory.path().join("pieces/chess.glb").canonicalize()?
    );
    assert_eq!(resources.engine(), resources.root().join("engine"));
    assert!(!resources.engine().exists());
    Ok(())
}

#[test]
fn relative_resource_roots_are_rejected_before_filesystem_access() {
    assert!(matches!(
        DesktopNativeResources::from_directory(Path::new("resources")),
        Err(DesktopNativeResourcesError::Relative(_))
    ));
}

#[test]
fn altered_packaged_piece_geometry_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let directory = DesktopNativeResourcesFixture::prepare()?;
    let path = directory.path().join("pieces/chess.glb");
    let mut bytes = std::fs::read(&path)?;
    let byte = bytes.get_mut(100).ok_or("piece fixture is truncated")?;
    *byte ^= 1;
    std::fs::write(&path, bytes)?;
    assert!(matches!(
        DesktopNativeResources::from_directory(directory.path()),
        Err(DesktopNativeResourcesError::Integrity(_))
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn piece_symlinks_cannot_escape_the_packaged_resource_root()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let outside = DesktopNativeResourcesFixture::prepare()?;
    std::fs::create_dir(directory.path().join("pieces"))?;
    std::os::unix::fs::symlink(
        outside.path().join("pieces/chess.glb"),
        directory.path().join("pieces/chess.glb"),
    )?;
    assert!(matches!(
        DesktopNativeResources::from_directory(directory.path()),
        Err(DesktopNativeResourcesError::InvalidFile(_))
    ));
    Ok(())
}
