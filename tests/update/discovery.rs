use semver::Version;

use chess::update::credentials::UpdateCredential;
use chess::update::discovery::{
    UpdateDiscovery, UpdateDiscoveryError, UpdateOrigins, UpdateRepository,
};
use chess::update::http::UpdateHttpError;
use chess::update::manifest::UpdateTarget;
use chess::update::verifier::UpdateVerifier;

use crate::update_fixture::{
    MANIFEST, PUBLIC_KEY, SIGNATURE, UpdateFixtureHttp, UpdateFixtureResponse,
};

const LATEST: &str = "https://api.github.com/repos/owner/repo/releases/latest";
const MANIFEST_ASSET: &str = "https://api.github.com/repos/owner/repo/releases/assets/11";
const SIGNATURE_ASSET: &str = "https://api.github.com/repos/owner/repo/releases/assets/12";
const OBJECT: &str = "https://objects.githubusercontent.com/manifest";

type UpdateDiscoveryTestResult = Result<(), Box<dyn std::error::Error>>;

fn listing(tag: &str, draft: bool, prerelease: bool, macos_size: u64) -> String {
    format!(
        r#"{{"id":77,"tag_name":"{tag}","draft":{draft},"prerelease":{prerelease},"assets":[{{"id":11,"name":"manifest.json","size":733}},{{"id":12,"name":"manifest.json.minisig","size":281}},{{"id":1001,"name":"chess-1.2.0-macos-arm64.zip","size":{macos_size}}},{{"id":1002,"name":"chess-1.2.0-windows-x86_64-setup.exe","size":17}}]}}"#
    )
}

fn wired(tag: &str) -> UpdateFixtureHttp {
    let http = UpdateFixtureHttp::default();
    http.route(LATEST, UpdateFixtureResponse::ok(listing(tag, false, false, 11).as_bytes()));
    http.route(MANIFEST_ASSET, UpdateFixtureResponse::redirect(OBJECT));
    http.route(OBJECT, UpdateFixtureResponse::ok(MANIFEST));
    http.route(SIGNATURE_ASSET, UpdateFixtureResponse::ok(SIGNATURE));
    http
}

fn discovery(
    http: &UpdateFixtureHttp,
) -> Result<UpdateDiscovery<UpdateFixtureHttp, UpdateVerifier>, Box<dyn std::error::Error>> {
    Ok(UpdateDiscovery::new(
        http.clone(),
        UpdateVerifier::new(PUBLIC_KEY)?,
        UpdateRepository::new("owner", "repo")?,
        UpdateOrigins::github(),
    ))
}

fn credential() -> Result<UpdateCredential, Box<dyn std::error::Error>> {
    Ok(UpdateCredential::new("github_pat_fixture".to_owned())?)
}

#[tokio::test]
async fn a_newer_signed_release_binds_release_asset_and_manifest_identity() -> UpdateDiscoveryTestResult {
    let http = wired("v1.2.0");
    let found = discovery(&http)?
        .discover(&Version::new(1, 1, 0), UpdateTarget::MacosArm64, Some(&credential()?))
        .await?
        .ok_or("expected a release")?;
    assert_eq!(found.release.release_id, 77);
    assert_eq!(found.release.asset_id, 1001);
    assert_eq!(found.release.version, Version::new(1, 2, 0));
    assert_eq!(found.asset.length(), 11);
    let log = http.log();
    let bearer_for = |needle: &str| {
        log.iter()
            .filter(|(url, _bearer)| url.contains(needle))
            .all(|(_url, bearer)| bearer.is_some())
    };
    assert!(bearer_for("api.github.com"));
    assert!(
        log.iter()
            .filter(|(url, _bearer)| url.contains("objects.githubusercontent.com"))
            .all(|(_url, bearer)| bearer.is_none())
    );
    Ok(())
}

