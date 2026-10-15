//! Edge switching: decides when this computer hands control to the other one and back. A port of the Swift
//! core's `SwitchController` with the same rules and the same tests, so both cores switch identically.
//!
//! Pure logic: no operating-system calls, so it is tested on any machine. A platform feeds it the real cursor
//! position and device deltas and carries out its [`Actions`].

mod controller;
mod geometry;

pub use controller::{Actions, Mode, SwitchController, BAND};
pub use geometry::{Point, Rect, Side};
