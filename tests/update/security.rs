use url::Url;

use chess::package::digest::PackageDigest;
use chess::update::cache::UpdateCache;
use chess::update::discovery::{UpdateOrigins, UpdateRedirects};
use chess::update::http::UpdateHttpRequest;
use chess::update::manifest::UpdateManifest;
use chess::update::verifier::{UpdateSignatureVerifier, UpdateVerifier, UpdateVerifierError};

use crate::update_fixture::{PUBLIC_KEY, UpdateFixtureHttp, UpdateFixtureResponse};

type UpdateSecurityResult = Result<(), Box<dyn std::error::Error>>;

const API: &str = "https://api.github.com/repos/owner/repo/releases/assets/9";
const OBJECT: &str = "https://objects.githubusercontent.com/asset";
const SECRET: &str = "github_pat_secret_value";

#[test]
fn a_request_never_prints_its_bearer_token() -> UpdateSecurityResult {
    let request = UpdateHttpRequest {
        url: Url::parse(API)?,
        bearer: Some(SECRET.to_owned()),
        accept: "application/octet-stream",
    };
    let shown = format!("{request:?} {:#?}", request.clone());
    assert!(!shown.contains(SECRET));
    assert!(shown.contains("redacted"));
    Ok(())
}

#[tokio::test]
async fn the_bearer_is_dropped_on_a_cross_origin_redirect_and_never_restored()
-> UpdateSecurityResult {
    let http = UpdateFixtureHttp::default();
    let returned = format!("{API}?again=1");
    http.route(API, UpdateFixtureResponse::redirect(OBJECT));
    http.route(OBJECT, UpdateFixtureResponse::redirect(&returned));
    http.route(&returned, UpdateFixtureResponse::ok(b"done"));
    let (head, _stream) = UpdateRedirects::open(
        &http,
        &UpdateOrigins::github(),
        UpdateHttpRequest {
            url: Url::parse(API)?,
            bearer: Some(SECRET.to_owned()),
            accept: "application/octet-stream",
        },
    )
    .await?;
    assert_eq!(head.status, 200);
    let log = http.log();
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].1.as_deref(), Some(SECRET));
    assert_eq!(log[1].1, None);
    assert_eq!(log[2].1, None);
    Ok(())
}

#[test]
fn lookalike_hosts_userinfo_ports_and_plain_http_are_not_update_origins() -> UpdateSecurityResult {
    let origins = UpdateOrigins::github();
    for allowed in [
        "https://api.github.com/repos/o/r",
        "https://objects.githubusercontent.com/x",
        "https://release-assets.githubusercontent.com/y",
    ] {
        assert!(origins.allows(&Url::parse(allowed)?), "{allowed}");
    }
    for refused in [
        "http://api.github.com/repos/o/r",
        "https://user@api.github.com/",
        "https://user:pass@api.github.com/",
        "https://api.github.com:8443/",
        "https://api.github.com.evil.test/",
        "https://evilapi.github.com/",
        "https://githubusercontent.com/",
        "https://127.0.0.1/",
    ] {
        assert!(!origins.allows(&Url::parse(refused)?), "{refused}");
    }
    Ok(())
}

#[test]
fn asset_names_with_separators_streams_devices_or_trailing_dots_are_refused() {
    for good in [
        "chess-1.2.0-macos-arm64.zip",
        "chess-1.2.0-windows-x86_64-setup.exe",
        "Chess_1.2.0_amd64.deb",
    ] {
        assert!(UpdateManifest::is_safe_asset_name(good), "{good}");
    }
    for bad in [
        "",
        ".",
        "..",
        "a/b",
        "a\\b",
        "C:evil.exe",
        "installer.exe:stream",
        "installer.exe.",
        "installer.exe ",
        "CON",
        "con.txt",
        "NUL.exe",
        "COM1",
        "lpt9.log",
        "bad\u{1}name",
    ] {
        assert!(!UpdateManifest::is_safe_asset_name(bad), "{bad:?}");
    }
    assert!(!UpdateManifest::is_safe_asset_name(&"a".repeat(256)));
}

