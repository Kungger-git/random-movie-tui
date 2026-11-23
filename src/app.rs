use std::collections::HashMap;

use image::DynamicImage;
use ratatui::layout::{Margin, Rect, Size};
use ratatui_image::{FontSize, protocol::Protocol};

use crate::{
    config::{MAX_COUNT, SourceChoice},
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

pub struct App {
    pub count: usize,
    pub movies: Vec<Movie>,
    pub posters: HashMap<String, PosterSlot>,
    pub mode: SourceChoice,
    pub active_source: Option<Source>,
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
    pub fn new(count: usize, mode: SourceChoice) -> Self {
        let count = count.clamp(1, MAX_COUNT);
        let font = FontSize::new(10, 20);
        Self {
            count,
            movies: Vec::new(),
            posters: HashMap::new(),
            mode,
            active_source: None,
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
