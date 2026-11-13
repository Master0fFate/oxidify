//! One running instance at a time, and its remote-control channel.
//!
//! Two copies of Oxidify fight over things a user notices: two Spotify
//! Connect devices with the same name, two MPRIS players for the media keys
//! to disagree about, two tray icons. So a second launch does not start a
//! second app; it asks the one already running to show itself and exits.
//!
//! Detection is a D-Bus well-known name, requested without queuing. The bus
//! grants it to exactly one process and releases it the moment that process
//! ends, crash included, so there is no stale lock file to clean up and no
//! race between two launches at once. Surfacing the running instance is the
//! MPRIS `Raise` method it already implements, which is also what a desktop's
//! own "jump to the running player" gesture calls.
//!
//! This uses zbus's blocking API deliberately. Both of zbus's executors are
//! compiled in here (the tray brings async-io, MPRIS brings tokio), so an
//! async connection awaited from an arbitrary runtime is not guaranteed to be
//! driven. The blocking API owns that problem, and a check that runs once
//! before the window exists has no reason to be asynchronous anyway.
//!
//! Linux needs none of this: MPRIS already gives `playerctl` the same verbs,
//! so the D-Bus name stays a pure instance guard there.
//!
//! The same channel is what the Stream Deck plugin speaks. That is why the
//! verbs cover more than a media key can ask for -- an explicit shuffle or
//! repeat state rather than only a toggle, saving the playing track, playing
//! a URI, listing devices and moving playback to one -- and why the
//! now-playing snapshot carries the artwork URL and the saved flag a key
//! needs to draw itself. A client polls; nothing is pushed.
//!

/// The name held for the lifetime of the running instance.
#[cfg(target_os = "linux")]
const INSTANCE_NAME: &str = "rocks.oxidify.Instance";

/// The MPRIS player to ask when another instance already holds the name.
#[cfg(target_os = "linux")]
const MPRIS_NAME: &str = "org.mpris.MediaPlayer2.oxidify";

pub enum Outcome {
    /// This process is the only instance. Hold the guard until it exits.
    Only(Guard),
    /// Another instance is running and has been asked to show its window.
    Surfaced,
}

/// What a control client asked the running instance to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlCommand {
    /// Bring the window forward, creating it if the app lives in the tray.
    Show,
    /// Shut down the running application instead of merely hiding its window.
    Quit,
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
    /// Milliseconds; negative seeks backwards.
    SeekBy(i64),
    /// Percentage points; negative lowers the volume.
    VolumeBy(i8),
    /// Absolute percentage.
    SetVolume(u8),
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    /// Shuffle set outright. A key that draws the current state wants to say
    /// which state it is asking for, or a missed update leaves the two
    /// disagreeing until the next press.
    SetShuffle(bool),
    /// Repeat set outright, for the same reason.
    SetRepeat(crate::player::RepeatMode),
    /// Absolute position, in milliseconds.
    SeekTo(u32),
    /// Save the playing track to the library, or take it back out.
    ToggleSaved,
    /// Play a `spotify:` URI: a track, album, playlist, artist, or show.
    PlayUri(String),
    /// Move playback to a Spotify Connect device, by id.
    Transfer(String),
    /// Refresh the device list. Sent by the `devices` read, which answers
    /// from a snapshot that is only as fresh as the app's last look.
    RefreshDevices,
}

/// Holds whatever marks this process as the running instance. Dropping it
/// gives that up.
pub struct Guard {
    #[cfg(target_os = "linux")]
    _connection: Option<mpris_server::zbus::blocking::Connection>,
    /// Filled by control clients, drained by the app every frame. On Linux
    /// the same requests arrive through MPRIS instead and this stays empty.
    commands: std::sync::Arc<std::sync::Mutex<Vec<ControlCommand>>>,
    /// One line about the current track, kept fresh by the app so the
    /// listener can answer `nowplaying` without touching app state.
    now_playing: std::sync::Arc<std::sync::Mutex<String>>,
    /// The Spotify Connect devices the app last saw, as one line of JSON,
    /// kept fresh the same way and for the same reason.
    devices: std::sync::Arc<std::sync::Mutex<String>>,
}

impl Guard {
    /// The queue a control client's commands land in. The app drains it.
    pub fn commands(&self) -> std::sync::Arc<std::sync::Mutex<Vec<ControlCommand>>> {
        std::sync::Arc::clone(&self.commands)
    }

