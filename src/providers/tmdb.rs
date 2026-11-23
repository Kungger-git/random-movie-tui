use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use reqwest::{Client, RequestBuilder, Url};
use serde::Deserialize;

use crate::{
    config::{CredentialKind, Credentials},
    movie::{Movie, Source},
};

const DISCOVER_URL: &str = "https://api.themoviedb.org/3/discover/movie";
const POSTER_BASE: &str = "https://image.tmdb.org/t/p/w500";
const MAX_PAGES_TO_TRY: usize = 6;

#[derive(Debug, Deserialize)]
struct DiscoverResponse {
    #[serde(default)]
    results: Vec<RawMovie>,
}

#[derive(Debug, Deserialize)]
struct RawMovie {
    id: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    vote_average: Option<f32>,
    #[serde(default)]
    genre_ids: Vec<u16>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    poster_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    #[serde(default)]
    status_message: Option<String>,
}

pub async fn fetch(client: &Client, credentials: &Credentials, count: usize) -> Result<Vec<Movie>> {
    if count == 0 {
        return Ok(Vec::new());
    }

    let mut pages: Vec<u32> = (1..=50).collect();
    fastrand::shuffle(&mut pages);

    let mut movies = Vec::new();
    let mut seen = HashSet::new();
    let mut last_error = None;

    for page in pages.into_iter().take(MAX_PAGES_TO_TRY) {
        let raw = match fetch_page(client, credentials, page).await {
            Ok(raw) => raw,
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };

        for raw in raw {
            if let Some(movie) = into_movie(raw)
                && seen.insert(movie.id.clone())
            {
                movies.push(movie);
            }
        }

        if movies.len() >= count {
            break;
        }
    }

    if movies.is_empty() {
        if let Some(error) = last_error {
            return Err(error);
        }
        bail!("TMDB returned no movies with posters")
    }

    fastrand::shuffle(&mut movies);
    movies.truncate(count);
    Ok(movies)
}

async fn fetch_page(
    client: &Client,
    credentials: &Credentials,
    page: u32,
) -> Result<Vec<RawMovie>> {
    let mut url = Url::parse(DISCOVER_URL).expect("static URL is valid");
    url.query_pairs_mut()
        .append_pair("include_adult", "false")
        .append_pair("include_video", "false")
        .append_pair("language", "en-US")
        .append_pair("sort_by", "popularity.desc")
        .append_pair("vote_count.gte", "250")
        .append_pair("page", &page.to_string());

    let response = authorized(client.get(url), credentials)
        .send()
        .await
        .context("TMDB request failed")?;

    let status = response.status();
    let body = response
        .text()
        .await
        .context("could not read TMDB response")?;

    if !status.is_success() {
        let message = serde_json::from_str::<ApiError>(&body)
            .ok()
            .and_then(|error| error.status_message)
            .filter(|message| !message.is_empty());

        return Err(match status.as_u16() {
            401 => anyhow::anyhow!(
                "TMDB rejected the API key (401). Check TMDB_API_KEY or your config file"
            ),
            429 => anyhow::anyhow!("TMDB rate limit reached (429); try again in a moment"),
            code => match message {
                Some(message) => anyhow::anyhow!("TMDB request failed (HTTP {code}): {message}"),
                None => anyhow::anyhow!("TMDB request failed (HTTP {code})"),
            },
        });
    }

    let parsed: DiscoverResponse = serde_json::from_str(&body).context("invalid TMDB response")?;
    Ok(parsed.results)
}

fn authorized(request: RequestBuilder, credentials: &Credentials) -> RequestBuilder {
    match credentials.kind {
        CredentialKind::ApiKey => request.query(&[("api_key", credentials.value.as_str())]),
        CredentialKind::Bearer => request.bearer_auth(&credentials.value),
    }
}

