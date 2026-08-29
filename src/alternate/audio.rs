//! Local decode and output. Tests inject a null sink; runtime uses rodio.

use anyhow::{Result, anyhow};
use rodio::Source;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::decode::{DecodeHandle, DecodeStatus, PcmSource};

const PCM_STALL: Duration = Duration::from_millis(750);

pub struct PlayInfo {
    pub duration_ms: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputStatus {
    Playing,
    Buffering,
    Ended,
    Failed(String),
    DeviceLost,
}

pub trait AudioOutput {
    fn play_bytes(&mut self, bytes: Vec<u8>, start_ms: u32) -> Result<PlayInfo>;
    fn play_pcm(&mut self, source: PcmSource, decode: DecodeHandle) -> Result<PlayInfo>;
    fn pause(&mut self);
    fn resume(&mut self);
    fn stop(&mut self);
    fn seek(&mut self, ms: u32) -> Result<()>;
    fn set_volume(&mut self, volume: f32);
    fn is_finished(&self) -> bool;
    fn status(&self) -> OutputStatus;
    fn recover(&mut self) -> Result<(), String> {
        Ok(())
    }
}

#[allow(dead_code)]
pub struct NullOutput {
    playing: bool,
    finished: bool,
    volume: f32,
}

impl NullOutput {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            playing: false,
            finished: false,
            volume: 1.0,
        }
    }
}

impl Default for NullOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioOutput for NullOutput {
    fn play_bytes(&mut self, _bytes: Vec<u8>, _start_ms: u32) -> Result<PlayInfo> {
        self.playing = true;
        self.finished = false;
        Ok(PlayInfo { duration_ms: None })
    }

    fn play_pcm(&mut self, _source: PcmSource, _decode: DecodeHandle) -> Result<PlayInfo> {
        self.playing = true;
        self.finished = false;
        Ok(PlayInfo { duration_ms: None })
    }

    fn pause(&mut self) {
        self.playing = false;
    }

    fn resume(&mut self) {
        self.playing = true;
    }

    fn stop(&mut self) {
        self.playing = false;
        self.finished = false;
    }

    fn seek(&mut self, _ms: u32) -> Result<()> {
        Ok(())
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
    }

    fn is_finished(&self) -> bool {
        self.finished
    }

    fn status(&self) -> OutputStatus {
        if self.finished {
            OutputStatus::Ended
        } else {
            OutputStatus::Playing
        }
    }
}

pub struct DirectRodioOutput {
    stream: rodio::OutputStream,
    sink: rodio::Sink,
    decode: Option<DecodeHandle>,
    lost: Arc<AtomicBool>,
    volume: f32,
}

impl DirectRodioOutput {
    pub fn open() -> Result<Self> {
        let lost = Arc::new(AtomicBool::new(false));
        let (stream, sink) = Self::open_pair(Arc::clone(&lost))?;
        Ok(Self {
            stream,
            sink,
            decode: None,
            lost,
            volume: 1.0,
        })
    }

    fn open_pair(
        lost: Arc<AtomicBool>,
    ) -> Result<(rodio::OutputStream, rodio::Sink), anyhow::Error> {
        let flag = Arc::clone(&lost);
        let stream = rodio::OutputStreamBuilder::from_default_device()
            .map_err(|error| anyhow!("{error}"))?
            .with_error_callback(move |error| {
                log::warn!("audio stream error: {error}");
                flag.store(true, Ordering::SeqCst);
            })
            .open_stream_or_fallback()
            .map_err(|error| anyhow!("{error}"))?;
        let sink = rodio::Sink::connect_new(stream.mixer());
        Ok((stream, sink))
    }
}

impl AudioOutput for DirectRodioOutput {
    fn play_bytes(&mut self, bytes: Vec<u8>, start_ms: u32) -> Result<PlayInfo> {
        self.sink.stop();
        if let Some(decode) = self.decode.take() {
            decode.stop();
        }
        let cursor = Cursor::new(bytes);
        let decoder = rodio::Decoder::new(cursor).map_err(|error| anyhow!("{error}"))?;
        let duration_ms = decoder
            .total_duration()
            .map(|duration| duration.as_millis() as u32);
        self.sink.append(decoder);
        if start_ms > 0 {
            let _ = self
                .sink
                .try_seek(Duration::from_millis(u64::from(start_ms)));
        }
        self.sink.play();
        Ok(PlayInfo { duration_ms })
    }

    fn play_pcm(&mut self, source: PcmSource, decode: DecodeHandle) -> Result<PlayInfo> {
        self.sink.stop();
        if let Some(old) = self.decode.take() {
            old.stop();
        }
        self.decode = Some(decode);
        self.sink.append(source);
        self.sink.play();
        Ok(PlayInfo { duration_ms: None })
    }

