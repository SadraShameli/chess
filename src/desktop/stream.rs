use std::collections::VecDeque;

use schemars::JsonSchema;
use serde::Serialize;
use tauri::ipc::Channel;

use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::desktop::envelope::DesktopProtocolVersion;
use crate::desktop::error::DesktopError;
use crate::desktop::identity::{DesktopRequestId, DesktopRevision};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesktopEnumStreamClass {
    Control,
    Snapshot,
    JobProgress,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesktopStreamEvent<T> {
    protocol_version: DesktopProtocolVersion,
    subscription_id: DesktopRequestId,
    sequence: DesktopRevision,
    class: DesktopEnumStreamClass,
    payload: T,
}

impl<T> DesktopStreamEvent<T> {
    pub fn sequence(&self) -> DesktopRevision {
        self.sequence
    }

    pub fn subscription_id(&self) -> DesktopRequestId {
        self.subscription_id
    }

    pub fn payload(&self) -> &T {
        &self.payload
    }
}

pub struct DesktopStream<T> {
    subscription_id: DesktopRequestId,
    sequence: DesktopRevision,
    acknowledged: DesktopRevision,
    unacknowledged: VecDeque<(DesktopRevision, ClockInstant)>,
    pending_snapshot: Option<T>,
    pending_progress: Option<T>,
    last_progress: Option<ClockInstant>,
    last_time: ClockInstant,
    resync_required: bool,
}

impl<T> DesktopStream<T> {
    pub const MAX_UNACKNOWLEDGED: usize = 64;
    const ACK_DEADLINE: ClockDuration = ClockDuration::from_nanos(10_000_000_000);
    const PROGRESS_INTERVAL: ClockDuration = ClockDuration::from_nanos(200_000_000);

    pub fn new(subscription_id: DesktopRequestId, now: ClockInstant) -> Self {
        Self {
            subscription_id,
            sequence: DesktopRevision::ZERO,
            acknowledged: DesktopRevision::ZERO,
            unacknowledged: VecDeque::with_capacity(Self::MAX_UNACKNOWLEDGED),
            pending_snapshot: None,
            pending_progress: None,
            last_progress: None,
            last_time: now,
            resync_required: false,
        }
    }

    pub fn is_resync_required(&self) -> bool {
        self.resync_required
    }

    pub fn acknowledgement_deadline(&self) -> Result<Option<ClockInstant>, DesktopError> {
        if self.resync_required {
            return Ok(None);
        }
        self.unacknowledged
            .front()
            .map(|(_sequence, sent)| sent.checked_add(Self::ACK_DEADLINE))
            .transpose()
            .map_err(|_error| DesktopError::TimeOrder)
    }

    pub fn require_resync(&mut self) {
        self.resync_required = true;
        self.pending_snapshot = None;
        self.pending_progress = None;
        self.unacknowledged.clear();
    }

    pub fn check_health(&mut self, now: ClockInstant) -> Result<(), DesktopError> {
        if self.resync_required {
            return Err(DesktopError::ResyncRequired);
        }
        if now < self.last_time {
            return Err(DesktopError::TimeOrder);
        }
        self.last_time = now;
        if let Some((_sequence, sent)) = self.unacknowledged.front() {
            let elapsed = now
                .elapsed_since(*sent)
                .map_err(|_error| DesktopError::TimeOrder)?;
            if elapsed >= Self::ACK_DEADLINE {
                self.require_resync();
                return Err(DesktopError::ResyncRequired);
            }
        }
        Ok(())
    }

    pub fn publish(
        &mut self,
        payload: T,
        class: DesktopEnumStreamClass,
        now: ClockInstant,
    ) -> Result<Option<DesktopStreamEvent<T>>, DesktopError> {
        self.check_health(now)?;
        if self.unacknowledged.len() == Self::MAX_UNACKNOWLEDGED {
            match class {
                DesktopEnumStreamClass::Control => {
                    self.require_resync();
                    return Err(DesktopError::ResyncRequired);
                }
                DesktopEnumStreamClass::Snapshot => self.pending_snapshot = Some(payload),
                DesktopEnumStreamClass::JobProgress => self.pending_progress = Some(payload),
            }
            return Ok(None);
        }
        match class {
            DesktopEnumStreamClass::JobProgress if !self.progress_due(now)? => {
                self.pending_progress = Some(payload);
                return Ok(None);
            }
            DesktopEnumStreamClass::JobProgress => {
                self.last_progress = Some(now);
                self.pending_progress = None;
            }
            DesktopEnumStreamClass::Snapshot => self.pending_snapshot = None,
            DesktopEnumStreamClass::Control => {}
        }
        self.sequence = self
            .sequence
            .next()
            .inspect_err(|_error| self.require_resync())?;
        self.unacknowledged.push_back((self.sequence, now));
        Ok(Some(DesktopStreamEvent {
            protocol_version: DesktopProtocolVersion::CURRENT,
            subscription_id: self.subscription_id,
            sequence: self.sequence,
            class,
            payload,
        }))
    }

    pub fn acknowledge(
        &mut self,
        sequence: DesktopRevision,
        now: ClockInstant,
    ) -> Result<(), DesktopError> {
        self.check_health(now)?;
        if sequence < self.acknowledged || sequence > self.sequence {
            return Err(DesktopError::InvalidAcknowledgement);
        }
        self.acknowledged = sequence;
        while self
            .unacknowledged
            .front()
            .is_some_and(|(sent, _time)| *sent <= sequence)
        {
            self.unacknowledged.pop_front();
        }
        Ok(())
    }

    pub fn flush_pending(
        &mut self,
        now: ClockInstant,
    ) -> Result<Vec<DesktopStreamEvent<T>>, DesktopError> {
        self.check_health(now)?;
        let mut events = Vec::with_capacity(2);
        if let Some(snapshot) = self.pending_snapshot.take()
            && let Some(event) = self.publish(snapshot, DesktopEnumStreamClass::Snapshot, now)?
        {
            events.push(event);
        }
        if self.progress_due(now)?
            && let Some(progress) = self.pending_progress.take()
            && let Some(event) = self.publish(progress, DesktopEnumStreamClass::JobProgress, now)?
        {
            events.push(event);
        }
        Ok(events)
    }

    pub fn resynchronize(
        &mut self,
        snapshot: T,
        now: ClockInstant,
    ) -> Result<DesktopStreamEvent<T>, DesktopError> {
        if now < self.last_time {
            return Err(DesktopError::TimeOrder);
        }
        self.require_resync();
        self.resync_required = false;
        self.acknowledged = self.sequence;
        self.publish(snapshot, DesktopEnumStreamClass::Snapshot, now)?
            .ok_or(DesktopError::ResyncRequired)
    }

    fn progress_due(&self, now: ClockInstant) -> Result<bool, DesktopError> {
        self.last_progress
            .map(|previous| {
                now.elapsed_since(previous)
                    .map(|elapsed| elapsed >= Self::PROGRESS_INTERVAL)
                    .map_err(|_error| DesktopError::TimeOrder)
            })
            .transpose()
            .map(|due| due.unwrap_or(true))
    }
}

pub struct DesktopSubscription<T> {
    stream: DesktopStream<T>,
    channel: Channel<DesktopStreamEvent<T>>,
}

impl<T: Serialize> DesktopSubscription<T> {
    pub fn new(
        subscription_id: DesktopRequestId,
        channel: Channel<DesktopStreamEvent<T>>,
        now: ClockInstant,
    ) -> Self {
        Self {
            stream: DesktopStream::new(subscription_id, now),
            channel,
        }
    }

    pub fn publish(
        &mut self,
        payload: T,
        class: DesktopEnumStreamClass,
        now: ClockInstant,
    ) -> Result<(), DesktopError> {
        if let Some(event) = self.stream.publish(payload, class, now)? {
            self.send(event)?;
        }
        Ok(())
    }

    pub fn acknowledge(
        &mut self,
        sequence: DesktopRevision,
        now: ClockInstant,
    ) -> Result<(), DesktopError> {
        self.stream.acknowledge(sequence, now)?;
        self.tick(now)
    }

    pub fn tick(&mut self, now: ClockInstant) -> Result<(), DesktopError> {
        for event in self.stream.flush_pending(now)? {
            self.send(event)?;
        }
        Ok(())
    }

    pub fn resynchronize(&mut self, snapshot: T, now: ClockInstant) -> Result<(), DesktopError> {
        let event = self.stream.resynchronize(snapshot, now)?;
        self.send(event)
    }

    fn send(&mut self, event: DesktopStreamEvent<T>) -> Result<(), DesktopError> {
        self.channel.send(event).map_err(|_error| {
            self.stream.require_resync();
            DesktopError::ResyncRequired
        })
    }
}
