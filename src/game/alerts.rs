use crate::board::enums::BoardEnumColor;
use crate::clock::duration::ClockDuration;
use crate::clock::state::ClockState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GameAlertsLowTime {
    white_used: bool,
    black_used: bool,
}

impl GameAlertsLowTime {
    pub const THRESHOLD_NANOS: u64 = 10_000_000_000;

    pub const fn restore(white_used: bool, black_used: bool) -> Self {
        Self {
            white_used,
            black_used,
        }
    }

    pub fn from_clock(clock: &ClockState) -> Self {
        Self::restore(
            clock.warning_used(BoardEnumColor::White),
            clock.warning_used(BoardEnumColor::Black),
        )
    }

    pub fn merge_into(self, clock: &mut ClockState) {
        if self.white_used {
            clock.merge_warning_used(BoardEnumColor::White);
        }
        if self.black_used {
            clock.merge_warning_used(BoardEnumColor::Black);
        }
    }

    pub const fn is_used(self, color: BoardEnumColor) -> bool {
        match color {
            BoardEnumColor::White => self.white_used,
            BoardEnumColor::Black => self.black_used,
        }
    }

    pub const fn flags(self) -> (bool, bool) {
        (self.white_used, self.black_used)
    }

    pub fn observe(&mut self, color: BoardEnumColor, remaining: ClockDuration) -> bool {
        if remaining.as_nanos() > Self::THRESHOLD_NANOS || self.is_used(color) {
            return false;
        }
        match color {
            BoardEnumColor::White => self.white_used = true,
            BoardEnumColor::Black => self.black_used = true,
        }
        true
    }
}
