use edgehop_proto::MonitorInfo;

/// A rectangle in global desktop coordinates (the primary display's top-left is the origin).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    /// The smallest rectangle containing every monitor, or `None` without monitors.
    pub fn union_of(monitors: &[MonitorInfo]) -> Option<Self> {
        let first = monitors.first()?;
        let (mut min_x, mut min_y) = (first.x, first.y);
        let (mut max_x, mut max_y) = (first.x + first.width, first.y + first.height);
        for monitor in monitors {
            min_x = min_x.min(monitor.x);
            min_y = min_y.min(monitor.y);
            max_x = max_x.max(monitor.x + monitor.width);
            max_y = max_y.max(monitor.y + monitor.height);
        }
        Some(Self::new(min_x, min_y, max_x - min_x, max_y - min_y))
    }
}

/// Where the injected cursor is. Relative moves and clicks need a starting point even if no absolute move was
/// ever received, so on first use (or after being idle) the state is seeded from the real cursor.
///
/// Regression note (found in the Swift core): dropping relative moves until an absolute move had been seen, and
/// clicking at (0, 0), made a receiver that only gets deltas look dead.
#[derive(Debug, Default, Clone)]
pub struct PointerState {
    position: Option<(f64, f64)>,
}

impl PointerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn position(&self) -> Option<(f64, f64)> {
        self.position
    }

    pub fn reset(&mut self) {
        self.position = None;
    }

    pub fn set(&mut self, position: (f64, f64)) {
        self.position = Some(position);
    }

    /// The current position, seeded from `actual` (the real cursor) when unknown. Falls back to the origin if
    /// the real cursor cannot be read.
    pub fn current(&mut self, actual: impl FnOnce() -> Option<(f64, f64)>) -> (f64, f64) {
        if let Some(position) = self.position {
            return position;
        }
        let seeded = actual().unwrap_or((0.0, 0.0));
        self.position = Some(seeded);
        seeded
    }

    /// Applies a delta, clamped to `bounds` (the last pixel is `max - 1`), and returns the new position.
    pub fn moved(
        &mut self,
        dx: f64,
        dy: f64,
        actual: impl FnOnce() -> Option<(f64, f64)>,
        bounds: &Rect,
    ) -> (f64, f64) {
        let (x, y) = self.current(actual);
        let next = (
            (x + dx).clamp(bounds.x, (bounds.max_x() - 1.0).max(bounds.x)),
            (y + dy).clamp(bounds.y, (bounds.max_y() - 1.0).max(bounds.y)),
        );
        self.position = Some(next);
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESKTOP: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 800.0,
    };

    #[test]
    fn relative_move_works_before_any_absolute_move() {
        let mut pointer = PointerState::new();
        let moved = pointer.moved(30.0, -10.0, || Some((500.0, 400.0)), &DESKTOP);
        assert_eq!(moved, (530.0, 390.0));
        assert_eq!(pointer.position(), Some(moved));
    }

    #[test]
    fn seeds_from_the_real_cursor_only_once() {
        let mut pointer = PointerState::new();
        let mut reads = 0;
        for _ in 0..2 {
            pointer.moved(
                1.0,
                0.0,
                || {
                    reads += 1;
                    Some((100.0, 100.0))
                },
                &DESKTOP,
            );
        }
        assert_eq!(reads, 1);
        assert_eq!(pointer.position(), Some((102.0, 100.0)));
    }

    #[test]
    fn clamps_to_the_last_pixel_of_the_desktop() {
        let mut pointer = PointerState::new();
        assert_eq!(
            pointer.moved(5000.0, -5000.0, || Some((10.0, 10.0)), &DESKTOP),
            (999.0, 0.0)
        );
        assert_eq!(
            pointer.moved(-5000.0, 5000.0, || None, &DESKTOP),
            (0.0, 799.0)
        );
    }

    #[test]
    fn clicks_would_use_the_real_cursor_not_the_origin() {
        let mut pointer = PointerState::new();
        assert_eq!(pointer.current(|| Some((640.0, 360.0))), (640.0, 360.0));
    }

    #[test]
    fn an_unreadable_cursor_falls_back_to_the_origin() {
        assert_eq!(PointerState::new().current(|| None), (0.0, 0.0));
    }

    #[test]
    fn reset_reseeds_from_the_real_cursor() {
        let mut pointer = PointerState::new();
        pointer.moved(10.0, 10.0, || Some((0.0, 0.0)), &DESKTOP);
        pointer.reset();
        assert_eq!(pointer.current(|| Some((300.0, 300.0))), (300.0, 300.0));
    }

    #[test]
    fn a_desktop_with_negative_origin_clamps_correctly() {
        let desktop = Rect::new(-223.0, -1080.0, 1920.0, 2062.0);
        let mut pointer = PointerState::new();
        assert_eq!(
            pointer.moved(-9999.0, -9999.0, || Some((0.0, 0.0)), &desktop),
            (-223.0, -1080.0)
        );
        assert_eq!(
            pointer.moved(99999.0, 99999.0, || None, &desktop),
            (1696.0, 981.0)
        );
    }

    #[test]
    fn the_union_of_monitors_covers_all_of_them() {
        let monitor = |id: &str, x, y, width, height| MonitorInfo {
            id: id.into(),
            x,
            y,
            width,
            height,
        };
        let union = Rect::union_of(&[
            monitor("a", 0.0, 0.0, 1512.0, 982.0),
            monitor("b", -223.0, -1080.0, 1920.0, 1080.0),
        ]);
        assert_eq!(union, Some(Rect::new(-223.0, -1080.0, 1920.0, 2062.0)));
        assert_eq!(Rect::union_of(&[]), None);
    }
}
