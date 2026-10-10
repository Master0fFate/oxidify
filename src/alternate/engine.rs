//! Alternate local engine: resolve, fetch, decode, and drive the session.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use tokio::sync::{Notify as FormatNotify, mpsc, watch};
use tokio::task::JoinHandle;

use super::AlternateConfig;
use super::audio::{AudioOutput, OutputStatus, RodioOutput, volume_f32};
use super::buffer::SharedAudio;
use super::decode::{DecodeHandle, DurationProbe, FormatHint, PcmSource, spawn_decoder};
use super::fetch::{self, FetchPolicy};
use super::hydrate::{expand_load, offset_index, seed_queue};
use super::matching::{TrackQuery, rank_candidates};
use super::provider::{MediaLookup, Resolver};
use super::session::{Advance, Session};
use super::streams::select_audio_stream;
use crate::api::ApiClient;
use crate::player::{EngineEvent, LoadSpec, LocalTrack, Notify, Playback, PlayerCommand};
use crate::util::uri_kind;

const TICK: Duration = Duration::from_millis(50);
const MAX_MISS_SKIPS: u32 = 16;
const SEEK_MATERIAL_MS: u32 = 50;
const DEVICE_BACKOFF_INITIAL: Duration = Duration::from_millis(200);
const DEVICE_BACKOFF_CAP: Duration = Duration::from_secs(8);

enum Internal {
    Command(PlayerCommand),
    Job(Job),
    Shutdown,
    #[cfg(test)]
    TestLoad {
        tracks: Vec<LocalTrack>,
        play: bool,
        /// Behave as a seeded play: the list is a single clicked track
        /// and the rest of the context arrives with `TestHydrate`.
        seeded: bool,
    },
    #[cfg(test)]
    TestHydrate {
        tracks: Vec<LocalTrack>,
    },
}

enum Job {
    Hydrated {
        token: u64,
        spec: LoadSpec,
        result: Result<Vec<LocalTrack>, String>,
    },
    Canned {
        token: u64,
        uri: String,
        bytes: Vec<u8>,
        label: String,
        video_id: String,
    },
    MatchFailed {
        token: u64,
        uri: String,
        error: String,
    },
    Ready {
        token: u64,
        uri: String,
        buffer: SharedAudio,
        label: String,
        video_id: String,
        hint: FormatHint,
    },
    TransportFailed {
        token: u64,
        uri: String,
        error: String,
    },
}

struct PendingPlay {
    pcm: Option<PcmSource>,
    decode: Option<DecodeHandle>,
    label: String,
    start_ms: u32,
}

struct CachedMatch {
    video_id: String,
}

struct Prefetched {
    uri: String,
    buffer: SharedAudio,
    hint: FormatHint,
    label: String,
    video_id: String,
}

/// A background task and the track it resolves, so one can be aborted or
/// kept on its own.
struct Spawned {
    uri: Option<String>,
    handle: JoinHandle<()>,
}

pub struct AlternateHandle {
    tx: mpsc::UnboundedSender<Internal>,
    cancel: watch::Sender<bool>,
    join: std::sync::Mutex<Option<JoinHandle<()>>>,
}

impl AlternateHandle {
    pub fn command(&self, command: PlayerCommand) -> Result<()> {
        self.tx
            .send(Internal::Command(command))
            .map_err(|_| anyhow!("alternate engine is not running"))
    }

    pub async fn shutdown(&self) {
        let _ = self.cancel.send(true);
        let _ = self.tx.send(Internal::Shutdown);
        let join = self.join.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(join) = join {
            join.abort();
            let _ = join.await;
        }
    }

    #[cfg(test)]
    fn test_load(&self, tracks: Vec<LocalTrack>, play: bool) {
        let _ = self.tx.send(Internal::TestLoad {
            tracks,
            play,
            seeded: false,
        });
    }

    #[cfg(test)]
    fn test_load_seeded(&self, tracks: Vec<LocalTrack>, play: bool) {
        let _ = self.tx.send(Internal::TestLoad {
            tracks,
            play,
            seeded: true,
        });
    }

    #[cfg(test)]
    fn test_hydrate(&self, tracks: Vec<LocalTrack>) {
        let _ = self.tx.send(Internal::TestHydrate { tracks });
    }
}

pub fn spawn(
    config: AlternateConfig,
    api: Arc<ApiClient>,
    http: reqwest::Client,
    notify: Notify,
    output: Option<Box<dyn AudioOutput + Send>>,
    ytdlp_dir: PathBuf,
) -> Result<AlternateHandle, String> {
    config.validate()?;
    let lookup: Arc<dyn MediaLookup> =
        Arc::new(Resolver::from_config(&config, &ytdlp_dir, http.clone())?);
    let output = match output {
        Some(output) => output,
        None => Box::new(RodioOutput::open().map_err(|error| error.to_string())?),
    };
    Ok(spawn_inner(config, api, http, notify, output, lookup))
}

fn spawn_inner(
    config: AlternateConfig,
    api: Arc<ApiClient>,
    http: reqwest::Client,
    notify: Notify,
    output: Box<dyn AudioOutput + Send>,
    lookup: Arc<dyn MediaLookup>,
) -> AlternateHandle {
    let media_http = http;
    let (tx, rx) = mpsc::unbounded_channel();
    let (cancel, cancel_rx) = watch::channel(false);
    let join = tokio::spawn(run(
        config,
        api,
        media_http,
        notify,
        output,
        lookup,
        tx.clone(),
        rx,
        cancel_rx,
    ));
    AlternateHandle {
        tx,
        cancel,
        join: std::sync::Mutex::new(Some(join)),
    }
}

struct Engine {
    config: AlternateConfig,
    api: Arc<ApiClient>,
    media_http: reqwest::Client,
    notify: Notify,
    output: Box<dyn AudioOutput + Send>,
    lookup: Arc<dyn MediaLookup>,
    tx: mpsc::UnboundedSender<Internal>,
    cancel_rx: watch::Receiver<bool>,
    session: Session,
    matches: HashMap<String, CachedMatch>,
    play_generation: u64,
    jobs: Vec<Spawned>,
    /// A job started under an earlier generation that is still wanted:
    /// the prefetch adopted by a Next press, as (its token, its uri).
    adopted: Option<(u64, String)>,
    active_buffer: Option<SharedAudio>,
    active_hint: Option<FormatHint>,
    pending: Option<PendingPlay>,
    /// Where the playing audio's own length can be read, once decoded.
    active_duration: Option<DurationProbe>,
    overlap_uri: Option<String>,
    prefetch: Option<Prefetched>,
    /// The next track's resolve while it runs, before its `Ready` lands.
    prefetch_inflight: Option<String>,
    outgoing: Option<SharedAudio>,
    miss_skips: u32,
    seeded_play: bool,
    device_retry_at: Option<Instant>,
    device_backoff: Duration,
}

#[allow(clippy::too_many_arguments)]
async fn run(
    config: AlternateConfig,
    api: Arc<ApiClient>,
    media_http: reqwest::Client,
    notify: Notify,
    output: Box<dyn AudioOutput + Send>,
    lookup: Arc<dyn MediaLookup>,
    tx: mpsc::UnboundedSender<Internal>,
    mut commands: mpsc::UnboundedReceiver<Internal>,
    cancel_rx: watch::Receiver<bool>,
) {
    let volume = config.volume;
    let mut engine = Engine {
        config,
        api,
        media_http,
        notify,
        output,
        lookup,
        tx,
        cancel_rx: cancel_rx.clone(),
        session: Session::new(volume),
        matches: HashMap::new(),
        play_generation: 0,
        jobs: Vec::new(),
        adopted: None,
        active_buffer: None,
        active_hint: None,
        pending: None,
        active_duration: None,
        overlap_uri: None,
        prefetch: None,
        prefetch_inflight: None,
        outgoing: None,
        miss_skips: 0,
        seeded_play: false,
        device_retry_at: None,
        device_backoff: DEVICE_BACKOFF_INITIAL,
    };
    engine.output.set_volume(volume_f32(volume));
    engine.emit();
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut cancel_rx = cancel_rx;

    loop {
        // While a start is pending, the decoder's own signal that it has
        // read the headers starts the output at once instead of on the
        // next 50 ms tick.
        let format_ready = engine.pending_format_ready();
        tokio::select! {
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() {
                    break;
                }
            }
            command = commands.recv() => {
                match command {
                    None | Some(Internal::Shutdown) => break,
                    Some(Internal::Command(command)) => engine.handle_command(command),
                    Some(Internal::Job(job)) => engine.handle_job(job),
                    #[cfg(test)]
                    Some(Internal::TestLoad { tracks, play, seeded }) => {
                        engine.test_load(tracks, play, seeded)
                    }
                    #[cfg(test)]
                    Some(Internal::TestHydrate { tracks }) => engine.test_hydrate(tracks),
                }
            }
            _ = wait_format_ready(format_ready) => {
                engine.try_start_pending();
            }
            _ = tick.tick() => {
                engine.try_start_pending();
                engine.poll_output();
            }
        }
        if *cancel_rx.borrow() {
            break;
        }
    }
    engine.abort_jobs();
    engine.output.stop();
}

impl Engine {
    fn cancelled(&self) -> bool {
        *self.cancel_rx.borrow()
    }

    fn emit(&self) {
        if self.cancelled() {
            return;
        }
        (self.notify)(EngineEvent::State(self.session.snapshot()));
    }

    fn abort_jobs(&mut self) {
        for job in self.jobs.drain(..) {
            job.handle.abort();
        }
        self.adopted = None;
        self.prefetch_inflight = None;
    }

    /// Aborts every job but the one resolving `keep`, whose token stays
    /// accepted through `adopted` after the generation moves on.
    fn abort_jobs_except(&mut self, keep: &str) {
        let mut kept = Vec::new();
        for job in self.jobs.drain(..) {
            if job.uri.as_deref() == Some(keep) {
                kept.push(job);
            } else {
                job.handle.abort();
            }
        }
        self.jobs = kept;
        self.adopted = Some((self.play_generation, keep.to_string()));
    }

    fn abort_job(&mut self, uri: &str) {
        let mut kept = Vec::new();
        for job in self.jobs.drain(..) {
            if job.uri.as_deref() == Some(uri) {
                job.handle.abort();
            } else {
                kept.push(job);
            }
        }
        self.jobs = kept;
    }

    fn spawn_job(&mut self, uri: Option<String>, handle: JoinHandle<()>) {
        self.jobs.push(Spawned { uri, handle });
    }

    fn pending_format_ready(&self) -> Option<Arc<FormatNotify>> {
        self.pending
            .as_ref()
            .and_then(|pending| pending.decode.as_ref())
            .map(DecodeHandle::format_ready)
    }

    fn bump(&mut self) -> u64 {
        self.seeded_play = false;
        self.abort_jobs();
        self.active_hint = None;
        self.active_duration = None;
        self.overlap_uri = None;
        if let Some(prefetch) = self.prefetch.take() {
            prefetch.buffer.cancel();
        }
        if let Some(outgoing) = self.outgoing.take() {
            outgoing.cancel();
        }
        self.device_retry_at = None;
        self.device_backoff = DEVICE_BACKOFF_INITIAL;
        if let Some(buffer) = self.active_buffer.take() {
            buffer.cancel();
        }
        if let Some(pending) = self.pending.take()
            && let Some(decode) = pending.decode
        {
            decode.stop();
        }
        self.play_generation = self.play_generation.wrapping_add(1);
        self.play_generation
    }

