//! Concurrent native resolver with Piped and yt-dlp fallbacks.
//! Spotify credentials never enter this module.

use anyhow::{Result, anyhow};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;

use super::AlternateConfig;
use super::bundle;
use super::matching::{Candidate, TrackQuery, rank_candidates};
use super::native_youtube::NativeYoutube;
use super::piped::PipedClient;
use super::streams::AudioStream;
use super::ytdlp::YtDlp;

const NATIVE_SEARCH_TIMEOUT: Duration = Duration::from_secs(6);
const PIPED_SEARCH_TIMEOUT: Duration = Duration::from_secs(5);
const STREAM_TIMEOUT: Duration = Duration::from_secs(7);
const YTDLP_FALLBACK_TIMEOUT: Duration = Duration::from_secs(18);
const STRONG_MATCH_SCORE: f32 = 0.90;
const SEARCH_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const STREAM_CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const SEARCH_CACHE_CAPACITY: usize = 256;
const STREAM_CACHE_CAPACITY: usize = 256;

pub type LookupFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    NativeYoutube,
    Piped,
    YtDlpYoutube,
}

impl ProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::NativeYoutube => "YouTube match · not Spotify audio",
            Self::Piped => "Piped match · not Spotify audio",
            Self::YtDlpYoutube => "yt-dlp YouTube match · not Spotify audio",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreamLookup {
    pub streams: Vec<AudioStream>,
    pub provider: ProviderKind,
}

pub trait MediaLookup: Send + Sync {
    fn search(
        &self,
        query: &TrackQuery,
        min_score: f32,
    ) -> LookupFuture<Result<Vec<Candidate>, String>>;
    fn streams(&self, id: &str) -> LookupFuture<Result<StreamLookup, String>>;

    fn refresh_streams(&self, id: &str) -> LookupFuture<Result<StreamLookup, String>> {
        self.streams(id)
    }

    fn canned_audio(&self) -> Option<Vec<u8>> {
        None
    }

    fn scripted_body(&self) -> Option<ScriptedBody> {
        None
    }
}

#[derive(Clone, Debug)]
pub struct ScriptedBody {
    pub chunks: Vec<Vec<u8>>,
    pub fail: Option<String>,
    pub content_length: Option<u64>,
    pub fail_after_ms: u64,
}

#[derive(Clone)]
pub struct Resolver {
    native: NativeYoutube,
    piped: Option<PipedClient>,
    ytdlp: Option<YtDlp>,
    search_cache: std::sync::Arc<Mutex<TimedCache<Vec<Candidate>>>>,
    stream_cache: std::sync::Arc<Mutex<TimedCache<StreamLookup>>>,
}

impl Resolver {
    pub fn from_config(
        config: &AlternateConfig,
        ytdlp_dir: &Path,
        http: reqwest::Client,
    ) -> Result<Self, String> {
        config.validate()?;
        let native = NativeYoutube::new(http.clone()).map_err(|error| error.to_string())?;
        let piped = config
            .piped_api_base
            .as_deref()
            .map(|base| PipedClient::new(base, http.clone()))
            .transpose()
            .map_err(|error| error.to_string())?;
        let want_ytdlp =
            bundle::has_bundled_ytdlp() || bundle::user_ytdlp_present(config.ytdlp_path.as_deref());
        let ytdlp = if want_ytdlp {
            match bundle::resolve(config.ytdlp_path.as_deref(), ytdlp_dir) {
                Some(resolved) => {
                    bundle::log_choice(&resolved);
                    Some(YtDlp::new(resolved.path))
                }
                None => None,
            }
        } else {
            None
        };
        Ok(Self {
            native,
            piped,
            ytdlp,
            search_cache: std::sync::Arc::new(Mutex::new(TimedCache::new(
                SEARCH_CACHE_TTL,
                SEARCH_CACHE_CAPACITY,
            ))),
            stream_cache: std::sync::Arc::new(Mutex::new(TimedCache::new(
                STREAM_CACHE_TTL,
                STREAM_CACHE_CAPACITY,
            ))),
        })
    }

    pub async fn search(&self, query: &TrackQuery, min_score: f32) -> Result<Vec<Candidate>> {
        let search_text = search_text(query);
        let cache_key = format!(
            "{}|{}",
            search_text.to_lowercase(),
            query.duration_ms.unwrap_or(0)
        );
        if let Some(cached) = self
            .search_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&cache_key)
        {
            return Ok(cached);
        }

