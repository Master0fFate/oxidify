//! Concurrent native resolver with Piped and yt-dlp fallbacks.
//! Spotify credentials never enter this module.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::task::JoinSet;

use super::AlternateConfig;
use super::bundle;
use super::matching::{Candidate, TrackQuery, rank_candidates};
use super::native_youtube::NativeYoutube;
use super::piped::PipedClient;
use super::streams::{AudioStream, select_audio_stream};
use super::ytdlp::YtDlp;

const NATIVE_SEARCH_TIMEOUT: Duration = Duration::from_secs(6);
const PIPED_SEARCH_TIMEOUT: Duration = Duration::from_secs(5);
const STREAM_TIMEOUT: Duration = Duration::from_secs(7);
const YTDLP_FALLBACK_TIMEOUT: Duration = Duration::from_secs(18);
const STRONG_MATCH_SCORE: f32 = 0.90;
/// Matches outlive the process now that they are kept on disk; a week keeps
/// replays off the network while a removed video still gets re-searched.
const SEARCH_CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// For a stream URL without an `expire=` query parameter.
const STREAM_CACHE_TTL: Duration = Duration::from_secs(10 * 60);
/// A googlevideo URL is cached until its own `expire=` minus this margin.
const STREAM_EXPIRY_MARGIN: Duration = Duration::from_secs(5 * 60);
const SEARCH_CACHE_CAPACITY: usize = 2000;
const STREAM_CACHE_CAPACITY: usize = 256;
/// The on-disk copy of both caches, next to yt-dlp's own cache directory.
/// It holds search results and stream URLs only: no Spotify data beyond
/// the track title and artists used as the search key, and no tokens.
const MATCH_CACHE_FILE: &str = "alternate-matches.json";
const MATCH_CACHE_VERSION: u32 = 1;
const SAVE_DEBOUNCE: Duration = Duration::from_secs(1);

pub type LookupFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    search_cache: Arc<Mutex<TimedCache<Vec<Candidate>>>>,
    stream_cache: Arc<Mutex<TimedCache<StreamLookup>>>,
    /// Where the caches are written between runs; `None` keeps them in
    /// memory only (tests).
    store: Option<Arc<MatchStore>>,
}

struct MatchStore {
    path: PathBuf,
    debounce: Duration,
    dirty: AtomicBool,
    scheduled: AtomicBool,
}