    fn bump_jobs(&mut self) -> u64 {
        self.abort_jobs();
        self.play_generation = self.play_generation.wrapping_add(1);
        self.play_generation
    }

    fn handle_command(&mut self, command: PlayerCommand) {
        match command {
            PlayerCommand::Toggle => match self.session.toggle() {
                Advance::Stay => {
                    match self.session.playback() {
                        Playback::Paused => self.output.pause(),
                        Playback::Playing => self.output.resume(),
                        _ => {}
                    }
                    self.emit();
                }
                Advance::CancelLoad => {
                    self.bump();
                    self.output.stop();
                    self.emit();
                }
                other => self.follow(other),
            },
            PlayerCommand::Next => self.skip_keep_audio(true),
            PlayerCommand::Previous => {
                if self.session.position_now() > 3_000 {
                    self.session.seek(0);
                    let _ = self.output.seek(0);
                    self.emit();
                    return;
                }
                self.skip_keep_audio(false);
            }
            PlayerCommand::Seek(position_ms) => {
                self.session.seek(position_ms);
                if let Some(pending) = &mut self.pending {
                    pending.start_ms = position_ms;
                    if let Some(decode) = &pending.decode {
                        decode.seek(position_ms);
                    }
                }
                let _ = self.output.seek(position_ms);
                self.emit();
            }
            PlayerCommand::Volume(volume) | PlayerCommand::VolumePreview(volume) => {
                self.session.set_volume(volume);
                self.output.set_volume(volume_f32(volume));
                self.emit();
            }
            PlayerCommand::Shuffle(enabled) => {
                self.session.set_shuffle(enabled);
                self.emit();
                self.sync_prefetch();
            }
            PlayerCommand::Repeat(mode) => {
                self.session.set_repeat(mode);
                self.emit();
                self.sync_prefetch();
            }
            PlayerCommand::Activate => self.emit(),
            PlayerCommand::Stop => {
                self.bump();
                self.session.stop();
                self.output.stop();
                self.emit();
            }
            PlayerCommand::AddToQueue(track) => {
                if track.is_episode || uri_kind(&track.uri) == Some("episode") {
                    self.session
                        .set_error("Podcasts are not supported in alternate playback.".into());
                    self.emit();
                    return;
                }
                self.session.add_to_queue(track);
                self.emit();
                self.sync_prefetch();
            }
            PlayerCommand::Load(spec) => self.start_hydrate(spec),
        }
    }

    fn follow(&mut self, advance: Advance) {
        match advance {
            Advance::Stay => self.emit(),
            Advance::SeekZero => {
                let _ = self.output.seek(0);
                self.emit();
            }
            Advance::Stop => {
                self.output.stop();
                self.emit();
            }
            Advance::PlayCurrent => {
                let current = self.session.current().map(|track| track.uri.clone());
                if let Some(prefetch) = self.prefetch.take() {
                    if current.as_deref() == Some(prefetch.uri.as_str()) {
                        self.play_ready(prefetch);
                        return;
                    }
                    prefetch.buffer.cancel();
                }
                if self.prefetch_inflight.is_some() && self.prefetch_inflight == current {
                    // The track ended while its successor's resolve was
                    // still running: wait for that one instead of
                    // starting a second.
                    self.prefetch_inflight = None;
                    self.session.set_loading();
                    self.emit();
                    return;
                }
                self.start_resolve();
            }
            Advance::CancelLoad => {
                self.bump();
                self.output.stop();
                self.emit();
            }
        }
    }

    fn start_hydrate(&mut self, spec: LoadSpec) {
        let token = self.bump();
        let seed = seed_queue(&spec);
        if seed.is_empty() {
            self.session.set_loading();
            self.emit();
        } else {
            let offset = offset_index(&spec, &seed);
            self.session
                .load(seed, offset, spec.play, spec.shuffle, spec.position_ms);
            self.seeded_play = true;
            if spec.play {
                self.start_resolve();
            } else {
                self.output.stop();
                self.emit();
            }
        }
        let api = Arc::clone(&self.api);
        let tx = self.tx.clone();
        let mut cancel_rx = self.cancel_rx.clone();
        let job = tokio::spawn(async move {
            let result = tokio::select! {
                _ = wait_cancel(&mut cancel_rx) => return,
                result = expand_load(&api, &spec) => result,
            };
            let _ = tx.send(Internal::Job(Job::Hydrated {
                token,
                spec,
                result: result.map_err(|error| error.to_string()),
            }));
        });
        self.spawn_job(None, job);
    }

    fn skip_keep_audio(&mut self, forward: bool) {
        let next = if forward {
            self.session.peek_next().cloned()
        } else {
            self.session.peek_previous().cloned()
        };
        let Some(track) = next else {
            self.bump();
            let advance = if forward {
                self.session.skip_forward()
            } else {
                self.session.previous()
            };
            self.follow(advance);
            return;
        };
        if let Some(prefetch) = self.prefetch.take()
            && prefetch.uri == track.uri
        {
            self.bump_jobs();
            if forward {
                self.session.skip_forward();
            } else {
                self.session.previous();
            }
            self.play_ready(prefetch);
            return;
        }
        if let Some(prefetch) = self.prefetch.take() {
            prefetch.buffer.cancel();
        }
        if self.prefetch_inflight.as_deref() == Some(track.uri.as_str()) {
            // Its resolve is already running: keep it as the transition
            // instead of aborting it and starting from the search again.
            self.prefetch_inflight = None;
            self.abort_jobs_except(&track.uri);
            self.play_generation = self.play_generation.wrapping_add(1);
            self.overlap_uri = Some(track.uri);
            return;
        }
        self.bump_jobs();
        self.overlap_uri = Some(track.uri.clone());
        self.start_resolve_track(track, false);
    }

    fn start_resolve(&mut self) {
        let Some(track) = self.session.current().cloned() else {
            self.session.stop();
            self.output.stop();
            self.emit();
            return;
        };
        self.start_resolve_track(track, true);
    }

    fn start_resolve_track(&mut self, track: LocalTrack, loading: bool) {
        if track.is_episode {
            self.fail_or_skip("Podcasts are not supported in alternate playback.".into());
            return;
        }
        let token = self.play_generation;
        if loading {
            self.session.set_loading();
            self.emit();
        }
        let lookup = Arc::clone(&self.lookup);
        let http = self.media_http.clone();
        let config = self.config.clone();
        let cached = self
            .matches
            .get(&track.uri)
            .map(|entry| entry.video_id.clone());
        let tx = self.tx.clone();
        let mut cancel_rx = self.cancel_rx.clone();
        let uri = track.uri.clone();
        let job = tokio::spawn(async move {
            tokio::select! {
                _ = wait_cancel(&mut cancel_rx) => {}
                _ = resolve_and_stream(
                    &config,
                    lookup.as_ref(),
                    &http,
                    cached,
                    &track,
                    token,
                    &tx,
                ) => {}
            }
        });
        self.spawn_job(Some(uri), job);
    }

    fn handle_job(&mut self, job: Job) {
        if self.cancelled() {
            return;
        }
        match job {
            Job::Hydrated {
                token,
                spec,
                result,
            } => {
                if token != self.play_generation {
                    return;
                }
                self.on_hydrated(spec, result);
            }
            Job::Canned {
                token,
                uri,
                bytes,
                label,
                video_id,
            } => {
                if !self.job_accepts(token, &uri) {
                    return;
                }
                self.cache_match(uri, video_id);
                let start = self.session.position_now();
                match self.output.play_bytes(bytes, start) {
                    Ok(info) => {
                        self.miss_skips = 0;
                        self.session.set_playing(Some(label));
                        if let Some(ms) = info.duration_ms {
                            self.session.set_current_duration(ms);
                        }
                        self.emit();
                    }
                    Err(error) => self.fail_transport(format!("Couldn't decode audio: {error}")),
                }
            }
            Job::MatchFailed { token, uri, error } => {
                if !self.job_accepts(token, &uri) {
                    return;
                }
                self.note_job_done(&uri);
                if self.drop_failed_prefetch(&uri, &error) {
                    return;
                }
                if self.overlap_uri.as_deref() == Some(uri.as_str()) {
                    self.overlap_uri = None;
                    self.session.set_error(error);
                    self.emit();
                    return;
                }
                self.fail_or_skip(error);
            }
            Job::Ready {
                token,
                uri,
                buffer,
                label,
                video_id,
                hint,
            } => {
                if !self.job_accepts(token, &uri) {
                    buffer.cancel();
                    return;
                }
                self.note_job_done(&uri);
                if self.prefetch_inflight.as_deref() == Some(uri.as_str()) {
                    self.prefetch_inflight = None;
                }
                let overlapping = self.overlap_uri.as_deref() == Some(uri.as_str());
                let prefetching = !overlapping
                    && self.session.current().map(|track| track.uri.as_str()) != Some(uri.as_str())
                    && self.config.gapless
                    && self.session.peek_next().map(|track| track.uri.as_str())
                        == Some(uri.as_str());
                if prefetching {
                    self.cache_match(uri.clone(), video_id.clone());
                    self.prefetch = Some(Prefetched {
                        uri,
                        buffer,
                        hint,
                        label,
                        video_id,
                    });
                    return;
                }
                if overlapping {
                    let _ = self.session.select_uri(&uri);
                    self.overlap_uri = None;
                }
                self.play_ready(Prefetched {
                    uri,
                    buffer,
                    hint,
                    label,
                    video_id,
                });
            }
            Job::TransportFailed { token, uri, error } => {
                if !self.job_accepts(token, &uri) {
                    return;
                }
                self.note_job_done(&uri);
                if self.drop_failed_prefetch(&uri, &error) {
                    return;
                }
                if self.overlap_uri.as_deref() == Some(uri.as_str()) {
                    self.overlap_uri = None;
                    self.session.set_error(error);
                    self.emit();
                    return;
                }
                self.fail_transport(error);
            }
        }
    }

    fn on_hydrated(&mut self, spec: LoadSpec, result: Result<Vec<LocalTrack>, String>) {
        match result {
            Ok(tracks) => {
                self.miss_skips = 0;
                let offset = offset_index(&spec, &tracks);
                if self.seeded_play {
                    let prev = self.session.current().map(|track| track.uri.clone());
                    self.session.adopt_tracks(tracks, offset);
                    let now = self.session.current().map(|track| track.uri.clone());
                    if spec.play && now != prev {
                        let _ = self.bump();
                        self.start_resolve();
                    } else {
                        self.emit();
                        // The clicked track may already be playing with
                        // nothing after it; now that the rest of the
                        // context is known, its successor can be fetched.
                        self.sync_prefetch();
                    }
                } else {
                    self.session
                        .load(tracks, offset, spec.play, spec.shuffle, spec.position_ms);
                    if spec.play {
                        self.start_resolve();
                    } else {
                        self.output.stop();
                        self.emit();
                    }
                }
            }
            Err(error) => {
                if self.seeded_play {
                    return;
                }
                self.session.set_error(error);
                self.emit();
            }
        }
    }