    /// The slot the app writes the now-playing snapshot into.
    pub fn now_playing_slot(&self) -> std::sync::Arc<std::sync::Mutex<String>> {
        std::sync::Arc::clone(&self.now_playing)
    }

    /// The slot the app writes the device list into.
    pub fn devices_slot(&self) -> std::sync::Arc<std::sync::Mutex<String>> {
        std::sync::Arc::clone(&self.devices)
    }
}

/// What the app writes into the snapshot slot before anything plays, and
/// what `nowplaying` reports when nothing does.
pub const NOTHING_PLAYING: &str = "stopped";

/// What the device slot holds before the app has looked, and what `devices`
/// reports when Spotify lists none. A JSON array either way, so a client
/// never has to special-case the empty answer.
pub const NO_DEVICES: &str = "[]";

/// Loopback port that marks a running instance on platforms without a bus.
/// Registered to nothing; chosen high and out of the ephemeral range.
#[cfg(not(target_os = "linux"))]
const INSTANCE_PORT: u16 = 47_113;

/// Every request and reply starts with this, so a foreign program that
/// happens to hold the port is never mistaken for Oxidify.
#[cfg(not(target_os = "linux"))]
const PREFIX: &str = "oxidify:";
#[cfg(not(target_os = "linux"))]
const OK_REPLY: &str = "oxidify:ok";
#[cfg(not(target_os = "linux"))]
const NOW_REPLY: &str = "oxidify:now ";
#[cfg(not(target_os = "linux"))]
const DEVICES_REPLY: &str = "oxidify:devices ";
#[cfg(not(target_os = "linux"))]
const OTHER_USER_REPLY: &str = "oxidify:other-user";

#[cfg(not(target_os = "linux"))]
const READ_DEADLINE: std::time::Duration = std::time::Duration::from_secs(2);

#[cfg(not(target_os = "linux"))]
const MAX_CLIENTS: usize = 16;

/// What the running instance said back.
#[cfg(not(target_os = "linux"))]
pub enum Reply {
    /// The command was accepted.
    Ok,
    /// The `nowplaying` snapshot: [`NOTHING_PLAYING`], or tab-separated
    /// `state, title, artists, album, position_ms, duration_ms, volume,
    /// shuffle, repeat, art_url, saved, device`.
    NowPlaying(String),
    /// The `devices` snapshot: a JSON array of objects with `id`, `name`,
    /// `kind`, and `active`, or [`NO_DEVICES`]. Free text (a speaker someone
    /// named) makes tab-separated fields a poor fit here.
    Devices(String),
}

/// Sends one verb to the running instance and reads its reply.
#[cfg(not(target_os = "linux"))]
pub fn send(verb: &str) -> std::io::Result<Reply> {
    #[cfg(target_os = "macos")]
    if !matches!(verb, "nowplaying" | "devices") {
        let mut stream = std::os::unix::net::UnixStream::connect(control_socket_path())?;
        if !same_user(&stream) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "control socket belongs to another user",
            ));
        }
        stream.set_read_timeout(Some(READ_DEADLINE))?;
        return exchange(&mut stream, verb);
    }
    send_to(INSTANCE_PORT, verb)
}

#[cfg(not(target_os = "linux"))]
pub fn control(verb: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        verb.to_owned()
    }
    #[cfg(not(target_os = "macos"))]
    {
        match user_key() {
            Some(key) => format!("{verb} {key}"),
            None => "authentication-unavailable".to_owned(),
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn send_to(port: u16, verb: &str) -> std::io::Result<Reply> {
    use std::net::{Ipv4Addr, TcpStream};

    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
    stream.set_read_timeout(Some(READ_DEADLINE))?;
    exchange(&mut stream, verb)
}

#[cfg(not(target_os = "linux"))]
fn exchange(
    stream: &mut (impl std::io::Read + std::io::Write),
    verb: &str,
) -> std::io::Result<Reply> {
    stream.write_all(format!("{PREFIX}{verb}\n").as_bytes())?;
    // The listener writes one line and closes, so read to end and keep the
    // line. An instance predating the control channel ignores unknown verbs
    // without replying; the read times out and that surfaces as an error.
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    let line = reply.lines().next().unwrap_or("");
    if line == OK_REPLY {
        Ok(Reply::Ok)
    } else if let Some(snapshot) = line.strip_prefix(NOW_REPLY) {
        Ok(Reply::NowPlaying(snapshot.to_owned()))
    } else if let Some(snapshot) = line.strip_prefix(DEVICES_REPLY) {
        Ok(Reply::Devices(snapshot.to_owned()))
    } else if line == OTHER_USER_REPLY {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "the running Oxidify belongs to another user",
        ))
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the port is held by something other than Oxidify",
        ))
    }
}