fn into_movie(raw: RawMovie) -> Option<Movie> {
    let poster_path = raw.poster_path?.trim().to_owned();
    if poster_path.is_empty() {
        return None;
    }
    let title = raw.title.trim().to_owned();
    if title.is_empty() {
        return None;
    }

    Some(Movie {
        id: format!("tmdb-{}", raw.id),
        title,
        year: raw.release_date.as_deref().and_then(year_from_date),
        rating: raw
            .vote_average
            .filter(|rating| *rating > 0.0)
            .map(|rating| rating.clamp(0.0, 10.0)),
        genres: genres_from_ids(&raw.genre_ids),
        overview: raw
            .overview
            .map(|overview| overview.trim().to_owned())
            .filter(|overview| !overview.is_empty()),
        poster_url: Some(poster_url(&poster_path)),
        source: Source::Tmdb,
        source_url: Some(format!("https://www.themoviedb.org/movie/{}", raw.id)),
    })
}

fn year_from_date(date: &str) -> Option<u16> {
    date.split('-')
        .next()?
        .parse::<u16>()
        .ok()
        .filter(|year| (1880..=2100).contains(year))
}

fn poster_url(path: &str) -> String {
    if path.starts_with('/') {
        format!("{POSTER_BASE}{path}")
    } else {
        format!("{POSTER_BASE}/{path}")
    }
}

fn genres_from_ids(ids: &[u16]) -> Vec<String> {
    let mut genres = Vec::new();
    for id in ids {
        if let Some(name) = genre_name(*id)
            && !genres.iter().any(|genre| genre == name)
        {
            genres.push(name.to_owned());
        }
    }
    genres.truncate(3);
    genres
}

fn genre_name(id: u16) -> Option<&'static str> {
    Some(match id {
        28 => "Action",
        12 => "Adventure",
        16 => "Animation",
        35 => "Comedy",
        80 => "Crime",
        99 => "Documentary",
        18 => "Drama",
        10751 => "Family",
        14 => "Fantasy",
        36 => "History",
        27 => "Horror",
        10402 => "Music",
        9648 => "Mystery",
        10749 => "Romance",
        878 => "Sci-Fi",
        10770 => "TV Movie",
        53 => "Thriller",
        10752 => "War",
        37 => "Western",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_year() {
        assert_eq!(year_from_date("2024-03-01"), Some(2024));
        assert_eq!(year_from_date(""), None);
        assert_eq!(year_from_date("not-a-date"), None);
    }

    #[test]
    fn builds_poster_url() {
        assert_eq!(
            poster_url("/abc.jpg"),
            "https://image.tmdb.org/t/p/w500/abc.jpg"
        );
        assert_eq!(
            poster_url("abc.jpg"),
            "https://image.tmdb.org/t/p/w500/abc.jpg"
        );
    }

    #[test]
    fn maps_genres_and_skips_unknowns() {
        assert_eq!(
            genres_from_ids(&[28, 12, 9999]),
            vec!["Action", "Adventure"]
        );
        assert_eq!(genres_from_ids(&[28, 28, 35]), vec!["Action", "Comedy"]);
    }

    #[tokio::test]
    #[ignore = "hits the live TMDB API (needs TMDB_API_KEY)"]
    async fn live_fetch_returns_movies_with_posters() {
        let Some(key) = std::env::var("TMDB_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty())
        else {
            return;
        };
        let credentials = Credentials {
            kind: CredentialKind::ApiKey,
            value: key,
        };
        let client = Client::builder()
            .user_agent("random-movie-tui-test/0.1")
            .build()
            .unwrap();
        let movies = fetch(&client, &credentials, 3).await.unwrap();
        assert!(!movies.is_empty());
        assert!(movies.iter().all(|movie| movie.poster_url.is_some()));
        assert!(movies.iter().all(|movie| movie.source == Source::Tmdb));
    }

    #[test]
    fn movie_without_poster_is_skipped() {
        let raw = RawMovie {
            id: 1,
            title: "Test".into(),
            release_date: Some("2020-01-01".into()),
            vote_average: Some(7.0),
            genre_ids: vec![18],
            overview: Some("Hi".into()),
            poster_path: None,
        };
        assert!(into_movie(raw).is_none());
    }
}
