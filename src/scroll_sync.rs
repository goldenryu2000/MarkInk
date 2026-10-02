//! Estimates the editor's top visible line so the preview can follow it.
//!
//! iced's editor does not expose its scroll offset, so we track wheel scrolls
//! and mirror its "keep the cursor visible" rule.

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ViewEstimate {
    top: f32,
}

impl ViewEstimate {
    pub fn top(&self) -> f32 {
        self.top
    }

    /// A wheel scroll of `lines` in a note of `total` lines.
    pub fn scrolled(&mut self, lines: f32, total: usize) {
        let max = total.saturating_sub(1) as f32;
        self.top = (self.top + lines).clamp(0.0, max);
    }

    /// Scrolls the minimum needed to keep `cursor_line` within `visible` lines.
    pub fn follow_cursor(&mut self, cursor_line: usize, visible: f32) {
        let line = cursor_line as f32;
        if line < self.top {
            self.top = line;
        } else if line >= self.top + visible {
            self.top = line - visible + 1.0;
        }
    }

    /// Relative scroll position from 0 (top) to 1 (bottom).
    pub fn fraction(&self, total: usize, visible: f32) -> f32 {
        let scrollable = total as f32 - visible;
        if scrollable <= 0.0 {
            return 0.0;
        }
        (self.top / scrollable).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_scroll_is_clamped() {
        let mut view = ViewEstimate::default();
        view.scrolled(-5.0, 100);
        assert_eq!(view.top(), 0.0);
        view.scrolled(30.0, 100);
        assert_eq!(view.top(), 30.0);
        view.scrolled(500.0, 100);
        assert_eq!(view.top(), 99.0);
    }

    #[test]
    fn follows_cursor_only_when_it_leaves_the_view() {
        let mut view = ViewEstimate::default();
        view.follow_cursor(10, 20.0);
        assert_eq!(view.top(), 0.0);
        view.follow_cursor(45, 20.0);
        assert_eq!(view.top(), 26.0);
        view.follow_cursor(30, 20.0);
        assert_eq!(view.top(), 26.0);
        view.follow_cursor(3, 20.0);
        assert_eq!(view.top(), 3.0);
    }

    #[test]
    fn fraction_maps_top_and_bottom() {
        let mut view = ViewEstimate::default();
        assert_eq!(view.fraction(100, 20.0), 0.0);
        view.scrolled(40.0, 100);
        assert_eq!(view.fraction(100, 20.0), 0.5);
        view.scrolled(60.0, 100);
        assert_eq!(view.fraction(100, 20.0), 1.0);
        assert_eq!(ViewEstimate::default().fraction(10, 20.0), 0.0);
    }
}