#[cfg(not(target_os = "linux"))]
pub fn acquire(waker: &crate::backend::Waker) -> Outcome {
    use std::net::{Ipv4Addr, TcpListener};
    use std::sync::{Arc, Mutex};

    let unguarded = || Guard {
        commands: Default::default(),
        now_playing: Arc::new(Mutex::new(NOTHING_PLAYING.to_owned())),
        devices: Arc::new(Mutex::new(NO_DEVICES.to_owned())),
    };

    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, INSTANCE_PORT)) {
        Ok(listener) => listener,
        Err(_) => {
            // Someone holds the port. Ask them to show themselves, and only
            // stand down if they answer as Oxidify.
            #[cfg(target_os = "macos")]
            let answered = send("show").is_ok_and(|reply| matches!(reply, Reply::Ok));
            #[cfg(not(target_os = "macos"))]
            let answered = match user_key() {
                Some(key) => match send(&format!("show-for {key}")) {
                    Ok(reply) => matches!(reply, Reply::Ok),
                    Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                        log::warn!(
                            "port {INSTANCE_PORT} is held by another user's Oxidify; running without remote control"
                        );
                        return Outcome::Only(unguarded());
                    }
                    Err(_) => false,
                },
                None => false,
            };
            if answered {
                return Outcome::Surfaced;
            }
            log::warn!("port {INSTANCE_PORT} is busy but not with Oxidify; running unguarded");
            return Outcome::Only(unguarded());
        }
    };

    let guard = unguarded();
    #[cfg(target_os = "macos")]
    start_local_control(&guard, waker);
    #[cfg(target_os = "windows")]
    let own_key = {
        let key = create_user_key();
        if key.is_none() {
            log::warn!("cannot secure remote control; control commands are disabled");
        }
        key
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let own_key: Option<String> = None;
    #[cfg(target_os = "macos")]
    let own_key: Option<String> = None;
    let commands = Arc::clone(&guard.commands);
    let now_playing = Arc::clone(&guard.now_playing);
    let devices = Arc::clone(&guard.devices);
    let waker = waker.clone();
    let spawned = std::thread::Builder::new()
        .name("oxidify-instance".to_owned())
        .spawn(move || serve(listener, own_key, &commands, &now_playing, &devices, &waker));
    if let Err(error) = spawned {
        log::warn!("cannot listen for other launches: {error}");
    }
    Outcome::Only(guard)
}

