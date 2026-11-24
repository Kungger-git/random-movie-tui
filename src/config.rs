use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use serde::Deserialize;

pub const MAX_COUNT: usize = 20;
pub const MIN_YEAR: u16 = 1880;
/// The lower bound used when no year range is selected.
pub const DEFAULT_FROM_YEAR: u16 = 1920;

pub fn current_year() -> u16 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    1970 + (secs / 31_556_952) as u16
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YearRange {
    pub from: u16,
    pub to: u16,
}

impl YearRange {
    /// The range used when the user has not picked one.
    pub fn full() -> Self {
        Self {
            from: DEFAULT_FROM_YEAR,
            to: current_year(),
        }
    }

    /// Clamps to supported years, swaps reversed bounds, and reports any fixes.
    pub fn normalize(from: u16, to: u16) -> (Self, Option<String>) {
        let current = current_year();
        let mut warnings = Vec::new();
        let mut from = from;
        let mut to = to;

        let clamped_from = from.clamp(MIN_YEAR, current);
        let clamped_to = to.clamp(MIN_YEAR, current);
        if clamped_from != from || clamped_to != to {
            warnings.push(format!("years must be between {MIN_YEAR} and {current}"));
            from = clamped_from;
            to = clamped_to;
        }

        if from > to {
            std::mem::swap(&mut from, &mut to);
            warnings.push("swapped the year range".to_owned());
        }

        let range = Self { from, to };
        let warning = (!warnings.is_empty())
            .then(|| format!("{}; using {}", warnings.join(", "), range.label()));
        (range, warning)
    }

    pub fn label(&self) -> String {
        if self.from == self.to {
            self.from.to_string()
        } else {
            format!("{}–{}", self.from, self.to)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum SourceChoice {
    /// Use TMDB when a key is available, otherwise Wikipedia.
    Auto,
    /// Only use TMDB (requires an API key).
    Tmdb,
    /// Only use Wikipedia (no key required).
    Wikipedia,
}

/// Original language of the movies. `Any` disables the filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Language {
    #[default]
    Any,
    English,
    Spanish,
    French,
    German,
    Italian,
    Portuguese,
    Japanese,
    Korean,
    Chinese,
    Hindi,
    Russian,
    Arabic,
    Turkish,
    Swedish,
    Dutch,
    Polish,
    Danish,
}

impl Language {
    /// Languages offered by the in-app selector, in display order.
    pub const ALL: [Language; 18] = [
        Language::Any,
        Language::English,
        Language::Spanish,
        Language::French,
        Language::German,
        Language::Italian,
        Language::Portuguese,
        Language::Japanese,
        Language::Korean,
        Language::Chinese,
        Language::Hindi,
        Language::Russian,
        Language::Arabic,
        Language::Turkish,
        Language::Swedish,
        Language::Dutch,
        Language::Polish,
        Language::Danish,
    ];

    /// The TMDB ISO 639-1 original-language code.
    pub const fn code(self) -> Option<&'static str> {
        match self {
            Self::Any => None,
            Self::English => Some("en"),
            Self::Spanish => Some("es"),
            Self::French => Some("fr"),
            Self::German => Some("de"),
            Self::Italian => Some("it"),
            Self::Portuguese => Some("pt"),
            Self::Japanese => Some("ja"),
            Self::Korean => Some("ko"),
            Self::Chinese => Some("zh"),
            Self::Hindi => Some("hi"),
            Self::Russian => Some("ru"),
            Self::Arabic => Some("ar"),
            Self::Turkish => Some("tr"),
            Self::Swedish => Some("sv"),
            Self::Dutch => Some("nl"),
            Self::Polish => Some("pl"),
            Self::Danish => Some("da"),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::English => "English",
            Self::Spanish => "Spanish",
            Self::French => "French",
            Self::German => "German",
            Self::Italian => "Italian",
            Self::Portuguese => "Portuguese",
            Self::Japanese => "Japanese",
            Self::Korean => "Korean",
            Self::Chinese => "Chinese",
            Self::Hindi => "Hindi",
            Self::Russian => "Russian",
            Self::Arabic => "Arabic",
            Self::Turkish => "Turkish",
            Self::Swedish => "Swedish",
            Self::Dutch => "Dutch",
            Self::Polish => "Polish",
            Self::Danish => "Danish",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|language| *language == self);
        match index {
            Some(index) => Self::ALL[(index + 1) % Self::ALL.len()],
            None => Self::Any,
        }
    }

    pub fn prev(self) -> Self {
        let index = Self::ALL.iter().position(|language| *language == self);
        match index {
            Some(index) => Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()],
            None => Self::Any,
        }
    }

