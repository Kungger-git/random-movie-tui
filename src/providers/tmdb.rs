use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use reqwest::{Client, RequestBuilder, Url};
use serde::Deserialize;

use crate::{
    config::{CredentialKind, Credentials, Filters},
    movie::{Movie, Source},
};

const DISCOVER_URL: &str = "https://api.themoviedb.org/3/discover/movie";
const POSTER_BASE: &str = "https://image.tmdb.org/t/p/w500";
const MAX_PAGES_TO_TRY: usize = 6;
/// TMDB caps discover results at page 500.
const MAX_DISCOVER_PAGE: u32 = 500;

#[derive(Debug, Deserialize)]
struct DiscoverResponse {
    #[serde(default)]
    results: Vec<RawMovie>,
    #[serde(default)]
    total_pages: u32,
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

pub async fn fetch(
    client: &Client,
    credentials: &Credentials,
    count: usize,
    filters: &Filters,
) -> Result<Vec<Movie>> {
    if count == 0 {
        return Ok(Vec::new());
    }

    let mut movies = Vec::new();
    let mut seen = HashSet::new();
    let mut last_error = None;

    // Page 1 tells us how many pages the current filters have, so narrow year
    // ranges never request a page past the end.
    let first = match fetch_page(client, credentials, filters, 1).await {
        Ok(response) => Some(response),
        Err(error) => {
            last_error = Some(error);
            None
        }
    };

    let mut pages = Vec::new();
    if let Some(response) = &first {
        let total_pages = response.total_pages.clamp(1, MAX_DISCOVER_PAGE);
        let mut extra: Vec<u32> = (2..=total_pages).collect();
        fastrand::shuffle(&mut extra);
        pages.extend(extra.into_iter().take(MAX_PAGES_TO_TRY - 1));
    }

    if let Some(response) = first {
        collect(response.results, &mut movies, &mut seen);
    }

    for page in pages {
        if movies.len() >= count {
            break;
        }
        match fetch_page(client, credentials, filters, page).await {
            Ok(response) => collect(response.results, &mut movies, &mut seen),
            Err(error) => last_error = Some(error),
        }
    }

    if movies.is_empty() {
        if let Some(error) = last_error {
            return Err(error);
        }
        match filters.summary() {
            summary if summary.is_empty() => bail!("TMDB returned no movies with posters"),
            summary => bail!("TMDB returned no movies with posters for {summary}"),
        }
    }

    fastrand::shuffle(&mut movies);
    movies.truncate(count);
    Ok(movies)
}

fn collect(raw: Vec<RawMovie>, movies: &mut Vec<Movie>, seen: &mut HashSet<String>) {
    for raw in raw {
        if let Some(movie) = into_movie(raw)
            && seen.insert(movie.id.clone())
        {
            movies.push(movie);
        }
    }
}

async fn fetch_page(
    client: &Client,
    credentials: &Credentials,
    filters: &Filters,
    page: u32,
) -> Result<DiscoverResponse> {
    let url = discover_url(filters, page);

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

    serde_json::from_str(&body).context("invalid TMDB response")
}

fn discover_url(filters: &Filters, page: u32) -> Url {
    let mut url = Url::parse(DISCOVER_URL).expect("static URL is valid");
    {
        let mut pairs = url.query_pairs_mut();
        pairs
            .append_pair("include_adult", "false")
            .append_pair("include_video", "false")
            .append_pair("language", "en-US")
            .append_pair("sort_by", "popularity.desc")
            .append_pair(
                "vote_count.gte",
                &filters.popularity.min_votes().to_string(),
            )
            .append_pair("page", &page.to_string());
        if let Some(max_votes) = filters.popularity.max_votes() {
            pairs.append_pair("vote_count.lte", &max_votes.to_string());
        }
        if let Some(min_rating) = filters.popularity.min_rating() {
            pairs.append_pair("vote_average.gte", &min_rating.to_string());
        }
        if let Some(code) = filters.language.code() {
            pairs.append_pair("with_original_language", code);
        }
        if let Some(range) = filters.year_range {
            pairs
                .append_pair("primary_release_date.gte", &format!("{}-01-01", range.from))
                .append_pair("primary_release_date.lte", &format!("{}-12-31", range.to));
        }
    }
    url
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
    use crate::config::{Language, Popularity, YearRange};

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
        let movies = fetch(&client, &credentials, 3, &Filters::default())
            .await
            .unwrap();
        assert!(!movies.is_empty());
        assert!(movies.iter().all(|movie| movie.poster_url.is_some()));
        assert!(movies.iter().all(|movie| movie.source == Source::Tmdb));
    }

    #[test]
    fn discover_url_includes_year_range() {
        let filters = Filters {
            year_range: Some(YearRange {
                from: 1990,
                to: 1999,
            }),
            ..Filters::default()
        };
        let url = discover_url(&filters, 3);
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(
            query
                .get("primary_release_date.gte")
                .map(|value| value.as_ref()),
            Some("1990-01-01")
        );
        assert_eq!(
            query
                .get("primary_release_date.lte")
                .map(|value| value.as_ref()),
            Some("1999-12-31")
        );
        assert_eq!(query.get("page").map(|value| value.as_ref()), Some("3"));
    }

    #[test]
    fn discover_url_omits_year_range_when_unset() {
        let url = discover_url(&Filters::default(), 1);
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert!(!query.contains_key("primary_release_date.gte"));
        assert!(!query.contains_key("primary_release_date.lte"));
    }

    #[test]
    fn discover_url_includes_original_language() {
        let filters = Filters {
            language: Language::Japanese,
            ..Filters::default()
        };
        let url = discover_url(&filters, 1);
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(
            query
                .get("with_original_language")
                .map(|value| value.as_ref()),
            Some("ja")
        );
    }

    #[test]
    fn discover_url_omits_language_when_any() {
        let url = discover_url(&Filters::default(), 1);
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert!(!query.contains_key("with_original_language"));
    }

    #[test]
    fn discover_url_applies_popularity_tiers() {
        let any = discover_url(&Filters::default(), 1);
        let any_query: std::collections::HashMap<_, _> = any.query_pairs().collect();
        assert_eq!(
            any_query.get("vote_count.gte").map(|value| value.as_ref()),
            Some("250")
        );
        assert!(!any_query.contains_key("vote_count.lte"));
        assert!(!any_query.contains_key("vote_average.gte"));

        let popular = Filters {
            popularity: Popularity::Popular,
            ..Filters::default()
        };
        let url = discover_url(&popular, 1);
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(
            query.get("vote_count.gte").map(|value| value.as_ref()),
            Some("1000")
        );
        assert!(!query.contains_key("vote_count.lte"));

        let hidden = Filters {
            popularity: Popularity::HiddenGems,
            ..Filters::default()
        };
        let url = discover_url(&hidden, 1);
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(
            query.get("vote_count.gte").map(|value| value.as_ref()),
            Some("50")
        );
        assert_eq!(
            query.get("vote_count.lte").map(|value| value.as_ref()),
            Some("750")
        );
        assert_eq!(
            query.get("vote_average.gte").map(|value| value.as_ref()),
            Some("6.5")
        );
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