/// Answers control clients until the listener closes. One request line and
/// one reply line per connection.
#[cfg(not(target_os = "linux"))]
fn serve(
    listener: std::net::TcpListener,
    own_key: Option<String>,
    commands: &std::sync::Mutex<Vec<ControlCommand>>,
    now_playing: &std::sync::Mutex<String>,
    devices: &std::sync::Mutex<String>,
    waker: &crate::backend::Waker,
) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    let active = AtomicUsize::new(0);
    let mut accept_failing = false;
    std::thread::scope(|scope| {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(stream) => stream,
                Err(error) => {
                    if !accept_failing {
                        log::warn!("cannot accept control clients: {error}");
                        accept_failing = true;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
            };
            accept_failing = false;
            if active.fetch_add(1, Ordering::SeqCst) >= MAX_CLIENTS {
                active.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            let active = &active;
            let own_key = own_key.as_deref();
            let spawned = std::thread::Builder::new()
                .name("oxidify-control".to_owned())
                .spawn_scoped(scope, move || {
                    answer(
                        &mut stream,
                        own_key,
                        false,
                        commands,
                        now_playing,
                        devices,
                        waker,
                    );
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            if spawned.is_err() {
                active.fetch_sub(1, Ordering::SeqCst);
            }
        }
    });
}

#[cfg(not(target_os = "linux"))]
fn answer(
    stream: &mut impl ControlStream,
    own_key: Option<&str>,
    authenticated: bool,
    commands: &std::sync::Mutex<Vec<ControlCommand>>,
    now_playing: &std::sync::Mutex<String>,
    devices: &std::sync::Mutex<String>,
    waker: &crate::backend::Waker,
) {
    let queue = |command| {
        commands
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(command);
        waker.wake();
    };

    let Some(line) = read_line(stream) else {
        return;
    };
    match parse(&line, authenticated) {
        Some(Request::ShowFor(key)) => {
            if authenticated || own_key.is_some_and(|own| own == key) {
                let _ = stream.write_all(format!("{OK_REPLY}\n").as_bytes());
                queue(ControlCommand::Show);
            } else {
                let _ = stream.write_all(format!("{OTHER_USER_REPLY}\n").as_bytes());
            }
        }
        Some(Request::Command { command, key }) => {
            if !authenticated && (own_key.is_none() || key.as_deref() != own_key) {
                let _ = stream.write_all(format!("{OTHER_USER_REPLY}\n").as_bytes());
                return;
            }
            let _ = stream.write_all(format!("{OK_REPLY}\n").as_bytes());
            queue(command);
        }
        Some(Request::NowPlaying) => {
            let snapshot = now_playing
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            let _ = stream.write_all(format!("{NOW_REPLY}{snapshot}\n").as_bytes());
        }
        Some(Request::Devices) => {
            let snapshot = devices.lock().unwrap_or_else(|p| p.into_inner()).clone();
            let _ = stream.write_all(format!("{DEVICES_REPLY}{snapshot}\n").as_bytes());
            // The app only refreshes the list while its own picker is
            // open, so a client asking is reason enough to look again.
            // The answer above is whatever was known a moment ago; the
            // next read has the fresh one.
            // ponytail: one read behind on a cold list. Push updates
            // only if a client ever needs it sooner than that.
            queue(ControlCommand::RefreshDevices);
        }
        // Not our client; say nothing and hang up.
        None => {}
    }
}

/// A parsed request line: a command for the app, or a read the listener
/// answers itself.
#[cfg(not(target_os = "linux"))]
enum Request {
    Command {
        command: ControlCommand,
        key: Option<String>,
    },
    NowPlaying,
    Devices,
    ShowFor(String),
}

#[cfg(not(target_os = "linux"))]
fn parse(line: &str, authenticated: bool) -> Option<Request> {
    let line = line.trim_end().strip_prefix(PREFIX)?;
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let verb = *tokens.first()?;
    if verb == "show-for" {
        return tokens
            .get(1)
            .and_then(|key| user_key_shaped(key))
            .map(Request::ShowFor);
    }
    let key = if authenticated {
        None
    } else {
        tokens.last().and_then(|token| user_key_shaped(token))
    };
    let arg_span = tokens.len() - usize::from(key.is_some());
    let argument = (arg_span == 2).then(|| tokens[1]);
    if tokens.len() != 1 + usize::from(argument.is_some()) + usize::from(key.is_some()) {
        return None;
    }
    if argument.is_none() && key.is_none() {
        match verb {
            "nowplaying" => return Some(Request::NowPlaying),
            "devices" => return Some(Request::Devices),
            _ => {}
        }
    }
    let command = match (verb, argument) {
        ("show", None) => ControlCommand::Show,
        ("quit", None) => ControlCommand::Quit,
        ("playpause", None) => ControlCommand::PlayPause,
        ("play", None) => ControlCommand::Play,
        ("pause", None) => ControlCommand::Pause,
        ("next", None) => ControlCommand::Next,
        ("previous", None) => ControlCommand::Previous,
        ("seek-by", Some(ms)) => ControlCommand::SeekBy(ms.parse().ok()?),
        ("seek-to", Some(ms)) => ControlCommand::SeekTo(ms.parse().ok()?),
        ("volume-by", Some(delta)) => ControlCommand::VolumeBy(delta.parse().ok()?),
        ("volume-set", Some(volume)) => ControlCommand::SetVolume(volume.parse().ok()?),
        ("mute", None) => ControlCommand::ToggleMute,
        ("shuffle", None) => ControlCommand::ToggleShuffle,
        ("shuffle-set", Some("on")) => ControlCommand::SetShuffle(true),
        ("shuffle-set", Some("off")) => ControlCommand::SetShuffle(false),
        ("repeat", None) => ControlCommand::CycleRepeat,
        // Spelled out rather than through `RepeatMode::from_api`, which
        // reads an unknown word as `off`. A verb nobody meant to send
        // should be refused, not obeyed as something else.
        ("repeat-set", Some("off")) => ControlCommand::SetRepeat(crate::player::RepeatMode::Off),
        ("repeat-set", Some("context")) => {
            ControlCommand::SetRepeat(crate::player::RepeatMode::Context)
        }
        ("repeat-set", Some("track")) => {
            ControlCommand::SetRepeat(crate::player::RepeatMode::Track)
        }
        ("save-toggle", None) => ControlCommand::ToggleSaved,
        ("play-uri", Some(uri)) => ControlCommand::PlayUri(spotify_uri(uri)?),
        ("transfer", Some(id)) => ControlCommand::Transfer(device_id(id)?),
        _ => return None,
    };
    Some(Request::Command { command, key })
}

/// A `spotify:` URI, as far as this side can tell. Anything on the machine
/// can reach the port, so what goes on to become a Web API play request is
/// checked here rather than taken on trust: the scheme, and only the
/// characters Spotify's own URIs and their percent-escapes are made of.
#[cfg(not(target_os = "linux"))]
fn spotify_uri(text: &str) -> Option<String> {
    let shaped = text.starts_with("spotify:")
        && text.len() <= 128
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_' | '.' | '%' | '+'));
    shaped.then(|| text.to_owned())
}

/// A Spotify Connect device id: the opaque hex-ish string the Web API hands
/// out. Checked for the same reason as [`spotify_uri`].
#[cfg(not(target_os = "linux"))]
fn device_id(text: &str) -> Option<String> {
    let shaped = !text.is_empty()
        && text.len() <= 64
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    shaped.then(|| text.to_owned())
}

#[cfg(not(target_os = "linux"))]
fn user_key_shaped(text: &str) -> Option<String> {
    let shaped =
        !text.is_empty() && text.len() <= 64 && text.chars().all(|c| c.is_ascii_hexdigit());
    shaped.then(|| text.to_owned())
}

#[cfg(target_os = "windows")]
fn create_user_key() -> Option<String> {
    use rand::{TryRngCore, rand_core::OsRng};
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_SESSION, CRED_TYPE_GENERIC, CREDENTIALW, CredWriteW,
    };

    let mut bytes = [0u8; 32];
    OsRng.try_fill_bytes(&mut bytes).ok()?;
    let key: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: windows_sys::core::w!("Oxidify/remote-control").cast_mut(),
        CredentialBlobSize: u32::try_from(key.len()).ok()?,
        CredentialBlob: key.as_ptr().cast_mut(),
        Persist: CRED_PERSIST_SESSION,
        ..Default::default()
    };
    (unsafe { CredWriteW(&credential, 0) } != 0).then_some(key)
}

