use crate::update::state::UpdateAttemptId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateNetworkEnumCost {
    Unmetered,
    Metered,
    Unknown,
}

impl UpdateNetworkEnumCost {
    pub const fn requires_approval(self) -> bool {
        !matches!(self, Self::Unmetered)
    }
}

pub trait UpdateNetworkCostProvider {
    fn current(&self) -> UpdateNetworkEnumCost;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct UpdateNetworkCostUnsupported;

impl UpdateNetworkCostProvider for UpdateNetworkCostUnsupported {
    fn current(&self) -> UpdateNetworkEnumCost {
        UpdateNetworkEnumCost::Unknown
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateNetworkApproval {
    attempt: UpdateAttemptId,
    cost: UpdateNetworkEnumCost,
}

impl UpdateNetworkApproval {
    pub const fn new(attempt: UpdateAttemptId, cost: UpdateNetworkEnumCost) -> Self {
        Self { attempt, cost }
    }

    pub const fn attempt(self) -> UpdateAttemptId {
        self.attempt
    }

    pub const fn cost(self) -> UpdateNetworkEnumCost {
        self.cost
    }

    pub fn covers(self, attempt: UpdateAttemptId, observed: UpdateNetworkEnumCost) -> bool {
        self.attempt == attempt && self.cost == observed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateNetworkEnumVerdict {
    Continue,
    Stop,
}

#[derive(Debug, Clone, Copy)]
pub struct UpdateNetworkMonitor {
    approval: UpdateNetworkApproval,
    last_poll_ms: u64,
}

impl UpdateNetworkMonitor {
    pub const POLL_INTERVAL_MS: u64 = 5_000;

    pub const fn new(approval: UpdateNetworkApproval, now_ms: u64) -> Self {
        Self {
            approval,
            last_poll_ms: now_ms,
        }
    }

    pub const fn poll_due(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.last_poll_ms) >= Self::POLL_INTERVAL_MS
    }

    pub fn observe(
        &mut self,
        observed: UpdateNetworkEnumCost,
        now_ms: u64,
    ) -> UpdateNetworkEnumVerdict {
        self.last_poll_ms = now_ms;
        if observed == self.approval.cost() {
            UpdateNetworkEnumVerdict::Continue
        } else {
            UpdateNetworkEnumVerdict::Stop
        }
    }

    pub fn os_change(&mut self, observed: UpdateNetworkEnumCost) -> UpdateNetworkEnumVerdict {
        self.observe(observed, self.last_poll_ms)
    }
}
