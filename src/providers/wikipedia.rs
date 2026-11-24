use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

use anyhow::{Context, Result, bail};
use reqwest::{Client, Url};
use serde::Deserialize;

use crate::{
    config::{Filters, Language, YearRange},
    movie::{Movie, Source},
};

const API_URL: &str = "https://en.wikipedia.org/w/api.php";
const MAX_TITLES_PER_BATCH: usize = 30;

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    query: Option<T>,
}

#[derive(Debug, Deserialize)]
struct CategoryQuery {
    #[serde(default)]
    categorymembers: Vec<CategoryMember>,
}

#[derive(Debug, Deserialize)]
struct CategoryMember {
    title: String,
}

#[derive(Debug, Deserialize)]
struct PagesQuery {
    #[serde(default)]
    pages: Vec<Page>,
}

#[derive(Debug, Deserialize)]
struct Page {
    pageid: u64,
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    thumbnail: Option<Thumbnail>,
    #[serde(default)]
    fullurl: Option<String>,
    #[serde(default)]
    missing: bool,
}

#[derive(Debug, Deserialize)]
struct Thumbnail {
    source: String,
    width: u32,
    height: u32,
}

type CategoryCache = HashMap<(u16, Option<char>, Language), Vec<String>>;

pub struct WikipediaProvider {
    categories: Mutex<CategoryCache>,
}

impl WikipediaProvider {
    pub fn new() -> Self {
        Self {
            categories: Mutex::new(HashMap::new()),
        }
    }

    pub async fn fetch(
        &self,
        client: &Client,
        count: usize,
        filters: &Filters,
    ) -> Result<Vec<Movie>> {
        let range = filters.year_range.unwrap_or_else(YearRange::full);
        let count = count.max(1);
        let years_target = count.div_ceil(3).clamp(1, 4);
        let per_year = count.div_ceil(years_target).max(1);
        let max_attempts = years_target + 5;

        let mut movies = Vec::new();
        let mut seen_titles: HashSet<String> = HashSet::new();
        let mut used_years: HashSet<u16> = HashSet::new();
        let mut last_error = None;

        for attempt in 0..max_attempts {
            if movies.len() >= count {
                break;
            }

            let year = pick_year(&range, &used_years, attempt >= years_target);
            used_years.insert(year);

            let members = match self.members(client, year, filters.language).await {
                Ok(members) => members,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };
            if members.is_empty() {
                continue;
            }

            let mut candidates: Vec<String> = members
                .into_iter()
                .filter(|title| !seen_titles.contains(title) && is_candidate_title(title))
                .collect();
            fastrand::shuffle(&mut candidates);
            candidates.truncate(
                (per_year + 10)
                    .min(MAX_TITLES_PER_BATCH)
                    .min(candidates.len()),
            );
            if candidates.is_empty() {
                continue;
            }

            let pages = match self.details(client, &candidates).await {
                Ok(pages) => pages,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };

            let mut added = 0;
            for page in pages {
                if added >= per_year || movies.len() >= count {
                    break;
                }
                let Some(movie) = page_into_movie(page, year) else {
                    continue;
                };
                if seen_titles.insert(movie.title.clone()) {
                    movies.push(movie);
                    added += 1;
                }
            }
        }

        if movies.is_empty() {
            if let Some(error) = last_error {
                return Err(error);
            }
            match wikipedia_summary(filters) {
                summary if summary.is_empty() => {
                    bail!("Wikipedia returned no movies with posters; try refreshing")
                }
                summary => bail!("Wikipedia returned no movies with posters for {summary}"),
            }
        }

        fastrand::shuffle(&mut movies);
        Ok(movies)
    }

    async fn members(&self, client: &Client, year: u16, language: Language) -> Result<Vec<String>> {
        let prefix = (language == Language::Any && fastrand::bool())
            .then(|| (b'A' + fastrand::u8(0..26)) as char);
        let key = (year, prefix, language);

        if let Some(cached) = self.cached(key) {
            return Ok(cached);
        }

        let titles = fetch_members(client, year, prefix, language).await?;
        let titles = if language == Language::Any && titles.len() < 8 && prefix.is_some() {
            if let Some(cached) = self.cached((year, None, language)) {
                cached
            } else {
                let all = fetch_members(client, year, None, language).await?;
                self.store((year, None, language), &all);
                all
            }
        } else {
            titles
        };

        self.store(key, &titles);
        Ok(titles)
    }