#[cfg(target_os = "windows")]
fn user_key() -> Option<String> {
    use windows_sys::Win32::Security::Credentials::{
        CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW,
    };

    let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
    if unsafe {
        CredReadW(
            windows_sys::core::w!("Oxidify/remote-control"),
            CRED_TYPE_GENERIC,
            0,
            &mut credential,
        )
    } == 0
    {
        return None;
    }
    let key = unsafe {
        let credential = &*credential;
        if credential.CredentialBlobSize == 64 && !credential.CredentialBlob.is_null() {
            std::str::from_utf8(std::slice::from_raw_parts(credential.CredentialBlob, 64))
                .ok()
                .and_then(user_key_shaped)
        } else {
            None
        }
    };
    unsafe { CredFree(credential.cast()) };
    key
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn user_key() -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn control_socket_path() -> std::path::PathBuf {
    std::path::PathBuf::from(format!("/private/tmp/oxidify-control-{}.sock", unsafe {
        geteuid()
    }))
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn geteuid() -> u32;
    fn getpeereid(socket: std::os::raw::c_int, uid: *mut u32, gid: *mut u32)
    -> std::os::raw::c_int;
}

#[cfg(target_os = "macos")]
fn same_user(stream: &std::os::unix::net::UnixStream) -> bool {
    use std::os::fd::AsRawFd;
    let mut uid = 0;
    let mut gid = 0;
    unsafe { getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) == 0 && uid == geteuid() }
}