    /// A job finished for `uri`; an adopted token is spent with it.
    fn note_job_done(&mut self, uri: &str) {
        if self.adopted.as_ref().is_some_and(|(_, kept)| kept == uri) {
            self.adopted = None;
        }
    }

    /// A failure that belongs to the next track's prefetch costs only the
    /// prefetch; the track that is playing is not touched. It is resolved
    /// again, and its miss handled, when it becomes current.
    fn drop_failed_prefetch(&mut self, uri: &str, error: &str) -> bool {
        if self.prefetch_inflight.as_deref() != Some(uri)
            || self.overlap_uri.as_deref() == Some(uri)
            || self.session.current().map(|track| track.uri.as_str()) == Some(uri)
        {
            return false;
        }
        log::info!("alternate prefetch dropped: {error}");
        self.prefetch_inflight = None;
        self.abort_job(uri);
        true
    }

    fn job_accepts(&self, token: u64, uri: &str) -> bool {
        let adopted = self
            .adopted
            .as_ref()
            .is_some_and(|(kept_token, kept)| *kept_token == token && kept == uri);
        if token != self.play_generation && !adopted {
            return false;
        }
        if self.session.current().map(|track| track.uri.as_str()) == Some(uri) {
            return true;
        }
        if self.overlap_uri.as_deref() == Some(uri) {
            return true;
        }
        self.config.gapless && self.session.peek_next().map(|track| track.uri.as_str()) == Some(uri)
    }

    fn play_ready(&mut self, ready: Prefetched) {
        self.cache_match(ready.uri, ready.video_id);
        self.outgoing = self.active_buffer.replace(ready.buffer.clone());
        self.active_hint = Some(ready.hint.clone());
        let start_ms = self.session.position_now();
        match spawn_decoder(ready.buffer, ready.hint, start_ms) {
            Ok((pcm, decode)) => {
                self.pending = Some(PendingPlay {
                    pcm: Some(pcm),
                    decode: Some(decode),
                    label: ready.label,
                    start_ms,
                });
                self.try_start_pending();
            }
            Err(error) => {
                if let Some(previous) = self.outgoing.take() {
                    self.active_buffer = Some(previous);
                }
                self.fail_transport(format!("Couldn't decode audio: {error}"));
            }
        }
    }

    fn maybe_prefetch_next(&mut self) {
        if !self.config.gapless
            || self.overlap_uri.is_some()
            || self.prefetch.is_some()
            || self.prefetch_inflight.is_some()
        {
            return;
        }
        let Some(next) = self.session.peek_next().cloned() else {
            return;
        };
        if next.is_episode
            || self.session.current().map(|track| track.uri.as_str()) == Some(next.uri.as_str())
        {
            return;
        }
        self.prefetch_inflight = Some(next.uri.clone());
        self.start_resolve_track(next, false);
    }

    /// Points the prefetch at whatever follows the current track now:
    /// after the queue, shuffle or repeat changed, or the context arrived.
    /// A prefetch for a track that no longer comes next is dropped; one is
    /// started when the current track is already going and none is held.
    fn sync_prefetch(&mut self) {
        if !self.config.gapless {
            return;
        }
        let next = self.session.peek_next().map(|track| track.uri.clone());
        if self
            .prefetch
            .as_ref()
            .is_some_and(|ready| Some(ready.uri.as_str()) != next.as_deref())
            && let Some(stale) = self.prefetch.take()
        {
            stale.buffer.cancel();
        }
        if let Some(inflight) = self.prefetch_inflight.clone()
            && Some(inflight.as_str()) != next.as_deref()
        {
            self.abort_job(&inflight);
            self.prefetch_inflight = None;
        }
        if self.pending.is_none()
            && self.active_buffer.is_some()
            && self.session.playback() != Playback::Loading
        {
            self.maybe_prefetch_next();
        }
    }

    fn cache_match(&mut self, uri: String, video_id: String) {
        if !video_id.is_empty() {
            self.matches.insert(uri, CachedMatch { video_id });
        }
    }

    fn try_start_pending(&mut self) {
        let decode_error = self
            .pending
            .as_ref()
            .and_then(|pending| pending.decode.as_ref())
            .and_then(DecodeHandle::error);
        if let Some(message) = decode_error {
            self.pending = None;
            self.fail_transport(format!("Couldn't decode audio: {message}"));
            return;
        }
        if matches!(self.output.status(), OutputStatus::DeviceLost) {
            return;
        }
        let ready = self
            .pending
            .as_ref()
            .and_then(|pending| pending.decode.as_ref())
            .is_some_and(|decode| decode.format().is_some());
        if !ready {
            return;
        }
        let Some(mut pending) = self.pending.take() else {
            return;
        };
        let (Some(pcm), Some(decode)) = (pending.pcm.take(), pending.decode.take()) else {
            return;
        };
        let now = self.session.position_now();
        if now.abs_diff(pending.start_ms) >= SEEK_MATERIAL_MS {
            decode.seek(now);
        }
        let probe = pcm.duration_probe();
        match self.output.play_pcm(pcm, decode) {
            Ok(info) => {
                self.miss_skips = 0;
                if let Some(previous) = self.outgoing.take() {
                    previous.cancel();
                }
                self.session.set_playing(Some(pending.label));
                self.active_duration = Some(probe);
                if let Some(ms) = info.duration_ms {
                    self.session.set_current_duration(ms);
                }
                self.adopt_media_duration();
                self.emit();
                self.maybe_prefetch_next();
            }
            Err(error) => self.fail_transport(format!("Couldn't start audio: {error}")),
        }
    }

    fn fail_or_skip(&mut self, message: String) {
        self.output.stop();
        self.session.set_error(message);
        self.emit();
        if self.config.skip_on_miss && self.miss_skips < MAX_MISS_SKIPS {
            self.miss_skips = self.miss_skips.saturating_add(1);
            match self.session.fail_next() {
                Advance::PlayCurrent => self.start_resolve(),
                other => self.follow(other),
            }
        }
    }

    fn fail_transport(&mut self, message: String) {
        self.bump();
        self.output.stop();
        self.session.set_error(message);
        self.emit();
    }

    /// The matched audio's own length, once the decoder has read its
    /// headers. Spotify's duration belongs to a different recording; the
    /// bar, and seeks on it, follow what is actually playing.
    fn adopt_media_duration(&mut self) {
        let Some(ms) = self
            .active_duration
            .as_ref()
            .and_then(|probe| probe.duration_ms())
        else {
            return;
        };
        if self.session.set_current_duration(ms) {
            self.emit();
        }
    }

    fn poll_output(&mut self) {
        self.adopt_media_duration();
        match self.output.status() {
            OutputStatus::DeviceLost => self.handle_device_lost(),
            OutputStatus::Buffering if self.session.playback() == Playback::Playing => {
                self.freeze_output_clock();
            }
            OutputStatus::Playing if self.session.playback() == Playback::Playing => {
                self.resume_output_clock();
            }
            OutputStatus::Ended if self.session.playback() == Playback::Playing => {
                let advance = self.session.on_ended();
                self.follow(advance);
            }
            OutputStatus::Failed(message) if self.session.playback() == Playback::Playing => {
                self.fail_transport(message);
            }
            OutputStatus::Playing
            | OutputStatus::Buffering
            | OutputStatus::Ended
            | OutputStatus::Failed(_) => {}
        }
    }

    fn freeze_output_clock(&mut self) {
        if self.session.clock_running() {
            self.session.freeze_clock();
            self.emit();
        }
    }

    fn resume_output_clock(&mut self) {
        if self.session.playback() == Playback::Playing && !self.session.clock_running() {
            self.session.resume_clock();
            self.emit();
        }
    }

    fn handle_device_lost(&mut self) {
        if self.session.playback() == Playback::Playing {
            self.freeze_output_clock();
        }
        if let Some(at) = self.device_retry_at
            && Instant::now() < at
        {
            return;
        }
        match self.output.recover() {
            Ok(()) => {
                self.device_retry_at = None;
                self.device_backoff = DEVICE_BACKOFF_INITIAL;
                if self.active_buffer.is_some()
                    && self.pending.is_none()
                    && let Err(error) = self.reattach_pcm()
                {
                    self.fail_transport(error);
                }
            }
            Err(_) => {
                self.device_retry_at = Some(Instant::now() + self.device_backoff);
                self.device_backoff = self
                    .device_backoff
                    .saturating_mul(2)
                    .min(DEVICE_BACKOFF_CAP);
            }
        }
    }

    fn reattach_pcm(&mut self) -> Result<(), String> {
        let buffer = self
            .active_buffer
            .clone()
            .ok_or_else(|| "Couldn't start audio.".to_string())?;
        let hint = self
            .active_hint
            .clone()
            .ok_or_else(|| "Couldn't start audio.".to_string())?;
        let pos = self.session.position_now();
        let paused = self.session.playback() == Playback::Paused;
        let (pcm, decode) = spawn_decoder(buffer, hint, pos)
            .map_err(|error| format!("Couldn't decode audio: {error}"))?;
        let probe = pcm.duration_probe();
        self.output
            .play_pcm(pcm, decode)
            .map_err(|error| format!("Couldn't start audio: {error}"))?;
        self.active_duration = Some(probe);
        if paused {
            self.output.pause();
        }
        if self.session.playback() == Playback::Playing {
            self.session.resume_clock();
            self.emit();
        }
        Ok(())
    }

    #[cfg(test)]
    fn test_load(&mut self, tracks: Vec<LocalTrack>, play: bool, seeded: bool) {
        self.bump();
        self.miss_skips = 0;
        self.session.load(tracks, 0, play, Some(false), 0);
        self.seeded_play = seeded;
        if play {
            self.start_resolve();
        } else {
            self.emit();
        }
    }

    /// The rest of a seeded context arriving, as `Job::Hydrated` would
    /// deliver it for the current generation.
    #[cfg(test)]
    fn test_hydrate(&mut self, tracks: Vec<LocalTrack>) {
        let spec = LoadSpec {
            context_uri: None,
            uris: tracks.iter().map(|track| track.uri.clone()).collect(),
            offset_uri: self.session.current().map(|track| track.uri.clone()),
            offset_index: None,
            position_ms: 0,
            play: true,
            shuffle: Some(false),
            known_tracks: Vec::new(),
        };
        self.on_hydrated(spec, Ok(tracks));
    }
}

async fn wait_format_ready(notify: Option<Arc<FormatNotify>>) {
    match notify {
        Some(notify) => notify.notified().await,
        None => std::future::pending().await,
    }
}