    fn cached(&self, key: (u16, Option<char>, Language)) -> Option<Vec<String>> {
        self.categories
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).cloned())
    }

    fn store(&self, key: (u16, Option<char>, Language), titles: &[String]) {
        if let Ok(mut cache) = self.categories.lock() {
            cache.insert(key, titles.to_vec());
        }
    }

    async fn details(&self, client: &Client, titles: &[String]) -> Result<Vec<Page>> {
        let mut url = Url::parse(API_URL).expect("static URL is valid");
        url.query_pairs_mut()
            .append_pair("action", "query")
            .append_pair("format", "json")
            .append_pair("formatversion", "2")
            .append_pair("prop", "pageimages|description|info")
            .append_pair("piprop", "thumbnail")
            .append_pair("pithumbsize", "500")
            .append_pair("pilicense", "any")
            .append_pair("inprop", "url")
            .append_pair("titles", &titles.join("|"));

        let response = client
            .get(url)
            .send()
            .await
            .context("Wikipedia request failed")?
            .error_for_status()
            .context("Wikipedia returned an error")?;

        let envelope: ApiEnvelope<PagesQuery> = response
            .json()
            .await
            .context("invalid Wikipedia response")?;

        Ok(envelope.query.map(|query| query.pages).unwrap_or_default())
    }
}

async fn fetch_members(
    client: &Client,
    year: u16,
    prefix: Option<char>,
    language: Language,
) -> Result<Vec<String>> {
    let mut url = Url::parse(API_URL).expect("static URL is valid");
    {
        let mut pairs = url.query_pairs_mut();
        pairs
            .append_pair("action", "query")
            .append_pair("format", "json")
            .append_pair("formatversion", "2")
            .append_pair("list", "categorymembers")
            .append_pair("cmtitle", &category_title(year, language))
            .append_pair("cmtype", "page")
            .append_pair("cmlimit", "500");
        if let Some(prefix) = prefix {
            pairs.append_pair("cmstartsortkeyprefix", &prefix.to_string());
        }
    }

    let response = client
        .get(url)
        .send()
        .await
        .context("Wikipedia category request failed")?
        .error_for_status()
        .context("Wikipedia category request returned an error")?;

    let envelope: ApiEnvelope<CategoryQuery> = response
        .json()
        .await
        .context("invalid Wikipedia category response")?;

    let mut seen = HashSet::new();
    Ok(envelope
        .query
        .map(|query| query.categorymembers)
        .unwrap_or_default()
        .into_iter()
        .map(|member| member.title)
        .filter(|title| seen.insert(title.clone()))
        .collect())
}

fn category_title(year: u16, language: Language) -> String {
    match language {
        Language::Any => format!("Category:{year} films"),
        _ => format!("Category:{year} {}-language films", language.label()),
    }
}

fn wikipedia_summary(filters: &Filters) -> String {
    let mut parts = Vec::new();
    if let Some(range) = filters.year_range {
        parts.push(range.label());
    }
    if filters.language != Language::Any {
        parts.push(filters.language.label().to_owned());
    }
    parts.join(" · ")
}

fn pick_year(range: &YearRange, used: &HashSet<u16>, allow_repeat: bool) -> u16 {
    for _ in 0..12 {
        let year = fastrand::u16(range.from..=range.to);
        if allow_repeat || !used.contains(&year) {
            return year;
        }
    }
    fastrand::u16(range.from..=range.to)
}

