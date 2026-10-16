use tokio::sync::OwnedSemaphorePermit;

pub struct CapacityLease {
    permits: OwnedSemaphorePermit,
}

impl CapacityLease {
    pub(super) fn new(permits: OwnedSemaphorePermit) -> Self {
        Self { permits }
    }
    pub fn workers(&self) -> usize {
        self.permits.num_permits()
    }
}
