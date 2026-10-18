use std::path::Path;

use semver::Version;
use tokio::sync::watch;

use chess::update::cache::UpdateCache;
use chess::update::credentials::UpdateCredential;
use chess::update::discovery::UpdateOrigins;
use chess::update::download::{
    UpdateDiskSpace, UpdateDownloadError, UpdateDownloadRequest, UpdateDownloader,
};
use chess::update::manifest::{UpdateManifest, UpdateManifestAsset, UpdateTarget};
use chess::update::network_cost::{
    UpdateNetworkApproval, UpdateNetworkEnumCost, UpdateNetworkEnumVerdict, UpdateNetworkMonitor,
};
use chess::update::state::{
    UpdateCacheReceipt, UpdateEnumFailure, UpdateEnumPhase, UpdateReleaseIdentity, UpdateState,
};
use chess::update::verifier::UpdateVerifier;
use url::Url;

use crate::update_fixture::{
    MACOS_PACKAGE, MANIFEST, PUBLIC_KEY, SIGNATURE, UpdateFixtureHttp, UpdateFixtureResponse,
};

type UpdateDownloadTestResult = Result<(), Box<dyn std::error::Error>>;

const ASSET_URL: &str = "https://api.github.com/repos/owner/repo/releases/assets/1001";

struct UpdateFixtureSpace(u64);

impl UpdateDiskSpace for UpdateFixtureSpace {
    fn available(&self, _path: &Path) -> Result<u64, std::io::Error> {
        Ok(self.0)
    }
}

fn manifest() -> Result<UpdateManifest, Box<dyn std::error::Error>> {
    Ok(UpdateManifest::verify_and_parse(
        &UpdateVerifier::new(PUBLIC_KEY)?,
        MANIFEST,
        SIGNATURE,
    )?)
}

fn asset(target: UpdateTarget) -> Result<UpdateManifestAsset, Box<dyn std::error::Error>> {
    Ok(manifest()?
        .asset_for(target)
        .ok_or("missing asset")?
        .clone())
}

fn receipt(asset: &UpdateManifestAsset) -> Result<UpdateCacheReceipt, Box<dyn std::error::Error>> {
    Ok(UpdateCacheReceipt {
        release: UpdateReleaseIdentity {
            release_id: 77,
            asset_id: asset.id(),
            version: Version::new(1, 2, 0),
            target: asset.target(),
            asset_name: asset.name().to_owned(),
        },
        manifest_digest: manifest()?.digest(),
        package_sha256: asset.sha256(),
        length: asset.length(),
    })
}

async fn run(
    http: &UpdateFixtureHttp,
    asset: &UpdateManifestAsset,
    partial: &Path,
    space: u64,
    cancel: watch::Receiver<bool>,
) -> Result<(), UpdateDownloadError> {
    let credential = UpdateCredential::new("github_pat_fixture".to_owned())
        .map_err(|_error| UpdateDownloadError::Mismatch)?;
    let url = Url::parse(ASSET_URL).map_err(|_error| UpdateDownloadError::Mismatch)?;
    UpdateDownloader::new(http.clone(), UpdateOrigins::github())
        .download(
            UpdateDownloadRequest {
                asset,
                url,
                partial,
                credential: &credential,
            },
            &UpdateFixtureSpace(space),
            cancel,
            |_received| {},
        )
        .await
}

#[tokio::test]
async fn a_matching_download_is_written_and_then_cached_durably() -> UpdateDownloadTestResult {
    let directory = tempfile::tempdir()?;
    let cache = UpdateCache::new(directory.path().to_path_buf());
    let asset = asset(UpdateTarget::MacosArm64)?;
    let http = UpdateFixtureHttp::default();
    http.route(ASSET_URL, UpdateFixtureResponse::ok(MACOS_PACKAGE));
    let (_sender, cancel) = watch::channel(false);
    let partial = cache.begin_partial(chess::update::state::UpdateAttemptId::new())?;
    run(&http, &asset, &partial, u64::MAX, cancel).await?;
    let package = cache.complete(&partial, &asset, &receipt(&asset)?)?;
    assert_eq!(std::fs::read(package)?, MACOS_PACKAGE);
    assert!(cache.completed()?.is_some());
    assert!(!partial.exists());
    Ok(())
}