    fn pause(&mut self) {
        self.sink.pause();
    }

    fn resume(&mut self) {
        self.sink.play();
    }

    fn stop(&mut self) {
        self.sink.stop();
        if let Some(decode) = self.decode.take() {
            decode.stop();
        }
    }

    fn seek(&mut self, ms: u32) -> Result<()> {
        if let Some(decode) = &self.decode {
            decode.seek(ms);
            return Ok(());
        }
        self.sink
            .try_seek(Duration::from_millis(u64::from(ms)))
            .map_err(|error| anyhow!("{error}"))
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.sink.set_volume(self.volume);
    }

    fn is_finished(&self) -> bool {
        matches!(self.status(), OutputStatus::Ended)
    }

    fn status(&self) -> OutputStatus {
        if self.lost.load(Ordering::SeqCst) {
            return OutputStatus::DeviceLost;
        }
        if let Some(decode) = &self.decode {
            if let Some(message) = decode.error() {
                return OutputStatus::Failed(message);
            }
            match decode.status() {
                DecodeStatus::Failed => {
                    return OutputStatus::Failed(
                        decode
                            .error()
                            .unwrap_or_else(|| "Couldn't decode audio.".into()),
                    );
                }
                DecodeStatus::Cancelled => return OutputStatus::Ended,
                DecodeStatus::Ended if self.sink.empty() => return OutputStatus::Ended,
                DecodeStatus::Ended | DecodeStatus::Running | DecodeStatus::Starting => {
                    if !self.sink.is_paused() && decode.starved(PCM_STALL) {
                        return OutputStatus::Buffering;
                    }
                    return OutputStatus::Playing;
                }
            }
        }
        if self.sink.empty() {
            OutputStatus::Ended
        } else {
            OutputStatus::Playing
        }
    }

    fn recover(&mut self) -> Result<(), String> {
        if !self.lost.load(Ordering::SeqCst) {
            return Ok(());
        }
        let lost = Arc::clone(&self.lost);
        lost.store(false, Ordering::SeqCst);
        match Self::open_pair(lost) {
            Ok((stream, sink)) => {
                self.sink.stop();
                self.stream = stream;
                self.sink = sink;
                self.sink.set_volume(self.volume);
                Ok(())
            }
            Err(error) => {
                self.lost.store(true, Ordering::SeqCst);
                Err(error.to_string())
            }
        }
    }
}

impl Drop for DirectRodioOutput {
    fn drop(&mut self) {
        self.sink.stop();
        if let Some(decode) = self.decode.take() {
            decode.stop();
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub type RodioOutput = DirectRodioOutput;

// CoreAudio streams are deliberately !Send in cpal. Keep the real stream on
// the thread that opened it and expose a Send command proxy to the async
// alternate engine instead of lying to the type system.
#[cfg(target_os = "macos")]
pub struct RodioOutput {
    tx: std::sync::mpsc::Sender<MacCommand>,
    join: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "macos")]
enum MacCommand {
    PlayBytes(
        Vec<u8>,
        u32,
        std::sync::mpsc::SyncSender<std::result::Result<PlayInfo, String>>,
    ),
    PlayPcm(
        PcmSource,
        DecodeHandle,
        std::sync::mpsc::SyncSender<std::result::Result<PlayInfo, String>>,
    ),
    Pause,
    Resume,
    Stop,
    Seek(
        u32,
        std::sync::mpsc::SyncSender<std::result::Result<(), String>>,
    ),
    SetVolume(f32),
    Status(std::sync::mpsc::SyncSender<OutputStatus>),
    Recover(std::sync::mpsc::SyncSender<std::result::Result<(), String>>),
    Shutdown,
}

#[cfg(target_os = "macos")]
impl RodioOutput {
    pub fn open() -> Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let join = std::thread::Builder::new()
            .name("oxidify-coreaudio".into())
            .spawn(move || match DirectRodioOutput::open() {
                Ok(mut output) => {
                    let _ = ready_tx.send(Ok(()));
                    while let Ok(command) = rx.recv() {
                        match command {
                            MacCommand::PlayBytes(bytes, start_ms, reply) => {
                                let result = output
                                    .play_bytes(bytes, start_ms)
                                    .map_err(|error| error.to_string());
                                let _ = reply.send(result);
                            }
                            MacCommand::PlayPcm(source, decode, reply) => {
                                let result = output
                                    .play_pcm(source, decode)
                                    .map_err(|error| error.to_string());
                                let _ = reply.send(result);
                            }
                            MacCommand::Pause => output.pause(),
                            MacCommand::Resume => output.resume(),
                            MacCommand::Stop => output.stop(),
                            MacCommand::Seek(ms, reply) => {
                                let result = output.seek(ms).map_err(|error| error.to_string());
                                let _ = reply.send(result);
                            }
                            MacCommand::SetVolume(volume) => output.set_volume(volume),
                            MacCommand::Status(reply) => {
                                let _ = reply.send(output.status());
                            }
                            MacCommand::Recover(reply) => {
                                let _ = reply.send(output.recover());
                            }
                            MacCommand::Shutdown => break,
                        }
                    }
                    output.stop();
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                }
            })
            .map_err(|error| anyhow!("couldn't start CoreAudio thread: {error}"))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                tx,
                join: Some(join),
            }),
            Ok(Err(error)) => {
                let _ = join.join();
                Err(anyhow!(error))
            }
            Err(error) => {
                let _ = join.join();
                Err(anyhow!("CoreAudio thread stopped during startup: {error}"))
            }
        }
    }

    fn play_request(
        &self,
        command: impl FnOnce(
            std::sync::mpsc::SyncSender<std::result::Result<PlayInfo, String>>,
        ) -> MacCommand,
    ) -> Result<PlayInfo> {
        let (reply, result) = std::sync::mpsc::sync_channel(1);
        self.tx
            .send(command(reply))
            .map_err(|_| anyhow!("CoreAudio thread stopped"))?;
        result
            .recv()
            .map_err(|_| anyhow!("CoreAudio thread stopped"))?
            .map_err(anyhow::Error::msg)
    }
}

