use std::fmt;

/// Where a movie and its poster came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Tmdb,
    Wikipedia,
}

impl Source {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tmdb => "TMDB",
            Self::Wikipedia => "Wikipedia",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Debug)]
pub struct Movie {
    /// Stable, source-prefixed id, also used as the poster cache key.
    pub id: String,
    pub title: String,
    pub year: Option<u16>,
    pub rating: Option<f32>,
    pub genres: Vec<String>,
    pub overview: Option<String>,
    pub poster_url: Option<String>,
    pub source: Source,
    pub source_url: Option<String>,
}

impl Movie {
    pub fn year_text(&self) -> Option<String> {
        self.year.map(|year| year.to_string())
    }

    pub fn rating_text(&self) -> Option<String> {
        self.rating.map(|rating| format!("{rating:.1}"))
    }

    /// A compact `2024 · ★ 7.5 · Drama` line for the poster grid.
    pub fn meta_line(&self, max_genres: usize) -> String {
        let mut parts = Vec::new();
        if let Some(year) = self.year_text() {
            parts.push(year);
        }
        if let Some(rating) = self.rating_text() {
            parts.push(format!("★ {rating}"));
        }
        if max_genres > 0 {
            let genres = self
                .genres
                .iter()
                .take(max_genres)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            if !genres.is_empty() {
                parts.push(genres);
            }
        }
        parts.join(" · ")
    }
}