#[cfg(target_os = "macos")]
fn start_local_control(guard: &Guard, waker: &crate::backend::Waker) {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::os::unix::net::UnixListener;
    use std::sync::Arc;

    let path = control_socket_path();
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if !metadata.file_type().is_socket()
            || metadata.uid() != unsafe { geteuid() }
            || std::fs::remove_file(&path).is_err()
        {
            log::warn!("cannot prepare local control socket");
            return;
        }
    }
    let listener = match UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(error) => {
            log::warn!("cannot bind local control socket: {error}");
            return;
        }
    };
    let commands = Arc::clone(&guard.commands);
    let now_playing = Arc::clone(&guard.now_playing);
    let devices = Arc::clone(&guard.devices);
    let waker = waker.clone();
    let result = std::thread::Builder::new()
        .name("oxidify-local-control".to_owned())
        .spawn(move || {
            use std::sync::atomic::{AtomicUsize, Ordering};
            let active = AtomicUsize::new(0);
            std::thread::scope(|scope| {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { break };
                    if !same_user(&stream) {
                        continue;
                    }
                    if active.fetch_add(1, Ordering::SeqCst) >= MAX_CLIENTS {
                        active.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                    let active = &active;
                    let commands = &commands;
                    let now_playing = &now_playing;
                    let devices = &devices;
                    let waker = &waker;
                    if std::thread::Builder::new()
                        .name("oxidify-control".to_owned())
                        .spawn_scoped(scope, move || {
                            answer(
                                &mut stream,
                                None,
                                true,
                                commands,
                                now_playing,
                                devices,
                                waker,
                            );
                            active.fetch_sub(1, Ordering::SeqCst);
                        })
                        .is_err()
                    {
                        active.fetch_sub(1, Ordering::SeqCst);
                    }
                }
            });
        });
    if let Err(error) = result {
        log::warn!("cannot listen for local controls: {error}");
    }
}

#[cfg(not(target_os = "linux"))]
trait ControlStream: std::io::Read + std::io::Write {
    fn set_timeout(&self, timeout: std::time::Duration) -> std::io::Result<()>;
}

#[cfg(not(target_os = "linux"))]
impl ControlStream for std::net::TcpStream {
    fn set_timeout(&self, timeout: std::time::Duration) -> std::io::Result<()> {
        self.set_read_timeout(Some(timeout))
    }
}

#[cfg(target_os = "macos")]
impl ControlStream for std::os::unix::net::UnixStream {
    fn set_timeout(&self, timeout: std::time::Duration) -> std::io::Result<()> {
        self.set_read_timeout(Some(timeout))
    }
}

/// Reads up to the first newline. A line too long to be one of ours, or any
/// read error, disqualifies the client.
#[cfg(not(target_os = "linux"))]
fn read_line(stream: &mut impl ControlStream) -> Option<String> {
    let deadline = std::time::Instant::now() + READ_DEADLINE;
    let mut buffer = [0u8; 256];
    let mut filled = 0;
    loop {
        if filled == buffer.len() {
            return None;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        stream.set_timeout(remaining).ok()?;
        match stream.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => {
                filled += read;
                if buffer[..filled].contains(&b'\n') {
                    break;
                }
            }
            Err(_) => return None,
        }
    }
    let line = buffer[..filled].split(|&byte| byte == b'\n').next()?;
    String::from_utf8(line.to_vec()).ok()
}

#[cfg(target_os = "linux")]
pub fn acquire(_waker: &crate::backend::Waker) -> Outcome {
    use mpris_server::zbus::blocking::Connection;
    use mpris_server::zbus::fdo::{RequestNameFlags, RequestNameReply};

    let guard = |connection: Option<Connection>| Guard {
        _connection: connection,
        commands: Default::default(),
        now_playing: std::sync::Arc::new(std::sync::Mutex::new(NOTHING_PLAYING.to_owned())),
        devices: std::sync::Arc::new(std::sync::Mutex::new(NO_DEVICES.to_owned())),
    };

    let connection = match Connection::session() {
        Ok(connection) => connection,
        Err(error) => {
            // No session bus at all: nothing to coordinate through, so run.
            log::debug!("no session bus, running unguarded: {error}");
            return Outcome::Only(guard(None));
        }
    };

    // Holding the name is how this process says it is the one running.
    // zbus reports a name another peer already owns as `NameTaken` rather
    // than as a reply, so that error is the ordinary second-launch path.
    match connection.request_name_with_flags(INSTANCE_NAME, RequestNameFlags::DoNotQueue.into()) {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
            Outcome::Only(guard(Some(connection)))
        }
        Ok(_) | Err(mpris_server::zbus::Error::NameTaken) => {
            if !raise_running_instance(&connection) {
                log::warn!(
                    "Oxidify is already running but did not answer; not starting a second copy"
                );
            }
            Outcome::Surfaced
        }
        Err(error) => {
            log::warn!("cannot check for a running instance, starting anyway: {error}");
            Outcome::Only(guard(None))
        }
    }
}

