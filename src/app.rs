use std::collections::HashMap;

use image::DynamicImage;
use ratatui::layout::{Margin, Rect, Size};
use ratatui_image::{FontSize, protocol::Protocol};

use crate::{
    config::{Filters, Language, MAX_COUNT, Popularity, SourceChoice, YearRange},
    layout::{self, Grid},
    movie::{Movie, Source},
    providers::Batch,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusKind {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Status {
    pub kind: StatusKind,
    pub text: String,
}

pub struct PosterSlot {
    pub image: Option<DynamicImage>,
    pub protocol: Option<Protocol>,
    pub protocol_size: Option<Size>,
    pub requested_size: Option<Size>,
    pub failed: bool,
}

impl PosterSlot {
    fn loading() -> Self {
        Self {
            image: None,
            protocol: None,
            protocol_size: None,
            requested_size: None,
            failed: false,
        }
    }

    fn unavailable() -> Self {
        Self {
            failed: true,
            ..Self::loading()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterField {
    From,
    To,
    Language,
    Popularity,
}

impl FilterField {
    const ALL: [Self; 4] = [Self::From, Self::To, Self::Language, Self::Popularity];

    fn index(self) -> usize {
        match self {
            Self::From => 0,
            Self::To => 1,
            Self::Language => 2,
            Self::Popularity => 3,
        }
    }

    fn is_year(self) -> bool {
        matches!(self, Self::From | Self::To)
    }
}

#[derive(Clone, Debug)]
pub struct FilterEditor {
    pub from: String,
    pub to: String,
    pub language: Language,
    pub popularity: Popularity,
    pub focus: FilterField,
    fresh: bool,
}

impl FilterEditor {
    fn new(filters: &Filters) -> Self {
        let range = filters.year_range.unwrap_or_else(YearRange::full);
        Self {
            from: range.from.to_string(),
            to: range.to.to_string(),
            language: filters.language,
            popularity: filters.popularity,
            focus: FilterField::From,
            fresh: true,
        }
    }

    pub fn push_digit(&mut self, digit: char) {
        if !digit.is_ascii_digit() || !self.focus.is_year() {
            return;
        }
        if self.fresh {
            self.fresh = false;
            if let Some(field) = self.current_mut() {
                field.clear();
            }
        }
        if let Some(field) = self.current_mut()
            && field.len() < 4
        {
            field.push(digit);
        }
    }

    pub fn backspace(&mut self) {
        if !self.focus.is_year() {
            return;
        }
        self.fresh = false;
        if let Some(field) = self.current_mut() {
            field.pop();
        }
    }

    pub fn focus_next(&mut self) {
        self.focus = FilterField::ALL[(self.focus.index() + 1) % FilterField::ALL.len()];
        self.fresh = true;
    }

    pub fn focus_prev(&mut self) {
        self.focus = FilterField::ALL
            [(self.focus.index() + FilterField::ALL.len() - 1) % FilterField::ALL.len()];
        self.fresh = true;
    }

    /// Cycles language/popularity, or swaps the two year fields.
    pub fn change(&mut self, forward: bool) {
        match self.focus {
            FilterField::From | FilterField::To => {
                self.focus = if self.focus == FilterField::From {
                    FilterField::To
                } else {
                    FilterField::From
                };
                self.fresh = true;
            }
            FilterField::Language => {
                self.language = if forward {
                    self.language.next()
                } else {
                    self.language.prev()
                };
            }
            FilterField::Popularity => {
                self.popularity = if forward {
                    self.popularity.next()
                } else {
                    self.popularity.prev()
                };
            }
        }
    }

    fn current_mut(&mut self) -> Option<&mut String> {
        match self.focus {
            FilterField::From => Some(&mut self.from),
            FilterField::To => Some(&mut self.to),
            FilterField::Language | FilterField::Popularity => None,
        }
    }

    /// Empty fields fall back to the default bounds.
    pub fn parse_years(&self) -> Option<(u16, u16)> {
        let full = YearRange::full();
        let from = if self.from.is_empty() {
            full.from
        } else {
            self.from.parse().ok()?
        };
        let to = if self.to.is_empty() {
            full.to
        } else {
            self.to.parse().ok()?
        };
        Some((from, to))
    }
}

pub struct App {
    pub count: usize,
    pub movies: Vec<Movie>,
    pub posters: HashMap<String, PosterSlot>,
    pub mode: SourceChoice,
    pub active_source: Option<Source>,
    pub filters: Filters,
    pub filter_editor: Option<FilterEditor>,
    pub loading: bool,
    pub status: Option<Status>,
    pub generation: u64,
    pub selection: usize,
    pub scroll: u16,
    pub help: bool,
    pub spinner: u8,
    pub grid: Grid,
    pub should_quit: bool,
}

impl App {
    pub fn new(count: usize, mode: SourceChoice, filters: Filters) -> Self {
        let count = count.clamp(1, MAX_COUNT);
        let font = FontSize::new(10, 20);
        Self {
            count,
            movies: Vec::new(),
            posters: HashMap::new(),
            mode,
            active_source: None,
            filters,
            filter_editor: None,
            loading: false,
            status: None,
            generation: 0,
            selection: 0,
            scroll: 0,
            help: false,
            spinner: 0,
            grid: layout::compute(Rect::new(0, 0, 80, 24), count, font),
            should_quit: false,
        }
    }

    pub fn set_status(&mut self, kind: StatusKind, text: impl Into<String>) {
        self.status = Some(Status {
            kind,
            text: text.into(),
        });
    }

    pub fn begin_refresh(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.loading = true;
        self.movies.clear();
        self.posters.clear();
        self.selection = 0;
        self.scroll = 0;
        self.status = None;
        self.generation
    }

    pub fn apply_batch(&mut self, generation: u64, batch: Batch) -> bool {
        if generation != self.generation {
            return false;
        }
        self.loading = false;
        self.active_source = Some(batch.source);
        self.movies = batch.movies;
        self.posters = self
            .movies
            .iter()
            .map(|movie| {
                let slot = if movie.poster_url.is_some() {
                    PosterSlot::loading()
                } else {
                    PosterSlot::unavailable()
                };
                (movie.id.clone(), slot)
            })
            .collect();
        self.selection = 0;
        self.scroll = 0;
        if let Some(warning) = batch.warning {
            self.set_status(StatusKind::Warning, warning);
        }
        true
    }

    pub fn fail_refresh(&mut self, generation: u64, message: impl Into<String>) -> bool {
        if generation != self.generation {
            return false;
        }
        self.loading = false;
        self.set_status(StatusKind::Error, message);
        true
    }

    pub fn set_poster_image(&mut self, generation: u64, id: &str, image: DynamicImage) {
        if generation != self.generation {
            return;
        }
        if let Some(slot) = self.posters.get_mut(id) {
            slot.image = Some(image);
            slot.protocol = None;
            slot.protocol_size = None;
            slot.requested_size = None;
            slot.failed = false;
        }
    }

    pub fn set_poster_error(&mut self, generation: u64, id: &str) {
        if generation != self.generation {
            return;
        }
        if let Some(slot) = self.posters.get_mut(id) {
            slot.image = None;
            slot.protocol = None;
            slot.protocol_size = None;
            slot.requested_size = None;
            slot.failed = true;
        }
    }

    pub fn set_protocol(
        &mut self,
        generation: u64,
        id: &str,
        size: Size,
        result: Result<Protocol, String>,
    ) {
        if generation != self.generation {
            return;
        }
        if let Some(slot) = self.posters.get_mut(id) {
            if slot.requested_size == Some(size) {
                slot.requested_size = None;
            }
            match result {
                Ok(protocol) => {
                    slot.protocol = Some(protocol);
                    slot.protocol_size = Some(size);
                    slot.failed = false;
                }
                Err(_) => {
                    slot.failed = true;
                    slot.protocol = None;
                    slot.protocol_size = None;
                }
            }
        }
    }

    /// Returns posters that need a graphics protocol for the current layout size.
    pub fn posters_needing_protocol(&mut self, desired: Size) -> Vec<(String, DynamicImage, Size)> {
        if desired.width == 0 || desired.height == 0 {
            return Vec::new();
        }
        let mut jobs = Vec::new();
        for movie in &self.movies {
            let Some(slot) = self.posters.get_mut(&movie.id) else {
                continue;
            };
            let Some(image) = slot.image.as_ref() else {
                continue;
            };
            if slot.protocol_size == Some(desired) || slot.requested_size == Some(desired) {
                continue;
            }
            slot.requested_size = Some(desired);
            jobs.push((movie.id.clone(), image.clone(), desired));
        }
        jobs
    }

    pub fn protocol_for(&self, id: &str, desired: Size) -> Option<&Protocol> {
        let slot = self.posters.get(id)?;
        if slot.failed || slot.protocol_size != Some(desired) {
            return None;
        }
        slot.protocol.as_ref()
    }

    pub fn slot(&self, id: &str) -> Option<&PosterSlot> {
        self.posters.get(id)
    }

    pub fn display_count(&self) -> usize {
        if self.movies.is_empty() {
            self.count
        } else {
            self.movies.len()
        }
    }

    pub fn prepare_layout(&mut self, area: Rect, font: FontSize) {
        let (_, body, _) = layout::screen_areas(area);
        let horizontal = if body.width >= 60 { 2 } else { 1 };
        let body = body.inner(Margin::new(horizontal, 0));
        self.grid = layout::compute(body, self.display_count(), font);
        self.selection = self.selection.min(self.movies.len().saturating_sub(1));
        self.scroll = self.grid.scroll_for_selection(self.selection, self.scroll);
    }

    pub fn move_selection(&mut self, dx: i32, dy: i32) {
        if self.movies.is_empty() {
            return;
        }
        let cols = self.grid.cols.max(1) as i32;
        let mut index = self.selection as i32 + dx + dy * cols;
        index = index.clamp(0, self.movies.len() as i32 - 1);
        self.selection = index as usize;
        self.scroll = self.grid.scroll_for_selection(self.selection, self.scroll);
    }

    pub fn move_selection_rows(&mut self, rows: i32) {
        if self.movies.is_empty() {
            return;
        }
        let visible = (self.grid.area.height / (self.grid.card_h + self.grid.gap_y)).max(1);
        let delta = rows * visible as i32 * self.grid.cols.max(1) as i32;
        let mut index = self.selection as i32 + delta;
        index = index.clamp(0, self.movies.len() as i32 - 1);
        self.selection = index as usize;
        self.scroll = self.grid.scroll_for_selection(self.selection, self.scroll);
    }

    pub fn select_first(&mut self) {
        if !self.movies.is_empty() {
            self.selection = 0;
            self.scroll = self.grid.scroll_for_selection(0, self.scroll);
        }
    }

    pub fn select_last(&mut self) {
        if !self.movies.is_empty() {
            self.selection = self.movies.len() - 1;
            self.scroll = self.grid.scroll_for_selection(self.selection, self.scroll);
        }
    }

    pub fn set_count(&mut self, count: usize) {
        self.count = count.clamp(1, MAX_COUNT);
    }

    pub fn filter_summary(&self) -> String {
        let mut summary = self.filters.summary();
        if self.active_source == Some(Source::Wikipedia)
            && self.filters.wikipedia_ignores_popularity()
        {
            summary.push_str(" (TMDB only)");
        }
        summary
    }

    pub fn open_filter_editor(&mut self, focus: FilterField) {
        if self.filter_editor.is_some() {
            return;
        }
        self.help = false;
        let mut editor = FilterEditor::new(&self.filters);
        editor.focus = focus;
        editor.fresh = true;
        self.filter_editor = Some(editor);
    }

    pub fn close_filter_editor(&mut self) {
        self.filter_editor = None;
    }

    pub fn filter_editor_digit(&mut self, digit: char) {
        if let Some(editor) = &mut self.filter_editor {
            editor.push_digit(digit);
        }
    }

    pub fn filter_editor_backspace(&mut self) {
        if let Some(editor) = &mut self.filter_editor {
            editor.backspace();
        }
    }

    pub fn filter_editor_focus(&mut self, forward: bool) {
        if let Some(editor) = &mut self.filter_editor {
            if forward {
                editor.focus_next();
            } else {
                editor.focus_prev();
            }
        }
    }

    pub fn filter_editor_change(&mut self, forward: bool) {
        if let Some(editor) = &mut self.filter_editor {
            editor.change(forward);
        }
    }

    /// Applies the editor's draft and closes it. Returns the new filters.
    pub fn apply_filter_editor(&mut self) -> Option<Filters> {
        let (from, to) = self.filter_editor.as_ref()?.parse_years()?;
        let (range, warning) = YearRange::normalize(from, to);
        let editor = self.filter_editor.take()?;
        let filters = Filters {
            year_range: Some(range),
            language: editor.language,
            popularity: editor.popularity,
        };
        self.filters = filters.clone();
        if let Some(warning) = warning {
            self.set_status(StatusKind::Warning, warning);
        }
        Some(filters)
    }

    pub fn mode_label(&self) -> &'static str {
        match self.mode {
            SourceChoice::Auto => "auto",
            SourceChoice::Tmdb => "TMDB",
            SourceChoice::Wikipedia => "Wikipedia",
        }
    }

    pub fn tick(&mut self) {
        self.spinner = self.spinner.wrapping_add(1);
    }

    pub fn posters_settled(&self) -> usize {
        self.posters
            .values()
            .filter(|slot| slot.failed || slot.protocol_size.is_some())
            .count()
    }

    pub fn posters_pending(&self) -> bool {
        self.posters.len() > self.posters_settled()
    }

    pub fn poster_summary(&self) -> String {
        if self.movies.is_empty() {
            return String::new();
        }
        let ready = self
            .posters
            .values()
            .filter(|slot| slot.protocol_size.is_some())
            .count();
        if ready == self.movies.len() {
            String::new()
        } else {
            format!("loading posters {ready}/{}", self.movies.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_editor_types_over_the_prefilled_value() {
        let mut app = App::new(1, SourceChoice::Auto, Filters::default());
        app.open_filter_editor(FilterField::From);
        for digit in ['1', '9', '9', '0'] {
            app.filter_editor_digit(digit);
        }
        let editor = app.filter_editor.as_ref().unwrap();
        assert_eq!(editor.from, "1990");
        assert_eq!(editor.to, YearRange::full().to.to_string());
    }

    #[test]
    fn filter_editor_switches_between_year_fields() {
        let mut app = App::new(1, SourceChoice::Auto, Filters::default());
        app.open_filter_editor(FilterField::From);
        app.filter_editor_change(true);
        app.filter_editor_digit('9');
        app.filter_editor_digit('9');
        let editor = app.filter_editor.as_ref().unwrap();
        assert_eq!(editor.to, "99");
        assert_eq!(editor.focus, FilterField::To);
    }

    #[test]
    fn filter_editor_applies_and_normalizes() {
        let mut app = App::new(1, SourceChoice::Auto, Filters::default());
        app.open_filter_editor(FilterField::From);
        for digit in ['2', '0', '0', '0'] {
            app.filter_editor_digit(digit);
        }
        app.filter_editor_change(true);
        for digit in ['1', '9', '9', '0'] {
            app.filter_editor_digit(digit);
        }
        let filters = app.apply_filter_editor().unwrap();
        assert_eq!(
            filters.year_range,
            Some(YearRange {
                from: 1990,
                to: 2000
            })
        );
        assert_eq!(app.filters, filters);
        assert!(app.filter_editor.is_none());
        assert!(app.status.is_some());
    }

    #[test]
    fn filter_editor_backspace_edits_the_field() {
        let mut app = App::new(1, SourceChoice::Auto, Filters::default());
        app.open_filter_editor(FilterField::From);
        app.filter_editor_digit('1');
        app.filter_editor_digit('9');
        app.filter_editor_backspace();
        assert_eq!(app.filter_editor.as_ref().unwrap().from, "1");
    }

    #[test]
    fn empty_fields_fall_back_to_default_bounds() {
        let mut app = App::new(1, SourceChoice::Auto, Filters::default());
        app.open_filter_editor(FilterField::From);
        for _ in 0..4 {
            app.filter_editor_backspace();
        }
        app.filter_editor_change(true);
        for _ in 0..4 {
            app.filter_editor_backspace();
        }
        let filters = app.apply_filter_editor().unwrap();
        assert_eq!(filters.year_range, Some(YearRange::full()));
    }

    #[test]
    fn filter_editor_cycles_language_and_popularity() {
        let mut app = App::new(1, SourceChoice::Auto, Filters::default());
        app.open_filter_editor(FilterField::Language);
        app.filter_editor_digit('9');
        assert_eq!(app.filter_editor.as_ref().unwrap().from, "1920");

        app.filter_editor_change(true);
        assert_eq!(
            app.filter_editor.as_ref().unwrap().language,
            Language::English
        );
        app.filter_editor_focus(true);
        assert_eq!(
            app.filter_editor.as_ref().unwrap().focus,
            FilterField::Popularity
        );
        app.filter_editor_change(false);
        assert_eq!(
            app.filter_editor.as_ref().unwrap().popularity,
            Popularity::HiddenGems
        );

        let filters = app.apply_filter_editor().unwrap();
        assert_eq!(filters.language, Language::English);
        assert_eq!(filters.popularity, Popularity::HiddenGems);
        assert!(app.filter_summary().contains("English"));
        assert!(app.filter_summary().contains("Hidden gems"));
    }

    #[test]
    fn closing_the_filter_editor_discards_changes() {
        let mut app = App::new(1, SourceChoice::Auto, Filters::default());
        app.open_filter_editor(FilterField::Language);
        app.filter_editor_change(true);
        app.close_filter_editor();
        assert!(app.filter_editor.is_none());
        assert_eq!(app.filters, Filters::default());
    }

    #[test]
    fn marks_popularity_as_tmdb_only_on_wikipedia() {
        let filters = Filters {
            popularity: Popularity::Popular,
            ..Filters::default()
        };
        let mut app = App::new(1, SourceChoice::Wikipedia, filters);
        app.active_source = Some(Source::Wikipedia);
        assert_eq!(app.filter_summary(), "Popular (TMDB only)");

        app.active_source = Some(Source::Tmdb);
        assert_eq!(app.filter_summary(), "Popular");
    }
}
