//! Where the widget sits on screen.

/// Rectangle in physical pixels: x, y, width, height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Top-left corner for a widget of `size` placed in the bottom-right of `work_area`
/// (the screen without the taskbar), `margin` pixels away from both edges.
pub fn bottom_right(work_area: Rect, size: (i32, i32), margin: i32) -> (i32, i32) {
    (
        work_area.x + work_area.w - size.0 - margin,
        work_area.y + work_area.h - size.1 - margin,
    )
}

/// Whether a widget at `pos` lies entirely on one of the `monitors` (a saved spot can end up
/// off-screen after a monitor is unplugged or the resolution changes).
pub fn fits(pos: (i32, i32), size: (i32, i32), monitors: &[Rect]) -> bool {
    monitors.iter().any(|m| {
        pos.0 >= m.x && pos.1 >= m.y && pos.0 + size.0 <= m.x + m.w && pos.1 + size.1 <= m.y + m.h
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: Rect = Rect {
        x: 0,
        y: 0,
        w: 1920,
        h: 1200,
    };
    const SECOND: Rect = Rect {
        x: 1920,
        y: 0,
        w: 2560,
        h: 1440,
    };

    #[test]
    fn saved_spot_on_a_monitor_fits() {
        assert!(fits((1400, 950), (500, 165), &[MAIN]));
        assert!(fits((3000, 100), (500, 165), &[MAIN, SECOND]));
    }

    #[test]
    fn saved_spot_off_screen_or_on_unplugged_monitor_does_not_fit() {
        assert!(!fits((3000, 100), (500, 165), &[MAIN]));
        assert!(!fits((1600, 950), (500, 165), &[MAIN]));
        assert!(!fits((-50, 10), (500, 165), &[MAIN]));
    }

    #[test]
    fn bottom_right_respects_taskbar_and_margin() {
        // 1920x1200 screen, 60 px taskbar at the bottom.
        let work = Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1140,
        };
        assert_eq!(bottom_right(work, (400, 140), 24), (1496, 976));
    }

    #[test]
    fn bottom_right_on_second_monitor() {
        let work = Rect {
            x: 1920,
            y: -200,
            w: 2560,
            h: 1400,
        };
        assert_eq!(bottom_right(work, (400, 140), 24), (4056, 1036));
    }
}