/// Asks the running instance to show its window, retrying briefly because it
/// may still be registering MPRIS when this launch arrives.
#[cfg(target_os = "linux")]
fn raise_running_instance(connection: &mpris_server::zbus::blocking::Connection) -> bool {
    for attempt in 0..10 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        let raised = connection.call_method(
            Some(MPRIS_NAME),
            "/org/mpris/MediaPlayer2",
            Some("org.mpris.MediaPlayer2"),
            "Raise",
            &(),
        );
        if raised.is_ok() {
            return true;
        }
    }
    false
}

#[cfg(all(test, not(target_os = "linux")))]
mod tests {
    use super::*;
    use crate::player::RepeatMode;

    fn command(line: &str) -> Option<ControlCommand> {
        match parse(&format!("{line} {TEST_KEY}"), false) {
            Some(Request::Command { command, .. }) => Some(command),
            _ => None,
        }
    }

    const TEST_KEY: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn parses_every_control_verb() {
        // #given / #when / #then
        assert_eq!(command("oxidify:show\n"), Some(ControlCommand::Show));
        assert_eq!(command("oxidify:quit"), Some(ControlCommand::Quit));
        assert_eq!(
            command("oxidify:playpause"),
            Some(ControlCommand::PlayPause)
        );
        assert_eq!(command("oxidify:play"), Some(ControlCommand::Play));
        assert_eq!(command("oxidify:pause"), Some(ControlCommand::Pause));
        assert_eq!(command("oxidify:next"), Some(ControlCommand::Next));
        assert_eq!(command("oxidify:previous"), Some(ControlCommand::Previous));
        assert_eq!(
            command("oxidify:seek-by -10000"),
            Some(ControlCommand::SeekBy(-10_000))
        );
        assert_eq!(
            command("oxidify:volume-by +5"),
            Some(ControlCommand::VolumeBy(5))
        );
        assert_eq!(
            command("oxidify:volume-set 40"),
            Some(ControlCommand::SetVolume(40))
        );
        assert_eq!(command("oxidify:mute"), Some(ControlCommand::ToggleMute));
        assert_eq!(
            command("oxidify:shuffle"),
            Some(ControlCommand::ToggleShuffle)
        );
        assert_eq!(command("oxidify:repeat"), Some(ControlCommand::CycleRepeat));
        assert_eq!(
            command("oxidify:shuffle-set on"),
            Some(ControlCommand::SetShuffle(true))
        );
        assert_eq!(
            command("oxidify:shuffle-set off"),
            Some(ControlCommand::SetShuffle(false))
        );
        assert_eq!(
            command("oxidify:repeat-set track"),
            Some(ControlCommand::SetRepeat(RepeatMode::Track))
        );
        assert_eq!(
            command("oxidify:repeat-set context"),
            Some(ControlCommand::SetRepeat(RepeatMode::Context))
        );
        assert_eq!(
            command("oxidify:repeat-set off"),
            Some(ControlCommand::SetRepeat(RepeatMode::Off))
        );
        assert_eq!(
            command("oxidify:seek-to 90000"),
            Some(ControlCommand::SeekTo(90_000))
        );
        assert_eq!(
            command("oxidify:save-toggle"),
            Some(ControlCommand::ToggleSaved)
        );
        assert_eq!(
            command("oxidify:play-uri spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"),
            Some(ControlCommand::PlayUri(
                "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M".to_owned()
            ))
        );
        assert_eq!(
            command("oxidify:transfer a1b2c3d4e5"),
            Some(ControlCommand::Transfer("a1b2c3d4e5".to_owned()))
        );
        assert!(matches!(
            parse("oxidify:nowplaying", false),
            Some(Request::NowPlaying)
        ));
        assert!(matches!(
            parse("oxidify:devices", false),
            Some(Request::Devices)
        ));
    }