#[test]
fn a_cached_receipt_naming_an_unsafe_asset_is_refused_before_touching_the_package()
-> UpdateSecurityResult {
    let digest = "00".repeat(32);
    for name in ["C:evil.exe", "..", "a/b", "a\\b", "", "CON", "x."] {
        let directory = tempfile::tempdir()?;
        let completed = directory.path().join("completed");
        std::fs::create_dir_all(&completed)?;
        let receipt = format!(
            "{{\"release_id\":1,\"asset_id\":2,\"asset_name\":{name:?},\"version\":\"1.2.0\",\"target\":\"macos-arm64\",\"manifest_sha256\":\"{digest}\",\"package_sha256\":\"{digest}\",\"length\":3}}"
        );
        std::fs::write(completed.join("receipt.json"), receipt)?;
        let cache = UpdateCache::new(directory.path().to_path_buf());
        assert!(cache.completed().is_err(), "{name:?}");
    }
    Ok(())
}

#[test]
fn an_unprovisioned_build_refuses_every_signature_and_oversize_signatures_are_refused()
-> UpdateSecurityResult {
    if UpdateVerifier::EMBEDDED_PUBLIC_KEY.is_none() {
        assert!(matches!(
            UpdateVerifier::embedded(),
            Err(UpdateVerifierError::NotProvisioned)
        ));
    }
    let verifier = UpdateVerifier::new(PUBLIC_KEY)?;
    let oversized = vec![b'a'; UpdateVerifier::MAX_SIGNATURE_BYTES + 1];
    assert!(matches!(
        verifier.verify(b"manifest", &oversized),
        Err(UpdateVerifierError::SignatureTooLarge(_))
    ));
    assert!(matches!(
        verifier.verify(b"manifest", &[0xff, 0xfe]),
        Err(UpdateVerifierError::SignatureEncoding)
    ));
    assert!(verifier.verify(b"manifest", b"not a signature").is_err());
    Ok(())
}

#[test]
fn installers_refuse_relative_packages_before_running_anything() {
    use chess::desktop::native::update::macos::DesktopNativeUpdateMacos;
    use chess::desktop::native::update::windows::DesktopNativeUpdateWindows;
    use std::path::Path;

    let digest = PackageDigest::hash_slice(b"package");
    assert!(
        DesktopNativeUpdateWindows::install(Path::new("setup.exe"), Path::new("C:\\Chess"), digest)
            .is_err()
    );
    assert!(
        DesktopNativeUpdateMacos::install(
            Path::new("package.zip"),
            Path::new("/Applications/Chess.app"),
            digest
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn the_appimage_swap_writes_an_exclusive_staging_file_and_leaves_no_residue() -> UpdateSecurityResult
{
    use chess::desktop::native::update::DesktopNativeUpdateEnumOutcome;
    use chess::desktop::native::update::linux::DesktopNativeUpdateLinux;
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    let destination = root.join("Chess.AppImage");
    std::fs::write(&destination, b"old image")?;
    let package = root.join("package.bin");
    std::fs::write(&package, b"new image")?;
    let outcome = DesktopNativeUpdateLinux::install_appimage(
        &package,
        &destination,
        PackageDigest::hash_slice(b"new image"),
    )?;
    assert_eq!(outcome, DesktopNativeUpdateEnumOutcome::Installed);
    assert_eq!(std::fs::read(&destination)?, b"new image");
    assert_eq!(
        std::fs::metadata(&destination)?.permissions().mode() & 0o777,
        0o755
    );
    let residue = std::fs::read_dir(&root)?
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".chess-update-")
        });
    assert!(!residue);
    Ok(())
}