#[tokio::test]
async fn mismatched_bytes_space_shortage_and_status_failures_leave_no_partial()
-> UpdateDownloadTestResult {
    let directory = tempfile::tempdir()?;
    let asset = asset(UpdateTarget::MacosArm64)?;
    let partial = directory.path().join("package.part");
    let (_sender, cancel) = watch::channel(false);
    let http = UpdateFixtureHttp::default();
    http.route(ASSET_URL, UpdateFixtureResponse::ok(b"hello w0rld"));
    assert!(matches!(
        run(&http, &asset, &partial, u64::MAX, cancel.clone()).await,
        Err(UpdateDownloadError::Mismatch)
    ));
    assert!(!partial.exists());
    assert!(matches!(
        run(&http, &asset, &partial, 10, cancel.clone()).await,
        Err(UpdateDownloadError::Space { .. })
    ));
    assert!(!partial.exists());
    http.route(ASSET_URL, UpdateFixtureResponse::status(404));
    assert!(matches!(
        run(&http, &asset, &partial, u64::MAX, cancel).await,
        Err(UpdateDownloadError::Status(404))
    ));
    Ok(())
}

#[tokio::test]
async fn cancellation_discards_the_partial_without_retry() -> UpdateDownloadTestResult {
    let directory = tempfile::tempdir()?;
    let asset = asset(UpdateTarget::MacosArm64)?;
    let partial = directory.path().join("package.part");
    let http = UpdateFixtureHttp::default();
    http.route(ASSET_URL, UpdateFixtureResponse::ok(MACOS_PACKAGE));
    let (sender, cancel) = watch::channel(false);
    sender.send(true)?;
    assert!(matches!(
        run(&http, &asset, &partial, u64::MAX, cancel).await,
        Err(UpdateDownloadError::Cancelled)
    ));
    assert!(!partial.exists());
    assert_eq!(http.log().len(), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn sixty_seconds_without_bytes_stops_the_download() -> UpdateDownloadTestResult {
    let directory = tempfile::tempdir()?;
    let asset = asset(UpdateTarget::MacosArm64)?;
    let partial = directory.path().join("package.part");
    let http = UpdateFixtureHttp::default();
    http.route(
        ASSET_URL,
        UpdateFixtureResponse {
            stall_body: true,
            ..UpdateFixtureResponse::ok(MACOS_PACKAGE)
        },
    );
    let (_sender, cancel) = watch::channel(false);
    let outcome = run(&http, &asset, &partial, u64::MAX, cancel).await;
    assert!(matches!(
        outcome,
        Err(UpdateDownloadError::Stalled(_)) | Err(UpdateDownloadError::Mismatch)
    ));
    assert!(!partial.exists());
    Ok(())
}

#[test]
fn the_cache_keeps_the_old_package_until_a_replacement_validates() -> UpdateDownloadTestResult {
    let directory = tempfile::tempdir()?;
    let cache = UpdateCache::new(directory.path().to_path_buf());
    let first = asset(UpdateTarget::MacosArm64)?;
    let partial = cache.begin_partial(chess::update::state::UpdateAttemptId::new())?;
    std::fs::write(&partial, MACOS_PACKAGE)?;
    cache.complete(&partial, &first, &receipt(&first)?)?;
    let second = asset(UpdateTarget::WindowsX86_64)?;
    let bad = cache.begin_partial(chess::update::state::UpdateAttemptId::new())?;
    std::fs::write(&bad, b"corrupt bytes")?;
    assert!(cache.complete(&bad, &second, &receipt(&second)?).is_err());
    assert_eq!(
        cache.completed()?.map(|(held, _path)| held.release.target),
        Some(UpdateTarget::MacosArm64)
    );
    let good = cache.begin_partial(chess::update::state::UpdateAttemptId::new())?;
    std::fs::write(&good, b"windows installer")?;
    cache.complete(&good, &second, &receipt(&second)?)?;
    assert_eq!(
        cache.completed()?.map(|(held, _path)| held.release.target),
        Some(UpdateTarget::WindowsX86_64)
    );
    assert!(
        !directory
            .path()
            .join("completed")
            .join(first.name())
            .exists()
    );
    Ok(())
}

#[test]
fn the_consumed_cache_is_removed_only_after_the_new_version_starts() -> UpdateDownloadTestResult {
    let directory = tempfile::tempdir()?;
    let cache = UpdateCache::new(directory.path().to_path_buf());
    let asset = asset(UpdateTarget::MacosArm64)?;
    let partial = cache.begin_partial(chess::update::state::UpdateAttemptId::new())?;
    std::fs::write(&partial, MACOS_PACKAGE)?;
    cache.complete(&partial, &asset, &receipt(&asset)?)?;
    assert!(!cache.consume_after_verified_startup(&Version::new(1, 1, 0))?);
    assert!(cache.completed()?.is_some());
    assert!(cache.consume_after_verified_startup(&Version::new(1, 2, 0))?);
    assert!(cache.completed()?.is_none());
    let stale = cache.begin_partial(chess::update::state::UpdateAttemptId::new())?;
    std::fs::write(&stale, b"left over")?;
    cache.cleanup_partials()?;
    assert!(!stale.exists());
    Ok(())
}

fn found_state() -> Result<UpdateState, Box<dyn std::error::Error>> {
    let asset = asset(UpdateTarget::MacosArm64)?;
    let mut state = UpdateState::default();
    state.begin_check()?;
    state.check_found(receipt(&asset)?.release, manifest()?.digest())?;
    Ok(state)
}

#[test]
fn metered_or_unknown_networks_need_attempt_scoped_approval() -> UpdateDownloadTestResult {
    let mut state = found_state()?;
    let first = state.request_download(UpdateNetworkEnumCost::Metered)?;
    assert_eq!(state.phase(), UpdateEnumPhase::AwaitingNetworkApproval);
    state.approve(UpdateNetworkEnumCost::Metered)?;
    assert_eq!(state.phase(), UpdateEnumPhase::Downloading);
    state.network_observed(UpdateNetworkEnumCost::Unmetered)?;
    assert_eq!(
        state.phase(),
        UpdateEnumPhase::Failed(UpdateEnumFailure::NetworkChanged)
    );
    assert!(state.can_retry());
    let second = state.request_download(UpdateNetworkEnumCost::Unknown)?;
    assert_ne!(first, second);
    assert_eq!(state.phase(), UpdateEnumPhase::AwaitingNetworkApproval);
    assert_eq!(state.approval(), None);
    Ok(())
}

#[test]
fn unmetered_downloads_start_directly_and_one_check_runs_at_a_time() -> UpdateDownloadTestResult {
    let mut state = found_state()?;
    state.request_download(UpdateNetworkEnumCost::Unmetered)?;
    assert_eq!(state.phase(), UpdateEnumPhase::Downloading);
    assert!(state.begin_check().is_err());
    state.cancel()?;
    assert_eq!(state.phase(), UpdateEnumPhase::Cancelled);
    assert!(state.can_retry());
    Ok(())
}

#[test]
fn the_network_monitor_polls_every_five_seconds_and_stops_on_any_cost_change() {
    let attempt = chess::update::state::UpdateAttemptId::new();
    let approval = UpdateNetworkApproval::new(attempt, UpdateNetworkEnumCost::Unmetered);
    let mut monitor = UpdateNetworkMonitor::new(approval, 0);
    assert!(!monitor.poll_due(4_999));
    assert!(monitor.poll_due(5_000));
    assert_eq!(
        monitor.observe(UpdateNetworkEnumCost::Unmetered, 5_000),
        UpdateNetworkEnumVerdict::Continue
    );
    assert_eq!(
        monitor.os_change(UpdateNetworkEnumCost::Metered),
        UpdateNetworkEnumVerdict::Stop
    );
}