        let mut tasks = JoinSet::new();
        let native = self.native.clone();
        let native_query = search_text.clone();
        tasks.spawn(async move {
            (
                ProviderKind::NativeYoutube,
                timed_search(NATIVE_SEARCH_TIMEOUT, native.search(&native_query)).await,
            )
        });
        if let Some(piped) = self.piped.clone() {
            let piped_query = search_text.clone();
            tasks.spawn(async move {
                (
                    ProviderKind::Piped,
                    timed_search(PIPED_SEARCH_TIMEOUT, piped.search(&piped_query)).await,
                )
            });
        }

        let race =
            collect_search_results(&mut tasks, query, min_score.max(STRONG_MATCH_SCORE)).await;
        let mut candidates = race.candidates;
        let mut answered = race.answered;

        if rank_candidates(query, &candidates, min_score).is_none()
            && let Some(ytdlp) = &self.ytdlp
        {
            match tokio::time::timeout(YTDLP_FALLBACK_TIMEOUT, ytdlp.search(&search_text)).await {
                Ok(Ok(items)) => {
                    answered = true;
                    candidates.extend(items);
                    deduplicate(&mut candidates);
                }
                Ok(Err(error)) => log::warn!("yt-dlp fallback search failed: {error}"),
                Err(_) => log::warn!("yt-dlp fallback search timed out"),
            }
        }

        if candidates.is_empty() && !answered {
            return Err(anyhow!("no alternate search provider answered"));
        }
        self.search_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(cache_key, candidates.clone());
        Ok(candidates)
    }

    pub async fn streams(&self, id: &str) -> Result<StreamLookup> {
        if let Some(cached) = self
            .stream_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
        {
            return Ok(cached);
        }
        self.resolve_streams(id, false).await
    }

    async fn refresh_streams(&self, id: &str) -> Result<StreamLookup> {
        self.stream_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(id);
        self.resolve_streams(id, true).await
    }

    async fn resolve_streams(&self, id: &str, refresh: bool) -> Result<StreamLookup> {
        let resolved = self.resolve_youtube(id).await?;
        if !refresh || !resolved.streams.is_empty() {
            self.stream_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(id.to_string(), resolved.clone());
        }
        Ok(resolved)
    }

    async fn resolve_youtube(&self, id: &str) -> Result<StreamLookup> {
        match tokio::time::timeout(STREAM_TIMEOUT, self.native.streams(id)).await {
            Ok(Ok(streams)) if !streams.is_empty() => {
                return Ok(StreamLookup {
                    streams,
                    provider: ProviderKind::NativeYoutube,
                });
            }
            Ok(Ok(_)) => log::debug!("native YouTube returned no streams"),
            Ok(Err(error)) => log::warn!("native YouTube streams failed: {error}"),
            Err(_) => log::warn!("native YouTube streams timed out"),
        }
        if let Some(piped) = &self.piped {
            match tokio::time::timeout(STREAM_TIMEOUT, piped.streams(id)).await {
                Ok(Ok(streams)) if !streams.is_empty() => {
                    return Ok(StreamLookup {
                        streams,
                        provider: ProviderKind::Piped,
                    });
                }
                Ok(Ok(_)) => log::debug!("Piped returned no streams"),
                Ok(Err(error)) => log::warn!("Piped streams failed: {error}"),
                Err(_) => log::warn!("Piped streams timed out"),
            }
        }
        let ytdlp = self
            .ytdlp
            .as_ref()
            .ok_or_else(|| anyhow!("no YouTube stream provider answered"))?;
        let streams = tokio::time::timeout(YTDLP_FALLBACK_TIMEOUT, ytdlp.streams(id))
            .await
            .map_err(|_| anyhow!("yt-dlp stream fallback timed out"))??;
        Ok(StreamLookup {
            streams,
            provider: ProviderKind::YtDlpYoutube,
        })
    }
}

impl MediaLookup for Resolver {
    fn search(
        &self,
        query: &TrackQuery,
        min_score: f32,
    ) -> LookupFuture<Result<Vec<Candidate>, String>> {
        let this = self.clone();
        let query = query.clone();
        Box::pin(async move {
            Resolver::search(&this, &query, min_score)
                .await
                .map_err(|error| error.to_string())
        })
    }

    fn streams(&self, id: &str) -> LookupFuture<Result<StreamLookup, String>> {
        let this = self.clone();
        let id = id.to_string();
        Box::pin(async move {
            Resolver::streams(&this, &id)
                .await
                .map_err(|error| error.to_string())
        })
    }

    fn refresh_streams(&self, id: &str) -> LookupFuture<Result<StreamLookup, String>> {
        let this = self.clone();
        let id = id.to_string();
        Box::pin(async move {
            Resolver::refresh_streams(&this, &id)
                .await
                .map_err(|error| error.to_string())
        })
    }
}

struct SearchRace {
    candidates: Vec<Candidate>,
    answered: bool,
}

