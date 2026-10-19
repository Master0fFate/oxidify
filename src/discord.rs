use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::app::NowPlaying;

const CLIENT_ID: &str = "1561781510099697806";
const LOGO_ASSET: &str = "oxidify";
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const RETRY: Duration = Duration::from_secs(15);
const SEND_SPACING: Duration = Duration::from_secs(1);
const FIELD_LIMIT: usize = 128;

#[derive(Clone, Debug, PartialEq)]
pub struct Presence {
    title: String,
    artists: String,
    album: String,
    cover: Option<String>,
    link: Option<String>,
    started_ms: i64,
    ends_ms: i64,
}

impl Presence {
    pub fn from_now_playing(now: &NowPlaying) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as i64);
        let started_ms = now_ms - i64::from(now.position_ms);
        Self {
            title: now.title.clone(),
            artists: now.subtitle.clone(),
            album: now.album_name.clone(),
            cover: now.art_url.clone(),
            link: crate::util::open_spotify_url(&now.uri),
            started_ms,
            ends_ms: started_ms + i64::from(now.duration_ms),
        }
    }

    fn same_song_and_timing(&self, other: &Self) -> bool {
        self.title == other.title
            && self.artists == other.artists
            && self.album == other.album
            && self.cover == other.cover
            && self.link == other.link
            && (self.started_ms - other.started_ms).abs() < 1500
            && self.ends_ms - self.started_ms == other.ends_ms - other.started_ms
    }

    fn activity(&self) -> Value {
        let mut assets = json!({
            "large_image": self.cover.as_deref().unwrap_or(LOGO_ASSET),
        });
        if !self.album.is_empty() {
            assets["large_text"] = json!(field(&self.album));
        }
        if self.cover.is_some() {
            assets["small_image"] = json!(LOGO_ASSET);
            assets["small_text"] = json!("Oxidify");
            assets["small_url"] = json!(REPOSITORY);
            if let Some(link) = &self.link {
                assets["large_url"] = json!(link);
            }
        } else {
            assets["large_url"] = json!(REPOSITORY);
        }
        let mut activity = json!({
            "type": 2,
            "details": field(&self.title),
            "timestamps": { "start": self.started_ms, "end": self.ends_ms },
            "assets": assets,
        });
        if !self.artists.is_empty() {
            activity["state"] = json!(field(&self.artists));
        }
        if let Some(link) = &self.link {
            activity["buttons"] = json!([{ "label": "Open in Spotify", "url": link }]);
        }
        activity
    }
}

fn field(text: &str) -> String {
    let mut text: String = text.chars().take(FIELD_LIMIT).collect();
    while text.chars().count() < 2 {
        text.push(' ');
    }
    text
}

pub struct DiscordPresence {
    sender: Sender<Option<Presence>>,
    shown: Option<Presence>,
}

impl DiscordPresence {
    pub fn spawn() -> Self {
        let (sender, receiver) = mpsc::channel();
        if let Err(error) = std::thread::Builder::new()
            .name("discord-presence".into())
            .spawn(move || run(receiver))
        {
            log::warn!("Discord presence is unavailable: {error}");
        }
        Self {
            sender,
            shown: None,
        }
    }

    pub fn show(&mut self, presence: Option<Presence>) {
        let unchanged = match (&self.shown, &presence) {
            (Some(shown), Some(next)) => shown.same_song_and_timing(next),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        self.shown = presence.clone();
        let _ = self.sender.send(presence);
    }
}

fn run(receiver: Receiver<Option<Presence>>) {
    let mut pipe: Option<Box<dyn Pipe>> = None;
    let mut wanted: Option<Presence> = None;
    let mut applied = true;
    let mut nonce = 0u64;
    loop {
        match receiver.recv_timeout(RETRY) {
            Ok(next) => {
                wanted = next;
                applied = false;
            }
            Err(RecvTimeoutError::Timeout) if applied => continue,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while let Ok(next) = receiver.try_recv() {
            wanted = next;
        }
        if pipe.is_none() && wanted.is_none() {
            applied = true;
            continue;
        }
        if pipe.is_none() {
            match connect() {
                Ok(connected) => pipe = Some(connected),
                Err(error) => {
                    log::debug!("Discord is not reachable: {error}");
                    continue;
                }
            }
        }
        if let Some(connected) = pipe.as_mut() {
            nonce += 1;
            match set_activity(connected.as_mut(), wanted.as_ref(), nonce) {
                Ok(()) => applied = true,
                Err(error) => {
                    log::debug!("Discord connection lost: {error}");
                    pipe = None;
                }
            }
        }
        std::thread::sleep(SEND_SPACING);
    }
}

trait Pipe: Read + Write + Send {}

impl<T: Read + Write + Send> Pipe for T {}

#[cfg(windows)]
fn open(index: u8) -> io::Result<Box<dyn Pipe>> {
    let pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!(r"\\.\pipe\discord-ipc-{index}"))?;
    Ok(Box::new(pipe))
}

#[cfg(unix)]
fn open(index: u8) -> io::Result<Box<dyn Pipe>> {
    let base = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .into_iter()
        .find_map(std::env::var_os)
        .map_or_else(
            || std::path::PathBuf::from("/tmp"),
            std::path::PathBuf::from,
        );
    let name = format!("discord-ipc-{index}");
    for folder in ["", "app/com.discordapp.Discord", "snap.discord"] {
        if let Ok(stream) = std::os::unix::net::UnixStream::connect(base.join(folder).join(&name)) {
            return Ok(Box::new(stream));
        }
    }
    Err(io::ErrorKind::NotFound.into())
}

fn connect() -> io::Result<Box<dyn Pipe>> {
    let mut pipe = (0..10)
        .find_map(|index| open(index).ok())
        .ok_or(io::ErrorKind::NotFound)?;
    send(pipe.as_mut(), 0, &json!({ "v": 1, "client_id": CLIENT_ID }))?;
    let ready = reply(pipe.as_mut())?;
    if ready["evt"] != "READY" {
        return Err(io::Error::other(format!(
            "unexpected handshake reply {ready}"
        )));
    }
    Ok(pipe)
}

fn set_activity(pipe: &mut dyn Pipe, presence: Option<&Presence>, nonce: u64) -> io::Result<()> {
    let request = json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": std::process::id(),
            "activity": presence.map(Presence::activity),
        },
        "nonce": nonce.to_string(),
    });
    send(pipe, 1, &request)?;
    let answer = reply(pipe)?;
    if answer["evt"] == "ERROR" {
        log::warn!(
            "Discord rejected the presence: {}",
            answer["data"]["message"]
        );
    }
    Ok(())
}

fn send(pipe: &mut dyn Pipe, op: u32, payload: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(payload)?;
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    pipe.write_all(&frame)?;
    pipe.flush()
}

fn reply(pipe: &mut dyn Pipe) -> io::Result<Value> {
    loop {
        let mut header = [0u8; 8];
        pipe.read_exact(&mut header)?;
        let [o0, o1, o2, o3, l0, l1, l2, l3] = header;
        let op = u32::from_le_bytes([o0, o1, o2, o3]);
        let mut body = vec![0u8; u32::from_le_bytes([l0, l1, l2, l3]) as usize];
        pipe.read_exact(&mut body)?;
        let payload: Value = serde_json::from_slice(&body)?;
        match op {
            2 => {
                return Err(io::Error::other(format!(
                    "Discord closed the pipe: {payload}"
                )));
            }
            3 => send(pipe, 4, &payload)?,
            _ => return Ok(payload),
        }
    }
}
