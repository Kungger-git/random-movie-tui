use ratatui::layout::{Constraint, Layout, Rect};
use ratatui_image::FontSize;

/// Rows reserved under each poster for the title and metadata.
pub const TEXT_ROWS: u16 = 2;

const MIN_CARD_WIDTH: u16 = 10;
const MIN_IMAGE_WIDTH: u16 = 9;
const MIN_IMAGE_HEIGHT: u16 = 7;
const MAX_CARD_WIDTH_SINGLE: u16 = 56;
const MAX_CARD_WIDTH_GRID: u16 = 36;
const MAX_COLUMNS: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub area: Rect,
    pub count: usize,
    pub cols: usize,
    pub rows: usize,
    /// Total card width, including the poster border.
    pub card_w: u16,
    /// Poster box height, including the border.
    pub poster_h: u16,
    /// Poster image size in terminal cells (inside the border).
    pub image_w: u16,
    pub image_h: u16,
    /// Total card height: poster + title + metadata.
    pub card_h: u16,
    pub gap_x: u16,
    pub gap_y: u16,
    /// Full height of all rows, including gaps.
    pub total_h: u16,
    /// Vertical centering offset when everything fits.
    pub top: u16,
}

impl Grid {
    pub fn card_rect(&self, index: usize, scroll: u16) -> Rect {
        if index >= self.count || self.cols == 0 {
            return Rect::new(self.area.x, self.area.y, 0, 0);
        }
        let row = index / self.cols;
        let col = index % self.cols;
        let items_in_row = (self.count - row * self.cols).min(self.cols);
        let row_w = row_items_width(items_in_row, self.card_w, self.gap_x);
        let x = self.area.x
            + self.area.width.saturating_sub(row_w) / 2
            + col as u16 * (self.card_w + self.gap_x);
        let y = self
            .area
            .y
            .saturating_add(self.top)
            .saturating_add(row as u16 * (self.card_h + self.gap_y))
            .saturating_sub(scroll);
        Rect::new(x, y, self.card_w, self.card_h)
    }

    pub fn max_scroll(&self) -> u16 {
        self.total_h.saturating_sub(self.area.height)
    }

    pub fn scroll_for_selection(&self, selection: usize, scroll: u16) -> u16 {
        if self.cols == 0 || self.count == 0 {
            return 0;
        }
        let row = (selection.min(self.count - 1) / self.cols) as u16;
        let row_top = self.top + row * (self.card_h + self.gap_y);
        let row_bottom = row_top + self.card_h;
        let view_h = self.area.height;
        let mut next = scroll.min(self.max_scroll());
        if row_top < next {
            next = row_top;
        } else if row_bottom > next + view_h {
            next = row_bottom.saturating_sub(view_h);
        }
        next.min(self.max_scroll())
    }
}

fn row_items_width(items: usize, card_w: u16, gap_x: u16) -> u16 {
    if items == 0 {
        return 0;
    }
    items as u16 * card_w + (items.saturating_sub(1)) as u16 * gap_x
}

/// Splits the terminal into header, content, and footer regions.
pub fn screen_areas(area: Rect) -> (Rect, Rect, Rect) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .areas(area);
    (header, body, footer)
}

pub fn compute(area: Rect, count: usize, font: FontSize) -> Grid {
    let count = count.max(1);
    let gap_x: u16 = if area.width >= 60 { 2 } else { 1 };
    let gap_y = 1;
    let max_card_w = if count == 1 {
        MAX_CARD_WIDTH_SINGLE
    } else {
        MAX_CARD_WIDTH_GRID
    };

    let mut best: Option<(u64, Grid)> = None;
    let max_cols = count.clamp(1, MAX_COLUMNS);

    for cols in 1..=max_cols {
        let needed_gaps = gap_x.saturating_mul(cols.saturating_sub(1) as u16);
        let available = area.width.saturating_sub(needed_gaps);
        if available < MIN_CARD_WIDTH {
            continue;
        }
        let column_w = (available / cols as u16).min(max_card_w);
        if column_w < MIN_CARD_WIDTH {
            continue;
        }

        let rows = count.div_ceil(cols);
        let full_rows = rows.saturating_sub(1) as u16;
        let usable_h = area.height.saturating_sub(full_rows.saturating_mul(gap_y));
        let max_card_h = (usable_h / rows as u16).max(MIN_IMAGE_HEIGHT + 2 + TEXT_ROWS);
        let max_image_h = max_card_h.saturating_sub(2 + TEXT_ROWS);
        let max_image_w = column_w.saturating_sub(2);
        if max_image_w == 0 {
            continue;
        }

        let (image_w, image_h) = fit_poster(max_image_w, max_image_h, font);
        let card_w = image_w.saturating_add(2);
        let poster_h = image_h.saturating_add(2);
        let card_h = poster_h.saturating_add(TEXT_ROWS);

        let total_h = rows as u16 * card_h + full_rows.saturating_mul(gap_y);
        let top = if total_h <= area.height {
            (area.height - total_h) / 2
        } else {
            0
        };

        let image_area = u64::from(image_w) * u64::from(image_h);
        // Fit everything on screen first, then prefer bigger posters, then
        // avoid a half-empty last row, and finally prefer wider grids.
        let fits = total_h <= area.height;
        let empty_cells = (cols - count % cols) % cols;
        let score = u64::from(fits) * 1_000_000_000
            + image_area * 10_000
            + (MAX_COLUMNS - empty_cells) as u64 * 100
            + cols as u64;

        let grid = Grid {
            area,
            count,
            cols,
            rows,
            card_w,
            poster_h,
            image_w,
            image_h,
            card_h,
            gap_x,
            gap_y,
            total_h,
            top,
        };

        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score)
        {
            best = Some((score, grid));
        }
    }

    best.map(|(_, grid)| grid).unwrap_or_else(|| {
        // Degenerate terminal: still return something callable.
        let card_w = area.width.max(1);
        let image_w = card_w.saturating_sub(2).max(1);
        let image_h = (area.height.saturating_sub(TEXT_ROWS + 2)).max(1);
        Grid {
            area,
            count,
            cols: 1,
            rows: count,
            card_w,
            poster_h: image_h + 2,
            image_w,
            image_h,
            card_h: image_h + 2 + TEXT_ROWS,
            gap_x,
            gap_y,
            total_h: count as u16 * (image_h + 2 + TEXT_ROWS),
            top: 0,
        }
    })
}

