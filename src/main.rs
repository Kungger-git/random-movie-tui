mod app;
mod cache;
mod config;
mod layout;
mod movie;
mod poster;
mod providers;
mod ui;

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEvent,
        KeyEventKind, KeyModifiers, MouseEventKind,
    },
    execute,
};
use futures::StreamExt;
use image::DynamicImage;
use ratatui::layout::{Rect, Size};
use ratatui_image::{
    Resize,
    picker::{Picker, ProtocolType},
    protocol::Protocol,
};
use tokio::sync::mpsc;

use crate::{
    app::{App, FilterField, StatusKind},
    cache::PosterCache,
    config::{Cli, Config, MAX_COUNT},
    movie::Movie,
    providers::{Batch as MovieBatch, Providers},
};

enum Msg {
    Batch {
        generation: u64,
        result: Result<MovieBatch, String>,
    },
    Poster {
        generation: u64,
        id: String,
        result: Result<DynamicImage, String>,
    },
    Protocol {
        generation: u64,
        id: String,
        size: Size,
        result: Result<Protocol, String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_cli(Cli::parse());
    let mut terminal = ratatui::init();
    let mouse = MouseCapture::enable();
    let result = run(&mut terminal, config).await;
    drop(mouse);
    ratatui::restore();
    result
}

/// Restores normal mouse behavior when the app exits or unwinds.
struct MouseCapture;

impl MouseCapture {
    fn enable() -> Self {
        let _ = execute!(std::io::stdout(), EnableMouseCapture);
        Self
    }
}

impl Drop for MouseCapture {
    fn drop(&mut self) {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
    }
}

async fn run(terminal: &mut ratatui::DefaultTerminal, config: Config) -> Result<()> {
    terminal.draw(ui::draw_boot)?;

    let picker = if config.halfblocks {
        Picker::halfblocks()
    } else {
        match Picker::from_query_stdio() {
            Ok(picker) => picker,
            Err(_) => Picker::halfblocks(),
        }
    };
    let image_backend = protocol_label(picker.protocol_type());

    let providers = Arc::new(Providers::new(config.mode, config.credentials.clone())?);
    let cache = PosterCache::new();
    let (tx, rx) = mpsc::unbounded_channel();

    let mut app = App::new(config.count, config.mode, config.filters.clone());
    let mut runtime = Runtime {
        terminal,
        picker,
        image_backend,
        app: &mut app,
        providers,
        cache,
        tx,
        rx,
        events: EventStream::new(),
        tick: tokio::time::interval(Duration::from_millis(120)),
        startup_warning: config.startup_warning,
    };

    runtime.refresh();
    runtime.run().await
}

struct Runtime<'a> {
    terminal: &'a mut ratatui::DefaultTerminal,
    picker: Picker,
    image_backend: &'static str,
    app: &'a mut App,
    providers: Arc<Providers>,
    cache: PosterCache,
    tx: mpsc::UnboundedSender<Msg>,
    rx: mpsc::UnboundedReceiver<Msg>,
    events: EventStream,
    tick: tokio::time::Interval,
    startup_warning: Option<String>,
}

