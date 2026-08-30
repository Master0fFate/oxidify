//! Native asynchronous YouTube search.
//!
//! Stream URLs still come from Piped or yt-dlp. rusty_ytdl lists AAC formats
//! but does not decrypt their URLs.

use anyhow::{Context, Result};
use rusty_ytdl::RequestOptions;
use rusty_ytdl::search::{SearchOptions, SearchResult, YouTube};

use super::matching::Candidate;
use super::piped::sanitize_video_id;

const SEARCH_LIMIT: u64 = 8;

#[derive(Clone, Debug)]
pub struct NativeYoutube {
    search: YouTube,
}

impl NativeYoutube {
    pub fn new(http: reqwest::Client) -> Result<Self> {
        let options = RequestOptions {
            client: Some(http),
            max_retries: Some(1),
            ..RequestOptions::default()
        };
        let search = YouTube::new_with_options(&options)
            .context("unable to initialize native YouTube search")?;
        Ok(Self { search })
    }

    pub async fn search(&self, query: &str) -> Result<Vec<Candidate>> {
        let options = SearchOptions {
            limit: SEARCH_LIMIT,
            ..SearchOptions::default()
        };
        let results = self
            .search
            .search(query, Some(&options))
            .await
            .context("native YouTube search failed")?;
        Ok(results
            .into_iter()
            .filter_map(|result| {
                let SearchResult::Video(video) = result else {
                    return None;
                };
                let id = sanitize_video_id(&video.id)?;
                if video.title.trim().is_empty() {
                    return None;
                }
                Some(Candidate {
                    id: id.to_string(),
                    title: video.title,
                    uploader: video.channel.name,
                    duration_ms: u32::try_from(video.duration).ok(),
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_limit_stays_bounded() {
        assert_eq!(SEARCH_LIMIT, 8);
    }
}