#[test]
fn a_verified_copy_hashes_the_bytes_it_writes_and_removes_a_mismatched_copy() -> UpdateSecurityResult
{
    use chess::desktop::native::update::{DesktopNativeUpdateError, DesktopNativeUpdateStaging};

    let directory = tempfile::tempdir()?;
    let source = directory.path().join("source.bin");
    std::fs::write(&source, b"signed bytes")?;
    let good = directory.path().join("good.bin");
    DesktopNativeUpdateStaging::copy_verified(
        &source,
        PackageDigest::hash_slice(b"signed bytes"),
        &good,
    )?;
    assert_eq!(std::fs::read(&good)?, b"signed bytes");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&good)?.permissions().mode() & 0o777,
            0o600
        );
    }
    let bad = directory.path().join("bad.bin");
    let mismatch = DesktopNativeUpdateStaging::copy_verified(
        &source,
        PackageDigest::hash_slice(b"other bytes"),
        &bad,
    );
    assert!(matches!(
        mismatch,
        Err(DesktopNativeUpdateError::DigestMismatch)
    ));
    assert!(!bad.exists());
    let occupied = DesktopNativeUpdateStaging::copy_verified(
        &source,
        PackageDigest::hash_slice(b"signed bytes"),
        &good,
    );
    assert!(occupied.is_err());
    assert_eq!(std::fs::read(&good)?, b"signed bytes");
    let folder = DesktopNativeUpdateStaging::copy_verified(
        directory.path(),
        PackageDigest::hash_slice(b"signed bytes"),
        &directory.path().join("never.bin"),
    );
    assert!(folder.is_err());
    Ok(())
}

#[test]
fn the_debian_plan_runs_one_root_helper_action_with_the_digest_and_refuses_unsafe_paths()
-> UpdateSecurityResult {
    use chess::desktop::native::update::linux::DesktopNativeUpdateLinuxDebPlan;
    use std::ffi::OsString;
    use std::path::Path;

    let digest = PackageDigest::hash_slice(b"deb");
    let plan = DesktopNativeUpdateLinuxDebPlan::new(
        Path::new("/usr/lib/chess/chess-update-helper"),
        Path::new("/home/user/.cache/chess/update/chess.deb"),
        digest,
    )?;
    assert_eq!(DesktopNativeUpdateLinuxDebPlan::PROGRAM, "pkexec");
    assert_eq!(
        plan.arguments(),
        vec![
            OsString::from("/usr/lib/chess/chess-update-helper"),
            OsString::from("--privileged-install-deb"),
            OsString::from("--package"),
            OsString::from("/home/user/.cache/chess/update/chess.deb"),
            OsString::from("--sha256"),
            OsString::from(digest.to_string()),
        ]
    );
    for (helper, package) in [
        ("relative/helper", "/tmp/chess.deb"),
        ("/usr/lib/chess/helper", "relative.deb"),
        ("/usr/lib/chess/helper", "-evil.deb"),
        ("/usr/lib/chess/../../tmp/helper", "/tmp/chess.deb"),
        ("/usr/lib/chess/helper", "/tmp/../etc/chess.deb"),
    ] {
        assert!(
            DesktopNativeUpdateLinuxDebPlan::new(Path::new(helper), Path::new(package), digest)
                .is_err(),
            "{helper} {package}"
        );
    }
    Ok(())
}

#[test]
fn only_a_root_owned_helper_in_root_owned_directories_may_run_privileged() -> UpdateSecurityResult {
    use chess::desktop::native::update::linux::DesktopNativeUpdateLinux;
    use std::path::Path;

    let directory = tempfile::tempdir()?;
    let helper = directory.path().canonicalize()?.join("chess-update-helper");
    std::fs::write(&helper, b"#!/bin/sh\n")?;
    assert!(DesktopNativeUpdateLinux::validate_privileged_helper(&helper).is_err());
    assert!(
        DesktopNativeUpdateLinux::validate_privileged_helper(Path::new("relative-helper")).is_err()
    );
    assert!(DesktopNativeUpdateLinux::validate_privileged_helper(directory.path()).is_err());
    Ok(())
}