    /// Accepts `any`, a display name, or an ISO 639-1 code.
    pub fn parse(value: &str) -> Result<Self, String> {
        let value = value.trim().to_ascii_lowercase();
        if let Some(language) = Self::ALL.into_iter().find(|language| {
            language.label().eq_ignore_ascii_case(&value) || language.code() == Some(value.as_str())
        }) {
            return Ok(language);
        }
        Err(format!(
            "unknown language '{value}'; use any, a name like japanese, or a code like ja"
        ))
    }
}

/// How well-known the picked movies should be.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Popularity {
    #[default]
    Any,
    Popular,
    #[value(name = "hidden-gems", alias = "hidden")]
    HiddenGems,
}

impl Popularity {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::Popular => "Popular",
            Self::HiddenGems => "Hidden gems",
        }
    }

    pub const fn min_votes(self) -> u32 {
        match self {
            Self::Any => 250,
            Self::Popular => 1_000,
            Self::HiddenGems => 50,
        }
    }

    pub const fn max_votes(self) -> Option<u32> {
        match self {
            Self::HiddenGems => Some(750),
            Self::Any | Self::Popular => None,
        }
    }

    pub const fn min_rating(self) -> Option<f32> {
        match self {
            Self::HiddenGems => Some(6.5),
            Self::Any | Self::Popular => None,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Any => Self::Popular,
            Self::Popular => Self::HiddenGems,
            Self::HiddenGems => Self::Any,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Any => Self::HiddenGems,
            Self::Popular => Self::Any,
            Self::HiddenGems => Self::Popular,
        }
    }
}

/// Everything that narrows which random movies are fetched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    pub year_range: Option<YearRange>,
    pub language: Language,
    pub popularity: Popularity,
}

impl Filters {
    pub fn is_default(&self) -> bool {
        self.year_range.is_none()
            && self.language == Language::Any
            && self.popularity == Popularity::Any
    }

    /// A compact `1990–1999 · Japanese · Popular` description.
    pub fn summary(&self) -> String {
        if self.is_default() {
            return String::new();
        }
        let mut parts = Vec::new();
        if let Some(range) = self.year_range {
            parts.push(range.label());
        }
        if self.language != Language::Any {
            parts.push(self.language.label().to_owned());
        }
        if self.popularity != Popularity::Any {
            parts.push(self.popularity.label().to_owned());
        }
        parts.join(" · ")
    }

    /// Wikipedia has no vote counts, so popularity cannot be applied there.
    pub fn wikipedia_ignores_popularity(&self) -> bool {
        self.popularity != Popularity::Any
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "random-movie-tui",
    version,
    about = "Pick random movies and see their posters, right in your terminal",
    long_about = None
)]
pub struct Cli {
    /// Number of movies to show (1-20)
    #[arg(short, long, default_value_t = 1)]
    pub count: u8,

    /// Where to get random movies from
    #[arg(long, value_enum, default_value_t = SourceChoice::Auto)]
    pub source: SourceChoice,

    /// TMDB API key or v4 read access token
    #[arg(long, value_name = "KEY")]
    pub api_key: Option<String>,

    /// Earliest release year to include (default 1920)
    #[arg(long, value_name = "YEAR")]
    pub from_year: Option<u16>,

    /// Latest release year to include (default this year)
    #[arg(long, value_name = "YEAR")]
    pub to_year: Option<u16>,

    /// Original language: a supported name or ISO 639-1 code (default any)
    #[arg(long, value_name = "LANG", default_value = "any", value_parser = parse_language)]
    pub language: Language,

    /// Popularity tier: any, popular, or hidden-gems
    #[arg(long, value_enum, default_value = "any")]
    pub popularity: Popularity,

    /// Force ANSI half-block posters even if the terminal supports graphics
    #[arg(long)]
    pub halfblocks: bool,
}