impl MatchStore {
    fn new(path: PathBuf, debounce: Duration) -> Self {
        Self {
            path,
            debounce,
            dirty: AtomicBool::new(false),
            scheduled: AtomicBool::new(false),
        }
    }
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
                    Some(YtDlp::new(resolved.path, ytdlp_dir.join("ytdlp-cache")))
                }
                None => None,
            }
        } else {
            None
        };
        Ok(Self::assemble(
            native,
            piped,
            ytdlp,
            Some(MatchStore::new(
                ytdlp_dir.join(MATCH_CACHE_FILE),
                SAVE_DEBOUNCE,
            )),
        ))
    }

    fn assemble(
        native: NativeYoutube,
        piped: Option<PipedClient>,
        ytdlp: Option<YtDlp>,
        store: Option<MatchStore>,
    ) -> Self {
        let resolver = Self {
            native,
            piped,
            ytdlp,
            search_cache: Arc::new(Mutex::new(TimedCache::new(
                SEARCH_CACHE_TTL,
                SEARCH_CACHE_CAPACITY,
            ))),
            stream_cache: Arc::new(Mutex::new(TimedCache::new(
                STREAM_CACHE_TTL,
                STREAM_CACHE_CAPACITY,
            ))),
            store: store.map(Arc::new),
        };
        if let Some(store) = &resolver.store
            && let Some(persisted) = load_persisted(&store.path)
        {
            resolver.restore(persisted);
        }
        resolver
    }

    /// In-memory caches, optionally backed by `path`, with no providers
    /// beyond native search.
    #[cfg(test)]
    fn for_test(path: Option<PathBuf>, debounce: Duration) -> Self {
        let native = NativeYoutube::new(reqwest::Client::new()).expect("native search client");
        Self::assemble(
            native,
            None,
            None,
            path.map(|path| MatchStore::new(path, debounce)),
        )
    }

    fn remember_search(&self, key: String, candidates: Vec<Candidate>) {
        self.search_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, candidates);
        self.schedule_save();
    }

    fn remember_streams(&self, id: &str, lookup: &StreamLookup) {
        let ttl = stream_cache_ttl(lookup, SystemTime::now());
        if ttl.is_zero() {
            return;
        }
        self.stream_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert_with_ttl(id.to_string(), lookup.clone(), ttl);
        self.schedule_save();
    }

    /// Writes the caches after a quiet second; one write covers a burst.
    fn schedule_save(&self) {
        let Some(store) = &self.store else {
            return;
        };
        store.dirty.store(true, Ordering::SeqCst);
        if store.scheduled.swap(true, Ordering::SeqCst) {
            return;
        }
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let this = self.clone();
                let debounce = store.debounce;
                handle.spawn(async move {
                    tokio::time::sleep(debounce).await;
                    this.flush().await;
                });
            }
            Err(_) => self.flush_blocking(),
        }
    }

    async fn flush(&self) {
        let Some(store) = &self.store else {
            return;
        };
        store.scheduled.store(false, Ordering::SeqCst);
        if !store.dirty.swap(false, Ordering::SeqCst) {
            return;
        }
        let snapshot = self.snapshot();
        let path = store.path.clone();
        match tokio::task::spawn_blocking(move || write_atomic(&path, &snapshot)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => log::warn!("couldn't write the alternate match cache: {error}"),
            Err(error) => log::warn!("alternate match cache write did not finish: {error}"),
        }
    }

    fn flush_blocking(&self) {
        let Some(store) = &self.store else {
            return;
        };
        store.scheduled.store(false, Ordering::SeqCst);
        if !store.dirty.swap(false, Ordering::SeqCst) {
            return;
        }
        if let Err(error) = write_atomic(&store.path, &self.snapshot()) {
            log::warn!("couldn't write the alternate match cache: {error}");
        }
    }

    fn snapshot(&self) -> PersistedCaches {
        let now = Instant::now();
        let now_ms = unix_ms_now();
        let mut searches: Vec<PersistedSearch> = self
            .search_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entries()
            .into_iter()
            .map(
                |(key, candidates, expires_at, inserted_at)| PersistedSearch {
                    key,
                    inserted_at_ms: instant_to_unix_ms(inserted_at, now, now_ms),
                    expires_at_ms: instant_to_unix_ms(expires_at, now, now_ms),
                    candidates,
                },
            )
            .collect();
        searches.sort_by_key(|entry| std::cmp::Reverse(entry.inserted_at_ms));
        searches.truncate(SEARCH_CACHE_CAPACITY);
        let mut streams: Vec<PersistedStream> = self
            .stream_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entries()
            .into_iter()
            .filter_map(|(id, lookup, expires_at, inserted_at)| {
                // Only what this player can decode is worth keeping; a
                // provider lists every format it knows.
                let streams: Vec<AudioStream> = lookup
                    .streams
                    .iter()
                    .filter(|stream| select_audio_stream(std::slice::from_ref(*stream)).is_some())
                    .cloned()
                    .collect();
                (!streams.is_empty()).then(|| PersistedStream {
                    id,
                    inserted_at_ms: instant_to_unix_ms(inserted_at, now, now_ms),
                    expires_at_ms: instant_to_unix_ms(expires_at, now, now_ms),
                    lookup: StreamLookup {
                        streams,
                        provider: lookup.provider,
                    },
                })
            })
            .collect();
        streams.sort_by_key(|entry| std::cmp::Reverse(entry.inserted_at_ms));
        streams.truncate(STREAM_CACHE_CAPACITY);
        PersistedCaches {
            version: MATCH_CACHE_VERSION,
            searches,
            streams,
        }
    }

    fn restore(&self, persisted: PersistedCaches) {
        if persisted.version != MATCH_CACHE_VERSION {
            return;
        }
        let now = Instant::now();
        let now_ms = unix_ms_now();
        let mut searches = persisted.searches;
        searches.sort_by_key(|entry| entry.inserted_at_ms);
        {
            let mut cache = self.search_cache.lock().unwrap_or_else(|p| p.into_inner());
            for entry in searches {
                if entry.expires_at_ms <= now_ms || entry.key.is_empty() {
                    continue;
                }
                let (Some(expires_at), Some(inserted_at)) = (
                    unix_ms_to_instant(entry.expires_at_ms, now, now_ms),
                    unix_ms_to_instant(entry.inserted_at_ms, now, now_ms),
                ) else {
                    continue;
                };
                cache.restore(entry.key, entry.candidates, expires_at, inserted_at);
            }
        }
        let mut streams = persisted.streams;
        streams.sort_by_key(|entry| entry.inserted_at_ms);
        let mut cache = self.stream_cache.lock().unwrap_or_else(|p| p.into_inner());
        for entry in streams {
            if entry.expires_at_ms <= now_ms
                || entry.id.is_empty()
                || select_audio_stream(&entry.lookup.streams).is_none()
            {
                continue;
            }
            let (Some(expires_at), Some(inserted_at)) = (
                unix_ms_to_instant(entry.expires_at_ms, now, now_ms),
                unix_ms_to_instant(entry.inserted_at_ms, now, now_ms),
            ) else {
                continue;
            };
            cache.restore(entry.id, entry.lookup, expires_at, inserted_at);
        }
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
        self.remember_search(cache_key, candidates.clone());
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
        self.resolve_streams(id).await
    }

    async fn refresh_streams(&self, id: &str) -> Result<StreamLookup> {
        self.stream_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(id);
        self.resolve_streams(id).await
    }

    async fn resolve_streams(&self, id: &str) -> Result<StreamLookup> {
        let resolved = self.resolve_youtube(id).await?;
        if select_audio_stream(&resolved.streams).is_some() {
            self.remember_streams(id, &resolved);
        }
        Ok(resolved)
    }

    async fn resolve_youtube(&self, id: &str) -> Result<StreamLookup> {
        // rusty_ytdl lists AAC itag 140 but leaves the URL empty. Asking it
        // for streams only delays Piped / yt-dlp, which can decrypt the URL.
        let mut tasks: JoinSet<(ProviderKind, Result<Vec<AudioStream>, String>)> = JoinSet::new();
        if let Some(piped) = self.piped.clone() {
            let id = id.to_string();
            tasks.spawn(async move {
                let result = tokio::time::timeout(STREAM_TIMEOUT, piped.streams(&id))
                    .await
                    .map_err(|_| "Piped streams timed out".to_string())
                    .and_then(|result| result.map_err(|error| error.to_string()));
                (ProviderKind::Piped, result)
            });
        }
        if let Some(ytdlp) = self.ytdlp.clone() {
            let id = id.to_string();
            tasks.spawn(async move {
                let result = tokio::time::timeout(YTDLP_FALLBACK_TIMEOUT, ytdlp.streams(&id))
                    .await
                    .map_err(|_| "yt-dlp stream lookup timed out".to_string())
                    .and_then(|result| result.map_err(|error| error.to_string()));
                (ProviderKind::YtDlpYoutube, result)
            });
        }
        if tasks.is_empty() {
            return Err(anyhow!("no YouTube stream provider answered"));
        }
        let mut last_error = None;
        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok((provider, Ok(streams))) => {
                    if let Some(lookup) = playable_lookup(streams, provider) {
                        tasks.abort_all();
                        return Ok(lookup);
                    }
                    log::info!("{provider:?} had no AAC/M4A or MP3 URL");
                }
                Ok((provider, Err(error))) => {
                    log::warn!("{provider:?} streams failed: {error}");
                    last_error = Some(error);
                }
                Err(error) if error.is_cancelled() => {}
                Err(error) => last_error = Some(error.to_string()),
            }
        }
        Err(anyhow!(last_error.unwrap_or_else(|| {
            "No playable audio stream (need AAC/M4A or MP3; Opus/WebM is not decoded).".into()
        })))
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

