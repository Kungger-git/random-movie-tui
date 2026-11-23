use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use serde::Deserialize;

pub const MAX_COUNT: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum SourceChoice {
    /// Use TMDB when a key is available, otherwise Wikipedia.
    Auto,
    /// Only use TMDB (requires an API key).
    Tmdb,
    /// Only use Wikipedia (no key required).
    Wikipedia,
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

    /// Force ANSI half-block posters even if the terminal supports graphics
    #[arg(long)]
    pub halfblocks: bool,
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
    pub halfblocks: bool,
    pub startup_warning: Option<String>,
}

impl Config {
    pub fn from_cli(cli: Cli) -> Self {
        let mut warning = None;

        let count = if (1..=MAX_COUNT).contains(&(cli.count as usize)) {
            cli.count as usize
        } else {
            let clamped = (cli.count as usize).clamp(1, MAX_COUNT);
            warning = Some(format!(
                "--count must be between 1 and {MAX_COUNT}; using {clamped}"
            ));
            clamped
        };

        let file = match load_file() {
            Ok(file) => file,
            Err(err) => {
                warning = Some(err);
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

        Self {
            count,
            mode: cli.source,
            credentials,
            halfblocks: cli.halfblocks,
            startup_warning: warning,
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
}
