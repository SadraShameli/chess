use std::collections::VecDeque;

use bevy_egui::egui::{Event, RawInput, accesskit::ActionRequest};
use thiserror::Error;

use crate::presentation::board::semantic_grid::PresentationBoardSemanticGrid;
use crate::scene::renderer::SceneRendererGeneration;

#[derive(Debug, Error)]
pub enum PresentationAccessibilityError {
    #[error("The accessibility input queue is full")]
    Busy,
    #[error("A board accessibility action requires its displayed scene generation")]
    MissingGeneration,
}

struct PresentationAccessibilityPending {
    request: ActionRequest,
    generation: Option<SceneRendererGeneration>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PresentationAccessibilityDelivery {
    delivered: usize,
    stale: usize,
}

impl PresentationAccessibilityDelivery {
    pub const fn delivered(self) -> usize {
        self.delivered
    }

    pub const fn stale(self) -> usize {
        self.stale
    }
}

pub struct PresentationAccessibilityInput {
    pending: VecDeque<PresentationAccessibilityPending>,
}

impl Default for PresentationAccessibilityInput {
    fn default() -> Self {
        Self::new()
    }
}

impl PresentationAccessibilityInput {
    pub const CAPACITY: usize = 64;

    pub fn new() -> Self {
        Self {
            pending: VecDeque::with_capacity(Self::CAPACITY),
        }
    }

    pub fn try_forward(
        &mut self,
        request: ActionRequest,
        generation: Option<SceneRendererGeneration>,
    ) -> Result<(), PresentationAccessibilityError> {
        let board_square =
            PresentationBoardSemanticGrid::square_for_accessibility_target(request.target_node);
        if board_square.is_some() && generation.is_none() {
            return Err(PresentationAccessibilityError::MissingGeneration);
        }
        if self.pending.len() == Self::CAPACITY {
            return Err(PresentationAccessibilityError::Busy);
        }
        self.pending.push_back(PresentationAccessibilityPending {
            request,
            generation: board_square.and(generation),
        });
        Ok(())
    }

    pub fn drain_into(
        &mut self,
        input: &mut RawInput,
        current_generation: Option<SceneRendererGeneration>,
    ) -> PresentationAccessibilityDelivery {
        let mut delivery = PresentationAccessibilityDelivery::default();
        for pending in self.pending.drain(..) {
            if pending.generation.is_some() && pending.generation != current_generation {
                delivery.stale += 1;
            } else {
                input
                    .events
                    .push(Event::AccessKitActionRequest(pending.request));
                delivery.delivered += 1;
            }
        }
        delivery
    }
}
