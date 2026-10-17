use semver::Version;
use thiserror::Error;
use uuid::Uuid;

use crate::update::manifest::UpdateTarget;
use crate::update::network_cost::{UpdateNetworkApproval, UpdateNetworkEnumCost};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UpdateAttemptId(Uuid);

impl UpdateAttemptId {
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }

    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for UpdateAttemptId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateReleaseIdentity {
    pub release_id: u64,
    pub asset_id: u64,
    pub version: Version,
    pub target: UpdateTarget,
    pub asset_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCacheReceipt {
    pub release: UpdateReleaseIdentity,
    pub manifest_digest: [u8; 32],
    pub package_sha256: [u8; 32],
    pub length: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateEnumFailure {
    Network,
    Timeout,
    Signature,
    Storage,
    Space,
    NetworkChanged,
    Verification,
    Install,
    RateLimited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateEnumPhase {
    Idle,
    Checking,
    UpToDate,
    CredentialsMissing,
    CredentialsRejected,
    Available,
    AwaitingNetworkApproval,
    Downloading,
    Downloaded,
    InstallPrerequisites,
    Handoff,
    Failed(UpdateEnumFailure),
    Cancelled,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("The update action {action} is not allowed while {from:?}")]
pub struct UpdateStateError {
    action: &'static str,
    from: UpdateEnumPhase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateState {
    phase: UpdateEnumPhase,
    attempt: Option<UpdateAttemptId>,
    release: Option<UpdateReleaseIdentity>,
    manifest_digest: Option<[u8; 32]>,
    receipt: Option<UpdateCacheReceipt>,
    approval: Option<UpdateNetworkApproval>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            phase: UpdateEnumPhase::Idle,
            attempt: None,
            release: None,
            manifest_digest: None,
            receipt: None,
            approval: None,
        }
    }
}

impl UpdateState {
    pub fn with_cached(receipt: UpdateCacheReceipt) -> Self {
        Self {
            phase: UpdateEnumPhase::Downloaded,
            attempt: None,
            release: Some(receipt.release.clone()),
            manifest_digest: Some(receipt.manifest_digest),
            receipt: Some(receipt),
            approval: None,
        }
    }

    pub const fn phase(&self) -> UpdateEnumPhase {
        self.phase
    }

    pub const fn attempt(&self) -> Option<UpdateAttemptId> {
        self.attempt
    }

    pub const fn release(&self) -> Option<&UpdateReleaseIdentity> {
        self.release.as_ref()
    }

    pub const fn manifest_digest(&self) -> Option<[u8; 32]> {
        self.manifest_digest
    }

    pub const fn receipt(&self) -> Option<&UpdateCacheReceipt> {
        self.receipt.as_ref()
    }

    pub const fn approval(&self) -> Option<UpdateNetworkApproval> {
        self.approval
    }

    pub const fn is_busy(&self) -> bool {
        matches!(
            self.phase,
            UpdateEnumPhase::Checking
                | UpdateEnumPhase::Downloading
                | UpdateEnumPhase::AwaitingNetworkApproval
                | UpdateEnumPhase::InstallPrerequisites
                | UpdateEnumPhase::Handoff
        )
    }

    pub const fn can_retry(&self) -> bool {
        matches!(
            self.phase,
            UpdateEnumPhase::Failed(_)
                | UpdateEnumPhase::Cancelled
                | UpdateEnumPhase::CredentialsMissing
                | UpdateEnumPhase::CredentialsRejected
        )
    }

    const fn refuse(&self, action: &'static str) -> UpdateStateError {
        UpdateStateError {
            action,
            from: self.phase,
        }
    }

    pub fn begin_check(&mut self) -> Result<UpdateAttemptId, UpdateStateError> {
        if self.is_busy() {
            return Err(self.refuse("check"));
        }
        let attempt = UpdateAttemptId::new();
        self.attempt = Some(attempt);
        self.approval = None;
        self.phase = UpdateEnumPhase::Checking;
        Ok(attempt)
    }

    pub fn check_found(
        &mut self,
        release: UpdateReleaseIdentity,
        manifest_digest: [u8; 32],
    ) -> Result<(), UpdateStateError> {
        if self.phase != UpdateEnumPhase::Checking {
            return Err(self.refuse("record a found release"));
        }
        self.release = Some(release);
        self.manifest_digest = Some(manifest_digest);
        self.phase = UpdateEnumPhase::Available;
        Ok(())
    }

    pub fn check_current(&mut self) -> Result<(), UpdateStateError> {
        if self.phase != UpdateEnumPhase::Checking {
            return Err(self.refuse("record an up-to-date result"));
        }
        self.phase = UpdateEnumPhase::UpToDate;
        Ok(())
    }

    pub fn credentials_missing(&mut self) -> Result<(), UpdateStateError> {
        self.require(UpdateEnumPhase::Checking, "report missing credentials")?;
        self.phase = UpdateEnumPhase::CredentialsMissing;
        Ok(())
    }

    pub fn credentials_rejected(&mut self) -> Result<(), UpdateStateError> {
        if !matches!(
            self.phase,
            UpdateEnumPhase::Checking | UpdateEnumPhase::Downloading
        ) {
            return Err(self.refuse("report rejected credentials"));
        }
        self.phase = UpdateEnumPhase::CredentialsRejected;
        self.approval = None;
        Ok(())
    }

    fn require(
        &self,
        phase: UpdateEnumPhase,
        action: &'static str,
    ) -> Result<(), UpdateStateError> {
        if self.phase == phase {
            Ok(())
        } else {
            Err(self.refuse(action))
        }
    }

    pub fn request_download(
        &mut self,
        cost: UpdateNetworkEnumCost,
    ) -> Result<UpdateAttemptId, UpdateStateError> {
        let allowed = matches!(
            self.phase,
            UpdateEnumPhase::Available | UpdateEnumPhase::Failed(_) | UpdateEnumPhase::Cancelled
        ) && self.release.is_some();
        if !allowed {
            return Err(self.refuse("download"));
        }
        let attempt = UpdateAttemptId::new();
        self.attempt = Some(attempt);
        self.approval = None;
        self.phase = if cost.requires_approval() {
            UpdateEnumPhase::AwaitingNetworkApproval
        } else {
            self.approval = Some(UpdateNetworkApproval::new(attempt, cost));
            UpdateEnumPhase::Downloading
        };
        Ok(attempt)
    }

    pub fn approve(&mut self, observed: UpdateNetworkEnumCost) -> Result<(), UpdateStateError> {
        self.require(UpdateEnumPhase::AwaitingNetworkApproval, "approve")?;
        let attempt = self.attempt.ok_or_else(|| self.refuse("approve"))?;
        self.approval = Some(UpdateNetworkApproval::new(attempt, observed));
        self.phase = UpdateEnumPhase::Downloading;
        Ok(())
    }

    pub fn network_observed(
        &mut self,
        observed: UpdateNetworkEnumCost,
    ) -> Result<(), UpdateStateError> {
        self.require(UpdateEnumPhase::Downloading, "observe the network")?;
        let attempt = self.attempt.ok_or_else(|| self.refuse("observe the network"))?;
        let covered = self
            .approval
            .is_some_and(|approval| approval.covers(attempt, observed));
        if !covered {
            self.approval = None;
            self.phase = UpdateEnumPhase::Failed(UpdateEnumFailure::NetworkChanged);
        }
        Ok(())
    }

    pub fn download_completed(
        &mut self,
        receipt: UpdateCacheReceipt,
    ) -> Result<(), UpdateStateError> {
        self.require(UpdateEnumPhase::Downloading, "complete a download")?;
        if Some(&receipt.release) != self.release.as_ref()
            || Some(receipt.manifest_digest) != self.manifest_digest
        {
            return Err(self.refuse("complete a download for another release"));
        }
        self.receipt = Some(receipt);
        self.approval = None;
        self.phase = UpdateEnumPhase::Downloaded;
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), UpdateStateError> {
        if !matches!(
            self.phase,
            UpdateEnumPhase::Checking
                | UpdateEnumPhase::AwaitingNetworkApproval
                | UpdateEnumPhase::Downloading
        ) {
            return Err(self.refuse("cancel"));
        }
        self.approval = None;
        self.phase = UpdateEnumPhase::Cancelled;
        Ok(())
    }

    pub fn fail(&mut self, failure: UpdateEnumFailure) -> Result<(), UpdateStateError> {
        if !self.is_busy() {
            return Err(self.refuse("fail"));
        }
        self.approval = None;
        self.phase = UpdateEnumPhase::Failed(failure);
        Ok(())
    }

    pub fn begin_install_prerequisites(&mut self) -> Result<(), UpdateStateError> {
        self.require(UpdateEnumPhase::Downloaded, "begin installation")?;
        self.phase = UpdateEnumPhase::InstallPrerequisites;
        Ok(())
    }

    pub fn abort_install(&mut self) -> Result<(), UpdateStateError> {
        self.require(UpdateEnumPhase::InstallPrerequisites, "abort installation")?;
        self.phase = UpdateEnumPhase::Downloaded;
        Ok(())
    }

    pub fn begin_handoff(&mut self) -> Result<(), UpdateStateError> {
        self.require(UpdateEnumPhase::InstallPrerequisites, "hand off to the helper")?;
        self.phase = UpdateEnumPhase::Handoff;
        Ok(())
    }
}
