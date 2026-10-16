//! What a platform provides to the sessions: its displays, an injection backend and mouse capture.
//!
//! The sessions in `edgehop-core` are written against [`Platform`], so they are tested on any machine with a
//! fake and run on Windows with `edgehop-win`'s implementation.

use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use edgehop_proto::{InputEvent, MonitorInfo};

use crate::OsInput;

/// What capture reports. Called from the capture thread (on Windows, inside the mouse hook): keep it fast.
pub trait CaptureSink: Send + Sync + 'static {
    /// Every mouse move: where the cursor is, in global desktop pixels, and how far the device moved.
    /// While control is on the other computer the delta is what drives the remote cursor.
    fn motion(&self, position: (f64, f64), delta: (f64, f64));
    /// Every mapped event (moves as absolute positions, buttons, wheel, keys where supported).
    fn event(&self, event: InputEvent);
    /// True if the event must not reach applications here (it is being forwarded).
    fn should_swallow(&self, event: &InputEvent) -> bool;
}

/// A running capture. All methods may be called from the capture thread itself, so none of them may wait for it
/// except [`Self::stop`], which must only be called from elsewhere.
pub trait CaptureHandle: Send + Sync {
    /// Control just moved to the other computer: keep the local cursor out of the way (pin it) so device
    /// movement can still be measured.
    fn enter_remote(&self);
    /// Control is back: put the local cursor at `to` (global pixels) and let it move again.
    fn leave_remote(&self, to: (f64, f64));
    /// Where the real cursor is right now, if the platform can say (diagnostics only).
    fn cursor_position(&self) -> Option<(f64, f64)> {
        None
    }
    /// Stops capturing and gives the cursor back. Idempotent.
    fn stop(&self);
}

/// Keeps a panic-chord watch alive; dropping it stops watching.
pub trait PanicGuard: Send {}

/// Keeps an announcement or a search for computers alive; dropping it stops it.
pub trait DiscoveryGuard: Send {}

/// A computer running a receiver, found on the local network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    /// The name it announces (its computer name).
    pub name: String,
    /// Where to connect, best first.
    pub addresses: Vec<IpAddr>,
    pub port: u16,
}

/// Finding computers on the local network (`_edgehop._tcp`, mDNS / DNS-SD), so nobody has to type addresses.
pub trait Discovery: Send + Sync {
    /// Announces a receiver listening on `port` under `name`. Dropping the guard withdraws the announcement.
    fn advertise(&self, name: &str, port: u16) -> Result<Box<dyn DiscoveryGuard>, PlatformError>;
    /// Looks for other computers' receivers until the guard is dropped. `on_change` is given the whole current
    /// list, sorted by name and without `own_name`, every time it changes.
    fn browse(
        &self,
        own_name: &str,
        on_change: Box<dyn Fn(Vec<DiscoveredPeer>) + Send + Sync>,
    ) -> Result<Box<dyn DiscoveryGuard>, PlatformError>;
}

/// Why capture could not start. The text is shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformError(pub String);

impl std::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PlatformError {}

pub trait Platform: Send + Sync + 'static {
    type Output: OsInput + Send + 'static;

    /// `"windows"`, `"macos"`, ... reported by `core.hello`.
    fn os_name(&self) -> &'static str;
    /// The computer name shown to the other side.
    fn device_name(&self) -> String;
    /// Whether the system lets this process inject and capture input (always true on Windows).
    fn input_permitted(&self) -> bool;
    /// The displays in global desktop coordinates, primary first.
    fn displays(&self) -> Vec<MonitorInfo>;
    /// A new injection backend (one per receiver session).
    fn output(&self) -> Self::Output;
    /// Starts capturing the local mouse (and keyboard where supported).
    fn start_capture(
        &self,
        sink: Arc<dyn CaptureSink>,
    ) -> Result<Arc<dyn CaptureHandle>, PlatformError>;
    /// Finding computers on the network, if this platform can.
    fn discovery(&self) -> Option<Arc<dyn Discovery>> {
        None
    }
    /// Calls `on_panic` when the panic chord (`edgehop_proto::is_panic_chord`) is typed on this computer's own
    /// keyboard, for as long as the returned guard lives. A receiver uses it so the person at the controlled
    /// computer can always stop being controlled. Platforms without keyboard access say so.
    fn watch_panic(
        &self,
        _on_panic: Box<dyn Fn() + Send + Sync>,
    ) -> Result<Box<dyn PanicGuard>, PlatformError> {
        Err(PlatformError(
            "this computer cannot watch for the panic chord".into(),
        ))
    }
}

/// Turns raw capture data into the `(position, delta)` pair [`CaptureSink::motion`] promises, for platforms
/// that cannot freeze the cursor and keep receiving unclamped deltas.
///
/// Locally the delta is the unaccelerated device movement, which keeps changing when the cursor is stopped at a
/// screen edge: that is how an outward push is recognised. While control is on the other computer the cursor is
/// pinned at a point inside the screen and the hook sees where each swallowed move *would* have gone, so the
/// delta is that position minus the pin.
#[derive(Debug, Default)]
pub struct MotionMapper {
    pin: Mutex<Option<(f64, f64)>>,
}

impl MotionMapper {
    pub fn new() -> Self {
        Self::default()
    }

    fn pin(&self) -> MutexGuard<'_, Option<(f64, f64)>> {
        self.pin.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// From now on deltas are measured from `pin`, where the platform has placed the cursor.
    pub fn enter_remote(&self, pin: (f64, f64)) {
        *self.pin() = Some(pin);
    }

    pub fn leave_remote(&self) {
        *self.pin() = None;
    }

    pub fn is_remote(&self) -> bool {
        self.pin().is_some()
    }

    /// `(position, delta)` for one move at pixel `(x, y)` that the device made `(raw_dx, raw_dy)` for.
    pub fn map(&self, x: i32, y: i32, raw_dx: i32, raw_dy: i32) -> ((f64, f64), (f64, f64)) {
        let position = (f64::from(x), f64::from(y));
        let delta = match *self.pin() {
            Some((px, py)) => (position.0 - px, position.1 - py),
            None => (f64::from(raw_dx), f64::from(raw_dy)),
        };
        (position, delta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locally_the_delta_is_the_device_movement() {
        let mapper = MotionMapper::new();
        assert_eq!(mapper.map(1919, 500, 7, -3), ((1919.0, 500.0), (7.0, -3.0)));
        assert!(!mapper.is_remote());
    }

    #[test]
    fn remotely_the_delta_is_measured_from_the_pin() {
        let mapper = MotionMapper::new();
        mapper.enter_remote((960.0, 540.0));
        assert!(mapper.is_remote());
        assert_eq!(mapper.map(965, 538, 99, 99), ((965.0, 538.0), (5.0, -2.0)));
        // A move that did not go anywhere is no movement, whatever the raw input says.
        assert_eq!(mapper.map(960, 540, 4, 4).1, (0.0, 0.0));
    }

    #[test]
    fn leaving_goes_back_to_device_movement() {
        let mapper = MotionMapper::new();
        mapper.enter_remote((10.0, 10.0));
        mapper.leave_remote();
        assert_eq!(mapper.map(50, 50, 1, 2).1, (1.0, 2.0));
    }
}
