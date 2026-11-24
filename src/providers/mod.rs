pub mod tmdb;
pub mod wikipedia;

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Client;

use crate::{
    config::{Credentials, Filters, SourceChoice},
    movie::{Movie, Source},
};

pub struct Batch {
    pub movies: Vec<Movie>,
    pub source: Source,
    pub warning: Option<String>,
}

pub struct Providers {
    client: Client,
    credentials: Option<Credentials>,
    mode: SourceChoice,
    wikipedia: wikipedia::WikipediaProvider,
}

impl Providers {
    pub fn new(mode: SourceChoice, credentials: Option<Credentials>) -> Result<Self> {
        let client = Client::builder()
            .user_agent(format!(
                "random-movie-tui/{} (+https://github.com/Kungger-git/random-movie-tui)",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            .build()
            .context("could not create HTTP client")?;

        Ok(Self {
            client,
            credentials,
            mode,
            wikipedia: wikipedia::WikipediaProvider::new(),
        })
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub async fn fetch(&self, count: usize, filters: &Filters) -> Result<Batch> {
        match self.mode {
            SourceChoice::Wikipedia => self.fetch_wikipedia(count, filters, None).await,
            SourceChoice::Tmdb => {
                let credentials = self.credentials.as_ref().context(
                    "No TMDB API key found. Set TMDB_API_KEY, add one to \
                     ~/.config/random-movie-tui/config.toml, or run with --source wikipedia.",
                )?;
                let movies = tmdb::fetch(&self.client, credentials, count, filters).await?;
                Ok(finish(Source::Tmdb, movies, count, None))
            }
            SourceChoice::Auto => {
                let Some(credentials) = self.credentials.as_ref() else {
                    return self.fetch_wikipedia(count, filters, None).await;
                };
                match tmdb::fetch(&self.client, credentials, count, filters).await {
                    Ok(movies) => Ok(finish(Source::Tmdb, movies, count, None)),
                    Err(tmdb_error) => {
                        let reason = short_error(&tmdb_error);
                        let warning = format!("TMDB unavailable ({reason}); using Wikipedia");
                        self.fetch_wikipedia(count, filters, Some(warning)).await
                    }
                }
            }
        }
    }

    async fn fetch_wikipedia(
        &self,
        count: usize,
        filters: &Filters,
        warning: Option<String>,
    ) -> Result<Batch> {
        let movies = self
            .wikipedia
            .fetch(&self.client, count, filters)
            .await
            .context("Wikipedia fallback failed")?;
        Ok(finish(Source::Wikipedia, movies, count, warning))
    }
}

fn finish(source: Source, movies: Vec<Movie>, requested: usize, warning: Option<String>) -> Batch {
    let mut warnings = Vec::new();
    if let Some(warning) = warning {
        warnings.push(warning);
    }
    if movies.len() < requested {
        warnings.push(format!(
            "only found {} of {} movies with posters",
            movies.len(),
            requested
        ));
    }
    Batch {
        movies,
        source,
        warning: if warnings.is_empty() {
            None
        } else {
            Some(warnings.join(" · "))
        },
    }
}

fn short_error(error: &anyhow::Error) -> String {
    let text = format!("{error:#}");
    let first = text.lines().next().unwrap_or("unknown error").trim();
    if first.chars().count() > 90 {
        let shortened: String = first.chars().take(89).collect();
        format!("{shortened}…")
    } else if first.is_empty() {
        "unknown error".to_owned()
    } else {
        first.to_owned()
    }
}
