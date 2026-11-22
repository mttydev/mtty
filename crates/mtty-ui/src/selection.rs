//! Grid selection helpers (pure geometry over the scrollback).

use mtty_core::ATerm;

/// A selection in scrollback-absolute `(line, column)` coordinates.
///
/// Rows are counted from the oldest buffer line, not from the viewport: a
/// selection made across pages must keep covering the same text when the
/// terminal scrolls. Convert to viewport rows only when painting.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub start: (usize, u16),
    pub end: (usize, u16),
}

impl Selection {
    pub fn cell(c: (usize, u16)) -> Self {
        Self { start: c, end: c }
    }

    pub fn ordered(&self) -> (usize, u16, usize, u16) {
        if self.end < self.start {
            (self.end.0, self.end.1, self.start.0, self.start.1)
        } else {
            (self.start.0, self.start.1, self.end.0, self.end.1)
        }
    }

    /// All visible cells covered, for highlighting.
    ///
    /// `history` is the number of scrollback lines and `offset` is the current
    /// display offset. A selection may start above or end below the viewport;
    /// the visible top/bottom rows then take the full row width instead of the
    /// anchor's column.
    pub fn visible_cells(
        &self,
        cols: u16,
        rows: u16,
        history: usize,
        offset: usize,
    ) -> Vec<(u16, u16)> {
        let (r1, c1, r2, c2) = self.ordered();
        let first = r1 as isize - history as isize + offset as isize;
        let last = r2 as isize - history as isize + offset as isize;
        let top = first.max(0);
        let bottom = last.min(rows as isize - 1);
        if bottom < top || cols == 0 {
            return Vec::new();
        }
        let mut cells = Vec::new();
        for row in top..=bottom {
            let start = if row == first { c1 } else { 0 };
            let end = if row == last {
                c2.min(cols - 1)
            } else {
                cols - 1
            };
            for col in start.min(end)..=end {
                cells.push((row as u16, col));
            }
        }
        cells
    }
}

/// Select the word (alphanumeric plus `_-./~`) around a cell.
pub fn word_selection(screen: &ATerm, row: u16, col: u16, cols: u16) -> Selection {
    let is_word = |c: u16| -> bool {
        screen
            .cell(row, c)
            .map(|cell| cell.wide_spacer || cell.ch.is_alphanumeric() || "_-./~".contains(cell.ch))
            .unwrap_or(false)
    };
    let line = (row as isize + screen.history_size() as isize - screen.scroll_offset() as isize)
        .max(0) as usize;
    if !is_word(col) {
        return Selection::cell((line, col));
    }
    let mut l = col;
    while l > 0 && is_word(l - 1) {
        l -= 1;
    }
    let mut r = col;
    while r + 1 < cols && is_word(r + 1) {
        r += 1;
    }
    Selection {
        start: (line, l),
        end: (line, r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_click_path_including_wide_character_spacer() {
        let mut screen = ATerm::new(30, 3, 100);
        screen.process("目录/file.txt suffix".as_bytes());
        for col in [0, 1, 2, 3, 8] {
            let s = word_selection(&screen, 0, col, 30);
            let (r1, c1, r2, c2) = s.ordered();
            assert_eq!(screen.contents_between_abs(r1, c1, r2, c2), "目录/file.txt");
        }
    }

    #[test]
    fn backwards_drag_and_highlight_cover_the_same_text() {
        let selection = Selection {
            start: (1, 2),
            end: (0, 3),
        };
        assert_eq!(
            selection.visible_cells(5, 2, 0, 0),
            vec![(0, 3), (0, 4), (1, 0), (1, 1), (1, 2)]
        );
    }

    #[test]
    fn scrolling_moves_the_highlight_with_the_text() {
        let selection = Selection {
            start: (10, 2),
            end: (12, 3),
        };
        assert_eq!(
            selection.visible_cells(5, 10, 5, 0),
            vec![
                (5, 2),
                (5, 3),
                (5, 4),
                (6, 0),
                (6, 1),
                (6, 2),
                (6, 3),
                (6, 4),
                (7, 0),
                (7, 1),
                (7, 2),
                (7, 3)
            ]
        );
        // Two more lines of scrollback put the same text two rows lower.
        assert_eq!(
            selection.visible_cells(5, 10, 5, 2),
            vec![
                (7, 2),
                (7, 3),
                (7, 4),
                (8, 0),
                (8, 1),
                (8, 2),
                (8, 3),
                (8, 4),
                (9, 0),
                (9, 1),
                (9, 2),
                (9, 3)
            ]
        );
    }

    #[test]
    fn a_selection_partly_above_the_viewport_paints_full_rows() {
        let selection = Selection {
            start: (2, 2),
            end: (4, 3),
        };
        // History 5, offset 2: the first selected line is above the viewport,
        // so the visible top row is a middle row and takes the full width.
        assert_eq!(
            selection.visible_cells(5, 4, 5, 2),
            vec![
                (0, 0),
                (0, 1),
                (0, 2),
                (0, 3),
                (0, 4),
                (1, 0),
                (1, 1),
                (1, 2),
                (1, 3)
            ]
        );
    }
}
