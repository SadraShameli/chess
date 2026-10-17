use std::collections::HashMap;

use crate::board::enums::BoardEnumColor;
use crate::desktop::game_snapshot::DesktopGameSnapshot;
use crate::desktop::preferences::DesktopPreferencesEnumAudioCategory;
use crate::game::enums::{GameEnumPhase, GameId};
use crate::presentation::audio::router::PresentationAudioEvent;

#[derive(Debug, Clone, Copy)]
struct PresentationAudioSeen {
    moves: usize,
    completed: bool,
    warnings: [bool; 2],
}

impl PresentationAudioSeen {
    fn of(snapshot: &DesktopGameSnapshot) -> Self {
        Self {
            moves: snapshot.moves().len(),
            completed: snapshot.phase() == GameEnumPhase::Completed,
            warnings: [
                snapshot.clock().warning_used(BoardEnumColor::White),
                snapshot.clock().warning_used(BoardEnumColor::Black),
            ],
        }
    }
}

#[derive(Debug, Default)]
pub struct PresentationAudioObserver {
    seen: HashMap<GameId, PresentationAudioSeen>,
}

impl PresentationAudioObserver {
    pub fn observe<'a>(
        &mut self,
        snapshots: impl IntoIterator<Item = &'a DesktopGameSnapshot>,
    ) -> Vec<PresentationAudioEvent> {
        let mut events = Vec::new();
        let mut present = Vec::new();
        for snapshot in snapshots {
            let id = snapshot.id();
            present.push(id);
            let current = PresentationAudioSeen::of(snapshot);
            if let Some(previous) = self.seen.get(&id) {
                if current.moves > previous.moves {
                    events.push(PresentationAudioEvent::new(
                        id,
                        DesktopPreferencesEnumAudioCategory::Move,
                    ));
                    if snapshot.board().is_check() {
                        events.push(PresentationAudioEvent::new(
                            id,
                            DesktopPreferencesEnumAudioCategory::Check,
                        ));
                    }
                }
                if current.completed && !previous.completed {
                    events.push(PresentationAudioEvent::new(
                        id,
                        DesktopPreferencesEnumAudioCategory::Result,
                    ));
                }
                let warned = current
                    .warnings
                    .iter()
                    .zip(previous.warnings)
                    .any(|(now, before)| *now && !before);
                if warned {
                    events.push(PresentationAudioEvent::new(
                        id,
                        DesktopPreferencesEnumAudioCategory::LowTime,
                    ));
                }
            }
            self.seen.insert(id, current);
        }
        self.seen.retain(|id, _| present.contains(id));
        events
    }
}
