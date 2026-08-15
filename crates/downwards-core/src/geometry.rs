/// An integer point. Content geometry is expressed in pixels; simulation
/// accessors whose names end in `_subpixels` use the same type in subpixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A half-open rectangle in pixels: `[x, x + width) x [y, y + height)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub const fn right(self) -> i32 {
        // Rectangles can be assembled from untrusted content before the room
        // validator has had a chance to reject them. Saturation keeps bounds
        // checks total for coordinates near the integer limits; valid room
        // geometry is tiny enough that this is identical to ordinary addition.
        self.x.saturating_add(self.width)
    }

    #[must_use]
    pub const fn bottom(self) -> i32 {
        self.y.saturating_add(self.height)
    }

    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extreme_rectangle_edges_and_predicates_do_not_overflow() {
        let screen = Rect::new(0, 0, 320, 180);
        let extreme = Rect::new(i32::MAX, i32::MAX, 8, 12);

        assert_eq!(extreme.right(), i32::MAX);
        assert_eq!(extreme.bottom(), i32::MAX);
        assert!(!screen.contains(extreme));
        assert!(!screen.intersects(extreme));

        let negative_extreme = Rect::new(i32::MIN, i32::MIN, -8, -12);
        assert_eq!(negative_extreme.right(), i32::MIN);
        assert_eq!(negative_extreme.bottom(), i32::MIN);
        assert!(!screen.contains(negative_extreme));
        assert!(!screen.intersects(negative_extreme));
    }
}