fn playable_lookup(streams: Vec<AudioStream>, provider: ProviderKind) -> Option<StreamLookup> {
    select_audio_stream(&streams)
        .is_some()
        .then_some(StreamLookup { streams, provider })
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
        self.insert_with_ttl(key, value, self.ttl);
    }

    fn insert_with_ttl(&mut self, key: String, value: T, ttl: Duration) {
        let now = Instant::now();
        self.restore(key, value, now + ttl, now);
    }

    /// Adds an entry with its own clock, evicting expired ones and, when
    /// full, the oldest.
    fn restore(&mut self, key: String, value: T, expires_at: Instant, inserted_at: Instant) {
        if self.capacity == 0 {
            return;
        }
        let now = Instant::now();
        self.entries.retain(|_, entry| entry.expires_at > now);
        if expires_at <= now {
            return;
        }
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
                expires_at,
                inserted_at,
            },
        );
    }

    fn remove(&mut self, key: &str) {
        self.entries.remove(key);
    }

    /// Live entries as (key, value, expires_at, inserted_at).
    fn entries(&self) -> Vec<(String, T, Instant, Instant)> {
        let now = Instant::now();
        self.entries
            .iter()
            .filter(|(_, entry)| entry.expires_at > now)
            .map(|(key, entry)| {
                (
                    key.clone(),
                    entry.value.clone(),
                    entry.expires_at,
                    entry.inserted_at,
                )
            })
            .collect()
    }
}