#[tokio::test]
async fn current_draft_and_prerelease_versions_offer_nothing() -> UpdateDiscoveryTestResult {
    let http = wired("v1.2.0");
    let current = discovery(&http)?
        .discover(&Version::new(1, 2, 0), UpdateTarget::MacosArm64, Some(&credential()?))
        .await?;
    assert!(current.is_none());
    http.route(LATEST, UpdateFixtureResponse::ok(listing("v1.2.0", true, false, 11).as_bytes()));
    assert!(
        discovery(&http)?
            .discover(&Version::new(1, 1, 0), UpdateTarget::MacosArm64, Some(&credential()?))
            .await?
            .is_none()
    );
    http.route(LATEST, UpdateFixtureResponse::ok(listing("v1.2.0", false, true, 11).as_bytes()));
    assert!(
        discovery(&http)?
            .discover(&Version::new(1, 1, 0), UpdateTarget::MacosArm64, Some(&credential()?))
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn a_tampered_manifest_fails_the_signature_before_parsing() -> UpdateDiscoveryTestResult {
    let http = wired("v1.2.0");
    let mut altered = MANIFEST.to_vec();
    if let Some(byte) = altered.get_mut(10) {
        *byte ^= 1;
    }
    http.route(OBJECT, UpdateFixtureResponse::ok(&altered));
    let outcome = discovery(&http)?
        .discover(&Version::new(1, 1, 0), UpdateTarget::MacosArm64, Some(&credential()?))
        .await;
    assert!(matches!(outcome, Err(UpdateDiscoveryError::Manifest(_))));
    Ok(())
}

#[tokio::test]
async fn redirects_to_other_origins_or_plain_http_are_refused() -> UpdateDiscoveryTestResult {
    for target in ["https://evil.example/manifest", "http://objects.githubusercontent.com/manifest"] {
        let http = wired("v1.2.0");
        http.route(MANIFEST_ASSET, UpdateFixtureResponse::redirect(target));
        let outcome = discovery(&http)?
            .discover(&Version::new(1, 1, 0), UpdateTarget::MacosArm64, Some(&credential()?))
            .await;
        assert!(matches!(
            outcome,
            Err(UpdateDiscoveryError::Http(UpdateHttpError::Redirect))
        ));
    }
    Ok(())
}

#[tokio::test]
async fn credential_and_rate_limit_failures_are_typed() -> UpdateDiscoveryTestResult {
    let http = wired("v1.2.0");
    assert!(matches!(
        discovery(&http)?
            .discover(&Version::new(1, 1, 0), UpdateTarget::MacosArm64, None)
            .await,
        Err(UpdateDiscoveryError::CredentialMissing)
    ));
    http.route(LATEST, UpdateFixtureResponse::status(401));
    assert!(matches!(
        discovery(&http)?
            .discover(&Version::new(1, 1, 0), UpdateTarget::MacosArm64, Some(&credential()?))
            .await,
        Err(UpdateDiscoveryError::CredentialRejected)
    ));
    http.route(
        LATEST,
        UpdateFixtureResponse {
            retry_after: Some(30),
            ..UpdateFixtureResponse::status(403)
        },
    );
    assert!(matches!(
        discovery(&http)?
            .discover(&Version::new(1, 1, 0), UpdateTarget::MacosArm64, Some(&credential()?))
            .await,
        Err(UpdateDiscoveryError::RateLimited(Some(30)))
    ));
    Ok(())
}

#[tokio::test]
async fn a_tag_that_disagrees_with_the_signed_version_or_asset_listing_is_refused() -> UpdateDiscoveryTestResult {
    let mismatch = wired("v1.3.0");
    assert!(matches!(
        discovery(&mismatch)?
            .discover(&Version::new(1, 1, 0), UpdateTarget::MacosArm64, Some(&credential()?))
            .await,
        Err(UpdateDiscoveryError::Release(_))
    ));
    let wrong_size = wired("v1.2.0");
    wrong_size.route(LATEST, UpdateFixtureResponse::ok(listing("v1.2.0", false, false, 12).as_bytes()));
    assert!(matches!(
        discovery(&wrong_size)?
            .discover(&Version::new(1, 1, 0), UpdateTarget::MacosArm64, Some(&credential()?))
            .await,
        Err(UpdateDiscoveryError::Release(_))
    ));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn the_whole_check_is_bounded_to_fifteen_seconds() -> UpdateDiscoveryTestResult {
    let http = wired("v1.2.0");
    http.route(
        LATEST,
        UpdateFixtureResponse {
            stall_open: true,
            ..UpdateFixtureResponse::ok(b"")
        },
    );
    assert!(matches!(
        discovery(&http)?
            .discover(&Version::new(1, 1, 0), UpdateTarget::MacosArm64, Some(&credential()?))
            .await,
        Err(UpdateDiscoveryError::Deadline(_))
    ));
    Ok(())
}
