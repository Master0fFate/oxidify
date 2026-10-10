//! Audio output for local playback.
//!
//! librespot ships a rodio sink, but it opens the output device with
//! `.unwrap()` on the player thread, and the release profile aborts on any
//! panic. A Windows PC with no default playback device (nothing in the jack,
//! a Bluetooth headset that is off, a remote desktop session) therefore took
//! the whole app down the moment playback was authorized, before the
//! credential was even stored. This sink opens the device only when playback
//! starts, reports a failure as a sink error (librespot answers by pausing),
//! and tells the interface why, so the app stays up as a Connect remote and
//! plays as soon as an output exists.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering, fence};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait};
use librespot_playback::audio_backend::{Sink, SinkError, SinkResult};
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::VolumeGetter;
use librespot_playback::{NUM_CHANNELS, SAMPLE_RATE};

use crate::resample::Resampler;

/// The backend name Settings uses for this sink.
pub const NAME: &str = "rodio";

/// Told about output failures, with a message fit for the interface.
pub type ErrorHook = Arc<dyn Fn(String) + Send + Sync>;

/// How many chunks may wait in rodio's queue before `write` blocks. Chunks
/// run from a few hundred to a few thousand samples; this is about a fifth
/// of a second, which is also how long a pause takes to be heard, since
/// librespot lets the queue play out first.
const QUEUE_LIMIT: usize = 12;

/// How much of a full queue rodio plays before `write` is woken to top it
/// up. librespot's packets hold 4 to 13 ms of sound, so polling every 10 ms
/// kept the decoder thread awake about 100 times a second; waking only once
/// this much has played brings it to about 30.
const REFILL: Duration = Duration::from_millis(20);

/// Longest `write` or `stop` waits for rodio before looking at the device
/// again: a stream that has failed finishes no chunk to wake them.
const QUEUE_WAIT: Duration = Duration::from_millis(50);

/// How long `stop` lets the queue play out before pausing regardless.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

pub struct RodioSink {
    /// The output device name from Settings; `None` means the default.
    device: Option<String>,
    output: Option<Output>,
    on_error: ErrorHook,
    /// The player's volume, applied here at the output so a change is heard
    /// at once instead of after the queue drains.
    volume: Box<dyn VolumeGetter + Send>,
    applied_volume: f32,
}

struct Output {
    sink: rodio::Sink,
    _stream: rodio::OutputStream,
    /// Set from the audio thread when the stream dies (device unplugged).
    failed: Arc<AtomicBool>,
    sample_rate: u32,
    resampler: Option<Resampler>,
    /// How much sound waits in rodio's queue, kept by the chunks themselves.
    queued: Arc<Queued>,
}

impl Output {
    fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }
}

/// The samples appended to rodio and the samples it has finished with, so a
/// writer can sleep until the queue has drained to a level instead of
/// polling it.
struct Queued {
    appended: AtomicU64,
    consumed: AtomicU64,
    /// While a writer waits for room, the queued level at or below which it
    /// is woken; zero while nothing waits.
    wake_at: AtomicU64,
}

impl Queued {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            appended: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            wake_at: AtomicU64::new(0),
        })
    }

    fn samples(&self) -> u64 {
        self.appended
            .load(Ordering::Relaxed)
            .saturating_sub(self.consumed.load(Ordering::Relaxed))
    }

    /// Sleeps the calling thread until rodio has played `refill` samples of
    /// what is queued, or `QUEUE_WAIT` has passed.
    fn wait_for_room(&self, refill: u64) {
        let wake_at = self.samples().saturating_sub(refill).max(1);
        self.wake_at.store(wake_at, Ordering::Relaxed);
        // Pairs with the fence in `drained`: either the chunk that reaches
        // the level sees the wait, or this sees that chunk already gone.
        fence(Ordering::SeqCst);
        if self.samples() > wake_at {
            thread::park_timeout(QUEUE_WAIT);
        }
        self.wake_at.store(0, Ordering::Relaxed);
    }

    /// rodio has finished with a chunk: wakes the writer once the queue has
    /// drained to the level it waits for, and only then.
    fn drained(&self, writer: &thread::Thread) {
        fence(Ordering::SeqCst);
        let wake_at = self.wake_at.load(Ordering::Relaxed);
        if wake_at != 0
            && self.samples() <= wake_at
            && self
                .wake_at
                .compare_exchange(wake_at, 0, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            writer.unpark();
        }
    }
}

/// One packet's samples in rodio's queue, which settle up with [`Queued`]
/// as they are played and when the chunk is dropped: rodio drops whole
/// sources on `stop`, so a chunk can end without being played, and the
/// count has to describe the queue either way.
struct Chunk {
    samples: rodio::buffer::SamplesBuffer,
    queued: Arc<Queued>,
    remaining: u64,
    /// The thread that queued this chunk, which may be waiting for room.
    writer: thread::Thread,
}

