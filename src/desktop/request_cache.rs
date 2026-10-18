use std::collections::{HashMap, VecDeque};

use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::desktop::error::DesktopError;
use crate::desktop::identity::DesktopRequestId;

struct DesktopRequestCacheEntry<T> {
    fingerprint: [u8; 32],
    result: T,
    created: ClockInstant,
}

pub struct DesktopRequestCache<T> {
    entries: HashMap<DesktopRequestId, DesktopRequestCacheEntry<T>>,
    order: VecDeque<DesktopRequestId>,
    last_time: Option<ClockInstant>,
}

impl<T> Default for DesktopRequestCache<T> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            last_time: None,
        }
    }
}

impl<T> DesktopRequestCache<T> {
    pub const CAPACITY: usize = 1024;
    const LIFETIME: ClockDuration = ClockDuration::from_nanos(300_000_000_000);

    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn remember(
        &mut self,
        id: DesktopRequestId,
        fingerprint: [u8; 32],
        result: T,
        now: ClockInstant,
    ) -> Result<(), DesktopError> {
        if self.lookup(id, &fingerprint, now)?.is_some() {
            return Ok(());
        }
        if self.entries.len() == Self::CAPACITY {
            self.remove_oldest();
        }
        self.entries.insert(
            id,
            DesktopRequestCacheEntry {
                fingerprint,
                result,
                created: now,
            },
        );
        self.order.push_back(id);
        Ok(())
    }

    pub fn lookup(
        &mut self,
        id: DesktopRequestId,
        fingerprint: &[u8; 32],
        now: ClockInstant,
    ) -> Result<Option<&T>, DesktopError> {
        self.prune(now)?;
        self.entries
            .get(&id)
            .map(|entry| {
                if entry.fingerprint == *fingerprint {
                    Ok(&entry.result)
                } else {
                    Err(DesktopError::RequestConflict)
                }
            })
            .transpose()
    }

    fn prune(&mut self, now: ClockInstant) -> Result<(), DesktopError> {
        if self.last_time.is_some_and(|previous| now < previous) {
            return Err(DesktopError::TimeOrder);
        }
        self.last_time = Some(now);
        while let Some(id) = self.order.front() {
            let Some(entry) = self.entries.get(id) else {
                self.order.pop_front();
                continue;
            };
            let elapsed = now
                .elapsed_since(entry.created)
                .map_err(|_error| DesktopError::TimeOrder)?;
            if elapsed < Self::LIFETIME {
                break;
            }
            self.remove_oldest();
        }
        Ok(())
    }

    fn remove_oldest(&mut self) {
        if let Some(id) = self.order.pop_front() {
            self.entries.remove(&id);
        }
    }
}
