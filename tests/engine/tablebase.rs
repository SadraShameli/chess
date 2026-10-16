use std::path::Path;

use chess::engine::tablebase::location::{
    EngineTablebaseLocationEnumPlatform, EngineTablebaseLocationError,
};

#[test]
fn alias_syntax_checks_complete_paths_and_platform_path_list_delimiters()
-> Result<(), Box<dyn std::error::Error>> {
    let unix = EngineTablebaseLocationEnumPlatform::Unix;
    let windows = EngineTablebaseLocationEnumPlatform::Windows;
    for value in ["/home/user name/tables", "/tmp/a\tb", "/tmp/a\nb"] {
        assert!(matches!(
            unix.validate_alias_path(Path::new(value)),
            Err(EngineTablebaseLocationError::AliasSyntax { .. })
        ));
    }
    assert!(unix.validate_alias_path(Path::new("/tmp/a:b")).is_err());
    assert!(
        windows
            .validate_alias_path(Path::new("C:\\tb;D:\\tb"))
            .is_err()
    );
    windows.validate_alias_path(Path::new("C:\\tb"))?;
    unix.validate_alias_path(Path::new("/tmp/tables"))?;
    Ok(())
}

#[cfg(unix)]
mod engine_tablebase_unix {
    use std::cell::Cell;
    use std::fs::{self, Permissions};
    use std::num::NonZeroU32;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::PathBuf;

    use chess::engine::enums::EngineEnumOption;
    use chess::engine::options::{
        EngineOptionsEnumOutcome, EngineOptionsEnumTablebases, EngineOptionsSyzygyPath,
    };
    use chess::engine::process_provider::EngineProcessEnumPipe;
    use chess::engine::search::enums::EngineSearchEnumState;
    use chess::engine::tablebase::location::{
        EngineTablebaseLocation, EngineTablebaseLocationError, EngineTablebaseLocationPolicy,
        EngineTablebaseLocationSelection,
    };
    use chess::engine::tablebase::status::{
        EngineTablebaseStatus, EngineTablebaseStatusEnumState, EngineTablebaseStatusError,
    };
    use tempfile::TempDir;

    use crate::engine_lifecycle::EngineLifecycleProvider;
    use crate::engine_package_selection::EnginePackageFixture;

    struct EngineTablebaseFixture {
        directory: TempDir,
        sources: PathBuf,
        aliases: PathBuf,
    }

    impl EngineTablebaseFixture {
        fn new() -> Result<Self, Box<dyn std::error::Error>> {
            let directory = tempfile::Builder::new()
                .prefix("chess-tb-test-")
                .tempdir_in("/tmp")?;
            let sources = directory.path().join("owner with spaces");
            let aliases = directory.path().join("aliases");
            fs::create_dir(&sources)?;
            fs::create_dir(&aliases)?;
            fs::write(sources.join("KQvK.rtbw"), b"local WDL fixture")?;
            fs::write(sources.join("KQvK.rtbz"), b"local DTZ fixture")?;
            fs::write(sources.join("README.txt"), b"preserve unrelated files")?;
            Ok(Self {
                directory,
                sources,
                aliases,
            })
        }

        fn selection(
            &self,
        ) -> Result<EngineTablebaseLocationSelection, EngineTablebaseLocationError> {
            EngineTablebaseLocationSelection::new(vec![self.sources.clone()])
        }

        fn prepare(&self) -> Result<EngineTablebaseLocation, EngineTablebaseLocationError> {
            EngineTablebaseLocation::prepare_blocking(
                self.selection()?,
                EngineTablebaseLocationPolicy::unix_temporary(self.aliases.clone()),
                || false,
            )
        }
    }