impl Chunk {
    fn new(samples: rodio::buffer::SamplesBuffer, count: u64, queued: &Arc<Queued>) -> Self {
        queued.appended.fetch_add(count, Ordering::Relaxed);
        Self {
            samples,
            queued: Arc::clone(queued),
            remaining: count,
            writer: thread::current(),
        }
    }
}

impl Iterator for Chunk {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.samples.next();
        if sample.is_some() {
            self.remaining -= 1;
            self.queued.consumed.fetch_add(1, Ordering::Relaxed);
        }
        sample
    }
}

impl rodio::Source for Chunk {
    fn current_span_len(&self) -> Option<usize> {
        self.samples.current_span_len()
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.samples.channels()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.samples.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.samples.total_duration()
    }
}

impl Drop for Chunk {
    fn drop(&mut self) {
        self.queued
            .consumed
            .fetch_add(self.remaining, Ordering::Relaxed);
        self.queued.drained(&self.writer);
    }
}

impl RodioSink {
    pub fn new(
        device: Option<String>,
        on_error: ErrorHook,
        volume: Box<dyn VolumeGetter + Send>,
    ) -> Self {
        Self {
            device,
            output: None,
            on_error,
            volume,
            applied_volume: -1.0,
        }
    }

    fn apply_volume(&mut self) {
        let factor = self.volume.attenuation_factor() as f32;
        if let Some(output) = &self.output
            && factor != self.applied_volume
        {
            output.sink.set_volume(factor);
            self.applied_volume = factor;
        }
    }

    /// Opens the output if it is not open, or if it died since.
    fn ensure_open(&mut self) -> SinkResult<()> {
        if self.output.as_ref().is_some_and(Output::failed) {
            log::warn!("the audio output stopped working; reopening it");
            self.output = None;
        }
        if self.output.is_some() {
            return Ok(());
        }
        match open_output(self.device.as_deref()) {
            Ok(output) => {
                self.output = Some(output);
                self.applied_volume = -1.0;
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                log::error!("{message}");
                (self.on_error)(message.clone());
                Err(SinkError::ConnectionRefused(message))
            }
        }
    }
}

impl Sink for RodioSink {
    fn start(&mut self) -> SinkResult<()> {
        self.ensure_open()?;
        self.apply_volume();
        if let Some(output) = &self.output {
            output.sink.play();
        }
        Ok(())
    }