async fn wait_cancel(rx: &mut watch::Receiver<bool>) {
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn resolve_and_stream(
    config: &AlternateConfig,
    lookup: &dyn MediaLookup,
    http: &reqwest::Client,
    cached_id: Option<String>,
    track: &LocalTrack,
    token: u64,
    tx: &mpsc::UnboundedSender<Internal>,
) {
    let send = |job: Job| {
        let _ = tx.send(Internal::Job(job));
    };
    let query = TrackQuery {
        title: track.title.clone(),
        artists: track.artists.clone(),
        duration_ms: (track.duration_ms > 0).then_some(track.duration_ms),
    };
    let video_id = if let Some(id) = cached_id {
        id
    } else {
        match lookup.search(&query, config.min_score).await {
            Ok(candidates) => match rank_candidates(&query, &candidates, config.min_score) {
                Some(ranked) => ranked.candidate.id,
                None => {
                    send(Job::MatchFailed {
                        token,
                        uri: track.uri.clone(),
                        error: format!(
                            "No confident match for {} — {}",
                            track.title,
                            track.artist_names()
                        ),
                    });
                    return;
                }
            },
            Err(error) => {
                send(Job::TransportFailed {
                    token,
                    uri: track.uri.clone(),
                    error,
                });
                return;
            }
        }
    };
    if let Some(bytes) = lookup.canned_audio() {
        send(Job::Canned {
            token,
            uri: track.uri.clone(),
            bytes,
            label: "test match · not Spotify audio".into(),
            video_id,
        });
        return;
    }
    let resolved = match lookup.streams(&video_id).await {
        Ok(value) => value,
        Err(error) => {
            send(Job::TransportFailed {
                token,
                uri: track.uri.clone(),
                error,
            });
            return;
        }
    };
    let Some(stream) = select_audio_stream(&resolved.streams) else {
        send(Job::MatchFailed {
            token,
            uri: track.uri.clone(),
            error: "No playable audio stream (need AAC/M4A or MP3; Opus/WebM is not decoded)."
                .into(),
        });
        return;
    };
    let label = resolved.provider.label();
    let hint = FormatHint::from_labels(
        stream.format.as_deref(),
        stream.mime.as_deref(),
        Some(stream.url.as_str()),
    );
    let buffer = match SharedAudio::new(None) {
        Ok(buffer) => buffer,
        Err(error) => {
            send(Job::TransportFailed {
                token,
                uri: track.uri.clone(),
                error,
            });
            return;
        }
    };
    let mut ready_sent = false;
    let mut on_ready = {
        let buffer = buffer.clone();
        let uri = track.uri.clone();
        let label = label.to_string();
        let video_id = video_id.clone();
        let hint = hint.clone();
        let tx = tx.clone();
        move || {
            if ready_sent {
                return;
            }
            ready_sent = true;
            let _ = tx.send(Internal::Job(Job::Ready {
                token,
                uri: uri.clone(),
                buffer: buffer.clone(),
                label: label.clone(),
                video_id: video_id.clone(),
                hint: hint.clone(),
            }));
        }
    };
    let result = if let Some(script) = lookup.scripted_body() {
        fetch::fetch_scripted(&buffer, script, &mut on_ready).await
    } else {
        fetch::fetch_with(
            http,
            stream.url.clone(),
            stream.http_headers.clone(),
            &buffer,
            &mut on_ready,
            FetchPolicy::production(),
            Some(lookup),
            Some(video_id.as_str()),
            Some(&hint),
        )
        .await
    };
    match result {
        Ok(()) => on_ready(),
        Err(error) if error == "cancelled" => {}
        Err(error) => send(Job::TransportFailed {
            token,
            uri: track.uri.clone(),
            error,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alternate::decode::{
        DecodeHandle, PcmSource, wait_matching_sample, wait_nonzero_sample,
    };
    use crate::alternate::provider::ScriptedBody;
    use crate::player::LocalTrack;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::oneshot;

    struct RecordingOutput {
        plays: Arc<AtomicUsize>,
        resumes: Arc<AtomicUsize>,
        pauses: Arc<AtomicUsize>,
    }

    impl AudioOutput for RecordingOutput {
        fn play_bytes(
            &mut self,
            _bytes: Vec<u8>,
            _start_ms: u32,
        ) -> Result<super::super::audio::PlayInfo> {
            self.plays.fetch_add(1, Ordering::SeqCst);
            Ok(super::super::audio::PlayInfo {
                duration_ms: Some(1_000),
            })
        }
        fn pause(&mut self) {
            self.pauses.fetch_add(1, Ordering::SeqCst);
        }
        fn resume(&mut self) {
            self.resumes.fetch_add(1, Ordering::SeqCst);
        }
        fn stop(&mut self) {}
        fn seek(&mut self, _ms: u32) -> Result<()> {
            Ok(())
        }
        fn set_volume(&mut self, _volume: f32) {}
        fn is_finished(&self) -> bool {
            false
        }
        fn play_pcm(
            &mut self,
            _source: super::super::decode::PcmSource,
            _decode: super::super::decode::DecodeHandle,
        ) -> Result<super::super::audio::PlayInfo> {
            self.plays.fetch_add(1, Ordering::SeqCst);
            Ok(super::super::audio::PlayInfo {
                duration_ms: Some(1_000),
            })
        }
        fn status(&self) -> super::super::audio::OutputStatus {
            super::super::audio::OutputStatus::Playing
        }
    }

    struct CaptureOutput {
        plays: Arc<AtomicUsize>,
        pcm: Arc<Mutex<Option<PcmSource>>>,
        decode: Arc<Mutex<Option<DecodeHandle>>>,
        start_ms: Arc<Mutex<Vec<u32>>>,
    }

    impl AudioOutput for CaptureOutput {
        fn play_bytes(
            &mut self,
            _bytes: Vec<u8>,
            start_ms: u32,
        ) -> Result<super::super::audio::PlayInfo> {
            self.start_ms
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(start_ms);
            self.plays.fetch_add(1, Ordering::SeqCst);
            Ok(super::super::audio::PlayInfo {
                duration_ms: Some(1_000),
            })
        }
        fn pause(&mut self) {}
        fn resume(&mut self) {}
        fn stop(&mut self) {}
        fn seek(&mut self, ms: u32) -> Result<()> {
            if let Some(decode) = self
                .decode
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
            {
                decode.seek(ms);
            }
            self.start_ms
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(ms);
            Ok(())
        }
        fn set_volume(&mut self, _volume: f32) {}
        fn is_finished(&self) -> bool {
            false
        }
        fn play_pcm(
            &mut self,
            source: PcmSource,
            decode: DecodeHandle,
        ) -> Result<super::super::audio::PlayInfo> {
            *self.pcm.lock().unwrap_or_else(|p| p.into_inner()) = Some(source);
            *self.decode.lock().unwrap_or_else(|p| p.into_inner()) = Some(decode);
            self.plays.fetch_add(1, Ordering::SeqCst);
            Ok(super::super::audio::PlayInfo {
                duration_ms: Some(1_000),
            })
        }
        fn status(&self) -> super::super::audio::OutputStatus {
            super::super::audio::OutputStatus::Playing
        }
    }

    struct HoldLookup {
        hold: Mutex<Option<oneshot::Receiver<()>>>,
        audio: Vec<u8>,
    }

    impl MediaLookup for HoldLookup {
        fn search(
            &self,
            _query: &TrackQuery,
            _min_score: f32,
        ) -> super::super::provider::LookupFuture<
            Result<Vec<super::super::matching::Candidate>, String>,
        > {
            let rx = self.hold.lock().unwrap_or_else(|p| p.into_inner()).take();
            Box::pin(async move {
                if let Some(rx) = rx {
                    let _ = rx.await;
                }
                Ok(vec![super::super::matching::Candidate {
                    id: "dQw4w9WgXcQ".into(),
                    title: "Song".into(),
                    uploader: "Artist - Topic".into(),
                    duration_ms: Some(1_000),
                }])
            })
        }

        fn streams(
            &self,
            _id: &str,
        ) -> super::super::provider::LookupFuture<
            Result<super::super::provider::StreamLookup, String>,
        > {
            Box::pin(async {
                Ok(super::super::provider::StreamLookup {
                    streams: vec![super::super::streams::AudioStream {
                        url: "https://example.invalid/a.m4a".into(),
                        mime: Some("audio/mp4".into()),
                        codec: Some("mp4a.40.2".into()),
                        format: Some("m4a".into()),
                        bitrate: Some(128_000),
                        video_only: false,
                        quality: None,
                        http_headers: Vec::new(),
                    }],
                    provider: super::super::provider::ProviderKind::YtDlpYoutube,
                })
            })
        }

        fn canned_audio(&self) -> Option<Vec<u8>> {
            Some(self.audio.clone())
        }
    }

    fn track(title: &str) -> LocalTrack {
        LocalTrack {
            uri: format!("spotify:track:{title}"),
            title: title.into(),
            artists: vec!["Artist".into()],
            album: "Album".into(),
            duration_ms: 1_000,
            ..LocalTrack::default()
        }
    }

    fn test_config() -> AlternateConfig {
        AlternateConfig {
            piped_api_base: Some("https://piped.example".into()),
            ytdlp_path: None,
            min_score: 0.1,
            skip_on_miss: false,
            gapless: false,
            volume: 1000,
        }
    }

    #[tokio::test]
    async fn stale_resolve_after_stop_does_not_play() {
        let (hold_tx, hold_rx) = oneshot::channel();
        let plays = Arc::new(AtomicUsize::new(0));
        let output = RecordingOutput {
            plays: Arc::clone(&plays),
            resumes: Arc::new(AtomicUsize::new(0)),
            pauses: Arc::new(AtomicUsize::new(0)),
        };
        let lookup = Arc::new(HoldLookup {
            hold: Mutex::new(Some(hold_rx)),
            audio: vec![1, 2, 3],
        });
        let handle = spawn_inner(
            test_config(),
            Arc::new(ApiClient::new(
                reqwest::Client::new(),
                Arc::new(crate::api::NetActivity::default()),
                20,
                50,
                crate::api::ApiSource::Shared,
            )),
            reqwest::Client::new(),
            Arc::new(|_| {}),
            Box::new(output),
            lookup,
        );
        handle.test_load(vec![track("a")], true);
        tokio::time::sleep(Duration::from_millis(30)).await;
        handle.command(PlayerCommand::Stop).unwrap();
        let _ = hold_tx.send(());
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(plays.load(Ordering::SeqCst), 0);
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn pause_resume_does_not_play_bytes_again() {
        let plays = Arc::new(AtomicUsize::new(0));
        let resumes = Arc::new(AtomicUsize::new(0));
        let pauses = Arc::new(AtomicUsize::new(0));
        let output = RecordingOutput {
            plays: Arc::clone(&plays),
            resumes: Arc::clone(&resumes),
            pauses: Arc::clone(&pauses),
        };
        let lookup = Arc::new(HoldLookup {
            hold: Mutex::new(None),
            audio: vec![1, 2, 3],
        });
        let handle = spawn_inner(
            test_config(),
            Arc::new(ApiClient::new(
                reqwest::Client::new(),
                Arc::new(crate::api::NetActivity::default()),
                20,
                50,
                crate::api::ApiSource::Shared,
            )),
            reqwest::Client::new(),
            Arc::new(|_| {}),
            Box::new(output),
            lookup,
        );
        handle.test_load(vec![track("a")], true);
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(plays.load(Ordering::SeqCst), 1);
        handle.command(PlayerCommand::Toggle).unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        handle.command(PlayerCommand::Toggle).unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(plays.load(Ordering::SeqCst), 1);
        assert!(pauses.load(Ordering::SeqCst) >= 1);
        assert!(resumes.load(Ordering::SeqCst) >= 1);
        handle.shutdown().await;
    }

    struct MissLookup {
        searches: Arc<AtomicUsize>,
    }

    impl MediaLookup for MissLookup {
        fn search(
            &self,
            _query: &TrackQuery,
            _min_score: f32,
        ) -> super::super::provider::LookupFuture<
            Result<Vec<super::super::matching::Candidate>, String>,
        > {
            self.searches.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Ok(vec![super::super::matching::Candidate {
                    id: "abcdefghijk".into(),
                    title: "totally unrelated karaoke nightcore mix".into(),
                    uploader: "RandomChannel".into(),
                    duration_ms: Some(9_000),
                }])
            })
        }

        fn streams(
            &self,
            _id: &str,
        ) -> super::super::provider::LookupFuture<
            Result<super::super::provider::StreamLookup, String>,
        > {
            Box::pin(async { Err("streams should not run on a ranked miss".into()) })
        }
    }

    struct SearchErrLookup {
        searches: Arc<AtomicUsize>,
        streams: Arc<AtomicUsize>,
    }

    impl MediaLookup for SearchErrLookup {
        fn search(
            &self,
            _query: &TrackQuery,
            _min_score: f32,
        ) -> super::super::provider::LookupFuture<
            Result<Vec<super::super::matching::Candidate>, String>,
        > {
            self.searches.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err("search provider failed".into()) })
        }

        fn streams(
            &self,
            _id: &str,
        ) -> super::super::provider::LookupFuture<
            Result<super::super::provider::StreamLookup, String>,
        > {
            self.streams.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err("no streams".into()) })
        }
    }

    struct StreamsErrLookup {
        searches: Arc<AtomicUsize>,
        streams: Arc<AtomicUsize>,
    }

    impl MediaLookup for StreamsErrLookup {
        fn search(
            &self,
            _query: &TrackQuery,
            _min_score: f32,
        ) -> super::super::provider::LookupFuture<
            Result<Vec<super::super::matching::Candidate>, String>,
        > {
            self.searches.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Ok(vec![super::super::matching::Candidate {
                    id: "dQw4w9WgXcQ".into(),
                    title: "Song".into(),
                    uploader: "Artist - Topic".into(),
                    duration_ms: Some(1_000),
                }])
            })
        }

        fn streams(
            &self,
            _id: &str,
        ) -> super::super::provider::LookupFuture<
            Result<super::super::provider::StreamLookup, String>,
        > {
            self.streams.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err("stream lookup failed".into()) })
        }
    }

    #[tokio::test]
    async fn toggle_while_loading_cancels_held_lookup() {
        let (hold_tx, hold_rx) = oneshot::channel();
        let plays = Arc::new(AtomicUsize::new(0));
        let output = RecordingOutput {
            plays: Arc::clone(&plays),
            resumes: Arc::new(AtomicUsize::new(0)),
            pauses: Arc::new(AtomicUsize::new(0)),
        };
        let lookup = Arc::new(HoldLookup {
            hold: Mutex::new(Some(hold_rx)),
            audio: vec![1, 2, 3],
        });
        let handle = spawn_inner(
            test_config(),
            Arc::new(ApiClient::new(
                reqwest::Client::new(),
                Arc::new(crate::api::NetActivity::default()),
                20,
                50,
                crate::api::ApiSource::Shared,
            )),
            reqwest::Client::new(),
            Arc::new(|_| {}),
            Box::new(output),
            lookup,
        );
        handle.test_load(vec![track("a")], true);
        tokio::time::sleep(Duration::from_millis(30)).await;
        handle.command(PlayerCommand::Toggle).unwrap();
        let _ = hold_tx.send(());
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(plays.load(Ordering::SeqCst), 0);
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn skip_on_miss_does_not_loop_under_repeat_context() {
        let searches = Arc::new(AtomicUsize::new(0));
        let plays = Arc::new(AtomicUsize::new(0));
        let output = RecordingOutput {
            plays: Arc::clone(&plays),
            resumes: Arc::new(AtomicUsize::new(0)),
            pauses: Arc::new(AtomicUsize::new(0)),
        };
        let mut config = test_config();
        config.skip_on_miss = true;
        let handle = spawn_inner(
            config,
            Arc::new(ApiClient::new(
                reqwest::Client::new(),
                Arc::new(crate::api::NetActivity::default()),
                20,
                50,
                crate::api::ApiSource::Shared,
            )),
            reqwest::Client::new(),
            Arc::new(|_| {}),
            Box::new(output),
            Arc::new(MissLookup {
                searches: Arc::clone(&searches),
            }),
        );
        handle
            .command(PlayerCommand::Repeat(crate::player::RepeatMode::Context))
            .unwrap();
        handle.test_load(vec![track("a"), track("b")], true);
        tokio::time::sleep(Duration::from_millis(80)).await;
        let count = searches.load(Ordering::SeqCst);
        assert!(count > 0, "expected at least one search");
        assert!(count <= 2, "looped under repeat-context: {count} searches");
        assert_eq!(plays.load(Ordering::SeqCst), 0);
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(searches.load(Ordering::SeqCst), count);
        handle.shutdown().await;
    }

    fn wav_bytes(samples: usize) -> Vec<u8> {
        let sample_rate: u32 = 8_000;
        let data_bytes = (samples * 2) as u32;
        let mut out = Vec::with_capacity(44 + data_bytes as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_bytes.to_le_bytes());
        for i in 0..samples {
            let sample: i16 = if i % 2 == 0 { 400 } else { -400 };
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }

    fn marked_wav(samples: usize, mark_at: usize) -> Vec<u8> {
        let sample_rate: u32 = 8_000;
        let data_bytes = (samples * 2) as u32;
        let mut out = Vec::with_capacity(44 + data_bytes as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_bytes.to_le_bytes());
        for i in 0..samples {
            let sample: i16 = if i >= mark_at { -20_000 } else { 20_000 };
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }

    struct ScriptLookup {
        searches: Arc<AtomicUsize>,
        body: ScriptedBody,
        hold: Mutex<Option<oneshot::Receiver<()>>>,
    }

    impl MediaLookup for ScriptLookup {
        fn search(
            &self,
            _query: &TrackQuery,
            _min_score: f32,
        ) -> super::super::provider::LookupFuture<
            Result<Vec<super::super::matching::Candidate>, String>,
        > {
            self.searches.fetch_add(1, Ordering::SeqCst);
            let rx = self.hold.lock().unwrap_or_else(|p| p.into_inner()).take();
            Box::pin(async move {
                if let Some(rx) = rx {
                    let _ = rx.await;
                }
                Ok(vec![super::super::matching::Candidate {
                    id: "dQw4w9WgXcQ".into(),
                    title: "Song".into(),
                    uploader: "Artist - Topic".into(),
                    duration_ms: Some(1_000),
                }])
            })
        }

        fn streams(
            &self,
            _id: &str,
        ) -> super::super::provider::LookupFuture<
            Result<super::super::provider::StreamLookup, String>,
        > {
            Box::pin(async {
                Ok(super::super::provider::StreamLookup {
                    streams: vec![super::super::streams::AudioStream {
                        url: "https://example.invalid/a.wav".into(),
                        mime: Some("audio/wav".into()),
                        codec: Some("pcm".into()),
                        format: Some("wav".into()),
                        bitrate: Some(8_000),
                        video_only: false,
                        quality: None,
                        http_headers: Vec::new(),
                    }],
                    provider: super::super::provider::ProviderKind::YtDlpYoutube,
                })
            })
        }

        fn scripted_body(&self) -> Option<ScriptedBody> {
            Some(self.body.clone())
        }
    }

    fn spawn_test(
        config: AlternateConfig,
        output: RecordingOutput,
        lookup: Arc<dyn MediaLookup>,
        notify: Notify,
    ) -> AlternateHandle {
        spawn_inner(
            config,
            Arc::new(ApiClient::new(
                reqwest::Client::new(),
                Arc::new(crate::api::NetActivity::default()),
                20,
                50,
                crate::api::ApiSource::Shared,
            )),
            reqwest::Client::new(),
            notify,
            Box::new(output),
            lookup,
        )
    }

    #[tokio::test]
    async fn io_failure_after_playback_does_not_skip() {
        let searches = Arc::new(AtomicUsize::new(0));
        let plays = Arc::new(AtomicUsize::new(0));
        let errors = Arc::new(Mutex::new(Vec::<String>::new()));
        let wav = wav_bytes(40_000);
        let split = wav.len() / 2;
        let lookup = Arc::new(ScriptLookup {
            searches: Arc::clone(&searches),
            hold: Mutex::new(None),
            body: ScriptedBody {
                chunks: vec![wav[..split].to_vec(), wav[split..].to_vec()],
                fail: Some("Matched audio stalled.".into()),
                content_length: None,
                fail_after_ms: 400,
            },
        });
        let mut config = test_config();
        config.skip_on_miss = true;
        let notify_errors = Arc::clone(&errors);
        let handle = spawn_test(
            config,
            RecordingOutput {
                plays: Arc::clone(&plays),
                resumes: Arc::new(AtomicUsize::new(0)),
                pauses: Arc::new(AtomicUsize::new(0)),
            },
            lookup,
            Arc::new(move |event| {
                if let EngineEvent::State(state) = event
                    && let Some(error) = state.error
                {
                    notify_errors
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(error);
                }
            }),
        );
        handle.test_load(vec![track("a"), track("b")], true);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline && plays.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(plays.load(Ordering::SeqCst), 1);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            let logged = errors.lock().unwrap_or_else(|p| p.into_inner()).clone();
            if logged.iter().any(|error| error.contains("stalled")) {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "expected stall error, got {logged:?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(searches.load(Ordering::SeqCst), 1);
        assert_eq!(plays.load(Ordering::SeqCst), 1);
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn stop_cancels_active_progressive_source() {
        let searches = Arc::new(AtomicUsize::new(0));
        let plays = Arc::new(AtomicUsize::new(0));
        let wav = wav_bytes(40_000);
        let lookup = Arc::new(ScriptLookup {
            searches: Arc::clone(&searches),
            hold: Mutex::new(None),
            body: ScriptedBody {
                chunks: vec![wav],
                fail: None,
                content_length: None,
                fail_after_ms: 0,
            },
        });
        let handle = spawn_test(
            test_config(),
            RecordingOutput {
                plays: Arc::clone(&plays),
                resumes: Arc::new(AtomicUsize::new(0)),
                pauses: Arc::new(AtomicUsize::new(0)),
            },
            lookup,
            Arc::new(|_| {}),
        );
        handle.test_load(vec![track("a")], true);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline && plays.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(plays.load(Ordering::SeqCst), 1);
        handle.command(PlayerCommand::Stop).unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(plays.load(Ordering::SeqCst), 1);
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn seek_does_not_re_resolve_or_skip() {
        let searches = Arc::new(AtomicUsize::new(0));
        let plays = Arc::new(AtomicUsize::new(0));
        let positions = Arc::new(Mutex::new(Vec::<u32>::new()));
        let wav = wav_bytes(40_000);
        let lookup = Arc::new(ScriptLookup {
            searches: Arc::clone(&searches),
            hold: Mutex::new(None),
            body: ScriptedBody {
                chunks: vec![wav],
                fail: None,
                content_length: None,
                fail_after_ms: 0,
            },
        });
        let mut config = test_config();
        config.skip_on_miss = true;
        let notify_pos = Arc::clone(&positions);
        let handle = spawn_test(
            config,
            RecordingOutput {
                plays: Arc::clone(&plays),
                resumes: Arc::new(AtomicUsize::new(0)),
                pauses: Arc::new(AtomicUsize::new(0)),
            },
            lookup,
            Arc::new(move |event| {
                if let EngineEvent::State(state) = event {
                    notify_pos
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(state.position_ms);
                }
            }),
        );
        handle.test_load(vec![track("a"), track("b")], true);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline && plays.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(plays.load(Ordering::SeqCst), 1);
        handle.command(PlayerCommand::Seek(500)).unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(searches.load(Ordering::SeqCst), 1);
        assert_eq!(plays.load(Ordering::SeqCst), 1);
        let logged = positions.lock().unwrap_or_else(|p| p.into_inner()).clone();
        assert!(
            logged.contains(&500),
            "seek did not keep session position, got {logged:?}"
        );
        handle.shutdown().await;
    }

    /// The bar follows the audio's own length: the output reports the
    /// decoded media as one second long while Spotify called the track
    /// three minutes, and the state carries the second from then on.
    #[tokio::test]
    async fn the_bar_follows_the_audio_s_own_length() {
        let plays = Arc::new(AtomicUsize::new(0));
        let durations = Arc::new(Mutex::new(Vec::<u32>::new()));
        let wav = wav_bytes(40_000);
        let lookup = Arc::new(ScriptLookup {
            searches: Arc::new(AtomicUsize::new(0)),
            hold: Mutex::new(None),
            body: ScriptedBody {
                chunks: vec![wav],
                fail: None,
                content_length: None,
                fail_after_ms: 0,
            },
        });
        let logged = Arc::clone(&durations);
        let handle = spawn_test(
            test_config(),
            RecordingOutput {
                plays: Arc::clone(&plays),
                resumes: Arc::new(AtomicUsize::new(0)),
                pauses: Arc::new(AtomicUsize::new(0)),
            },
            lookup,
            Arc::new(move |event| {
                if let EngineEvent::State(state) = event
                    && let Some(track) = state.track
                {
                    logged
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(track.duration_ms);
                }
            }),
        );
        let mut long = track("a");
        long.duration_ms = 180_000;
        handle.test_load(vec![long], true);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline && plays.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(60)).await;
        let logged = durations.lock().unwrap_or_else(|p| p.into_inner()).clone();
        assert_eq!(
            logged.first().copied(),
            Some(180_000),
            "the first state carries Spotify's length, got {logged:?}"
        );
        // The output's own guess (one second) is overtaken by what the
        // decoder read from the WAV: five seconds at 8 kHz.
        assert_eq!(
            logged.last().copied(),
            Some(5_000),
            "the bar did not take the audio's own length, got {logged:?}"
        );
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn seek_before_ready_starts_decoder_at_requested_position() {
        let (hold_tx, hold_rx) = oneshot::channel();
        let searches = Arc::new(AtomicUsize::new(0));
        let plays = Arc::new(AtomicUsize::new(0));
        let pcm_slot = Arc::new(Mutex::new(None));
        let decode_slot = Arc::new(Mutex::new(None));
        let positions = Arc::new(Mutex::new(Vec::<u32>::new()));
        let wav = marked_wav(8_000, 1_000);
        let lookup = Arc::new(ScriptLookup {
            searches: Arc::clone(&searches),
            hold: Mutex::new(Some(hold_rx)),
            body: ScriptedBody {
                chunks: vec![wav],
                fail: None,
                content_length: None,
                fail_after_ms: 0,
            },
        });
        let notify_pos = Arc::clone(&positions);
        let handle = spawn_inner(
            test_config(),
            Arc::new(ApiClient::new(
                reqwest::Client::new(),
                Arc::new(crate::api::NetActivity::default()),
                20,
                50,
                crate::api::ApiSource::Shared,
            )),
            reqwest::Client::new(),
            Arc::new(move |event| {
                if let EngineEvent::State(state) = event {
                    notify_pos
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(state.position_ms);
                }
            }),
            Box::new(CaptureOutput {
                plays: Arc::clone(&plays),
                pcm: Arc::clone(&pcm_slot),
                decode: Arc::clone(&decode_slot),
                start_ms: Arc::new(Mutex::new(Vec::new())),
            }),
            lookup,
        );
        handle.test_load(vec![track("a")], true);
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(plays.load(Ordering::SeqCst), 0);
        handle.command(PlayerCommand::Seek(500)).unwrap();
        let seek_deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        while tokio::time::Instant::now() < seek_deadline {
            let logged = positions.lock().unwrap_or_else(|p| p.into_inner()).clone();
            if logged.contains(&500) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let _ = hold_tx.send(());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline && plays.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(plays.load(Ordering::SeqCst), 1);
        assert_eq!(searches.load(Ordering::SeqCst), 1);
        let logged = positions.lock().unwrap_or_else(|p| p.into_inner()).clone();
        assert!(
            logged.contains(&500),
            "session lost the pre-ready seek, got {logged:?}"
        );
        let mut pcm = pcm_slot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .expect("decoder PCM");
        let sample = wait_matching_sample(&mut pcm, |s| s.abs() > 0.5, Duration::from_secs(3));
        assert!(
            sample.is_some_and(|s| s < -0.5),
            "first post-ready audio was not at the requested seek, got {sample:?}"
        );
        drop(pcm);
        *decode_slot.lock().unwrap_or_else(|p| p.into_inner()) = None;
        handle.shutdown().await;
    }

    type StateLog = Vec<(Playback, Option<String>)>;

    fn collect_states(notify_states: Arc<Mutex<StateLog>>) -> Notify {
        Arc::new(move |event| {
            if let EngineEvent::State(state) = event {
                notify_states
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push((state.playback, state.error));
            }
        })
    }

    #[tokio::test]
    async fn search_err_does_not_skip_on_miss() {
        let searches = Arc::new(AtomicUsize::new(0));
        let streams = Arc::new(AtomicUsize::new(0));
        let plays = Arc::new(AtomicUsize::new(0));
        let states = Arc::new(Mutex::new(Vec::new()));
        let mut config = test_config();
        config.skip_on_miss = true;
        let handle = spawn_test(
            config,
            RecordingOutput {
                plays: Arc::clone(&plays),
                resumes: Arc::new(AtomicUsize::new(0)),
                pauses: Arc::new(AtomicUsize::new(0)),
            },
            Arc::new(SearchErrLookup {
                searches: Arc::clone(&searches),
                streams: Arc::clone(&streams),
            }),
            collect_states(Arc::clone(&states)),
        );
        handle.test_load(vec![track("a"), track("b")], true);
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(searches.load(Ordering::SeqCst), 1);
        assert_eq!(streams.load(Ordering::SeqCst), 0);
        assert_eq!(plays.load(Ordering::SeqCst), 0);
        let logged = states.lock().unwrap_or_else(|p| p.into_inner()).clone();
        assert!(
            logged.iter().any(|(playback, error)| {
                *playback != Playback::Loading && error.as_deref() == Some("search provider failed")
            }),
            "expected transport stop, got {logged:?}"
        );
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn streams_err_does_not_skip_on_miss() {
        let searches = Arc::new(AtomicUsize::new(0));
        let streams = Arc::new(AtomicUsize::new(0));
        let plays = Arc::new(AtomicUsize::new(0));
        let states = Arc::new(Mutex::new(Vec::new()));
        let mut config = test_config();
        config.skip_on_miss = true;
        let handle = spawn_test(
            config,
            RecordingOutput {
                plays: Arc::clone(&plays),
                resumes: Arc::new(AtomicUsize::new(0)),
                pauses: Arc::new(AtomicUsize::new(0)),
            },
            Arc::new(StreamsErrLookup {
                searches: Arc::clone(&searches),
                streams: Arc::clone(&streams),
            }),
            collect_states(Arc::clone(&states)),
        );
        handle.test_load(vec![track("Song"), track("Other")], true);
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(searches.load(Ordering::SeqCst), 1);
        assert_eq!(streams.load(Ordering::SeqCst), 1);
        assert_eq!(plays.load(Ordering::SeqCst), 0);
        let logged = states.lock().unwrap_or_else(|p| p.into_inner()).clone();
        assert!(
            logged.iter().any(|(playback, error)| {
                *playback != Playback::Loading && error.as_deref() == Some("stream lookup failed")
            }),
            "expected transport stop, got {logged:?}"
        );
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn invalid_complete_body_fails_instead_of_loading_forever() {
        let searches = Arc::new(AtomicUsize::new(0));
        let plays = Arc::new(AtomicUsize::new(0));
        let states = Arc::new(Mutex::new(Vec::new()));
        let mut config = test_config();
        config.skip_on_miss = true;
        let handle = spawn_test(
            config,
            RecordingOutput {
                plays: Arc::clone(&plays),
                resumes: Arc::new(AtomicUsize::new(0)),
                pauses: Arc::new(AtomicUsize::new(0)),
            },
            Arc::new(ScriptLookup {
                searches: Arc::clone(&searches),
                hold: Mutex::new(None),
                body: ScriptedBody {
                    chunks: vec![vec![0u8; 8_192]],
                    fail: None,
                    content_length: None,
                    fail_after_ms: 0,
                },
            }),
            collect_states(Arc::clone(&states)),
        );
        handle.test_load(vec![track("Song"), track("Other")], true);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            let logged = states.lock().unwrap_or_else(|p| p.into_inner()).clone();
            if logged.iter().any(|(playback, error)| {
                *playback != Playback::Loading
                    && error
                        .as_deref()
                        .is_some_and(|message| message.contains("not a playable format"))
            }) {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "stuck loading on invalid body: {logged:?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(searches.load(Ordering::SeqCst), 1);
        assert_eq!(plays.load(Ordering::SeqCst), 0);
        handle.shutdown().await;
    }

    struct DeviceOutput {
        status: Arc<Mutex<OutputStatus>>,
        recover_fails: Arc<AtomicUsize>,
        recovers: Arc<AtomicUsize>,
        plays: Arc<AtomicUsize>,
        pauses: Arc<AtomicUsize>,
        pcm: Arc<Mutex<Option<PcmSource>>>,
        decode: Arc<Mutex<Option<DecodeHandle>>>,
    }

    impl AudioOutput for DeviceOutput {
        fn play_bytes(
            &mut self,
            _bytes: Vec<u8>,
            _start_ms: u32,
        ) -> Result<super::super::audio::PlayInfo> {
            self.plays.fetch_add(1, Ordering::SeqCst);
            Ok(super::super::audio::PlayInfo {
                duration_ms: Some(1_000),
            })
        }
        fn play_pcm(
            &mut self,
            source: PcmSource,
            decode: DecodeHandle,
        ) -> Result<super::super::audio::PlayInfo> {
            *self.pcm.lock().unwrap_or_else(|p| p.into_inner()) = Some(source);
            *self.decode.lock().unwrap_or_else(|p| p.into_inner()) = Some(decode);
            self.plays.fetch_add(1, Ordering::SeqCst);
            Ok(super::super::audio::PlayInfo {
                duration_ms: Some(1_000),
            })
        }
        fn pause(&mut self) {
            self.pauses.fetch_add(1, Ordering::SeqCst);
        }
        fn resume(&mut self) {}
        fn stop(&mut self) {}
        fn seek(&mut self, _ms: u32) -> Result<()> {
            Ok(())
        }
        fn set_volume(&mut self, _volume: f32) {}
        fn is_finished(&self) -> bool {
            matches!(
                *self.status.lock().unwrap_or_else(|p| p.into_inner()),
                OutputStatus::Ended
            )
        }
        fn status(&self) -> OutputStatus {
            self.status
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
        }
        fn recover(&mut self) -> Result<(), String> {
            self.recovers.fetch_add(1, Ordering::SeqCst);
            if self.recover_fails.load(Ordering::SeqCst) > 0 {
                self.recover_fails.fetch_sub(1, Ordering::SeqCst);
                return Err("no device".into());
            }
            *self.status.lock().unwrap_or_else(|p| p.into_inner()) = OutputStatus::Playing;
            Ok(())
        }
    }

    fn spawn_device(
        output: DeviceOutput,
        lookup: Arc<dyn MediaLookup>,
        notify: Notify,
    ) -> AlternateHandle {
        spawn_inner(
            test_config(),
            Arc::new(ApiClient::new(
                reqwest::Client::new(),
                Arc::new(crate::api::NetActivity::default()),
                20,
                50,
                crate::api::ApiSource::Shared,
            )),
            reqwest::Client::new(),
            notify,
            Box::new(output),
            lookup,
        )
    }

    fn wav_script() -> ScriptLookup {
        ScriptLookup {
            searches: Arc::new(AtomicUsize::new(0)),
            hold: Mutex::new(None),
            body: ScriptedBody {
                chunks: vec![wav_bytes(8_000)],
                fail: None,
                content_length: None,
                fail_after_ms: 0,
            },
        }
    }

    async fn wait_plays(plays: &Arc<AtomicUsize>, want: usize) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline && plays.load(Ordering::SeqCst) < want {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(plays.load(Ordering::SeqCst), want);
    }

    #[tokio::test]
    async fn playing_device_loss_recovers_same_track() {
        let plays = Arc::new(AtomicUsize::new(0));
        let recovers = Arc::new(AtomicUsize::new(0));
        let status = Arc::new(Mutex::new(OutputStatus::Playing));
        let lookup = Arc::new(wav_script());
        let searches = Arc::clone(&lookup.searches);
        let handle = spawn_device(
            DeviceOutput {
                status: Arc::clone(&status),
                recover_fails: Arc::new(AtomicUsize::new(0)),
                recovers: Arc::clone(&recovers),
                plays: Arc::clone(&plays),
                pauses: Arc::new(AtomicUsize::new(0)),
                pcm: Arc::new(Mutex::new(None)),
                decode: Arc::new(Mutex::new(None)),
            },
            lookup,
            Arc::new(|_| {}),
        );
        handle.test_load(vec![track("a"), track("b")], true);
        wait_plays(&plays, 1).await;
        *status.lock().unwrap_or_else(|p| p.into_inner()) = OutputStatus::DeviceLost;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline && plays.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(plays.load(Ordering::SeqCst), 2);
        assert!(recovers.load(Ordering::SeqCst) >= 1);
        assert_eq!(searches.load(Ordering::SeqCst), 1);
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn paused_device_loss_recovers_paused() {
        let plays = Arc::new(AtomicUsize::new(0));
        let pauses = Arc::new(AtomicUsize::new(0));
        let recovers = Arc::new(AtomicUsize::new(0));
        let status = Arc::new(Mutex::new(OutputStatus::Playing));
        let lookup = Arc::new(wav_script());
        let searches = Arc::clone(&lookup.searches);
        let handle = spawn_device(
            DeviceOutput {
                status: Arc::clone(&status),
                recover_fails: Arc::new(AtomicUsize::new(0)),
                recovers: Arc::clone(&recovers),
                plays: Arc::clone(&plays),
                pauses: Arc::clone(&pauses),
                pcm: Arc::new(Mutex::new(None)),
                decode: Arc::new(Mutex::new(None)),
            },
            lookup,
            Arc::new(|_| {}),
        );
        handle.test_load(vec![track("a")], true);
        wait_plays(&plays, 1).await;
        handle.command(PlayerCommand::Toggle).unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        *status.lock().unwrap_or_else(|p| p.into_inner()) = OutputStatus::DeviceLost;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline && plays.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(plays.load(Ordering::SeqCst), 2);
        assert!(recovers.load(Ordering::SeqCst) >= 1);
        assert!(pauses.load(Ordering::SeqCst) >= 2);
        assert_eq!(searches.load(Ordering::SeqCst), 1);
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn device_open_backoff_then_success() {
        let plays = Arc::new(AtomicUsize::new(0));
        let recovers = Arc::new(AtomicUsize::new(0));
        let status = Arc::new(Mutex::new(OutputStatus::Playing));
        let lookup = Arc::new(wav_script());
        let searches = Arc::clone(&lookup.searches);
        let handle = spawn_device(
            DeviceOutput {
                status: Arc::clone(&status),
                recover_fails: Arc::new(AtomicUsize::new(3)),
                recovers: Arc::clone(&recovers),
                plays: Arc::clone(&plays),
                pauses: Arc::new(AtomicUsize::new(0)),
                pcm: Arc::new(Mutex::new(None)),
                decode: Arc::new(Mutex::new(None)),
            },
            lookup,
            Arc::new(|_| {}),
        );
        handle.test_load(vec![track("a")], true);
        wait_plays(&plays, 1).await;
        *status.lock().unwrap_or_else(|p| p.into_inner()) = OutputStatus::DeviceLost;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
        while tokio::time::Instant::now() < deadline && plays.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        assert_eq!(plays.load(Ordering::SeqCst), 2);
        assert!(recovers.load(Ordering::SeqCst) >= 4);
        assert_eq!(searches.load(Ordering::SeqCst), 1);
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn ended_and_decode_failed_unchanged() {
        let plays = Arc::new(AtomicUsize::new(0));
        let status = Arc::new(Mutex::new(OutputStatus::Playing));
        let lookup = Arc::new(wav_script());
        let searches = Arc::clone(&lookup.searches);
        let handle = spawn_device(
            DeviceOutput {
                status: Arc::clone(&status),
                recover_fails: Arc::new(AtomicUsize::new(0)),
                recovers: Arc::new(AtomicUsize::new(0)),
                plays: Arc::clone(&plays),
                pauses: Arc::new(AtomicUsize::new(0)),
                pcm: Arc::new(Mutex::new(None)),
                decode: Arc::new(Mutex::new(None)),
            },
            lookup,
            Arc::new(|_| {}),
        );
        handle.test_load(vec![track("a"), track("b")], true);
        wait_plays(&plays, 1).await;
        *status.lock().unwrap_or_else(|p| p.into_inner()) = OutputStatus::Ended;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline && searches.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(searches.load(Ordering::SeqCst), 2);
        handle.shutdown().await;

        let plays = Arc::new(AtomicUsize::new(0));
        let status = Arc::new(Mutex::new(OutputStatus::Playing));
        let states = Arc::new(Mutex::new(Vec::new()));
        let lookup = Arc::new(wav_script());
        let searches = Arc::clone(&lookup.searches);
        let handle = spawn_device(
            DeviceOutput {
                status: Arc::clone(&status),
                recover_fails: Arc::new(AtomicUsize::new(0)),
                recovers: Arc::new(AtomicUsize::new(0)),
                plays: Arc::clone(&plays),
                pauses: Arc::new(AtomicUsize::new(0)),
                pcm: Arc::new(Mutex::new(None)),
                decode: Arc::new(Mutex::new(None)),
            },
            lookup,
            collect_states(Arc::clone(&states)),
        );
        handle.test_load(vec![track("a"), track("b")], true);
        wait_plays(&plays, 1).await;
        *status.lock().unwrap_or_else(|p| p.into_inner()) =
            OutputStatus::Failed("Couldn't decode audio.".into());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            let logged = states.lock().unwrap_or_else(|p| p.into_inner()).clone();
            if logged.iter().any(|(_, error)| {
                error
                    .as_deref()
                    .is_some_and(|message| message.contains("Couldn't decode audio"))
            }) {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "expected decode fail, got {logged:?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(searches.load(Ordering::SeqCst), 1);
        handle.shutdown().await;
    }

    /// Scripted WAV for every track; one title's search can be held back
    /// or made to fail, to pin down what the prefetch does.
    struct TitleLookup {
        searches: Arc<AtomicUsize>,
        body: ScriptedBody,
        hold_title: Option<String>,
        hold: Mutex<Option<oneshot::Receiver<()>>>,
        fail_title: Option<String>,
    }

    impl TitleLookup {
        fn new(searches: Arc<AtomicUsize>) -> Self {
            Self {
                searches,
                body: ScriptedBody {
                    chunks: vec![wav_bytes(8_000)],
                    fail: None,
                    content_length: None,
                    fail_after_ms: 0,
                },
                hold_title: None,
                hold: Mutex::new(None),
                fail_title: None,
            }
        }
    }

    impl MediaLookup for TitleLookup {
        fn search(
            &self,
            query: &TrackQuery,
            _min_score: f32,
        ) -> super::super::provider::LookupFuture<
            Result<Vec<super::super::matching::Candidate>, String>,
        > {
            self.searches.fetch_add(1, Ordering::SeqCst);
            let held = self.hold_title.as_deref() == Some(query.title.as_str());
            let rx = if held {
                self.hold.lock().unwrap_or_else(|p| p.into_inner()).take()
            } else {
                None
            };
            let fail = self.fail_title.as_deref() == Some(query.title.as_str());
            Box::pin(async move {
                if let Some(rx) = rx {
                    let _ = rx.await;
                }
                if fail {
                    return Err("search provider failed".into());
                }
                Ok(vec![super::super::matching::Candidate {
                    id: "dQw4w9WgXcQ".into(),
                    title: "Song".into(),
                    uploader: "Artist - Topic".into(),
                    duration_ms: Some(1_000),
                }])
            })
        }

        fn streams(
            &self,
            _id: &str,
        ) -> super::super::provider::LookupFuture<
            Result<super::super::provider::StreamLookup, String>,
        > {
            Box::pin(async {
                Ok(super::super::provider::StreamLookup {
                    streams: vec![super::super::streams::AudioStream {
                        url: "https://example.invalid/a.wav".into(),
                        mime: Some("audio/wav".into()),
                        codec: Some("pcm".into()),
                        format: Some("wav".into()),
                        bitrate: Some(8_000),
                        video_only: false,
                        quality: None,
                        http_headers: Vec::new(),
                    }],
                    provider: super::super::provider::ProviderKind::YtDlpYoutube,
                })
            })
        }

        fn scripted_body(&self) -> Option<ScriptedBody> {
            Some(self.body.clone())
        }
    }

    fn gapless_config() -> AlternateConfig {
        let mut config = test_config();
        config.gapless = true;
        config
    }

    async fn wait_searches(searches: &Arc<AtomicUsize>, want: usize, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        while tokio::time::Instant::now() < deadline && searches.load(Ordering::SeqCst) < want {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        searches.load(Ordering::SeqCst) == want
    }

    /// A clicked track plays alone until its context arrives; once it
    /// does, the track after it is fetched without waiting for the next
    /// start.
    #[tokio::test]
    async fn prefetch_follows_the_hydration_of_a_seeded_play() {
        let searches = Arc::new(AtomicUsize::new(0));
        let plays = Arc::new(AtomicUsize::new(0));
        let handle = spawn_test(
            gapless_config(),
            RecordingOutput {
                plays: Arc::clone(&plays),
                resumes: Arc::new(AtomicUsize::new(0)),
                pauses: Arc::new(AtomicUsize::new(0)),
            },
            Arc::new(TitleLookup::new(Arc::clone(&searches))),
            Arc::new(|_| {}),
        );
        handle.test_load_seeded(vec![track("a")], true);
        wait_plays(&plays, 1).await;
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(
            searches.load(Ordering::SeqCst),
            1,
            "nothing to prefetch yet"
        );
        handle.test_hydrate(vec![track("a"), track("b")]);
        assert!(
            wait_searches(&searches, 2, Duration::from_secs(2)).await,
            "the second track was not prefetched after hydration, searches={}",
            searches.load(Ordering::SeqCst)
        );
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(
            plays.load(Ordering::SeqCst),
            1,
            "the prefetch must not play"
        );
        assert_eq!(searches.load(Ordering::SeqCst), 2);
        handle.shutdown().await;
    }

    /// The next track's resolve failing is the prefetch's problem only.
    #[tokio::test]
    async fn prefetch_failure_leaves_the_playing_track_alone() {
        let searches = Arc::new(AtomicUsize::new(0));
        let plays = Arc::new(AtomicUsize::new(0));
        let states = Arc::new(Mutex::new(Vec::new()));
        let mut lookup = TitleLookup::new(Arc::clone(&searches));
        lookup.fail_title = Some("b".into());
        let handle = spawn_test(
            gapless_config(),
            RecordingOutput {
                plays: Arc::clone(&plays),
                resumes: Arc::new(AtomicUsize::new(0)),
                pauses: Arc::new(AtomicUsize::new(0)),
            },
            Arc::new(lookup),
            collect_states(Arc::clone(&states)),
        );
        handle.test_load(vec![track("a"), track("b")], true);
        wait_plays(&plays, 1).await;
        assert!(wait_searches(&searches, 2, Duration::from_secs(2)).await);
        tokio::time::sleep(Duration::from_millis(120)).await;
        let logged = states.lock().unwrap_or_else(|p| p.into_inner()).clone();
        assert!(
            logged.iter().all(|(_, error)| error.is_none()),
            "a prefetch failure reached the player: {logged:?}"
        );
        assert_eq!(
            logged.last().map(|(playback, _)| *playback),
            Some(Playback::Playing)
        );
        assert_eq!(plays.load(Ordering::SeqCst), 1);
        handle.shutdown().await;
    }

    /// Next, while the next track's prefetch is mid-flight, waits for that
    /// resolve instead of throwing it away and searching again.
    #[tokio::test]
    async fn next_adopts_the_in_flight_prefetch() {
        let (hold_tx, hold_rx) = oneshot::channel();
        let searches = Arc::new(AtomicUsize::new(0));
        let plays = Arc::new(AtomicUsize::new(0));
        let mut lookup = TitleLookup::new(Arc::clone(&searches));
        lookup.hold_title = Some("b".into());
        lookup.hold = Mutex::new(Some(hold_rx));
        let handle = spawn_test(
            gapless_config(),
            RecordingOutput {
                plays: Arc::clone(&plays),
                resumes: Arc::new(AtomicUsize::new(0)),
                pauses: Arc::new(AtomicUsize::new(0)),
            },
            Arc::new(lookup),
            Arc::new(|_| {}),
        );
        handle.test_load(vec![track("a"), track("b")], true);
        wait_plays(&plays, 1).await;
        assert!(wait_searches(&searches, 2, Duration::from_secs(2)).await);
        handle.command(PlayerCommand::Next).unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            searches.load(Ordering::SeqCst),
            2,
            "Next restarted the resolve already in flight"
        );
        assert_eq!(plays.load(Ordering::SeqCst), 1);
        let _ = hold_tx.send(());
        wait_plays(&plays, 2).await;
        assert_eq!(searches.load(Ordering::SeqCst), 2);
        handle.shutdown().await;
    }

    /// A track that ends while its successor is still resolving waits
    /// for that resolve rather than starting a duplicate.
    #[tokio::test]
    async fn end_of_track_waits_for_the_in_flight_prefetch() {
        let (hold_tx, hold_rx) = oneshot::channel();
        let searches = Arc::new(AtomicUsize::new(0));
        let plays = Arc::new(AtomicUsize::new(0));
        let status = Arc::new(Mutex::new(OutputStatus::Playing));
        let mut lookup = TitleLookup::new(Arc::clone(&searches));
        lookup.hold_title = Some("b".into());
        lookup.hold = Mutex::new(Some(hold_rx));
        let handle = spawn_inner(
            gapless_config(),
            Arc::new(ApiClient::new(
                reqwest::Client::new(),
                Arc::new(crate::api::NetActivity::default()),
                20,
                50,
                crate::api::ApiSource::Shared,
            )),
            reqwest::Client::new(),
            Arc::new(|_| {}),
            Box::new(DeviceOutput {
                status: Arc::clone(&status),
                recover_fails: Arc::new(AtomicUsize::new(0)),
                recovers: Arc::new(AtomicUsize::new(0)),
                plays: Arc::clone(&plays),
                pauses: Arc::new(AtomicUsize::new(0)),
                pcm: Arc::new(Mutex::new(None)),
                decode: Arc::new(Mutex::new(None)),
            }),
            Arc::new(lookup),
        );
        handle.test_load(vec![track("a"), track("b")], true);
        wait_plays(&plays, 1).await;
        assert!(wait_searches(&searches, 2, Duration::from_secs(2)).await);
        *status.lock().unwrap_or_else(|p| p.into_inner()) = OutputStatus::Ended;
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(
            searches.load(Ordering::SeqCst),
            2,
            "the end of the track started a second resolve"
        );
        *status.lock().unwrap_or_else(|p| p.into_inner()) = OutputStatus::Playing;
        let _ = hold_tx.send(());
        wait_plays(&plays, 2).await;
        assert_eq!(searches.load(Ordering::SeqCst), 2);
        handle.shutdown().await;
    }

    /// Queueing a track in front of the prefetched one moves the prefetch
    /// to the track that now comes next.
    #[tokio::test]
    async fn queue_change_retargets_the_prefetch() {
        let searches = Arc::new(AtomicUsize::new(0));
        let plays = Arc::new(AtomicUsize::new(0));
        let handle = spawn_test(
            gapless_config(),
            RecordingOutput {
                plays: Arc::clone(&plays),
                resumes: Arc::new(AtomicUsize::new(0)),
                pauses: Arc::new(AtomicUsize::new(0)),
            },
            Arc::new(TitleLookup::new(Arc::clone(&searches))),
            Arc::new(|_| {}),
        );
        handle.test_load(vec![track("a"), track("b")], true);
        wait_plays(&plays, 1).await;
        assert!(wait_searches(&searches, 2, Duration::from_secs(2)).await);
        tokio::time::sleep(Duration::from_millis(60)).await;
        handle
            .command(PlayerCommand::AddToQueue(track("c")))
            .unwrap();
        assert!(
            wait_searches(&searches, 3, Duration::from_secs(2)).await,
            "the queued track was not prefetched, searches={}",
            searches.load(Ordering::SeqCst)
        );
        tokio::time::sleep(Duration::from_millis(60)).await;
        handle.command(PlayerCommand::Next).unwrap();
        wait_plays(&plays, 2).await;
        assert_eq!(
            searches.load(Ordering::SeqCst),
            3,
            "Next to the queued track should use its prefetch"
        );
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn ready_start_mp3_does_not_seek_zero() {
        let plays = Arc::new(AtomicUsize::new(0));
        let pcm_slot = Arc::new(Mutex::new(None));
        let decode_slot = Arc::new(Mutex::new(None));
        let lookup = Arc::new(ScriptLookup {
            searches: Arc::new(AtomicUsize::new(0)),
            hold: Mutex::new(None),
            body: ScriptedBody {
                chunks: vec![crate::alternate::decode::TONE_MP3.to_vec()],
                fail: None,
                content_length: None,
                fail_after_ms: 0,
            },
        });
        let handle = spawn_inner(
            test_config(),
            Arc::new(ApiClient::new(
                reqwest::Client::new(),
                Arc::new(crate::api::NetActivity::default()),
                20,
                50,
                crate::api::ApiSource::Shared,
            )),
            reqwest::Client::new(),
            Arc::new(|_| {}),
            Box::new(CaptureOutput {
                plays: Arc::clone(&plays),
                pcm: Arc::clone(&pcm_slot),
                decode: Arc::clone(&decode_slot),
                start_ms: Arc::new(Mutex::new(Vec::new())),
            }),
            lookup,
        );
        handle.test_load(vec![track("a")], true);
        wait_plays(&plays, 1).await;
        let epoch = {
            let decode = decode_slot.lock().unwrap_or_else(|p| p.into_inner());
            decode.as_ref().map(DecodeHandle::epoch).unwrap_or(99)
        };
        assert_eq!(epoch, 0, "Ready→start issued a redundant seek");
        let mut pcm = pcm_slot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .expect("pcm");
        assert!(wait_nonzero_sample(&mut pcm, Duration::from_secs(5)));
        drop(pcm);
        drop(decode_slot.lock().unwrap_or_else(|p| p.into_inner()).take());
        handle.shutdown().await;
    }
}

// Keep a generation counter type in this module so stale-event tests stay honest.
#[allow(dead_code)]
pub(crate) struct EventGuard {
    current: Arc<AtomicU64>,
    mine: u64,
}

#[allow(dead_code)]
impl EventGuard {
    pub(crate) fn new(current: Arc<AtomicU64>) -> Self {
        let mine = current.load(Ordering::SeqCst);
        Self { current, mine }
    }

    pub(crate) fn allows(&self) -> bool {
        self.current.load(Ordering::SeqCst) == self.mine
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    #[test]
    fn bumped_generation_rejects_stale_events() {
        let current = Arc::new(AtomicU64::new(3));
        let guard = EventGuard::new(Arc::clone(&current));
        assert!(guard.allows());
        current.fetch_add(1, Ordering::SeqCst);
        assert!(!guard.allows());
    }
}
