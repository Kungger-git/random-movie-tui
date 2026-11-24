use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};
use ratatui_image::Image;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{App, FilterField, StatusKind},
    config, layout,
    movie::{Movie, Source},
};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const TMDB_BLUE: Color = Color::Rgb(1, 180, 228);

pub fn draw(frame: &mut Frame, app: &mut App, image_backend: &str) {
    let area = frame.area();
    if area.width < 46 || area.height < 16 {
        draw_too_small(frame, area);
        return;
    }

    let (header, body, footer) = layout::screen_areas(area);
    draw_header(frame, app, header);
    draw_body(frame, app, body);
    draw_footer(frame, app, footer, image_backend);

    if app.help {
        draw_help(frame, app);
    }
    if app.filter_editor.is_some() {
        draw_filter_editor(frame, app);
    }
}

pub fn draw_boot(frame: &mut Frame) {
    let area = frame.area();
    let text = Text::from(vec![
        Line::from("Random Movies"),
        Line::from(""),
        Line::from("Preparing the projector…"),
        Line::from(""),
        Line::from(Span::styled(
            "Checking terminal graphics support",
            Style::new().fg(DIM),
        )),
    ]);
    let paragraph = Paragraph::new(text).alignment(Alignment::Center).block(
        Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(DIM)),
    );
    let height = 7.min(area.height);
    let width = 44.min(area.width);
    let rect = centered_rect(area, width, height);
    frame.render_widget(paragraph, rect);
}

fn draw_too_small(frame: &mut Frame, area: Rect) {
    let paragraph = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "Terminal too small",
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Resize to at least 46 × 16."),
    ]))
    .alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(DIM))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Random Movies",
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]));

    let inner = block.inner(area).inner(Margin::new(1, 0));
    frame.render_widget(block, area);

    let [left, right] =
        Layout::horizontal([Constraint::Min(12), Constraint::Length(46)]).areas(inner);

    let source = app.active_source;
    let mut left_spans = Vec::new();
    if let Some(source) = source {
        left_spans.push(Span::styled(
            format!(" {} ", source.label()),
            Style::new()
                .bg(source_color(source))
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ));
        left_spans.push(Span::raw("  "));
    }
    left_spans.push(Span::styled(
        format!(
            "{} {}",
            app.count,
            if app.count == 1 { "movie" } else { "movies" }
        ),
        Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
    ));
    let filters = app.filter_summary();
    if !filters.is_empty() {
        left_spans.push(Span::styled(format!(" · {filters}"), Style::new().fg(DIM)));
    }
    if !app.movies.is_empty() && app.movies.len() != app.count {
        left_spans.push(Span::styled(
            format!(" ({} found)", app.movies.len()),
            Style::new().fg(DIM),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(left_spans)), left);

    let mut right_spans = Vec::new();
    let spinner = SPINNER[app.spinner as usize % SPINNER.len()];
    if app.loading {
        right_spans.push(Span::styled(spinner, Style::new().fg(ACCENT)));
        right_spans.push(Span::raw(" "));
        let text = if app.movies.is_empty() {
            "finding movies…"
        } else {
            "refreshing…"
        };
        right_spans.push(Span::styled(text, Style::new().fg(Color::Gray)));
    } else if app.posters_pending() {
        right_spans.push(Span::styled(spinner, Style::new().fg(ACCENT)));
        right_spans.push(Span::raw(" "));
        right_spans.push(Span::styled(
            app.poster_summary(),
            Style::new().fg(Color::Gray),
        ));
    } else if !app.movies.is_empty() {
        if app.movies.len() > 1 {
            right_spans.push(Span::styled(
                format!("{} / {}  ", app.selection + 1, app.movies.len()),
                Style::new().fg(Color::Gray),
            ));
        }
        right_spans.push(Span::styled("space for another", Style::new().fg(DIM)));
    } else {
        right_spans.push(Span::styled("space to start", Style::new().fg(DIM)));
    }
    frame.render_widget(
        Paragraph::new(Line::from(right_spans)).alignment(Alignment::Right),
        right,
    );
}