impl Runtime<'_> {
    async fn run(&mut self) -> Result<()> {
        loop {
            self.prepare()?;
            let app = &mut *self.app;
            let image_backend = self.image_backend;
            self.terminal
                .draw(|frame| ui::draw(frame, app, image_backend))?;

            if self.app.should_quit {
                break;
            }

            tokio::select! {
                event = self.events.next() => {
                    match event {
                        Some(Ok(event)) => self.handle_event(event),
                        Some(Err(error)) => return Err(error.into()),
                        None => break,
                    }
                }
                Some(message) = self.rx.recv() => self.handle_message(message),
                _ = self.tick.tick() => self.app.tick(),
            }
        }
        Ok(())
    }

    fn prepare(&mut self) -> Result<()> {
        let size = self.terminal.size()?;
        let area: Rect = size.into();
        self.app.prepare_layout(area, self.picker.font_size());

        let desired = Size::new(self.app.grid.image_w, self.app.grid.image_h);
        let jobs = self.app.posters_needing_protocol(desired);
        for (id, image, size) in jobs {
            self.spawn_protocol(id, image, size);
        }
        Ok(())
    }

    fn refresh(&mut self) {
        let generation = self.app.begin_refresh();
        let count = self.app.count;
        let filters = self.app.filters.clone();
        let providers = Arc::clone(&self.providers);
        let tx = self.tx.clone();

        tokio::spawn(async move {
            let result = providers
                .fetch(count, &filters)
                .await
                .map_err(|error| error_summary(&error));
            let _ = tx.send(Msg::Batch { generation, result });
        });
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key),
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => self.app.move_selection(0, -1),
                MouseEventKind::ScrollDown => self.app.move_selection(0, 1),
                _ => {}
            },
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.app.should_quit = true;
            return;
        }

        if self.app.filter_editor.is_some() {
            self.handle_filter_editor_key(key);
            return;
        }

        if self.app.help {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => self.app.help = false,
                KeyCode::Char('q') => self.app.should_quit = true,
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.app.should_quit = true,
            KeyCode::Char('?') => self.app.help = true,
            KeyCode::Char('f') | KeyCode::Char('F') => {
                self.app.open_filter_editor(FilterField::Language);
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.app.open_filter_editor(FilterField::From);
            }
            KeyCode::Char(' ') | KeyCode::Char('r') | KeyCode::Char('R') => self.refresh(),
            KeyCode::Enter | KeyCode::Char('o') => self.open_selected(),
            KeyCode::Char(digit @ '1'..='9') => {
                let count = digit as usize - '0' as usize;
                self.change_count(count);
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.change_count(self.app.count.saturating_add(1));
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.change_count(self.app.count.saturating_sub(1));
            }
            KeyCode::Left | KeyCode::Char('h') => self.app.move_selection(-1, 0),
            KeyCode::Right | KeyCode::Char('l') => self.app.move_selection(1, 0),
            KeyCode::Up | KeyCode::Char('k') => self.app.move_selection(0, -1),
            KeyCode::Down | KeyCode::Char('j') => self.app.move_selection(0, 1),
            KeyCode::PageUp => self.app.move_selection_rows(-1),
            KeyCode::PageDown => self.app.move_selection_rows(1),
            KeyCode::Home | KeyCode::Char('g') => self.app.select_first(),
            KeyCode::End | KeyCode::Char('G') => self.app.select_last(),
            _ => {}
        }
    }

    fn handle_filter_editor_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.app.close_filter_editor(),
            KeyCode::Enter => {
                if self.app.apply_filter_editor().is_some() {
                    self.refresh();
                }
            }
            KeyCode::Tab | KeyCode::Down | KeyCode::Char('j') => {
                self.app.filter_editor_focus(true);
            }
            KeyCode::BackTab | KeyCode::Up | KeyCode::Char('k') => {
                self.app.filter_editor_focus(false);
            }
            KeyCode::Left | KeyCode::Char('h') => self.app.filter_editor_change(false),
            KeyCode::Right | KeyCode::Char('l') => self.app.filter_editor_change(true),
            KeyCode::Backspace => self.app.filter_editor_backspace(),
            KeyCode::Char(digit) if digit.is_ascii_digit() => {
                self.app.filter_editor_digit(digit);
            }
            _ => {}
        }
    }

    fn open_selected(&mut self) {
        let Some(movie) = self.app.movies.get(self.app.selection) else {
            return;
        };
        let Some(url) = movie.source_url.clone() else {
            return;
        };
        let title = movie.title.clone();
        match open::that_detached(&url) {
            Ok(()) => self
                .app
                .set_status(StatusKind::Info, format!("Opened {title} in your browser")),
            Err(error) => self.app.set_status(
                StatusKind::Error,
                format!("Could not open browser: {error}"),
            ),
        }
    }

    fn change_count(&mut self, count: usize) {
        let count = count.clamp(1, MAX_COUNT);
        if count != self.app.count {
            self.app.set_count(count);
            self.refresh();
        }
    }

    fn handle_message(&mut self, message: Msg) {
        match message {
            Msg::Batch { generation, result } => match result {
                Ok(batch) => {
                    let warning = batch.warning.clone();
                    if self.app.apply_batch(generation, batch) {
                        self.spawn_poster_downloads();
                        if warning.is_none() {
                            self.show_idle_hint();
                        }
                    }
                }
                Err(message) => {
                    self.app.fail_refresh(generation, message);
                }
            },
            Msg::Poster {
                generation,
                id,
                result,
            } => match result {
                Ok(image) => self.app.set_poster_image(generation, &id, image),
                Err(_) => self.app.set_poster_error(generation, &id),
            },
            Msg::Protocol {
                generation,
                id,
                size,
                result,
            } => self.app.set_protocol(generation, &id, size, result),
        }
    }

    fn show_idle_hint(&mut self) {
        if let Some(warning) = &self.startup_warning {
            self.app.set_status(StatusKind::Warning, warning.clone());
        }
    }

    fn spawn_poster_downloads(&self) {
        for movie in &self.app.movies {
            if movie.poster_url.is_some() {
                self.spawn_poster(movie.clone());
            }
        }
    }

    fn spawn_poster(&self, movie: Movie) {
        let client = self.providers.client().clone();
        let cache = self.cache.clone();
        let tx = self.tx.clone();
        let generation = self.app.generation;

        tokio::spawn(async move {
            let result = poster::load_poster(&client, &cache, &movie)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.send(Msg::Poster {
                generation,
                id: movie.id,
                result,
            });
        });
    }

    fn spawn_protocol(&self, id: String, image: DynamicImage, size: Size) {
        let picker = self.picker.clone();
        let tx = self.tx.clone();
        let generation = self.app.generation;

        tokio::task::spawn_blocking(move || {
            let result = picker
                .new_protocol(image, size, Resize::Fit(None))
                .map_err(|error| error.to_string());
            let _ = tx.send(Msg::Protocol {
                generation,
                id,
                size,
                result,
            });
        });
    }
}

/// Keeps error panels readable: outermost context plus the root cause.
fn error_summary(error: &anyhow::Error) -> String {
    let mut text = error.to_string();
    let root = error.root_cause().to_string();
    if !root.is_empty() && !text.contains(&root) {
        text.push_str(": ");
        text.push_str(&root);
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn protocol_label(protocol: ProtocolType) -> &'static str {
    match protocol {
        ProtocolType::Halfblocks => "half-blocks",
        ProtocolType::Sixel => "sixel",
        ProtocolType::Kitty => "kitty",
        ProtocolType::Iterm2 => "iterm2",
    }
}