/// Fits a 2:3 poster inside the available cell box.
///
/// Returns the image size in cells, excluding the one-cell border.
fn fit_poster(max_image_w: u16, max_image_h: u16, font: FontSize) -> (u16, u16) {
    let font_w = u32::from(font.width.max(1));
    let font_h = u32::from(font.height.max(1));
    let aspect = 3.0 / 2.0;

    let height_for_width = |width: u16| {
        ((u32::from(width) as f32 * font_w as f32 * aspect / font_h as f32).round() as u16).max(1)
    };
    let width_for_height = |height: u16| {
        ((u32::from(height) as f32 * font_h as f32 / (font_w as f32 * aspect)).round() as u16)
            .max(1)
    };

    let mut w = max_image_w.max(1);
    let mut h = height_for_width(w);
    if h > max_image_h {
        h = max_image_h.max(1);
        w = width_for_height(h);
    }

    if h < MIN_IMAGE_HEIGHT {
        h = MIN_IMAGE_HEIGHT;
        w = width_for_height(h);
    }
    if w < MIN_IMAGE_WIDTH && max_image_w >= MIN_IMAGE_WIDTH {
        w = MIN_IMAGE_WIDTH;
        h = height_for_width(w);
    }

    (w.min(max_image_w.max(1)), h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT: FontSize = FontSize {
        width: 10,
        height: 20,
    };

    fn rect(width: u16, height: u16) -> Rect {
        Rect::new(0, 0, width, height)
    }

    #[test]
    fn single_movie_is_centered_and_inside() {
        let grid = compute(rect(120, 40), 1, FONT);
        assert_eq!(grid.cols, 1);
        assert_eq!(grid.rows, 1);
        let card = grid.card_rect(0, 0);
        assert!(card.right() <= 120);
        assert!(card.bottom() <= 40);
        assert!(card.height >= 10);
        assert!(grid.top + grid.total_h <= 40);
    }

    #[test]
    fn poster_keeps_roughly_two_by_three_cells() {
        let grid = compute(rect(120, 40), 1, FONT);
        let pixel_w = f32::from(grid.image_w) * 10.0;
        let pixel_h = f32::from(grid.image_h) * 20.0;
        let ratio = pixel_h / pixel_w;
        assert!((ratio - 1.5).abs() < 0.15, "ratio was {ratio}");
    }

    #[test]
    fn grid_cards_stay_within_width() {
        for count in [2, 3, 5, 9, 20] {
            for (w, h) in [(80, 24), (120, 40), (200, 50)] {
                let grid = compute(rect(w, h), count, FONT);
                assert!(grid.cols >= 1);
                for index in 0..count {
                    let card = grid.card_rect(index, 0);
                    assert!(card.right() <= w, "count {count} size {w}x{h}");
                }
            }
        }
    }

    #[test]
    fn multiple_movies_do_not_use_full_width_cards() {
        let grid = compute(rect(200, 50), 2, FONT);
        assert_eq!(grid.card_w, 36);
        assert_eq!(grid.cols, 2);
    }

    #[test]
    fn six_movies_form_a_balanced_two_row_grid() {
        let grid = compute(rect(120, 40), 6, FONT);
        assert_eq!(grid.cols, 3);
        assert_eq!(grid.rows, 2);
        assert!(grid.image_w >= 15);
        assert!(grid.image_h >= 12);
        assert!(grid.total_h <= 40);
    }

    #[test]
    fn selection_scrolls_into_view() {
        let grid = compute(rect(80, 20), 20, FONT);
        assert!(grid.max_scroll() > 0);
        let scroll = grid.scroll_for_selection(19, 0);
        assert!(scroll > 0);
        assert!(scroll <= grid.max_scroll());
        let card = grid.card_rect(19, scroll);
        assert!(card.bottom() <= 20);
    }

    #[test]
    fn tiny_area_does_not_panic() {
        let grid = compute(rect(5, 3), 3, FONT);
        assert_eq!(grid.cols, 1);
        let _ = grid.card_rect(0, 0);
    }
}