fn draw_body(frame: &mut Frame, app: &mut App, body: Rect) {
    if app.movies.is_empty() {
        if app.loading {
            draw_skeletons(frame, app);
        } else if let Some(status) = &app.status {
            if status.kind == StatusKind::Error {
                draw_error_panel(frame, app, body, &status.text);
            } else {
                draw_empty(frame, app, body, &status.text);
            }
        } else {
            draw_empty(frame, app, body, "No movies yet");
        }
        return;
    }

    if app.movies.len() == 1 {
        draw_single(frame, app, body);
        return;
    }

    for index in 0..app.movies.len() {
        draw_card(frame, app, index);
    }
}

fn draw_skeletons(frame: &mut Frame, app: &App) {
    for index in 0..app.count {
        let card = app.grid.card_rect(index, 0);
        if card.width == 0 || card.height == 0 || !fully_inside(card, app.grid.area) {
            continue;
        }
        let poster = Rect::new(card.x, card.y, card.width, app.grid.poster_h);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::Rgb(55, 58, 64)));
        let inner = block.inner(poster);
        frame.render_widget(block, poster);
        frame.render_widget(
            Paragraph::new(Span::styled("…", Style::new().fg(DIM))).alignment(Alignment::Center),
            inner,
        );

        let title_rect = Rect::new(card.x, card.y + app.grid.poster_h, card.width, 1);
        frame.render_widget(
            Paragraph::new(Span::styled(
                truncate("finding a movie", card.width),
                Style::new().fg(Color::Rgb(70, 73, 80)),
            ))
            .alignment(Alignment::Center),
            title_rect,
        );
    }
}

fn draw_empty(frame: &mut Frame, app: &App, body: Rect, message: &str) {
    let rect = centered_rect(body, body.width.min(64), 7);
    let paragraph = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            truncate(message, rect.width.saturating_sub(4)),
            Style::new().fg(Color::Yellow),
        )),
        Line::from(""),
        Line::from(Span::styled(
            match app.filter_summary() {
                summary if summary.is_empty() => {
                    format!("Press space to pick {} movie(s)", app.count)
                }
                summary => format!("Press space to pick {} movie(s) from {summary}", app.count),
            },
            Style::new().fg(DIM),
        )),
    ]))
    .alignment(Alignment::Center);
    frame.render_widget(paragraph, rect);
}

fn draw_error_panel(frame: &mut Frame, app: &App, body: Rect, message: &str) {
    let width = body.width.min(76);
    let rect = centered_rect(body, width, 11);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Red))
        .title(" Could not load movies ");
    let paragraph = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            message.to_owned(),
            Style::new().fg(Color::Rgb(255, 170, 170)),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("Press space or r to try again ({})", app.mode_label()),
            Style::new().fg(DIM),
        )),
    ]))
    .alignment(Alignment::Center)
    .wrap(Wrap { trim: true })
    .block(block);
    frame.render_widget(paragraph, rect);
}

fn draw_card(frame: &mut Frame, app: &App, index: usize) {
    let Some(movie) = app.movies.get(index) else {
        return;
    };
    let card = app.grid.card_rect(index, app.scroll);
    if card.width < 4 || card.height < 4 || !fully_inside(card, app.grid.area) {
        return;
    }
    draw_poster_card(
        frame,
        app,
        movie,
        card,
        index == app.selection,
        layout::TEXT_ROWS,
    );
}

struct SingleLayout {
    card: Rect,
    details: Option<Rect>,
    text_rows: u16,
}

fn draw_single(frame: &mut Frame, app: &App, body: Rect) {
    let Some(movie) = app.movies.first() else {
        return;
    };
    let single = single_layout(app, movie, body);
    draw_poster_card(frame, app, movie, single.card, true, single.text_rows);
    if let Some(rect) = single.details {
        draw_single_details(frame, movie, rect);
    }
}

