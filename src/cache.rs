use std::path::PathBuf;

use crate::config;

#[derive(Clone)]
pub struct PosterCache {
    dir: PathBuf,
}

impl PosterCache {
    pub fn new() -> Self {
        Self {
            dir: config::cache_dir().join("posters"),
        }
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.img", sanitize_id(id)))
    }

    pub async fn load(&self, id: &str) -> Option<Vec<u8>> {
        let path = self.path_for(id);
        match tokio::fs::read(&path).await {
            Ok(bytes) if !bytes.is_empty() => Some(bytes),
            _ => None,
        }
    }

    pub async fn store(&self, id: &str, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let _ = tokio::fs::create_dir_all(&self.dir).await;
        let path = self.path_for(id);
        let tmp = self
            .dir
            .join(format!(".{}.tmp-{}", sanitize_id(id), fastrand::u64(..)));
        if tokio::fs::write(&tmp, bytes).await.is_ok()
            && tokio::fs::rename(&tmp, &path).await.is_err()
        {
            let _ = tokio::fs::remove_file(&tmp).await;
        }
    }
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_safe_for_filenames() {
        assert_eq!(sanitize_id("tmdb-123"), "tmdb-123");
        assert_eq!(sanitize_id("wiki-12/34"), "wiki-12_34");
        assert_eq!(sanitize_id("../evil"), "___evil");
    }
}