    #[test]
    fn space_containing_sources_get_private_real_file_links_and_new_roots_after_relaunch()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = EngineTablebaseFixture::new()?;
        let mut first = fixture.prepare()?;
        let first_path = first.path().to_path_buf();
        assert!(!first.engine_path().as_str().contains(char::is_whitespace));
        assert_eq!(first.file_count(), 2);
        assert_eq!(
            fs::metadata(first.path())?.permissions().mode() & 0o777,
            0o700
        );
        for name in ["KQvK.rtbw", "KQvK.rtbz"] {
            let alias = first.path().join(name);
            assert!(alias.symlink_metadata()?.file_type().is_symlink());
            assert_eq!(
                fs::read_link(&alias)?,
                fixture.sources.join(name).canonicalize()?
            );
            assert_eq!(fs::read(alias)?, fs::read(fixture.sources.join(name))?);
        }
        first.cleanup_blocking()?;
        assert!(!first_path.exists());
        assert_eq!(fs::read_dir(&fixture.sources)?.count(), 3);
        let mut second = fixture.prepare()?;
        assert_ne!(second.path(), first_path);
        second.cleanup_blocking()?;
        second.cleanup_blocking()?;
        Ok(())
    }

    #[test]
    fn unsafe_roots_and_unsupported_platform_policy_never_modify_sources()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = EngineTablebaseFixture::new()?;
        let unsafe_root = fixture.directory.path().join("alias root");
        fs::create_dir(&unsafe_root)?;
        for root in [unsafe_root, fixture.directory.path().join("alias:list")] {
            if !root.exists() {
                fs::create_dir(&root)?;
            }
            assert!(matches!(
                EngineTablebaseLocation::prepare_blocking(
                    fixture.selection()?,
                    EngineTablebaseLocationPolicy::unix_temporary(root.clone()),
                    || false,
                ),
                Err(EngineTablebaseLocationError::AliasSyntax { .. })
            ));
            assert_eq!(fs::read_dir(root)?.count(), 0);
        }
        assert!(matches!(
            EngineTablebaseLocation::prepare_blocking(
                fixture.selection()?,
                EngineTablebaseLocationPolicy::windows_owner_selected(fixture.aliases.clone()),
                || false,
            ),
            Err(EngineTablebaseLocationError::UnsupportedPlatform { .. })
        ));
        assert!(EngineTablebaseLocationSelection::new(Vec::new()).is_err());
        assert_eq!(fs::read_dir(&fixture.sources)?.count(), 3);
        Ok(())
    }

    #[test]
    fn missing_unreadable_empty_and_nonregular_sources_return_typed_failures()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = EngineTablebaseFixture::new()?;
        let missing = EngineTablebaseLocationSelection::new(vec![fixture.sources.join("missing")])?;
        assert!(
            EngineTablebaseLocation::prepare_blocking(
                missing,
                EngineTablebaseLocationPolicy::unix_temporary(fixture.aliases.clone()),
                || false,
            )
            .is_err()
        );
        fs::set_permissions(&fixture.sources, Permissions::from_mode(0o000))?;
        let unreadable = fixture.prepare();
        fs::set_permissions(&fixture.sources, Permissions::from_mode(0o700))?;
        assert!(matches!(
            unreadable,
            Err(EngineTablebaseLocationError::NotReadable { .. })
        ));
        fs::set_permissions(&fixture.aliases, Permissions::from_mode(0o500))?;
        let unwritable = fixture.prepare();
        fs::set_permissions(&fixture.aliases, Permissions::from_mode(0o700))?;
        assert!(matches!(
            unwritable,
            Err(EngineTablebaseLocationError::NotWritable { .. })
        ));
        fs::write(fixture.sources.join("KQvK.rtbw"), [])?;
        assert!(matches!(
            fixture.prepare(),
            Err(EngineTablebaseLocationError::InvalidTable { .. })
        ));
        fs::remove_file(fixture.sources.join("KQvK.rtbw"))?;
        fs::create_dir(fixture.sources.join("KQvK.rtbw"))?;
        assert!(matches!(
            fixture.prepare(),
            Err(EngineTablebaseLocationError::InvalidTable { .. })
        ));
        assert_eq!(fs::read_dir(&fixture.aliases)?.count(), 0);
        Ok(())
    }

    #[test]
    fn duplicate_material_names_fail_without_overwriting_or_copying_tables()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = EngineTablebaseFixture::new()?;
        let second = fixture.directory.path().join("second");
        fs::create_dir(&second)?;
        fs::write(second.join("KQvK.rtbw"), b"another table")?;
        let selection =
            EngineTablebaseLocationSelection::new(vec![fixture.sources.clone(), second])?;
        assert!(matches!(
            EngineTablebaseLocation::prepare_blocking(
                selection,
                EngineTablebaseLocationPolicy::unix_temporary(fixture.aliases.clone()),
                || false,
            ),
            Err(EngineTablebaseLocationError::AliasCollision { .. })
        ));
        assert_eq!(fs::read_dir(&fixture.aliases)?.count(), 0);
        assert_eq!(
            fs::read(fixture.sources.join("KQvK.rtbw"))?,
            b"local WDL fixture"
        );
        Ok(())
    }

    #[test]
    fn empty_directories_unreadable_files_and_invalid_material_names_are_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = EngineTablebaseFixture::new()?;
        let table = fixture.sources.join("KQvK.rtbw");
        fs::set_permissions(&table, Permissions::from_mode(0o000))?;
        let unreadable = fixture.prepare();
        fs::set_permissions(&table, Permissions::from_mode(0o600))?;
        assert!(matches!(
            unreadable,
            Err(EngineTablebaseLocationError::NotReadable { .. })
        ));
        fs::remove_file(table)?;
        fs::remove_file(fixture.sources.join("KQvK.rtbz"))?;
        assert!(matches!(
            fixture.prepare(),
            Err(EngineTablebaseLocationError::NoTables)
        ));
        fs::write(fixture.sources.join("not-a-material.rtbw"), b"fixture")?;
        assert!(matches!(
            fixture.prepare(),
            Err(EngineTablebaseLocationError::InvalidTable { .. })
        ));
        assert_eq!(fs::read_dir(&fixture.aliases)?.count(), 0);
        Ok(())
    }

    #[test]
    fn source_delimiters_are_not_path_lists_and_canonical_alias_ancestors_are_validated()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = EngineTablebaseFixture::new()?;
        let source = fixture
            .directory
            .path()
            .join("source:directory with spaces");
        fs::create_dir(&source)?;
        symlink(fixture.sources.join("KQvK.rtbw"), source.join("KQvK.rtbw"))?;
        let selection = EngineTablebaseLocationSelection::new(vec![source.clone(), source])?;
        let mut location = EngineTablebaseLocation::prepare_blocking(
            selection,
            EngineTablebaseLocationPolicy::unix_temporary(fixture.aliases.clone()),
            || false,
        )?;
        assert_eq!(location.file_count(), 1);
        assert_eq!(
            fs::read(location.path().join("KQvK.rtbw"))?,
            b"local WDL fixture"
        );
        location.cleanup_blocking()?;
        let misleading_parent = fixture.directory.path().join("no-spaces");
        symlink(&fixture.sources, &misleading_parent)?;
        assert!(matches!(
            EngineTablebaseLocation::prepare_blocking(
                fixture.selection()?,
                EngineTablebaseLocationPolicy::unix_temporary(misleading_parent),
                || false,
            ),
            Err(EngineTablebaseLocationError::AliasSyntax { .. })
        ));
        Ok(())
    }

    #[test]
    fn link_creation_failure_preserves_foreign_collision_entries_and_reports_cleanup()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = EngineTablebaseFixture::new()?;
        let foreign = Cell::new(false);
        let result = EngineTablebaseLocation::prepare_blocking(
            fixture.selection()?,
            EngineTablebaseLocationPolicy::unix_temporary(fixture.aliases.clone()),
            || {
                if let Ok(roots) = fs::read_dir(&fixture.aliases) {
                    for root in roots.flatten() {
                        let path = root.path().join("KQvK.rtbw");
                        if !foreign.get() && fs::write(path, b"foreign owner data").is_ok() {
                            foreign.set(true);
                        }
                    }
                }
                false
            },
        );
        assert!(foreign.get());
        assert!(matches!(
            result,
            Err(EngineTablebaseLocationError::Cleanup { .. })
        ));
        let root = fs::read_dir(&fixture.aliases)?
            .next()
            .ok_or("missing preserved root")??
            .path();
        assert_eq!(fs::read(root.join("KQvK.rtbw"))?, b"foreign owner data");
        assert_eq!(
            fs::read(fixture.sources.join("KQvK.rtbw"))?,
            b"local WDL fixture"
        );
        Ok(())
    }

    #[test]
    fn cancellation_after_a_link_removes_only_operation_owned_aliases()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = EngineTablebaseFixture::new()?;
        let cancellation = || {
            fs::read_dir(&fixture.aliases).is_ok_and(|roots| {
                roots.filter_map(Result::ok).any(|root| {
                    fs::read_dir(root.path()).is_ok_and(|mut aliases| aliases.next().is_some())
                })
            })
        };
        assert!(matches!(
            EngineTablebaseLocation::prepare_blocking(
                fixture.selection()?,
                EngineTablebaseLocationPolicy::unix_temporary(fixture.aliases.clone()),
                cancellation,
            ),
            Err(EngineTablebaseLocationError::Cancelled)
        ));
        assert_eq!(fs::read_dir(&fixture.aliases)?.count(), 0);
        assert_eq!(fs::read_dir(&fixture.sources)?.count(), 3);
        let checked = Cell::new(false);
        let cancelled = EngineTablebaseLocation::prepare_blocking(
            fixture.selection()?,
            EngineTablebaseLocationPolicy::unix_temporary(fixture.aliases.clone()),
            || {
                checked.set(true);
                true
            },
        );
        assert!(checked.get());
        assert!(matches!(
            cancelled,
            Err(EngineTablebaseLocationError::Cancelled)
        ));
        assert_eq!(fs::read_dir(&fixture.aliases)?.count(), 0);
        Ok(())
    }

    #[test]
    fn cleanup_preserves_foreign_entries_replaced_links_and_replaced_roots()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = EngineTablebaseFixture::new()?;
        let mut location = fixture.prepare()?;
        let foreign = location.path().join("foreign.txt");
        fs::write(&foreign, b"owner data")?;
        let alias = location.path().join("KQvK.rtbw");
        fs::remove_file(&alias)?;
        symlink(fixture.sources.join("README.txt"), &alias)?;
        assert!(location.cleanup_blocking().is_err());
        assert_eq!(fs::read(&foreign)?, b"owner data");
        assert_eq!(fs::read(&alias)?, b"preserve unrelated files");
        fs::remove_file(alias)?;
        fs::remove_file(foreign)?;
        location.cleanup_blocking()?;
        let mut replaced = fixture.prepare()?;
        let original = fixture.directory.path().join("moved-root");
        fs::rename(replaced.path(), &original)?;
        fs::create_dir(replaced.path())?;
        fs::write(replaced.path().join("owner.txt"), b"preserve")?;
        assert!(matches!(
            replaced.cleanup_blocking(),
            Err(EngineTablebaseLocationError::OwnershipChanged { .. })
        ));
        assert_eq!(fs::read(replaced.path().join("owner.txt"))?, b"preserve");
        assert!(
            original
                .join("KQvK.rtbw")
                .symlink_metadata()?
                .file_type()
                .is_symlink()
        );
        Ok(())
    }

    #[test]
    fn status_keeps_configuration_separate_from_explicit_load_cardinality_and_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = EngineTablebaseFixture::new()?;
        let mut location = fixture.prepare()?;
        let mut status = EngineTablebaseStatus::configured(&location);
        assert!(matches!(
            status.state(),
            EngineTablebaseStatusEnumState::Configured
        ));
        assert_eq!(
            status.source_directories(),
            std::slice::from_ref(&fixture.sources)
        );
        let pieces = NonZeroU32::new(6).ok_or("invalid fixture cardinality")?;
        status.record_configuration(Ok(EngineOptionsEnumOutcome::Tablebases(
            EngineOptionsEnumTablebases::Loaded {
                path: location.engine_path().clone(),
                pieces,
            },
        )));
        assert!(
            matches!(status.state(), EngineTablebaseStatusEnumState::Loaded { pieces: actual } if *actual == pieces)
        );
        status.record_configuration(Ok(EngineOptionsEnumOutcome::Tablebases(
            EngineOptionsEnumTablebases::Unavailable {
                path: location.engine_path().clone(),
            },
        )));
        assert!(matches!(
            status.state(),
            EngineTablebaseStatusEnumState::Unavailable
        ));
        status.record_configuration(Ok(EngineOptionsEnumOutcome::Applied(
            EngineEnumOption::Minimal,
        )));
        assert!(matches!(
            status.state(),
            EngineTablebaseStatusEnumState::Failed(EngineTablebaseStatusError::UnexpectedOutcome)
        ));
        status.record_configuration(Ok(EngineOptionsEnumOutcome::Tablebases(
            EngineOptionsEnumTablebases::Loaded {
                path: EngineOptionsSyzygyPath::new("/different/tables".to_owned())?,
                pieces,
            },
        )));
        assert!(matches!(
            status.state(),
            EngineTablebaseStatusEnumState::Failed(EngineTablebaseStatusError::PathMismatch)
        ));
        location.cleanup_blocking()?;
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn setup_and_unavailable_tables_leave_the_real_session_api_usable_for_ordinary_chess()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = EngineTablebaseFixture::new()?;
        let mut location = fixture.prepare()?;
        let package = EnginePackageFixture::new()?;
        let mut provider = EngineLifecycleProvider::new();
        let mut session = provider.start(&package).await?;
        assert!(
            EngineTablebaseLocation::prepare_blocking(
                fixture.selection()?,
                EngineTablebaseLocationPolicy::unix_temporary(fixture.aliases.join("unsafe root")),
                || false,
            )
            .is_err()
        );
        session.readiness().await?;
        provider.expect("isready").await?;
        provider
            .emit_pipe(
                EngineProcessEnumPipe::Stderr,
                "Failed to load Syzygy tablebases",
            )
            .await?;
        let mut status = EngineTablebaseStatus::configured(&location);
        status.configure(&mut session).await;
        assert!(matches!(
            status.state(),
            EngineTablebaseStatusEnumState::Unavailable
        ));
        provider
            .expect(&format!(
                "setoption name SyzygyPath value {}",
                location.engine_path().as_str()
            ))
            .await?;
        provider.expect("isready").await?;
        session.readiness().await?;
        provider.expect("isready").await?;
        assert_eq!(session.state(), EngineSearchEnumState::Idle);
        session.shutdown().await?;
        provider.expect("shutdown").await?;
        location.cleanup_blocking()?;
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn readyok_without_a_load_diagnostic_is_a_status_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = EngineTablebaseFixture::new()?;
        let mut location = fixture.prepare()?;
        let package = EnginePackageFixture::new()?;
        let mut provider = EngineLifecycleProvider::new();
        let mut session = provider.start(&package).await?;
        provider.emit("readyok").await?;
        let mut status = EngineTablebaseStatus::configured(&location);
        status.configure(&mut session).await;
        assert!(matches!(
            status.state(),
            EngineTablebaseStatusEnumState::Failed(EngineTablebaseStatusError::Session(_))
        ));
        assert_eq!(session.state(), EngineSearchEnumState::Failed);
        provider
            .expect(&format!(
                "setoption name SyzygyPath value {}",
                location.engine_path().as_str()
            ))
            .await?;
        provider.expect("terminate").await?;
        provider.assert_no_command();
        location.cleanup_blocking()?;
        Ok(())
    }
}