#[cfg(target_os = "macos")]
impl AudioOutput for RodioOutput {
    fn play_bytes(&mut self, bytes: Vec<u8>, start_ms: u32) -> Result<PlayInfo> {
        self.play_request(|reply| MacCommand::PlayBytes(bytes, start_ms, reply))
    }

    fn play_pcm(&mut self, source: PcmSource, decode: DecodeHandle) -> Result<PlayInfo> {
        self.play_request(|reply| MacCommand::PlayPcm(source, decode, reply))
    }

    fn pause(&mut self) {
        let _ = self.tx.send(MacCommand::Pause);
    }

    fn resume(&mut self) {
        let _ = self.tx.send(MacCommand::Resume);
    }

    fn stop(&mut self) {
        let _ = self.tx.send(MacCommand::Stop);
    }

    fn seek(&mut self, ms: u32) -> Result<()> {
        let (reply, result) = std::sync::mpsc::sync_channel(1);
        self.tx
            .send(MacCommand::Seek(ms, reply))
            .map_err(|_| anyhow!("CoreAudio thread stopped"))?;
        result
            .recv()
            .map_err(|_| anyhow!("CoreAudio thread stopped"))?
            .map_err(anyhow::Error::msg)
    }

    fn set_volume(&mut self, volume: f32) {
        let _ = self.tx.send(MacCommand::SetVolume(volume));
    }

    fn is_finished(&self) -> bool {
        matches!(self.status(), OutputStatus::Ended)
    }

    fn status(&self) -> OutputStatus {
        let (reply, result) = std::sync::mpsc::sync_channel(1);
        if self.tx.send(MacCommand::Status(reply)).is_err() {
            return OutputStatus::Failed("CoreAudio thread stopped".into());
        }
        result
            .recv()
            .unwrap_or_else(|_| OutputStatus::Failed("CoreAudio thread stopped".into()))
    }

    fn recover(&mut self) -> std::result::Result<(), String> {
        let (reply, result) = std::sync::mpsc::sync_channel(1);
        self.tx
            .send(MacCommand::Recover(reply))
            .map_err(|_| "CoreAudio thread stopped".to_string())?;
        result
            .recv()
            .map_err(|_| "CoreAudio thread stopped".to_string())?
    }
}

#[cfg(target_os = "macos")]
impl Drop for RodioOutput {
    fn drop(&mut self) {
        let _ = self.tx.send(MacCommand::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub fn volume_f32(volume: u16) -> f32 {
    f32::from(volume) / f32::from(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_output_needs_no_hardware() {
        let mut output = NullOutput::new();
        output.set_volume(0.5);
        assert!(output.play_bytes(vec![0, 1, 2], 0).is_ok());
        assert!(!output.is_finished());
        output.pause();
        output.resume();
        assert!(output.seek(1000).is_ok());
        output.stop();
    }

    #[test]
    fn decoder_accepts_tiny_wav_without_a_device() {
        let bytes = tiny_wav();
        rodio::Decoder::new(Cursor::new(bytes)).expect("wav should decode");
    }

    fn tiny_wav() -> Vec<u8> {
        let sample_rate: u32 = 8_000;
        let samples: Vec<i16> = (0..160)
            .map(|i| if i % 2 == 0 { 64 } else { -64 })
            .collect();
        let data_bytes = (samples.len() * 2) as u32;
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
        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }
}