/// `expire=<unix seconds>` from a stream URL's query, as googlevideo
/// hosts carry it.
fn url_expiry(url: &str) -> Option<SystemTime> {
    let (_, query) = url.split_once('?')?;
    let query = query.split('#').next().unwrap_or(query);
    let seconds: u64 = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("expire="))?
        .parse()
        .ok()?;
    UNIX_EPOCH.checked_add(Duration::from_secs(seconds))
}

/// How long a resolved lookup stays trusted: until the chosen stream's
/// own expiry less a margin, or the fixed TTL when the URL names none.
/// Zero means it is not worth caching.
fn stream_cache_ttl(lookup: &StreamLookup, now: SystemTime) -> Duration {
    let Some(expiry) =
        select_audio_stream(&lookup.streams).and_then(|stream| url_expiry(&stream.url))
    else {
        return STREAM_CACHE_TTL;
    };
    expiry
        .duration_since(now)
        .unwrap_or(Duration::ZERO)
        .saturating_sub(STREAM_EXPIRY_MARGIN)
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedCaches {
    version: u32,
    #[serde(default)]
    searches: Vec<PersistedSearch>,
    #[serde(default)]
    streams: Vec<PersistedStream>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedSearch {
    key: String,
    inserted_at_ms: u64,
    expires_at_ms: u64,
    candidates: Vec<Candidate>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedStream {
    id: String,
    inserted_at_ms: u64,
    expires_at_ms: u64,
    lookup: StreamLookup,
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            since.as_millis().min(u128::from(u64::MAX)) as u64
        })
}

fn instant_to_unix_ms(at: Instant, now: Instant, now_ms: u64) -> u64 {
    if at >= now {
        now_ms.saturating_add(at.duration_since(now).as_millis().min(u128::from(u64::MAX)) as u64)
    } else {
        now_ms.saturating_sub(now.duration_since(at).as_millis().min(u128::from(u64::MAX)) as u64)
    }
}

fn unix_ms_to_instant(ms: u64, now: Instant, now_ms: u64) -> Option<Instant> {
    if ms >= now_ms {
        now.checked_add(Duration::from_millis(ms - now_ms))
    } else {
        now.checked_sub(Duration::from_millis(now_ms - ms))
    }
}

fn load_persisted(path: &Path) -> Option<PersistedCaches> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            log::warn!("couldn't read the alternate match cache: {error}");
            return None;
        }
    };
    match serde_json::from_str::<PersistedCaches>(&text) {
        Ok(persisted) => Some(persisted),
        Err(error) => {
            log::warn!("ignoring an unreadable alternate match cache: {error}");
            None
        }
    }
}