fn page_into_movie(page: Page, category_year: u16) -> Option<Movie> {
    if page.missing {
        return None;
    }
    let title = clean_title(&page.title);
    if title.is_empty() || title.to_lowercase().starts_with("list of ") {
        return None;
    }

    let description = page
        .description
        .as_deref()
        .map(str::trim)
        .filter(|description| !description.is_empty());
    if description.is_some_and(is_bad_description) {
        return None;
    }

    let thumbnail = page.thumbnail?;
    if !is_portrait(&thumbnail) {
        return None;
    }

    let year = description
        .and_then(parse_year)
        .or(Some(category_year))
        .filter(|year| (1880..=2100).contains(year));

    let genres = description.map(genres_from_description).unwrap_or_default();
    let overview = description.map(sentence_case);

    Some(Movie {
        id: format!("wiki-{}", page.pageid),
        title,
        year,
        rating: None,
        genres,
        overview,
        poster_url: Some(thumbnail.source),
        source: Source::Wikipedia,
        source_url: page
            .fullurl
            .or_else(|| Some(format!("https://en.wikipedia.org/?curid={}", page.pageid))),
    })
}

fn is_candidate_title(title: &str) -> bool {
    let lower = title.to_lowercase();
    !lower.starts_with("list of ")
        && !lower.starts_with("category:")
        && !lower.ends_with("(disambiguation)")
}

fn is_portrait(thumbnail: &Thumbnail) -> bool {
    thumbnail.width >= 80
        && thumbnail.height > thumbnail.width.saturating_mul(115) / 100
        && !thumbnail.source.trim().is_empty()
}

fn is_bad_description(description: &str) -> bool {
    let description = description.to_lowercase();
    [
        "television series",
        "tv series",
        "tv program",
        "television film",
        "miniseries",
        "web series",
        "film award",
        "film festival",
        "film director",
        "film producer",
        "film studio",
        "film series",
        "soundtrack",
        "video game",
        "novel",
        "list of",
    ]
    .iter()
    .any(|needle| description.contains(needle))
}

fn clean_title(title: &str) -> String {
    let title = title.trim();
    if title.ends_with(')')
        && let Some(index) = title.rfind(" (")
    {
        let qualifier = title[index + 2..title.len() - 1].to_lowercase();
        if qualifier.contains("film") || qualifier.contains("movie") {
            return title[..index].trim().to_owned();
        }
    }
    title.to_owned()
}

fn parse_year(text: &str) -> Option<u16> {
    let bytes = text.as_bytes();
    if bytes.len() < 4 {
        return None;
    }
    for index in 0..=bytes.len() - 4 {
        let window = &bytes[index..index + 4];
        if window.iter().all(u8::is_ascii_digit) {
            let year = std::str::from_utf8(window).ok()?.parse::<u16>().ok()?;
            if (1900..=2099).contains(&year) {
                return Some(year);
            }
        }
    }
    None
}