type SearchTaskResult = (ProviderKind, Result<Vec<Candidate>, String>);

async fn collect_search_results(
    tasks: &mut JoinSet<SearchTaskResult>,
    query: &TrackQuery,
    strong_score: f32,
) -> SearchRace {
    let mut candidates = Vec::new();
    let mut answered = false;
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok((provider, Ok(items))) => {
                answered = true;
                if items.is_empty() {
                    log::debug!("{provider:?} search returned no candidates");
                } else {
                    candidates.extend(items);
                    deduplicate(&mut candidates);
                    if rank_candidates(query, &candidates, strong_score).is_some() {
                        while let Some(Ok((_provider, Ok(items)))) = tasks.try_join_next() {
                            candidates.extend(items);
                        }
                        tasks.abort_all();
                        deduplicate(&mut candidates);
                        break;
                    }
                }
            }
            Ok((provider, Err(error))) => log::warn!("{provider:?} search failed: {error}"),
            Err(error) if error.is_cancelled() => {}
            Err(error) => log::warn!("alternate search task failed: {error}"),
        }
    }
    SearchRace {
        candidates,
        answered,
    }
}

async fn timed_search<F>(timeout: Duration, future: F) -> Result<Vec<Candidate>, String>
where
    F: Future<Output = anyhow::Result<Vec<Candidate>>>,
{
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| "provider timed out".to_string())?
        .map_err(|error| error.to_string())
}

fn search_text(query: &TrackQuery) -> String {
    let mut parts = query.artists.clone();
    parts.push(query.title.clone());
    parts.join(" ")
}

fn deduplicate(candidates: &mut Vec<Candidate>) {
    let mut ids = HashSet::new();
    candidates.retain(|candidate| ids.insert(candidate.id.clone()));
}

#[derive(Debug)]
struct CacheEntry<T> {
    value: T,
    expires_at: Instant,
    inserted_at: Instant,
}

#[derive(Debug)]
struct TimedCache<T> {
    entries: HashMap<String, CacheEntry<T>>,
    ttl: Duration,
    capacity: usize,
}

impl<T: Clone> TimedCache<T> {
    fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
            capacity,
        }
    }

    fn get(&mut self, key: &str) -> Option<T> {
        let now = Instant::now();
        self.entries.retain(|_, entry| entry.expires_at > now);
        self.entries.get(key).map(|entry| entry.value.clone())
    }

    fn insert(&mut self, key: String, value: T) {
        if self.capacity == 0 {
            return;
        }
        let now = Instant::now();
        self.entries.retain(|_, entry| entry.expires_at > now);
        if !self.entries.contains_key(&key)
            && self.entries.len() >= self.capacity
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.inserted_at)
                .map(|(key, _)| key.clone())
        {
            self.entries.remove(&oldest);
        }
        self.entries.insert(
            key,
            CacheEntry {
                value,
                expires_at: now + self.ttl,
                inserted_at: now,
            },
        );
    }

    fn remove(&mut self, key: &str) {
        self.entries.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_is_bounded_and_expires() {
        let mut cache = TimedCache::new(Duration::from_millis(1), 2);
        cache.insert("a".into(), 1);
        cache.insert("b".into(), 2);
        cache.insert("c".into(), 3);
        assert!(cache.entries.len() <= 2);
        std::thread::sleep(Duration::from_millis(3));
        assert_eq!(cache.get("c"), None);
    }

    #[test]
    fn provider_labels_name_the_real_audio_route() {
        assert!(ProviderKind::NativeYoutube.label().starts_with("YouTube"));
        assert!(ProviderKind::Piped.label().starts_with("Piped"));
        assert!(ProviderKind::YtDlpYoutube.label().starts_with("yt-dlp"));
    }

    #[tokio::test]
    async fn strong_match_cancels_the_slow_provider() {
        let query = TrackQuery {
            title: "Song".into(),
            artists: vec!["Artist".into()],
            duration_ms: Some(1_000),
        };
        let mut tasks: JoinSet<SearchTaskResult> = JoinSet::new();
        tasks.spawn(async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            (ProviderKind::Piped, Ok(Vec::new()))
        });
        tasks.spawn(async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            (
                ProviderKind::NativeYoutube,
                Ok(vec![Candidate {
                    id: "dQw4w9WgXcQ".into(),
                    title: "Song".into(),
                    uploader: "Artist - Topic".into(),
                    duration_ms: Some(1_000),
                }]),
            )
        });
        let started = Instant::now();
        let result = collect_search_results(&mut tasks, &query, 0.90).await;
        assert!(started.elapsed() < Duration::from_millis(500));
        assert_eq!(result.candidates.len(), 1);
        assert!(result.answered);
    }
}
