use std::fs;
use std::path::{Path, PathBuf};

type SecurityResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct SecurityBoundaries;

impl SecurityBoundaries {
    const ARGUMENT_OWNER: &str = "src/desktop/native/bootstrap.rs";
    const FORBIDDEN: [&str; 9] = [
        "TcpListener",
        "UdpSocket",
        "localhost",
        "127.0.0.1",
        "0.0.0.0",
        "env::var",
        "env::var_os",
        "option_env!",
        "danger_accept_invalid",
    ];
    const ALLOWED_FEATURE: &str = "cfg(feature = \"asset-tools\")";

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn sources(directory: &Path, found: &mut Vec<(String, String)>) -> SecurityResult {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_dir() {
                Self::sources(&path, found)?;
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let relative = path
                    .strip_prefix(Self::root())?
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push((relative, fs::read_to_string(&path)?));
            }
        }
        Ok(())
    }

    fn library() -> SecurityResult<Vec<(String, String)>> {
        let mut found = Vec::new();
        Self::sources(&Self::root().join("src"), &mut found)?;
        assert!(!found.is_empty());
        Ok(found)
    }
}

#[test]
fn shipped_sources_open_no_network_listener_and_read_no_environment_or_test_switch()
-> SecurityResult {
    for (name, text) in SecurityBoundaries::library()? {
        for marker in SecurityBoundaries::FORBIDDEN {
            assert!(!text.contains(marker), "{name} contains forbidden '{marker}'");
        }
    }
    Ok(())
}

#[test]
fn only_the_native_bootstrap_reads_process_arguments() -> SecurityResult {
    for (name, text) in SecurityBoundaries::library()? {
        if name != SecurityBoundaries::ARGUMENT_OWNER {
            assert!(
                !text.contains("env::args"),
                "{name} reads process arguments outside the bootstrap parser"
            );
        }
    }
    Ok(())
}

#[test]
fn the_only_conditional_feature_is_the_asset_tooling_gate() -> SecurityResult {
    for (name, text) in SecurityBoundaries::library()? {
        for (index, _matched) in text.match_indices("cfg(feature") {
            let rest = &text[index..];
            assert!(
                rest.starts_with(SecurityBoundaries::ALLOWED_FEATURE),
                "{name} gates code on an unapproved feature"
            );
        }
    }
    Ok(())
}

#[test]
fn private_activation_socket_and_directory_modes_stay_owner_only() -> SecurityResult {
    let unix = fs::read_to_string(
        SecurityBoundaries::root().join("src/desktop/native/activation/unix.rs"),
    )?;
    assert!(unix.contains("PRIVATE_DIRECTORY_MODE: u32 = 0o700"));
    assert!(unix.contains("PRIVATE_SOCKET_MODE: rustix::fs::RawMode = 0o600"));
    assert!(unix.contains("peer_creds"));
    let windows = fs::read_to_string(
        SecurityBoundaries::root().join("src/desktop/native/activation/windows.rs"),
    )?;
    assert!(windows.contains("authenticate_process_token"));
    Ok(())
}
