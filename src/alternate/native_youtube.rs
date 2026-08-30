//! Native asynchronous YouTube search and stream extraction.
//!
//! This is the normal YouTube path. Piped and yt-dlp remain compatibility
//! fallbacks because YouTube extraction changes independently of Oxidify.

use anyhow::{Context, Result};
use rusty_ytdl::search::{SearchOptions, SearchResult, YouTube};
use rusty_ytdl::{RequestOptions, Video, VideoOptions};

use super::matching::Candidate;
use super::piped::sanitize_video_id;
use super::streams::AudioStream;

const SEARCH_LIMIT: u64 = 8;

#[derive(Clone, Debug)]
pub struct NativeYoutube {
    http: reqwest::Client,
    search: YouTube,
}

impl NativeYoutube {
    pub fn new(http: reqwest::Client) -> Result<Self> {
        let options = RequestOptions {
            client: Some(http.clone()),
            max_retries: Some(1),
            ..RequestOptions::default()
        };
        let search = YouTube::new_with_options(&options)
            .context("unable to initialize native YouTube search")?;
        Ok(Self { http, search })
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

    pub async fn streams(&self, video_id: &str) -> Result<Vec<AudioStream>> {
        let id = sanitize_video_id(video_id).context("invalid YouTube video id")?;
        let options = VideoOptions {
            request_options: RequestOptions {
                client: Some(self.http.clone()),
                max_retries: Some(1),
                ..RequestOptions::default()
            },
            ..VideoOptions::default()
        };
        let video = Video::new_with_options(id, options).context("invalid YouTube video id")?;
        let info = video
            .get_basic_info()
            .await
            .context("native YouTube stream extraction failed")?;
        Ok(info
            .formats
            .into_iter()
            .filter(|format| format.has_audio)
            .map(|format| AudioStream {
                url: format.url,
                mime: Some(format.mime_type.mime.to_string()),
                codec: format.mime_type.audio_codec,
                format: Some(format.mime_type.container),
                bitrate: u32::try_from(format.average_bitrate.unwrap_or(format.bitrate)).ok(),
                video_only: format.has_video,
                quality: format.audio_quality,
                http_headers: Vec::new(),
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