fn single_layout(app: &App, movie: &Movie, body: Rect) -> SingleLayout {
    let desired = ratatui::layout::Size::new(app.grid.image_w, app.grid.image_h);
    let image = app
        .protocol_for(&movie.id, desired)
        .map(|protocol| protocol.size())
        .unwrap_or(desired);

    let card_w = image.width.saturating_add(2).min(body.width.max(1));
    let poster_h = image.height.saturating_add(2);

    let gap = 4;
    let max_details = body.width.saturating_sub(card_w.saturating_add(gap));
    // Keep the panel from dwarfing a small poster on short terminals.
    let details_w = max_details.min(48).min(card_w.saturating_mul(2).max(30));
    let details = (details_w >= 30 && body.height >= 8).then_some(details_w);
    let text_rows = if details.is_some() {
        0
    } else {
        layout::TEXT_ROWS
    };
    let card_h = poster_h.saturating_add(text_rows).min(body.height.max(1));

    let total_w = card_w + details.map_or(0, |width| gap + width);
    let x = body.x + body.width.saturating_sub(total_w) / 2;
    let y = body.y + body.height.saturating_sub(card_h) / 2;
    let card = Rect::new(x, y, card_w, card_h);
    let details = details.map(|width| Rect::new(x + card_w + gap, y, width, card_h));

    SingleLayout {
        card,
        details,
        text_rows,
    }
}

fn draw_poster_card(
    frame: &mut Frame,
    app: &App,
    movie: &Movie,
    card: Rect,
    selected: bool,
    text_rows: u16,
) {
    if card.width < 4 || card.height < 4 {
        return;
    }
    let poster_h = card.height.saturating_sub(text_rows);
    let poster = Rect::new(card.x, card.y, card.width, poster_h);
    let border_color = if selected {
        ACCENT
    } else {
        Color::Rgb(55, 58, 64)
    };
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(border_color));
    let inner = block.inner(poster);
    frame.render_widget(block, poster);

    if inner.width > 0 && inner.height > 0 {
        let desired = ratatui::layout::Size::new(app.grid.image_w, app.grid.image_h);
        if let Some(protocol) = app.protocol_for(&movie.id, desired) {
            // `Resize::Fit` may leave margins when a poster is not exactly
            // 2:3, so center the encoded image instead of pinning it top-left.
            let actual = protocol.size();
            let image_rect = centered_rect(
                inner,
                actual.width.min(inner.width),
                actual.height.min(inner.height),
            );
            frame.render_widget(Image::new(protocol), image_rect);
        } else {
            let (label, color) = match app.slot(&movie.id) {
                Some(slot) if slot.failed => ("no poster", DIM),
                Some(slot) if slot.image.is_some() => ("preparing…", DIM),
                _ => ("loading…", Color::Rgb(90, 94, 102)),
            };
            frame.render_widget(
                Paragraph::new(Span::styled(label, Style::new().fg(color)))
                    .alignment(Alignment::Center),
                inner,
            );
        }
    }

    if text_rows == 0 {
        return;
    }

    let title_rect = Rect::new(card.x, card.y + poster_h, card.width, 1);
    let title_style = if selected {
        Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::Gray)
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            truncate(&movie.title, card.width),
            title_style,
        ))
        .alignment(Alignment::Center),
        title_rect,
    );

    if text_rows > 1 {
        let meta = movie.meta_line(if card.width >= 26 { 2 } else { 1 });
        if !meta.is_empty() {
            let meta_rect = Rect::new(card.x, card.y + poster_h + 1, card.width, 1);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    truncate(&meta, card.width),
                    Style::new().fg(if selected { Color::Gray } else { DIM }),
                ))
                .alignment(Alignment::Center),
                meta_rect,
            );
        }
    }
}