    #[test]
    fn rejects_lines_that_are_not_ours() {
        assert!(parse("GET / HTTP/1.1", false).is_none());
        assert!(parse("oxidify:frobnicate", false).is_none());
        assert!(parse("oxidify:seek-by soon", false).is_none());
        assert!(parse("oxidify:volume-set 999", false).is_none());
        assert!(parse("oxidify:next please", false).is_none());
        assert!(parse("", false).is_none());
    }

    /// The two verbs that carry free text are the ones a hostile local
    /// process would reach for, so neither hands the app a string it has
    /// not looked at.
    #[test]
    fn refuses_arguments_that_are_not_shaped_like_spotifys_own() {
        // #given / #when / #then
        assert!(command("oxidify:play-uri http://example.com/pwn").is_none());
        assert!(command("oxidify:play-uri spotify:track:a b").is_none());
        assert!(command("oxidify:play-uri ../../etc/passwd").is_none());
        assert!(command("oxidify:play-uri").is_none());
        assert!(command(&format!("oxidify:play-uri spotify:{}", "x".repeat(200))).is_none());
        assert!(command("oxidify:transfer ../secrets").is_none());
        assert!(command("oxidify:transfer").is_none());
        // A word that is not one of the three is refused rather than read
        // as `off`, which is what `RepeatMode::from_api` would have done.
        assert!(command("oxidify:repeat-set sometimes").is_none());
        assert!(command("oxidify:shuffle-set maybe").is_none());
        assert!(command("oxidify:seek-to -1").is_none());
    }

    /// The whole channel over a real socket: what `oxidify next` sends is
    /// what the app finds in its queue, and the two reads come back from the
    /// snapshots the app published.
    #[test]
    fn a_client_reaches_the_command_queue_and_the_snapshot() {
        use std::net::{Ipv4Addr, TcpListener};
        use std::sync::{Arc, Mutex};

        // #given
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a loopback port");
        let port = listener.local_addr().expect("a bound address").port();
        let commands: Arc<Mutex<Vec<ControlCommand>>> = Default::default();
        let now_playing = Arc::new(Mutex::new("playing\tGo\tThe Band".to_owned()));
        let devices = Arc::new(Mutex::new(
            r#"[{"id":"abc","name":"Kitchen","kind":"Speaker","active":true}]"#.to_owned(),
        ));
        let served = {
            let commands = Arc::clone(&commands);
            let now_playing = Arc::clone(&now_playing);
            let devices = Arc::clone(&devices);
            let waker = crate::backend::Waker::default();
            std::thread::spawn(move || {
                serve(
                    listener,
                    Some(TEST_KEY.to_owned()),
                    &commands,
                    &now_playing,
                    &devices,
                    &waker,
                )
            })
        };

        // #when
        let key = TEST_KEY;
        let accepted = send_to(port, &format!("next {key}")).expect("a reply");
        let volume = send_to(port, &format!("volume-by -5 {key}")).expect("a reply");
        let liked = send_to(port, &format!("save-toggle {key}")).expect("a reply");
        let snapshot = send_to(port, "nowplaying").expect("a reply");
        let listed = send_to(port, "devices").expect("a reply");
        let refused = send_to(port, "frobnicate");

        // #then
        assert!(matches!(accepted, Reply::Ok));
        assert!(matches!(volume, Reply::Ok));
        assert!(matches!(liked, Reply::Ok));
        match snapshot {
            Reply::NowPlaying(line) => assert_eq!(line, "playing\tGo\tThe Band"),
            _ => panic!("nowplaying answered with something else"),
        }
        match listed {
            Reply::Devices(json) => assert!(json.contains("Kitchen")),
            _ => panic!("devices answered with something else"),
        }
        // An unknown verb gets no reply at all, so the client sees a closed
        // connection rather than a command it never sent being obeyed.
        assert!(refused.is_err());
        // Reading the devices also asks the app to look again, so the next
        // read is fresh.
        assert_eq!(
            *commands.lock().expect("the queue"),
            vec![
                ControlCommand::Next,
                ControlCommand::VolumeBy(-5),
                ControlCommand::ToggleSaved,
                ControlCommand::RefreshDevices,
            ]
        );

        drop(served);
    }
}