    /// Never fails: librespot exits the process when a sink cannot stop.
    fn stop(&mut self) -> SinkResult<()> {
        if let Some(output) = &self.output {
            let deadline = Instant::now() + DRAIN_TIMEOUT;
            while !output.sink.empty() && !output.failed() && Instant::now() < deadline {
                output.queued.wait_for_room(u64::MAX);
            }
            output.sink.pause();
        }
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let samples = packet
            .samples()
            .map_err(|error| SinkError::OnWrite(error.to_string()))?;
        let samples = converter.f64_to_f32(samples);
        self.ensure_open()?;
        self.apply_volume();
        let Some(output) = &mut self.output else {
            return Err(SinkError::NotConnected(
                "the audio output is not open".into(),
            ));
        };
        let samples = match &mut output.resampler {
            Some(resampler) => resampler.process(&samples),
            None => samples,
        };
        if !samples.is_empty() {
            let count = samples.len() as u64;
            let buffer = rodio::buffer::SamplesBuffer::new(
                NUM_CHANNELS as rodio::ChannelCount,
                output.sample_rate as rodio::SampleRate,
                samples,
            );
            output
                .sink
                .append(Chunk::new(buffer, count, &output.queued));
        }
        // Let rodio drain a little; without this the whole track would be
        // decoded into memory at once. A full queue sleeps until rodio has
        // played `REFILL` of it, then is topped up in one go.
        let refill = u64::from(output.sample_rate)
            * u64::from(NUM_CHANNELS as u32)
            * REFILL.as_millis() as u64
            / 1_000;
        let mut failed = false;
        while output.sink.len() > QUEUE_LIMIT {
            if output.failed() {
                failed = true;
                break;
            }
            output.queued.wait_for_room(refill);
        }
        if failed {
            // Headphones connected or the default output changed: the old
            // stream is gone, so open the output to use now and carry on.
            // Only an output that will not open stops the music.
            log::warn!("the audio output stopped working; reopening it");
            self.output = None;
            self.applied_volume = -1.0;
            self.ensure_open()?;
            self.apply_volume();
            if let Some(output) = &self.output {
                output.sink.play();
            }
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
enum OpenError {
    #[error("No audio output device was found. Connect or enable one, then press play again.")]
    NoDevice,
    #[error("Cannot list the audio devices: {0}")]
    Devices(#[from] cpal::DevicesError),
    #[error("Cannot open the audio output: {0}")]
    Stream(#[from] rodio::StreamError),
}

fn open_output(preferred: Option<&str>) -> Result<Output, OpenError> {
    let host = cpal::default_host();
    let device = match preferred.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => {
            let chosen = host
                .output_devices()?
                .find(|device| device.name().is_ok_and(|found| found == name));
            match chosen {
                Some(device) => device,
                None => {
                    log::warn!("audio device {name:?} is not available; using the default");
                    host.default_output_device().ok_or(OpenError::NoDevice)?
                }
            }
        }
        None => host.default_output_device().ok_or(OpenError::NoDevice)?,
    };
    log::info!(
        "audio output: {}",
        device.name().unwrap_or_else(|_| "[unknown device]".into())
    );

    let failed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&failed);
    // Spotify's native stereo 44.1 kHz first, so nothing is resampled; rodio
    // falls back to whatever the device does support.
    let mut stream = rodio::OutputStreamBuilder::from_device(device)?
        .with_channels(NUM_CHANNELS as rodio::ChannelCount)
        .with_sample_rate(SAMPLE_RATE as rodio::SampleRate)
        .with_error_callback(move |error: cpal::StreamError| {
            log::error!("audio stream error: {error}");
            flag.store(true, Ordering::Relaxed);
        })
        .open_stream_or_fallback()?;
    stream.log_on_drop(false);
    let sample_rate = stream.config().sample_rate();
    let resampler = Resampler::new(SAMPLE_RATE, sample_rate, NUM_CHANNELS as usize);
    let sink = rodio::Sink::connect_new(stream.mixer());
    Ok(Output {
        sink,
        _stream: stream,
        failed,
        sample_rate,
        resampler,
        queued: Queued::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A machine without audio (CI, a PC with nothing plugged in) must get
    /// an error and a message for the interface, never a panic. A machine
    /// with audio opens its default device.
    #[test]
    fn starting_without_a_device_is_an_error_not_a_panic() {
        let reported: Arc<Mutex<Option<String>>> = Arc::default();
        let store = Arc::clone(&reported);
        let mut sink = RodioSink::new(
            Some("no such device".into()),
            Arc::new(move |message| *store.lock().unwrap() = Some(message)),
            Box::new(librespot_playback::mixer::NoOpVolume),
        );
        match sink.start() {
            Ok(()) => assert!(reported.lock().unwrap().is_none()),
            Err(SinkError::ConnectionRefused(message)) => {
                assert_eq!(reported.lock().unwrap().as_deref(), Some(message.as_str()));
            }
            Err(other) => panic!("unexpected error: {other}"),
        }
        assert!(sink.stop().is_ok());
    }

    /// A writer waiting for room sleeps through the chunks that leave the
    /// queue above its level, and the one that reaches it wakes the writer,
    /// once. Polling instead woke the decoder thread every 10 ms.
    #[test]
    fn a_waiting_writer_wakes_once_the_queue_has_drained_enough() {
        let queued = Queued::new();
        let chunk = |samples: usize| {
            let buffer = rodio::buffer::SamplesBuffer::new(2, 44_100, vec![0.0; samples]);
            Chunk::new(buffer, samples as u64, &queued)
        };
        let mut chunks: Vec<_> = (0..4).map(|_| chunk(10)).collect();
        assert_eq!(queued.samples(), 40);
        queued.wake_at.store(20, Ordering::SeqCst);

        drop(chunks.remove(0));
        assert_eq!(queued.samples(), 30);
        assert_eq!(queued.wake_at.load(Ordering::SeqCst), 20, "still asleep");

        drop(chunks.remove(0));
        assert_eq!(queued.wake_at.load(Ordering::SeqCst), 0, "woken, once");
        // The chunks were queued from this thread, so the wake is its own:
        // the token is waiting, and parking returns at once.
        let started = Instant::now();
        thread::park_timeout(Duration::from_secs(5));
        assert!(started.elapsed() < Duration::from_secs(1));

        // Samples played count as they go, and a dropped chunk settles the rest.
        let mut played = chunks.pop().unwrap();
        assert!(played.next().is_some());
        assert_eq!(queued.samples(), 19);
        drop(played);
        drop(chunks);
        assert_eq!(queued.samples(), 0);
    }

    /// A stream that has stopped consuming finishes no chunk, so the wait
    /// gives up by itself and `write` gets to look at the device.
    #[test]
    fn waiting_for_room_gives_up_when_nothing_plays() {
        let queued = Queued::new();
        queued.appended.store(1_000, Ordering::SeqCst);
        let started = Instant::now();
        queued.wait_for_room(100);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(queued.wake_at.load(Ordering::SeqCst), 0);
    }
}