fn sentence_case(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn genres_from_description(description: &str) -> Vec<String> {
    const GENRES: &[(&str, &str)] = &[
        ("action", "Action"),
        ("adventure", "Adventure"),
        ("animated", "Animation"),
        ("animation", "Animation"),
        ("comedy", "Comedy"),
        ("crime", "Crime"),
        ("documentary", "Documentary"),
        ("drama", "Drama"),
        ("family", "Family"),
        ("fantasy", "Fantasy"),
        ("historical", "History"),
        ("history", "History"),
        ("horror", "Horror"),
        ("musical", "Music"),
        ("mystery", "Mystery"),
        ("romantic", "Romance"),
        ("romance", "Romance"),
        ("science fiction", "Sci-Fi"),
        ("sci-fi", "Sci-Fi"),
        ("thriller", "Thriller"),
        ("war", "War"),
        ("western", "Western"),
    ];

    let description = description.to_lowercase();
    let mut genres: Vec<String> = Vec::new();
    for (needle, name) in GENRES {
        if description.contains(needle) && !genres.iter().any(|genre| genre == name) {
            genres.push((*name).to_owned());
        }
        if genres.len() >= 3 {
            break;
        }
    }
    genres
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Popularity;

    fn thumbnail(width: u32, height: u32) -> Thumbnail {
        Thumbnail {
            source: "https://example.test/poster.jpg".into(),
            width,
            height,
        }
    }

    #[test]
    fn cleans_disambiguated_titles() {
        assert_eq!(clean_title("Any Day Now (2024 film)"), "Any Day Now");
        assert_eq!(clean_title("Foo (film)"), "Foo");
        assert_eq!(clean_title("Heat (1995 movie)"), "Heat");
        assert_eq!(clean_title("Heat (1995)"), "Heat (1995)");
    }

    #[test]
    fn parses_years_out_of_descriptions() {
        assert_eq!(parse_year("2024 American drama film"), Some(2024));
        assert_eq!(parse_year("film by someone"), None);
        assert_eq!(parse_year("1234 nonsense"), None);
    }

    #[test]
    fn filters_television_and_non_film_pages() {
        assert!(is_bad_description("2024 American television series"));
        assert!(is_bad_description("British film awards"));
        assert!(!is_bad_description("2024 American drama film"));
    }

    #[test]
    fn keeps_only_portrait_images() {
        assert!(is_portrait(&thumbnail(400, 600)));
        assert!(!is_portrait(&thumbnail(600, 400)));
        assert!(!is_portrait(&thumbnail(20, 40)));
    }

    #[test]
    fn extracts_genres() {
        assert_eq!(
            genres_from_description("2024 American science fiction horror film"),
            vec!["Horror", "Sci-Fi"]
        );
        assert_eq!(genres_from_description("2024 film"), Vec::<String>::new());
    }

    #[test]
    fn converts_page_to_movie() {
        let page = Page {
            pageid: 42,
            title: "Example (2020 film)".into(),
            description: Some("2020 American drama film".into()),
            thumbnail: Some(thumbnail(400, 600)),
            fullurl: Some("https://en.wikipedia.org/wiki/Example".into()),
            missing: false,
        };
        let movie = page_into_movie(page, 1999).unwrap();
        assert_eq!(movie.title, "Example");
        assert_eq!(movie.year, Some(2020));
        assert_eq!(movie.id, "wiki-42");
        assert_eq!(movie.genres, vec!["Drama"]);
    }

    #[tokio::test]
    #[ignore = "hits the live Wikipedia API"]
    async fn live_fetch_returns_movies_with_posters() {
        let client = Client::builder()
            .user_agent("random-movie-tui-test/0.1")
            .build()
            .unwrap();
        let provider = WikipediaProvider::new();
        let movies = provider
            .fetch(&client, 3, &Filters::default())
            .await
            .unwrap();
        assert!(!movies.is_empty());
        assert!(movies.iter().all(|movie| movie.poster_url.is_some()));
        assert!(movies.iter().all(|movie| movie.source == Source::Wikipedia));
    }

    #[test]
    fn builds_language_category_titles() {
        assert_eq!(category_title(2024, Language::Any), "Category:2024 films");
        assert_eq!(
            category_title(2024, Language::Japanese),
            "Category:2024 Japanese-language films"
        );
    }

    #[test]
    fn wikipedia_summary_omits_unsupported_popularity() {
        let filters = Filters {
            year_range: Some(YearRange {
                from: 1990,
                to: 1999,
            }),
            language: Language::Japanese,
            popularity: Popularity::Popular,
        };
        assert_eq!(wikipedia_summary(&filters), "1990–1999 · Japanese");
    }

    #[test]
    fn picks_years_inside_the_range() {
        let range = YearRange {
            from: 1990,
            to: 1999,
        };
        let used = HashSet::new();
        for _ in 0..200 {
            let year = pick_year(&range, &used, false);
            assert!((range.from..=range.to).contains(&year));
        }
    }

    #[test]
    fn picks_the_only_year_in_a_single_year_range() {
        let range = YearRange {
            from: 1999,
            to: 1999,
        };
        let used = HashSet::new();
        assert_eq!(pick_year(&range, &used, false), 1999);
    }

    #[test]
    fn rejects_pages_without_portrait_poster() {
        let page = Page {
            pageid: 1,
            title: "Example".into(),
            description: Some("2020 film".into()),
            thumbnail: None,
            fullurl: None,
            missing: false,
        };
        assert!(page_into_movie(page, 2020).is_none());
    }
}
