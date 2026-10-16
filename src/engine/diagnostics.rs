use std::collections::VecDeque;
use std::fmt;

use crate::engine::process_provider::{EngineProcessEnumPipe, EngineProcessLine};

pub struct EngineDiagnosticsEntry {
    pipe: EngineProcessEnumPipe,
    text: Box<str>,
}

impl EngineDiagnosticsEntry {
    pub fn pipe(&self) -> EngineProcessEnumPipe {
        self.pipe
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

pub struct EngineDiagnostics {
    entries: VecDeque<EngineDiagnosticsEntry>,
    payload_bytes: usize,
    evicted_lines: u64,
    evicted_bytes: u64,
}

impl Default for EngineDiagnostics {
    fn default() -> Self {
        Self {
            entries: VecDeque::with_capacity(Self::MAX_ENTRIES),
            payload_bytes: 0,
            evicted_lines: 0,
            evicted_bytes: 0,
        }
    }
}

impl fmt::Debug for EngineDiagnostics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EngineDiagnostics")
            .field("lines", &self.entries.len())
            .field("owned_bytes", &self.owned_bytes())
            .field("evicted_lines", &self.evicted_lines)
            .field("evicted_bytes", &self.evicted_bytes)
            .finish()
    }
}

impl EngineDiagnostics {
    pub const MAX_OWNED_BYTES: usize = 1024 * 1024;
    pub const MAX_ENTRIES: usize = 4096;

    pub fn entries(
        &self,
    ) -> impl DoubleEndedIterator<Item = &EngineDiagnosticsEntry> + ExactSizeIterator {
        self.entries.iter()
    }

    pub fn owned_bytes(&self) -> usize {
        size_of::<Self>()
            + self.entries.capacity() * size_of::<EngineDiagnosticsEntry>()
            + self.payload_bytes
    }

    pub fn evicted_lines(&self) -> u64 {
        self.evicted_lines
    }

    pub fn evicted_bytes(&self) -> u64 {
        self.evicted_bytes
    }

    pub fn record(&mut self, line: &EngineProcessLine) {
        while self.entries.len() == Self::MAX_ENTRIES
            || self.owned_bytes() + line.text().len() > Self::MAX_OWNED_BYTES
        {
            if let Some(entry) = self.entries.pop_front() {
                self.payload_bytes -= entry.text.len();
                self.evicted_lines = self.evicted_lines.saturating_add(1);
                self.evicted_bytes = self
                    .evicted_bytes
                    .saturating_add(u64::try_from(entry.text.len()).unwrap_or(u64::MAX));
            }
        }
        self.payload_bytes += line.text().len();
        self.entries.push_back(EngineDiagnosticsEntry {
            pipe: line.pipe(),
            text: line.text().into(),
        });
    }
}