fn parse_language(value: &str) -> Result<Language, String> {
    Language::parse(value)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialKind {
    ApiKey,
    Bearer,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub kind: CredentialKind,
    pub value: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.kind {
            CredentialKind::ApiKey => "api key",
            CredentialKind::Bearer => "bearer token",
        };
        f.debug_struct("Credentials")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub count: usize,
    pub mode: SourceChoice,
    pub credentials: Option<Credentials>,
    pub filters: Filters,
    pub halfblocks: bool,
    pub startup_warning: Option<String>,
}

impl Config {
    pub fn from_cli(cli: Cli) -> Self {
        let mut warnings = Vec::new();

        let count = if (1..=MAX_COUNT).contains(&(cli.count as usize)) {
            cli.count as usize
        } else {
            let clamped = (cli.count as usize).clamp(1, MAX_COUNT);
            warnings.push(format!(
                "--count must be between 1 and {MAX_COUNT}; using {clamped}"
            ));
            clamped
        };

        let file = match load_file() {
            Ok(file) => file,
            Err(err) => {
                warnings.push(err);
                FileConfig::default()
            }
        };

        let env_key = std::env::var("TMDB_API_KEY").ok();
        let env_token = std::env::var("TMDB_READ_ACCESS_TOKEN").ok();

        let credentials = resolve_credentials(
            cli.api_key.as_deref(),
            env_key.as_deref(),
            env_token.as_deref(),
            file.tmdb_api_key.as_deref(),
            file.tmdb_read_access_token.as_deref(),
        );

        let year_range = match (cli.from_year, cli.to_year) {
            (None, None) => None,
            (from, to) => {
                let full = YearRange::full();
                let (range, range_warning) =
                    YearRange::normalize(from.unwrap_or(full.from), to.unwrap_or(full.to));
                if let Some(range_warning) = range_warning {
                    warnings.push(range_warning);
                }
                Some(range)
            }
        };

        let filters = Filters {
            year_range,
            language: cli.language,
            popularity: cli.popularity,
        };

        Self {
            count,
            mode: cli.source,
            credentials,
            filters,
            halfblocks: cli.halfblocks,
            startup_warning: (!warnings.is_empty()).then(|| warnings.join(" · ")),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    tmdb_api_key: Option<String>,
    tmdb_read_access_token: Option<String>,
}

fn load_file() -> Result<FileConfig, String> {
    let path = config_path();
    if !path.exists() {
        return Ok(FileConfig::default());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    toml::from_str(&text).map_err(|err| format!("Invalid {}: {err}", path.display()))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn looks_like_bearer(value: &str) -> bool {
    value.starts_with("eyJ")
}

/// Credential precedence: CLI, environment, config file.
pub fn resolve_credentials(
    cli_key: Option<&str>,
    env_key: Option<&str>,
    env_token: Option<&str>,
    file_key: Option<&str>,
    file_token: Option<&str>,
) -> Option<Credentials> {
    if let Some(value) = non_empty(cli_key) {
        return Some(Credentials {
            kind: if looks_like_bearer(value) {
                CredentialKind::Bearer
            } else {
                CredentialKind::ApiKey
            },
            value: value.to_owned(),
        });
    }
    if let Some(value) = non_empty(env_key) {
        return Some(Credentials {
            kind: if looks_like_bearer(value) {
                CredentialKind::Bearer
            } else {
                CredentialKind::ApiKey
            },
            value: value.to_owned(),
        });
    }
    if let Some(value) = non_empty(env_token) {
        return Some(Credentials {
            kind: CredentialKind::Bearer,
            value: value.to_owned(),
        });
    }
    if let Some(value) = non_empty(file_key) {
        return Some(Credentials {
            kind: if looks_like_bearer(value) {
                CredentialKind::Bearer
            } else {
                CredentialKind::ApiKey
            },
            value: value.to_owned(),
        });
    }
    non_empty(file_token).map(|value| Credentials {
        kind: CredentialKind::Bearer,
        value: value.to_owned(),
    })
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("random-movie-tui")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("random-movie-tui")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_key_wins() {
        let creds = resolve_credentials(
            Some("cli-key"),
            Some("env-key"),
            Some("env-token"),
            Some("file-key"),
            Some("file-token"),
        )
        .unwrap();
        assert_eq!(creds.kind, CredentialKind::ApiKey);
        assert_eq!(creds.value, "cli-key");
    }

    #[test]
    fn bearer_detection() {
        let creds = resolve_credentials(Some("eyJhbGciOi"), None, None, None, None).unwrap();
        assert_eq!(creds.kind, CredentialKind::Bearer);
    }

    #[test]
    fn env_token_is_bearer() {
        let creds = resolve_credentials(None, None, Some("token"), None, None).unwrap();
        assert_eq!(creds.kind, CredentialKind::Bearer);
    }

    #[test]
    fn blank_values_are_ignored() {
        assert!(resolve_credentials(Some(" "), Some(""), None, None, None).is_none());
    }

    #[test]
    fn config_file_key_is_used_last() {
        let creds =
            resolve_credentials(None, None, None, Some("file-key"), Some("file-token")).unwrap();
        assert_eq!(creds.kind, CredentialKind::ApiKey);
        assert_eq!(creds.value, "file-key");
    }

    #[test]
    fn year_range_swaps_reversed_bounds() {
        let (range, warning) = YearRange::normalize(2000, 1990);
        assert_eq!(
            range,
            YearRange {
                from: 1990,
                to: 2000
            }
        );
        assert!(warning.is_some_and(|warning| warning.contains("swapped")));
    }

    #[test]
    fn year_range_clamps_out_of_bounds() {
        let (range, warning) = YearRange::normalize(1700, 5000);
        assert_eq!(range.from, MIN_YEAR);
        assert_eq!(range.to, current_year());
        assert!(warning.is_some_and(|warning| warning.contains("between")));
    }

    #[test]
    fn year_range_keeps_valid_bounds() {
        let (range, warning) = YearRange::normalize(1990, 1999);
        assert_eq!(
            range,
            YearRange {
                from: 1990,
                to: 1999
            }
        );
        assert!(warning.is_none());
    }

    #[test]
    fn year_range_label_handles_single_year() {
        assert_eq!(
            YearRange {
                from: 1999,
                to: 1999
            }
            .label(),
            "1999"
        );
        assert_eq!(
            YearRange {
                from: 1990,
                to: 1999
            }
            .label(),
            "1990–1999"
        );
    }

    #[test]
    fn cli_year_flags_default_to_none() {
        let cli = Cli::try_parse_from(["random-movie-tui"]).unwrap();
        assert_eq!(cli.from_year, None);
        assert_eq!(cli.to_year, None);
    }

    #[test]
    fn cli_parses_year_flags() {
        let cli = Cli::try_parse_from([
            "random-movie-tui",
            "--from-year",
            "1990",
            "--to-year",
            "1999",
        ])
        .unwrap();
        assert_eq!(cli.from_year, Some(1990));
        assert_eq!(cli.to_year, Some(1999));
    }

    #[test]
    fn parses_language_names_and_codes() {
        assert_eq!(Language::parse("any").unwrap(), Language::Any);
        assert_eq!(Language::parse(" Japanese ").unwrap(), Language::Japanese);
        assert_eq!(Language::parse("ja").unwrap(), Language::Japanese);
        assert_eq!(Language::parse("PT").unwrap(), Language::Portuguese);
        assert!(Language::parse("klingon").is_err());
    }

    #[test]
    fn language_cycles_through_all_choices() {
        let mut language = Language::Any;
        for _ in 0..Language::ALL.len() {
            language = language.next();
        }
        assert_eq!(language, Language::Any);
        assert_eq!(Language::Any.prev(), Language::Danish);
    }

    #[test]
    fn popularity_tiers_map_to_vote_bounds() {
        assert_eq!(Popularity::Any.min_votes(), 250);
        assert_eq!(Popularity::Any.max_votes(), None);
        assert_eq!(Popularity::Popular.min_votes(), 1_000);
        assert_eq!(Popularity::HiddenGems.min_votes(), 50);
        assert_eq!(Popularity::HiddenGems.max_votes(), Some(750));
        assert_eq!(Popularity::HiddenGems.min_rating(), Some(6.5));
        assert_eq!(Popularity::Any.next(), Popularity::Popular);
        assert_eq!(Popularity::Any.prev(), Popularity::HiddenGems);
    }

    #[test]
    fn filters_summary_skips_defaults() {
        assert_eq!(Filters::default().summary(), "");
        assert!(Filters::default().is_default());

        let filters = Filters {
            year_range: Some(YearRange {
                from: 1990,
                to: 1999,
            }),
            language: Language::Japanese,
            popularity: Popularity::Popular,
        };
        assert_eq!(filters.summary(), "1990–1999 · Japanese · Popular");
        assert!(!filters.is_default());
        assert!(filters.wikipedia_ignores_popularity());
        assert!(
            !Filters {
                language: Language::Japanese,
                ..Filters::default()
            }
            .wikipedia_ignores_popularity()
        );
    }

    #[test]
    fn cli_parses_language_and_popularity() {
        let cli = Cli::try_parse_from([
            "random-movie-tui",
            "--language",
            "ja",
            "--popularity",
            "hidden-gems",
        ])
        .unwrap();
        assert_eq!(cli.language, Language::Japanese);
        assert_eq!(cli.popularity, Popularity::HiddenGems);
    }

    #[test]
    fn cli_defaults_to_any_filters() {
        let cli = Cli::try_parse_from(["random-movie-tui"]).unwrap();
        assert_eq!(cli.language, Language::Any);
        assert_eq!(cli.popularity, Popularity::Any);
    }
}
