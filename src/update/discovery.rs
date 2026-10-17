use std::time::Duration;

use semver::Version;
use serde::Deserialize;
use thiserror::Error;
use url::Url;

use crate::update::credentials::UpdateCredential;
use crate::update::http::{
    UpdateHttp, UpdateHttpError, UpdateHttpHead, UpdateHttpRequest, UpdateHttpStream,
};
use crate::update::manifest::{UpdateManifest, UpdateManifestAsset, UpdateManifestError, UpdateTarget};
use crate::update::state::UpdateReleaseIdentity;
use crate::update::verifier::UpdateSignatureVerifier;

#[derive(Debug, Error)]
pub enum UpdateDiscoveryError {
    #[error(transparent)]
    Http(#[from] UpdateHttpError),
    #[error("No update credential is available")]
    CredentialMissing,
    #[error("The update credential was rejected")]
    CredentialRejected,
    #[error("GitHub rate-limited the check; retry after {0:?} seconds")]
    RateLimited(Option<u64>),
    #[error("The update server answered with status {0}")]
    Status(u16),
    #[error("The release metadata is not valid: {0}")]
    Release(String),
    #[error(transparent)]
    Manifest(#[from] UpdateManifestError),
    #[error("The signed manifest has no package for this platform")]
    NoTarget,
    #[error("The repository {0:?} is not a valid owner/name")]
    Repository(String),
    #[error("The update check exceeded {0:?}")]
    Deadline(Duration),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRepository {
    owner: String,
    name: String,
}

impl UpdateRepository {
    pub fn new(owner: &str, name: &str) -> Result<Self, UpdateDiscoveryError> {
        let valid = |part: &str| {
            !part.is_empty()
                && part.len() <= 100
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        };
        if !valid(owner) || !valid(name) {
            return Err(UpdateDiscoveryError::Repository(format!("{owner}/{name}")));
        }
        Ok(Self {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }

    pub fn latest_release(&self) -> Result<Url, UpdateDiscoveryError> {
        self.api("releases/latest")
    }

    pub fn asset(&self, id: u64) -> Result<Url, UpdateDiscoveryError> {
        self.api(&format!("releases/assets/{id}"))
    }

    fn api(&self, tail: &str) -> Result<Url, UpdateDiscoveryError> {
        Url::parse(&format!(
            "https://api.github.com/repos/{}/{}/{tail}",
            self.owner, self.name
        ))
        .map_err(|error| UpdateDiscoveryError::Release(error.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOrigins {
    hosts: Vec<String>,
}

impl UpdateOrigins {
    pub fn github() -> Self {
        Self {
            hosts: vec![
                "api.github.com".to_owned(),
                "objects.githubusercontent.com".to_owned(),
                "release-assets.githubusercontent.com".to_owned(),
            ],
        }
    }

    pub fn new(hosts: Vec<String>) -> Self {
        Self { hosts }
    }

    pub fn allows(&self, url: &Url) -> bool {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url
                .host_str()
                .is_some_and(|host| self.hosts.iter().any(|allowed| allowed == host))
            && url.port_or_known_default() == Some(443)
    }
}

pub struct UpdateRedirects;

impl UpdateRedirects {
    pub const MAX_HOPS: usize = 5;

    pub async fn open<H: UpdateHttp>(
        http: &H,
        origins: &UpdateOrigins,
        mut request: UpdateHttpRequest,
    ) -> Result<(UpdateHttpHead, H::Stream), UpdateHttpError> {
        if !origins.allows(&request.url) {
            return Err(UpdateHttpError::Redirect);
        }
        for _hop in 0..=Self::MAX_HOPS {
            let (head, redirect, stream) = http.open(request.clone()).await?;
            if !matches!(head.status, 301 | 302 | 303 | 307 | 308) {
                return Ok((head, stream));
            }
            let location = redirect.0.ok_or(UpdateHttpError::Redirect)?;
            let next = request
                .url
                .join(&location)
                .map_err(|_error| UpdateHttpError::Redirect)?;
            if !origins.allows(&next) {
                return Err(UpdateHttpError::Redirect);
            }
            if next.origin() != request.url.origin() {
                request.bearer = None;
            }
            request.url = next;
        }
        Err(UpdateHttpError::Redirect)
    }
}

#[derive(Deserialize)]
struct UpdateGithubAsset {
    id: u64,
    name: String,
    size: u64,
}

#[derive(Deserialize)]
struct UpdateGithubRelease {
    id: u64,
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<UpdateGithubAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateDiscovered {
    pub release: UpdateReleaseIdentity,
    pub manifest_digest: [u8; 32],
    pub asset: UpdateManifestAsset,
}

pub struct UpdateDiscovery<H: UpdateHttp, V: UpdateSignatureVerifier> {
    http: H,
    verifier: V,
    repository: UpdateRepository,
    origins: UpdateOrigins,
}

impl<H: UpdateHttp, V: UpdateSignatureVerifier> UpdateDiscovery<H, V> {
    pub const DEADLINE: Duration = Duration::from_secs(15);
    pub const MANIFEST_NAME: &'static str = "manifest.json";
    pub const SIGNATURE_NAME: &'static str = "manifest.json.minisig";
    const METADATA_BYTES: u64 = 1024 * 1024;

    pub fn new(
        http: H,
        verifier: V,
        repository: UpdateRepository,
        origins: UpdateOrigins,
    ) -> Self {
        Self {
            http,
            verifier,
            repository,
            origins,
        }
    }

    pub async fn discover(
        &self,
        installed: &Version,
        target: UpdateTarget,
        credential: Option<&UpdateCredential>,
    ) -> Result<Option<UpdateDiscovered>, UpdateDiscoveryError> {
        let credential = credential.ok_or(UpdateDiscoveryError::CredentialMissing)?;
        tokio::time::timeout(
            Self::DEADLINE,
            self.discover_within(installed, target, credential),
        )
        .await
        .map_err(|_elapsed| UpdateDiscoveryError::Deadline(Self::DEADLINE))?
    }

    async fn fetch(
        &self,
        url: Url,
        accept: &'static str,
        credential: &UpdateCredential,
        limit: u64,
    ) -> Result<Vec<u8>, UpdateDiscoveryError> {
        let request = UpdateHttpRequest {
            url,
            bearer: Some(credential.expose().to_owned()),
            accept,
        };
        let (head, mut stream) = UpdateRedirects::open(&self.http, &self.origins, request).await?;
        match head.status {
            200 => {}
            401 => return Err(UpdateDiscoveryError::CredentialRejected),
            403 | 429 => return Err(UpdateDiscoveryError::RateLimited(head.retry_after)),
            404 => return Err(UpdateDiscoveryError::CredentialRejected),
            other => return Err(UpdateDiscoveryError::Status(other)),
        }
        let mut body = Vec::new();
        while let Some(chunk) = stream.next_chunk().await? {
            let total = u64::try_from(body.len() + chunk.len()).unwrap_or(u64::MAX);
            if total > limit {
                return Err(UpdateHttpError::TooLarge(limit).into());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    async fn discover_within(
        &self,
        installed: &Version,
        target: UpdateTarget,
        credential: &UpdateCredential,
    ) -> Result<Option<UpdateDiscovered>, UpdateDiscoveryError> {
        let listing = self
            .fetch(
                self.repository.latest_release()?,
                "application/vnd.github+json",
                credential,
                Self::METADATA_BYTES,
            )
            .await?;
        let release: UpdateGithubRelease = serde_json::from_slice(&listing)
            .map_err(|error| UpdateDiscoveryError::Release(error.to_string()))?;
        if release.draft || release.prerelease {
            return Ok(None);
        }
        let tag = release
            .tag_name
            .strip_prefix('v')
            .ok_or_else(|| UpdateDiscoveryError::Release(release.tag_name.clone()))?;
        let version = Version::parse(tag)
            .map_err(|_error| UpdateDiscoveryError::Release(release.tag_name.clone()))?;
        if !version.pre.is_empty() || !version.build.is_empty() || version <= *installed {
            return Ok(None);
        }
        let find = |name: &str| {
            release
                .assets
                .iter()
                .find(|asset| asset.name == name)
                .map(|asset| asset.id)
                .ok_or_else(|| UpdateDiscoveryError::Release(format!("missing {name}")))
        };
        let manifest_id = find(Self::MANIFEST_NAME)?;
        let signature_id = find(Self::SIGNATURE_NAME)?;
        let manifest_bytes = self
            .fetch(
                self.repository.asset(manifest_id)?,
                "application/octet-stream",
                credential,
                u64::try_from(UpdateManifest::MAX_BYTES).unwrap_or(u64::MAX),
            )
            .await?;
        let signature_bytes = self
            .fetch(
                self.repository.asset(signature_id)?,
                "application/octet-stream",
                credential,
                u64::try_from(crate::update::verifier::UpdateVerifier::MAX_SIGNATURE_BYTES)
                    .unwrap_or(u64::MAX),
            )
            .await?;
        let manifest =
            UpdateManifest::verify_and_parse(&self.verifier, &manifest_bytes, &signature_bytes)?;
        if *manifest.version() != version {
            return Err(UpdateDiscoveryError::Release(
                "the signed version differs from the release tag".to_owned(),
            ));
        }
        manifest.require_newer(installed)?;
        let asset = manifest
            .asset_for(target)
            .ok_or(UpdateDiscoveryError::NoTarget)?
            .clone();
        let listed = release.assets.iter().any(|candidate| {
            candidate.id == asset.id()
                && candidate.name == asset.name()
                && candidate.size == asset.length()
        });
        if !listed {
            return Err(UpdateDiscoveryError::Release(
                "the signed asset is not in the release".to_owned(),
            ));
        }
        Ok(Some(UpdateDiscovered {
            release: UpdateReleaseIdentity {
                release_id: release.id,
                asset_id: asset.id(),
                version,
                target,
                asset_name: asset.name().to_owned(),
            },
            manifest_digest: manifest.digest(),
            asset,
        }))
    }
}
