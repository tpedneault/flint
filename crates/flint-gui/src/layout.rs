//! Where everything goes, as arithmetic: the window's size from the
//! settings, and the rectangles of the query line, the rows and the
//! footer. All in logical pixels; the renderer multiplies by the scale
//! factor. Kept apart from drawing so it is tested without a GPU.

/// The query line's height.
pub const QUERY_HEIGHT: f32 = 44.0;
/// One result row's height.
pub const ROW_HEIGHT: f32 = 34.0;
/// The footer's height.
pub const FOOTER_HEIGHT: f32 = 26.0;
/// Space between the window's edge and text.
pub const PAD: f32 = 12.0;
/// The window's border width.
pub const BORDER: f32 = 1.0;
/// Query text size.
pub const QUERY_FONT: f32 = 18.0;
/// Row text size.
pub const ROW_FONT: f32 = 15.0;
/// Footer text size.
pub const FOOTER_FONT: f32 = 13.0;

/// A rectangle, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// The same rectangle scaled to physical pixels.
    pub fn scaled(self, scale: f32) -> Rect {
        Rect {
            x: self.x * scale,
            y: self.y * scale,
            w: self.w * scale,
            h: self.h * scale,
        }
    }
}

/// The window's parts for a given width and row count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The whole window.
    pub window: Rect,
    /// The query line.
    pub query: Rect,
    /// The first row; row `i` is this moved down by `i * ROW_HEIGHT`.
    pub first_row: Rect,
    /// How many rows fit.
    pub rows: usize,
    /// The footer.
    pub footer: Rect,
}

impl Layout {
    /// Lays out a window `width` wide with room for `rows` rows. The
    /// height always has room for every row, so the window doesn't change
    /// size as results come and go.
    pub fn new(width: u32, rows: u32) -> Layout {
        let width = width as f32;
        let rows = rows.max(1) as usize;
        let query = Rect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: QUERY_HEIGHT,
        };
        let first_row = Rect {
            x: 0.0,
            y: QUERY_HEIGHT,
            w: width,
            h: ROW_HEIGHT,
        };
        let footer = Rect {
            x: 0.0,
            y: QUERY_HEIGHT + rows as f32 * ROW_HEIGHT,
            w: width,
            h: FOOTER_HEIGHT,
        };
        Layout {
            window: Rect {
                x: 0.0,
                y: 0.0,
                w: width,
                h: footer.y + FOOTER_HEIGHT,
            },
            query,
            first_row,
            rows,
            footer,
        }
    }

    /// Row `index`'s rectangle.
    pub fn row(&self, index: usize) -> Rect {
        Rect {
            y: self.first_row.y + index as f32 * ROW_HEIGHT,
            ..self.first_row
        }
    }
}

/// Where a window of `size` goes to be centred horizontally in `area` and
/// a little above the middle, where the eye already is (a third of the
/// free space above it, as Raycast and Spotlight place theirs).
pub fn place(area: (i32, i32, u32, u32), size: (u32, u32)) -> (i32, i32) {
    let (ax, ay, aw, ah) = area;
    let x = ax + (aw.saturating_sub(size.0) / 2) as i32;
    let y = ay + (ah.saturating_sub(size.1) / 3) as i32;
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_window_is_query_rows_and_footer() {
        let l = Layout::new(720, 9);
        assert_eq!(l.window.w, 720.0);
        assert_eq!(l.window.h, QUERY_HEIGHT + 9.0 * ROW_HEIGHT + FOOTER_HEIGHT);
        assert_eq!(l.row(0).y, QUERY_HEIGHT);
        assert_eq!(l.row(8).y + ROW_HEIGHT, l.footer.y);
    }

    #[test]
    fn zero_rows_still_leaves_one() {
        assert_eq!(Layout::new(500, 0).rows, 1);
    }

    #[test]
    fn scaling_multiplies_everything() {
        let r = Rect {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
        }
        .scaled(1.5);
        assert_eq!(
            r,
            Rect {
                x: 1.5,
                y: 3.0,
                w: 4.5,
                h: 6.0
            }
        );
    }

    #[test]
    fn placement_is_centred_and_above_the_middle() {
        assert_eq!(place((0, 0, 1920, 1080), (720, 400)), (600, 226));
        // A second monitor to the left of the primary.
        assert_eq!(place((-1920, 0, 1920, 1080), (720, 400)), (-1320, 226));
        // A window larger than the monitor sits at its corner.
        assert_eq!(place((0, 0, 600, 300), (720, 400)), (0, 0));
    }
}
