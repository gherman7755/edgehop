/// A point in global desktop coordinates (the primary display's top-left is the origin).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

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

    pub fn min_x(&self) -> f64 {
        self.x
    }

    pub fn min_y(&self) -> f64 {
        self.y
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    /// Half-open: the right and bottom edges belong to the next rectangle.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min_x() && p.x < self.max_x() && p.y >= self.min_y() && p.y < self.max_y()
    }

    /// Distance from `p` to the rectangle (0 inside).
    pub fn distance_to(&self, p: Point) -> f64 {
        let dx = (self.min_x() - p.x).max(0.0).max(p.x - self.max_x());
        let dy = (self.min_y() - p.y).max(0.0).max(p.y - self.max_y());
        dx.hypot(dy)
    }
}

/// Where the other computer sits relative to this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "top" => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            _ => None,
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_are_half_open() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(r.contains(Point::new(0.0, 0.0)));
        assert!(r.contains(Point::new(9.99, 9.99)));
        assert!(!r.contains(Point::new(10.0, 5.0)));
        assert!(!r.contains(Point::new(5.0, 10.0)));
        assert!(!r.contains(Point::new(-0.1, 5.0)));
    }

    #[test]
    fn distance_is_zero_inside_and_euclidean_outside() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(r.distance_to(Point::new(5.0, 5.0)), 0.0);
        assert_eq!(r.distance_to(Point::new(13.0, 5.0)), 3.0);
        assert_eq!(r.distance_to(Point::new(13.0, 14.0)), 5.0);
        assert_eq!(r.distance_to(Point::new(-4.0, 5.0)), 4.0);
    }

    #[test]
    fn sides_parse_strictly_and_have_opposites() {
        assert_eq!(Side::parse("left"), Some(Side::Left));
        assert_eq!(Side::parse("Right"), None);
        assert_eq!(Side::parse("up"), None);
        for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
            assert_eq!(side.opposite().opposite(), side);
            assert_ne!(side.opposite(), side);
        }
    }
}
