use anyhow::{Context, Result, bail};
use image::DynamicImage;
use reqwest::Client;

use crate::{cache::PosterCache, movie::Movie};

const MAX_POSTER_BYTES: usize = 15 * 1024 * 1024;
const MAX_WIDTH: u32 = 800;
const MAX_HEIGHT: u32 = 1200;

pub async fn load_poster(
    client: &Client,
    cache: &PosterCache,
    movie: &Movie,
) -> Result<DynamicImage> {
    let cached = match cache.load(&movie.id).await {
        Some(bytes) => decode(bytes).await.ok(),
        None => None,
    };
    if let Some(image) = cached {
        return Ok(image);
    }

    let url = movie.poster_url.as_deref().context("movie has no poster")?;

    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("could not download poster for {}", movie.title))?;
    let status = response.status();
    if !status.is_success() {
        bail!("poster download failed for {} (HTTP {status})", movie.title);
    }
    let bytes = response
        .bytes()
        .await
        .context("could not read poster response")?;
    if bytes.len() > MAX_POSTER_BYTES {
        bail!("poster for {} is unexpectedly large", movie.title);
    }

    cache.store(&movie.id, &bytes).await;
    decode(bytes.to_vec()).await
}

async fn decode(bytes: Vec<u8>) -> Result<DynamicImage> {
    tokio::task::spawn_blocking(move || {
        let image = image::load_from_memory(&bytes).context("invalid image data")?;
        if image.width() > MAX_WIDTH || image.height() > MAX_HEIGHT {
            Ok(image.thumbnail(MAX_WIDTH, MAX_HEIGHT))
        } else {
            Ok(image)
        }
    })
    .await
    .context("image decoding task failed")?
}
