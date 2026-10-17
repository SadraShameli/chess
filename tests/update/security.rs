use url::Url;

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
async fn the_bearer_is_dropped_on_a_cross_origin_redirect_and_never_restored() -> UpdateSecurityResult
{
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

    assert!(
        DesktopNativeUpdateWindows::install(Path::new("setup.exe"), Path::new("C:\\Chess"))
            .is_err()
    );
    assert!(
        DesktopNativeUpdateMacos::install(
            Path::new("package.zip"),
            Path::new("/Applications/Chess.app")
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
    let outcome = DesktopNativeUpdateLinux::install_appimage(&package, &destination)?;
    assert_eq!(outcome, DesktopNativeUpdateEnumOutcome::Installed);
    assert_eq!(std::fs::read(&destination)?, b"new image");
    assert_eq!(std::fs::metadata(&destination)?.permissions().mode() & 0o777, 0o755);
    let residue = std::fs::read_dir(&root)?
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().starts_with(".chess-update-"));
    assert!(!residue);
    Ok(())
}