/// Temp file beside the target, then rename, so a crash never leaves a
/// half-written cache.
fn write_atomic(path: &Path, caches: &PersistedCaches) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string(caches)?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, path)
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
    fn url_expiry_reads_the_googlevideo_query() {
        let url =
            "https://r1---sn-x.googlevideo.com/videoplayback?expire=1700000000&ei=abc&itag=140";
        assert_eq!(
            url_expiry(url),
            Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        );
        assert_eq!(url_expiry("https://cdn.example/a.m4a"), None);
        assert_eq!(url_expiry("https://cdn.example/a.m4a?expire=soon"), None);
        assert_eq!(url_expiry("https://cdn.example/a.m4a?expires=5"), None);
    }

    fn expiring_lookup(expire_in: Duration, now: SystemTime) -> StreamLookup {
        let seconds = now
            .checked_add(expire_in)
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        StreamLookup {
            streams: vec![stream(
                "m4a",
                "audio/mp4",
                "mp4a.40.2",
                &format!("https://r1.googlevideo.com/videoplayback?expire={seconds}&itag=140"),
                false,
            )],
            provider: ProviderKind::Piped,
        }
    }

    #[test]
    fn stream_url_expiry_sets_the_cache_ttl() {
        // Whole seconds, as the URL carries them.
        let now = UNIX_EPOCH
            + Duration::from_secs(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
        let hour = stream_cache_ttl(&expiring_lookup(Duration::from_secs(3_600), now), now);
        assert_eq!(hour, Duration::from_secs(3_600) - STREAM_EXPIRY_MARGIN);
        let soon = stream_cache_ttl(&expiring_lookup(Duration::from_secs(120), now), now);
        assert_eq!(soon, Duration::ZERO);
        let plain = StreamLookup {
            streams: vec![stream(
                "m4a",
                "audio/mp4",
                "mp4a.40.2",
                "https://cdn.example/140.m4a",
                false,
            )],
            provider: ProviderKind::YtDlpYoutube,
        };
        assert_eq!(stream_cache_ttl(&plain, now), STREAM_CACHE_TTL);

        let resolver = Resolver::for_test(None, Duration::ZERO);
        resolver.remember_streams("soon", &expiring_lookup(Duration::from_secs(120), now));
        resolver.remember_streams("later", &expiring_lookup(Duration::from_secs(3_600), now));
        let mut cache = resolver
            .stream_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        assert!(
            cache.get("soon").is_none(),
            "a URL about to expire is not cached"
        );
        assert!(cache.get("later").is_some());
    }

    fn scratch_file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("oxidify-match-cache-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("bin").join(MATCH_CACHE_FILE)
    }

    fn candidate(id: &str) -> Candidate {
        Candidate {
            id: id.into(),
            title: "Song".into(),
            uploader: "Artist - Topic".into(),
            duration_ms: Some(1_000),
        }
    }

    #[test]
    fn caches_round_trip_through_the_match_file() {
        let path = scratch_file("round-trip");
        let now = SystemTime::now();
        {
            let resolver = Resolver::for_test(Some(path.clone()), Duration::ZERO);
            resolver.remember_search("artist song|1000".into(), vec![candidate("dQw4w9WgXcQ")]);
            let mut lookup = expiring_lookup(Duration::from_secs(3_600), now);
            lookup.streams.push(stream(
                "webm",
                "audio/webm",
                "opus",
                "https://r1.googlevideo.com/videoplayback?itag=251",
                false,
            ));
            resolver.remember_streams("dQw4w9WgXcQ", &lookup);
            resolver
                .stream_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert_with_ttl("gone".into(), lookup.clone(), Duration::from_millis(40));
            // Outside a runtime the save is immediate, so "gone" is in the
            // file while still live; it must be dropped on the reload.
            resolver.schedule_save();
            std::thread::sleep(Duration::from_millis(60));
        }
        assert!(path.is_file(), "cache file was not written");
        assert!(
            !path.with_extension("json.tmp").exists(),
            "temporary file left behind"
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("itag=251"),
            "an undecodable stream was persisted"
        );

        let reloaded = Resolver::for_test(Some(path.clone()), Duration::ZERO);
        let searched = reloaded
            .search_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get("artist song|1000");
        assert_eq!(searched, Some(vec![candidate("dQw4w9WgXcQ")]));
        let mut streams = reloaded
            .stream_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let restored = streams.get("dQw4w9WgXcQ").expect("stream lookup restored");
        assert_eq!(restored.provider, ProviderKind::Piped);
        assert_eq!(restored.streams.len(), 1);
        assert!(restored.streams[0].url.contains("expire="));
        assert!(streams.get("gone").is_none(), "an expired entry came back");
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn unreadable_or_foreign_match_files_are_ignored() {
        let path = scratch_file("junk");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        let resolver = Resolver::for_test(Some(path.clone()), Duration::ZERO);
        assert!(
            resolver
                .search_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .entries()
                .is_empty()
        );

        let future = unix_ms_now() + 60_000;
        let foreign = PersistedCaches {
            version: MATCH_CACHE_VERSION + 1,
            searches: vec![PersistedSearch {
                key: "k".into(),
                inserted_at_ms: future - 1,
                expires_at_ms: future,
                candidates: vec![candidate("dQw4w9WgXcQ")],
            }],
            streams: Vec::new(),
        };
        write_atomic(&path, &foreign).unwrap();
        let resolver = Resolver::for_test(Some(path.clone()), Duration::ZERO);
        assert!(
            resolver
                .search_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get("k")
                .is_none(),
            "a cache from another version was loaded"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn persisted_entries_are_bounded_newest_first() {
        let path = scratch_file("bounded");
        let resolver = Resolver::for_test(Some(path.clone()), Duration::ZERO);
        let mut persisted = resolver.snapshot();
        let now_ms = unix_ms_now();
        for i in 0..(SEARCH_CACHE_CAPACITY as u64 + 50) {
            persisted.searches.push(PersistedSearch {
                key: format!("key-{i}"),
                inserted_at_ms: now_ms.saturating_sub(10_000 - i),
                expires_at_ms: now_ms + 60_000,
                candidates: vec![candidate("dQw4w9WgXcQ")],
            });
        }
        write_atomic(&path, &persisted).unwrap();
        let reloaded = Resolver::for_test(Some(path.clone()), Duration::ZERO);
        let snapshot = reloaded.snapshot();
        assert_eq!(snapshot.searches.len(), SEARCH_CACHE_CAPACITY);
        assert!(
            snapshot.searches.iter().all(|entry| {
                entry
                    .key
                    .strip_prefix("key-")
                    .and_then(|n| n.parse::<u64>().ok())
                    .is_some_and(|n| n >= 50)
            }),
            "the oldest entries should have been dropped"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[tokio::test]
    async fn saves_are_debounced_inside_the_runtime() {
        let path = scratch_file("debounced");
        let resolver = Resolver::for_test(Some(path.clone()), Duration::from_millis(30));
        resolver.remember_search("one|0".into(), vec![candidate("dQw4w9WgXcQ")]);
        resolver.remember_search("two|0".into(), vec![candidate("abcdefghijk")]);
        assert!(!path.exists(), "written before the quiet period");
        tokio::time::sleep(Duration::from_millis(150)).await;
        let persisted = load_persisted(&path).expect("cache written after the debounce");
        let mut keys: Vec<&str> = persisted
            .searches
            .iter()
            .map(|entry| entry.key.as_str())
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["one|0", "two|0"]);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    fn stream(format: &str, mime: &str, codec: &str, url: &str, video_only: bool) -> AudioStream {
        AudioStream {
            url: url.into(),
            mime: Some(mime.into()),
            codec: Some(codec.into()),
            format: Some(format.into()),
            bitrate: Some(128_000),
            video_only,
            quality: None,
            http_headers: Vec::new(),
        }
    }

    #[test]
    fn opus_and_empty_native_urls_fall_through() {
        let unusable = vec![
            stream("m4a", "audio/mp4", "mp4a.40.2", "", false),
            stream(
                "webm",
                "audio/webm",
                "opus",
                "https://cdn.example/a.webm",
                false,
            ),
            stream(
                "mp4",
                "video/mp4",
                "avc1",
                "https://cdn.example/18.mp4",
                true,
            ),
        ];
        assert!(playable_lookup(unusable, ProviderKind::NativeYoutube).is_none());
        let playable = vec![stream(
            "m4a",
            "audio/mp4",
            "mp4a.40.2",
            "https://cdn.example/140.m4a",
            false,
        )];
        let lookup = playable_lookup(playable, ProviderKind::NativeYoutube).unwrap();
        assert_eq!(lookup.provider, ProviderKind::NativeYoutube);
        assert_eq!(lookup.streams[0].format.as_deref(), Some("m4a"));
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
