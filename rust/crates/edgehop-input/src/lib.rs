//! Input injection logic shared by every platform core.
//!
//! The same rules as the Swift core's `EventInjector`, so a Mac and a Windows PC behave alike:
//!
//! * relative moves start from the real cursor when no position is known yet (the controller only sends deltas
//!   once control has moved over), and when the injector was idle, so a locally moved mouse is respected;
//! * a small absolute jump becomes a normal move (apps see hover and drag), a large one warps the cursor;
//! * everything pressed through the injector is released by [`Injector::release_all`].
//!
//! The operating system is reached through the small [`OsInput`] trait, so all of this is unit-tested on any
//! machine with a fake. `edgehop-win` implements the trait for Windows.

mod injector;
mod modifiers;
mod os;
mod platform;
mod pointer;

pub use injector::{Injector, DEFAULT_IDLE_RESYNC, DEFAULT_WARP_THRESHOLD};
pub use modifiers::ModifierTracker;
pub use os::OsInput;
pub use platform::{
    CaptureHandle, CaptureSink, DiscoveredPeer, Discovery, DiscoveryGuard, MotionMapper, PanicGuard,
    Platform, PlatformError,
};
pub use pointer::{PointerState, Rect};