fn draw_single_details(frame: &mut Frame, movie: &Movie, rect: Rect) {
    if rect.width < 8 || rect.height < 3 {
        return;
    }

    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Rgb(55, 58, 64)));
    let inner = block.inner(rect).inner(Margin::new(2, 1));
    frame.render_widget(block, rect);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut lines = vec![
        Line::from(Span::styled(
            movie.title.clone(),
            Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    let mut meta = Vec::new();
    if let Some(year) = movie.year {
        meta.push(Span::styled(
            year.to_string(),
            Style::new().fg(Color::Gray).add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(rating) = movie.rating_text() {
        if !meta.is_empty() {
            meta.push(Span::styled("  ·  ", Style::new().fg(DIM)));
        }
        meta.push(Span::styled(
            format!("★ {rating}"),
            Style::new().fg(Color::Yellow),
        ));
    }
    if !meta.is_empty() {
        lines.push(Line::from(meta));
    }
    if !movie.genres.is_empty() {
        lines.push(Line::from(Span::styled(
            movie.genres.join(" · "),
            Style::new().fg(ACCENT),
        )));
    }
    lines.push(Line::from(""));
    if let Some(overview) = &movie.overview {
        lines.push(Line::from(overview.clone()));
    } else {
        lines.push(Line::from(Span::styled(
            "No overview available.",
            Style::new().fg(DIM),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("{}  ·  space for another", movie.source.label()),
        Style::new().fg(DIM),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Left),
        inner,
    );
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect, image_backend: &str) {
    let inner = area.inner(Margin::new(2, 0));
    let [hints, status] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);

    let mut spans = Vec::new();
    let compact = inner.width < 96;
    spans.extend(key_hint("space", "another"));
    if !compact {
        spans.push(Span::styled("   ", Style::new()));
        spans.extend(key_hint("1-9", "movies"));
        spans.push(Span::styled("   ", Style::new()));
        spans.extend(key_hint("+/-", "count"));
        spans.push(Span::styled("   ", Style::new()));
    } else {
        spans.push(Span::styled("  ", Style::new()));
    }
    spans.extend(key_hint("f", "filters"));
    spans.push(Span::styled("  ", Style::new()));
    spans.extend(key_hint("o", "open"));
    spans.push(Span::styled("  ", Style::new()));
    spans.extend(key_hint("?", "help"));
    spans.push(Span::styled("  ", Style::new()));
    spans.extend(key_hint("q", "quit"));
    frame.render_widget(Paragraph::new(Line::from(spans)), hints);

    let [left, right] =
        Layout::horizontal([Constraint::Min(10), Constraint::Length(30)]).areas(status);

    let status_line = match &app.status {
        Some(status) => Line::from(Span::styled(
            status.text.clone(),
            Style::new().fg(status_color(status.kind)),
        )),
        None => Line::from(Span::styled(
            attribution(app.active_source),
            Style::new().fg(Color::Rgb(90, 94, 102)),
        )),
    };
    frame.render_widget(Paragraph::new(status_line), left);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("images: {image_backend}"),
            Style::new().fg(Color::Rgb(70, 73, 80)),
        )))
        .alignment(Alignment::Right),
        right,
    );
}

fn draw_help(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let width = 70.min(area.width.saturating_sub(4));
    let height = 27.min(area.height.saturating_sub(2));
    let rect = centered_rect(area, width, height);
    if rect.width < 20 || rect.height < 5 {
        return;
    }

    frame.render_widget(Clear, rect);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                " Help ",
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
        .style(Style::new().bg(Color::Reset));
    let inner = block.inner(rect).inner(Margin::new(2, 1));
    frame.render_widget(block, rect);

    let source = match app.active_source {
        Some(source) => source.label().to_owned(),
        None => match app.mode {
            config::SourceChoice::Auto => "automatic".to_owned(),
            config::SourceChoice::Tmdb => "TMDB".to_owned(),
            config::SourceChoice::Wikipedia => "Wikipedia".to_owned(),
        },
    };

    let filters = app.filter_summary();
    let filters = if filters.is_empty() {
        "any filters".to_owned()
    } else {
        filters
    };

    let lines = vec![
        section("Controls"),
        help_line("space / r", "pick another set of movies"),
        help_line("1 – 9", "show 1 to 9 movies"),
        help_line("+ / -", "adjust how many movies"),
        help_line("f", "choose years, language, and popularity"),
        help_line("y", "choose a year range"),
        help_line("← → ↑ ↓", "move between movies"),
        help_line("mouse wheel", "move between movies"),
        help_line("PgUp / PgDn", "jump a screenful"),
        help_line("Home / End", "first or last movie"),
        help_line("o / Enter", "open the movie page in a browser"),
        help_line("? / Esc", "toggle or close this help"),
        help_line("q", "quit"),
        Line::from(""),
        section("Sources"),
        Line::from(vec![
            Span::styled("Active: ", Style::new().fg(DIM)),
            Span::styled(source, Style::new().fg(Color::White)),
            Span::styled("  ·  ", Style::new().fg(DIM)),
            Span::styled(filters, Style::new().fg(Color::White)),
        ]),
        Line::from(Span::styled(
            "TMDB needs a free API key. Wikipedia needs no key.",
            Style::new().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            "Wikipedia language is best-effort; popularity needs TMDB.",
            Style::new().fg(Color::Gray),
        )),
        Line::from(vec![
            Span::styled("Config: ", Style::new().fg(DIM)),
            Span::raw(config::config_path().display().to_string()),
        ]),
        Line::from(vec![
            Span::styled("Cache:  ", Style::new().fg(DIM)),
            Span::raw(config::cache_dir().join("posters").display().to_string()),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "This product uses the TMDB API but is not endorsed or certified by TMDB.",
            Style::new().fg(DIM),
        )),
        Line::from(Span::styled(
            "Wikipedia content is available under CC BY-SA.",
            Style::new().fg(DIM),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_filter_editor(frame: &mut Frame, app: &App) {
    let Some(editor) = &app.filter_editor else {
        return;
    };
    let area = frame.area();
    let width = 54.min(area.width.saturating_sub(4));
    let height = 15.min(area.height.saturating_sub(2));
    let rect = centered_rect(area, width, height);
    if rect.width < 30 || rect.height < 10 {
        return;
    }

    frame.render_widget(Clear, rect);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                " Filters ",
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
        .style(Style::new().bg(Color::Reset));
    let inner = block.inner(rect).inner(Margin::new(2, 1));
    frame.render_widget(block, rect);

    let field = |label: &'static str, value: &str, focused: bool| {
        let text = if value.is_empty() {
            "····".to_owned()
        } else {
            value.to_owned()
        };
        let style = if focused {
            Style::new()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::White)
        };
        Line::from(vec![
            Span::styled(format!("{label:<12}"), Style::new().fg(DIM)),
            Span::styled(format!(" {text} "), style),
        ])
    };
    let choice = |label: &'static str, value: &str, focused: bool| {
        field(label, &format!("‹ {value} ›"), focused)
    };

    let popularity_focused = editor.focus == FilterField::Popularity;
    let popularity_style = if popularity_focused {
        Style::new()
            .fg(Color::Black)
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::White)
    };
    let mut popularity_spans = vec![
        Span::styled(format!("{:<12}", "Popularity"), Style::new().fg(DIM)),
        Span::styled(
            format!(" ‹ {} › ", editor.popularity.label()),
            popularity_style,
        ),
    ];
    if app.active_source == Some(Source::Wikipedia) && editor.popularity != config::Popularity::Any
    {
        popularity_spans.push(Span::styled("TMDB only", Style::new().fg(Color::Yellow)));
    }

    let lines = vec![
        field("From", &editor.from, editor.focus == FilterField::From),
        field("To", &editor.to, editor.focus == FilterField::To),
        choice(
            "Language",
            editor.language.label(),
            editor.focus == FilterField::Language,
        ),
        Line::from(popularity_spans),
        Line::from(""),
        Line::from(Span::styled(
            "Tab/↑↓ field · ←→ change",
            Style::new().fg(DIM),
        )),
        Line::from(Span::styled(
            "Enter apply · Esc cancel",
            Style::new().fg(DIM),
        )),
        Line::from(Span::styled(
            "Empty years use the default bound.",
            Style::new().fg(DIM),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn help_line(key: &'static str, description: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {key:<14}"), Style::new().fg(ACCENT)),
        Span::raw(description),
    ])
}

fn section(title: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        title,
        Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
    ))
}

fn key_hint(key: &str, label: &'static str) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!(" {key} "),
            Style::new()
                .bg(Color::Rgb(48, 51, 58))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {label}"), Style::new().fg(Color::Gray)),
    ]
}

fn attribution(source: Option<Source>) -> String {
    match source {
        Some(Source::Tmdb) => "Data & images: TMDB".to_owned(),
        Some(Source::Wikipedia) => "Data & images: Wikipedia (CC BY-SA)".to_owned(),
        None => "Random movies for your terminal".to_owned(),
    }
}

fn status_color(kind: StatusKind) -> Color {
    match kind {
        StatusKind::Info => Color::Cyan,
        StatusKind::Warning => Color::Yellow,
        StatusKind::Error => Color::Rgb(255, 120, 120),
    }
}

fn source_color(source: Source) -> Color {
    match source {
        Source::Tmdb => TMDB_BLUE,
        Source::Wikipedia => Color::Rgb(210, 210, 210),
    }
}

fn fully_inside(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn truncate(text: &str, max_width: u16) -> String {
    let max_width = max_width as usize;
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut width = 0;
    for ch in text.chars() {
        let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + char_width + 1 > max_width {
            break;
        }
        out.push(ch);
        width += char_width;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};
    use ratatui_image::FontSize;

    use super::*;
    use crate::{
        app::App,
        config::{Filters, Language, Popularity, SourceChoice, YearRange},
        movie::Movie,
    };

    fn movie(index: u64) -> Movie {
        Movie {
            id: format!("tmdb-{index}"),
            title: format!("Test Movie {index}"),
            year: Some(2024),
            rating: Some(7.5),
            genres: vec!["Drama".into()],
            overview: Some("A test movie overview.".into()),
            poster_url: Some("https://example.test/poster.jpg".into()),
            source: Source::Tmdb,
            source_url: Some("https://example.test/movie".into()),
        }
    }

    fn render(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let area: Rect = terminal.size().unwrap().into();
        app.prepare_layout(area, FontSize::new(10, 20));
        terminal
            .draw(|frame| draw(frame, app, "half-blocks"))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..height {
            for x in 0..width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn draws_single_movie() {
        let mut app = App::new(1, SourceChoice::Tmdb, Filters::default());
        app.movies = vec![movie(1)];
        app.active_source = Some(Source::Tmdb);
        render(&mut app, 100, 35);
    }

    #[test]
    fn draws_grid_and_keeps_titles_inside() {
        let mut app = App::new(6, SourceChoice::Tmdb, Filters::default());
        app.movies = (1..=6).map(movie).collect();
        app.active_source = Some(Source::Tmdb);
        render(&mut app, 120, 40);
    }

    #[test]
    fn draws_loading_and_help_states() {
        let mut app = App::new(4, SourceChoice::Wikipedia, Filters::default());
        app.loading = true;
        render(&mut app, 80, 24);
        app.help = true;
        render(&mut app, 80, 24);
    }

    #[test]
    fn draws_filter_editor() {
        let mut app = App::new(
            2,
            SourceChoice::Tmdb,
            Filters {
                year_range: Some(YearRange {
                    from: 1990,
                    to: 1999,
                }),
                language: Language::Japanese,
                popularity: Popularity::HiddenGems,
            },
        );
        app.movies = vec![movie(1), movie(2)];
        app.active_source = Some(Source::Tmdb);
        app.open_filter_editor(FilterField::From);
        let text = render(&mut app, 100, 35);
        assert!(text.contains("Filters"));
        assert!(text.contains("Language"));
        assert!(text.contains("Japanese"));
        assert!(text.contains("Popularity"));
        assert!(text.contains("Hidden gems"));
        assert!(!text.contains("TMDB only"));
    }

    #[test]
    fn marks_popularity_as_tmdb_only_when_wikipedia_is_active() {
        let mut app = App::new(
            1,
            SourceChoice::Wikipedia,
            Filters {
                popularity: Popularity::Popular,
                ..Filters::default()
            },
        );
        app.active_source = Some(Source::Wikipedia);
        app.open_filter_editor(FilterField::Popularity);
        let text = render(&mut app, 100, 35);
        assert!(text.contains("Popular (TMDB only)"));
        assert!(text.contains("TMDB only"));
    }

    #[test]
    fn truncates_to_width() {
        assert_eq!(truncate("Short", 10), "Short");
        assert_eq!(truncate("A long movie title", 8), "A long …");
        assert_eq!(truncate("abc", 0), "");
    }

    #[test]
    fn centers_rect() {
        let inner = Rect::new(0, 0, 100, 40);
        let rect = centered_rect(inner, 20, 10);
        assert_eq!(rect.x, 40);
        assert_eq!(rect.y, 15);
    }
}
