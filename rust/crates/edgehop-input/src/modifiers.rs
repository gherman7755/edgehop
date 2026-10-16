use edgehop_proto::{modifier_for, Modifiers};

/// Which modifier keys are down, from the key events a platform sees. Key events on the wire carry the modifier
/// state in effect when the key went down (the macOS injector needs it); platforms whose capture does not report
/// it (Windows reports keys one at a time) keep it here.
///
/// Left and right keys are tracked apart, so releasing one Shift while the other is held keeps Shift down.
#[derive(Debug, Default, Clone)]
pub struct ModifierTracker {
    /// Bit `n` is HID usage `0xE0 + n` (left control, left shift, left alt, left GUI, then the right ones).
    down: u8,
}

impl ModifierTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one key event and returns the state after it. Keys that are not modifiers change nothing.
    pub fn apply(&mut self, hid: u16, is_down: bool) -> Modifiers {
        if modifier_for(hid).is_some() {
            let bit = 1u8 << (hid - 0xE0);
            if is_down {
                self.down |= bit;
            } else {
                self.down &= !bit;
            }
        }
        self.state()
    }

    pub fn state(&self) -> Modifiers {
        let held = |left: u8| self.down & (1 << left) != 0 || self.down & (1 << (left + 4)) != 0;
        let mut state = Modifiers::NONE;
        if held(0) {
            state = state | Modifiers::CONTROL;
        }
        if held(1) {
            state = state | Modifiers::SHIFT;
        }
        if held(2) {
            state = state | Modifiers::OPTION;
        }
        if held(3) {
            state = state | Modifiers::COMMAND;
        }
        state
    }

    pub fn reset(&mut self) {
        self.down = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LCTRL: u16 = 0xE0;
    const LSHIFT: u16 = 0xE1;
    const RSHIFT: u16 = 0xE5;
    const LGUI: u16 = 0xE3;

    #[test]
    fn follows_modifier_presses_and_releases() {
        let mut tracker = ModifierTracker::new();
        assert_eq!(tracker.apply(LCTRL, true), Modifiers::CONTROL);
        assert_eq!(
            tracker.apply(LGUI, true),
            Modifiers::CONTROL | Modifiers::COMMAND
        );
        assert_eq!(tracker.apply(LCTRL, false), Modifiers::COMMAND);
        assert_eq!(tracker.apply(LGUI, false), Modifiers::NONE);
    }

    #[test]
    fn left_and_right_keys_are_tracked_apart() {
        let mut tracker = ModifierTracker::new();
        tracker.apply(LSHIFT, true);
        tracker.apply(RSHIFT, true);
        assert_eq!(tracker.apply(LSHIFT, false), Modifiers::SHIFT);
        assert_eq!(tracker.apply(RSHIFT, false), Modifiers::NONE);
    }

    #[test]
    fn other_keys_do_not_change_the_state() {
        let mut tracker = ModifierTracker::new();
        tracker.apply(LSHIFT, true);
        assert_eq!(tracker.apply(0x04, true), Modifiers::SHIFT);
        assert_eq!(tracker.apply(0x04, false), Modifiers::SHIFT);
        tracker.reset();
        assert_eq!(tracker.state(), Modifiers::NONE);
    }
}
