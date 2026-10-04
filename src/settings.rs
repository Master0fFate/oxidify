//! User preferences, stored as one readable JSON file.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
    System,
}

/// What the mini player's display shows of the sound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisMode {
    #[default]
    Bars,
    Scope,
    Off,
}

impl VisMode {
    /// The next mode round, the order a click on the display goes through.
    pub fn next(self) -> Self {
        match self {
            Self::Bars => Self::Scope,
            Self::Scope => Self::Off,
            Self::Off => Self::Bars,
        }
    }
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [Self::Dark, Self::Light, Self::System];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::System => "Follow system",
        }
    }
}

/// The colour that marks what is playing, selected, and saved. Green is
/// the one Spotify's own client uses; blue is Oxidify's older accent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    #[default]
    Green,
    Blue,
}

impl Accent {
    pub const ALL: [Accent; 2] = [Self::Green, Self::Blue];

    pub fn label(self) -> &'static str {
        match self {
            Self::Green => "Green",
            Self::Blue => "Blue",
        }
    }
}

/// Where this computer plays audio. Spotify Connect (librespot) is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackBackend {
    #[default]
    Spotify,
    Alternate,
}

impl PlaybackBackend {
    pub const ALL: [PlaybackBackend; 2] = [Self::Spotify, Self::Alternate];

    pub fn label(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify Connect",
            Self::Alternate => "Alternate local audio",
        }
    }
}

fn default_alternate_min_score() -> f32 {
    0.55
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The Spotify Connect name other devices see.
    pub device_name: String,
    /// 96, 160, or 320 kbps.
    pub bitrate: u16,
    pub normalisation: bool,
    pub autoplay: bool,
    pub gapless: bool,
    /// librespot backend name; `None` picks the platform default.
    pub audio_backend: Option<String>,
    pub audio_device: Option<String>,
    pub audio_cache: bool,
    pub audio_cache_mb: u64,
    pub theme: ThemeChoice,
    /// The accent colour: green as in Spotify's client, or the older blue.
    pub accent: Accent,
    /// Tint the interface with the colour of the playing album's art.
    pub accent_from_art: bool,
    /// Last local volume, 0..=65535.
    pub volume: u16,
    /// Whether the library sidebar is visible.
    pub sidebar_visible: bool,
    /// Your Library is folded down to a rail of covers.
    pub sidebar_collapsed: bool,
    /// Your Library shows a grid of covers instead of rows.
    pub sidebar_grid: bool,
    pub sidebar_width: f32,
    pub lyrics_width: f32,
    pub queue_width: f32,
    /// The Now Playing view's width.
    pub now_playing_width: f32,
    pub search_history: Vec<String>,
    pub show_shortcut_hints: bool,
    /// An optional personal Spotify Web API application id. The shared
    /// application remains active for coverage when this is present.
    pub web_client_id: Option<String>,
    /// Local playback has been authorized at least once on this machine, so
    /// the app can resume it silently instead of prompting.
    pub playback_authorized: bool,
    /// Closing the window hides to the tray and keeps the music playing.
    pub keep_playing_in_background: bool,
    /// Ask GitHub once a day whether a newer release exists.
    pub check_for_updates: bool,
    /// Context URIs pinned to the top of the sidebar, in pin order.
    pub pinned_contexts: Vec<String>,
    /// Local audio engine. Missing from older files; defaults to Spotify Connect.
    pub playback_backend: PlaybackBackend,
    /// User-configured Piped-compatible API base URL. Empty means unused.
    pub piped_api_base: String,
    /// Optional path to a user-installed yt-dlp binary. Empty means PATH, or
    /// the official pinned build this app extracts locally.
    pub ytdlp_path: String,
    /// Minimum rank score (0..=1) required to play a third-party match.
    #[serde(default = "default_alternate_min_score")]
    pub alternate_min_score: f32,
    /// Skip to the next track when no match meets `alternate_min_score`.
    /// HTTP, stall, and decode failures still stop the current track.
    #[serde(default = "default_true")]
    pub alternate_skip_on_miss: bool,
    /// The sidebar's own playlist order, set by dragging rows. Empty means
    /// the automatic order: the pinned block first, then recently played.
    pub sidebar_order: Vec<String>,
    /// Interface zoom, egui's zoom factor; Ctrl+plus/minus changes it.
    pub zoom: f32,
    /// The Winamp window is open.
    pub winamp_window: bool,
    /// The skin the Winamp window wears: a file or folder name in the skins
    /// folder. `None` is the built-in skin.
    pub skin: Option<String>,
    /// Screen pixels per skin pixel; `None` picks double size for the
    /// display.
    pub skin_scale: Option<u8>,
    /// The Winamp window stays above other windows.
    pub winamp_on_top: bool,
    /// The mini player's visualiser: bars, scope, or off.
    pub vis: VisMode,
    /// The playlist window is open under the mini player.
    pub playlist_open: bool,
    /// How tall the playlist window is, in skin pixels.
    pub playlist_height: u32,
    /// The equalizer window is open under the mini player.
    pub eq_open: bool,
    /// The equalizer shapes local playback.
    pub eq_on: bool,
    /// The preamp, in decibels, never above zero.
    pub eq_preamp_db: f32,
    /// The ten bands, in decibels, 60 Hz to 16 kHz.
    pub eq_bands_db: [f32; 10],
    /// The balance, -1 all left to 1 all right.
    pub balance: f32,
    /// Play both channels the same.
    pub mono: bool,
    /// The playlist window is rolled up to its title bar.
    pub playlist_shaded: bool,
    /// The main window is rolled up to its title bar.
    pub winamp_shaded: bool,
    /// Library sidebar rows show a name only, without covers.
    pub compact_library: bool,
    /// Track tables use one-line rows without covers.
    pub compact_tracks: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_name: "Oxidify".to_string(),
            bitrate: 320,
            normalisation: false,
            autoplay: true,
            gapless: true,
            audio_backend: None,
            audio_device: None,
            audio_cache: true,
            audio_cache_mb: 1024,
            theme: ThemeChoice::Dark,
            accent: Accent::Green,
            accent_from_art: true,
            volume: (u16::MAX as u32 * 70 / 100) as u16,
            sidebar_visible: true,
            sidebar_collapsed: false,
            sidebar_grid: false,
            sidebar_width: 280.0,
            lyrics_width: 360.0,
            queue_width: 360.0,
            now_playing_width: 340.0,
            search_history: Vec::new(),
            show_shortcut_hints: true,
            web_client_id: None,
            playback_authorized: false,
            keep_playing_in_background: true,
            check_for_updates: true,
            pinned_contexts: Vec::new(),
            playback_backend: PlaybackBackend::Spotify,
            piped_api_base: String::new(),
            ytdlp_path: String::new(),
            alternate_min_score: default_alternate_min_score(),
            alternate_skip_on_miss: true,
            sidebar_order: Vec::new(),
            zoom: 1.0,
            winamp_window: false,
            skin: None,
            skin_scale: None,
            winamp_on_top: false,
            vis: VisMode::default(),
            playlist_open: false,
            playlist_height: 174,
            eq_open: false,
            eq_on: false,
            eq_preamp_db: 0.0,
            eq_bands_db: [0.0; 10],
            balance: 0.0,
            mono: false,
            playlist_shaded: false,
            winamp_shaded: false,
            compact_library: false,
            compact_tracks: false,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let mut settings = match std::fs::read_to_string(path) {
            Ok(text) => {
                // Windows editors can prefix UTF-8 JSON with a byte order mark.
                let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
                serde_json::from_str(text).unwrap_or_else(|error| {
                    log::warn!("settings at {} are unreadable: {error}", path.display());
                    Self::default()
                })
            }
            Err(_) => Self::default(),
        };
        settings.sanitize();
        settings
    }

    fn sanitize(&mut self) {
        if !self.alternate_min_score.is_finite() {
            self.alternate_min_score = default_alternate_min_score();
        }
        self.alternate_min_score = self.alternate_min_score.clamp(0.2, 0.95);
        self.piped_api_base = self.piped_api_base.trim().to_string();
        self.ytdlp_path = self.ytdlp_path.trim().to_string();
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let text = match serde_json::to_string_pretty(self) {
            Ok(text) => text,
            Err(error) => {
                log::warn!("unable to encode settings: {error}");
                return;
            }
        };
        let temporary = path.with_extension("json.tmp");
        let written =
            std::fs::write(&temporary, text).and_then(|()| std::fs::rename(&temporary, path));
        if let Err(error) = written {
            log::warn!("unable to save settings to {}: {error}", path.display());
        }
    }

    pub fn platform_backend(&self) -> Option<String> {
        self.audio_backend.clone().or_else(|| {
            if cfg!(target_os = "linux") {
                Some("pulseaudio".to_string())
            } else {
                None
            }
        })
    }

    pub fn remember_search(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            return;
        }
        self.search_history.retain(|entry| entry != query);
        self.search_history.insert(0, query.to_string());
        self.search_history.truncate(12);
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn older_settings_keep_the_sidebar_visible() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.sidebar_visible);
    }

    #[test]
    fn a_fresh_install_opens_the_full_window_not_the_mini_player() {
        // The Winamp mini player is opt-in: it appears only when the
        // settings explicitly say so, never by default or by migration.
        let fresh = Settings::default();
        assert!(!fresh.winamp_window);
        let legacy_without_the_field: Settings =
            serde_json::from_str(r#"{"device_name":"Desk"}"#).unwrap();
        assert!(!legacy_without_the_field.winamp_window);
    }

    #[test]
    fn older_settings_keep_the_winamp_window_closed_and_the_built_in_skin() {
        let settings: Settings = serde_json::from_str(r#"{"zoom": 1.2}"#).unwrap();
        assert!(!settings.winamp_window);
        assert_eq!(settings.skin, None);
        assert_eq!(settings.skin_scale, None);
        assert!(!settings.winamp_on_top);
        assert_eq!(settings.vis, super::VisMode::Bars);
        assert!(!settings.playlist_open);
        assert_eq!(settings.playlist_height, 174);
        assert!(!settings.eq_on);
        assert_eq!(settings.eq_bands_db, [0.0; 10]);
        assert_eq!(settings.balance, 0.0);
        assert!(!settings.mono);
        assert!(!settings.playlist_shaded);
        assert!(!settings.winamp_shaded);
        assert!(!settings.compact_library);
        assert!(!settings.compact_tracks);
    }

    #[test]
    fn compact_rows_round_trip() {
        let settings = Settings {
            compact_library: true,
            compact_tracks: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(restored.compact_library);
        assert!(restored.compact_tracks);
    }

    #[test]
    fn the_visualiser_cycles_bars_scope_off() {
        use super::VisMode;
        assert_eq!(VisMode::Bars.next(), VisMode::Scope);
        assert_eq!(VisMode::Scope.next(), VisMode::Off);
        assert_eq!(VisMode::Off.next(), VisMode::Bars);
        let settings: Settings = serde_json::from_str(r#"{"vis": "scope"}"#).unwrap();
        assert_eq!(settings.vis, VisMode::Scope);
    }

    #[test]
    fn a_chosen_skin_round_trips() {
        let settings = Settings {
            winamp_window: true,
            skin: Some("Zaxon.wsz".into()),
            skin_scale: Some(3),
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, settings);
    }

    #[test]
    fn older_settings_take_the_green_accent_and_keep_blue_when_chosen() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.accent, super::Accent::Green);
        assert!(!settings.sidebar_grid);
        let blue: Settings =
            serde_json::from_str(r#"{"accent": "blue", "sidebar_grid": true}"#).unwrap();
        assert_eq!(blue.accent, super::Accent::Blue);
        assert!(blue.sidebar_grid);
        let json = serde_json::to_string(&blue).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.accent, super::Accent::Blue);
    }

    #[test]
    fn older_settings_open_the_library_out_not_as_a_rail() {
        let settings: Settings = serde_json::from_str(r#"{"sidebar_width": 250.0}"#).unwrap();
        assert!(!settings.sidebar_collapsed);
        assert_eq!(settings.sidebar_width, 250.0);
        assert_eq!(settings.now_playing_width, 340.0);
        let collapsed = Settings {
            sidebar_collapsed: true,
            now_playing_width: 400.0,
            ..Settings::default()
        };
        let json = serde_json::to_string(&collapsed).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(restored.sidebar_collapsed);
        assert_eq!(restored.now_playing_width, 400.0);
    }

    #[test]
    fn hidden_sidebar_round_trips() {
        let settings = Settings {
            sidebar_visible: false,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(!restored.sidebar_visible);
    }
}

/// Restorable UI session: what was open when the app last closed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    pub last_page: Option<String>,
    /// Context URIs most recently played, newest first.
    pub recent_contexts: Vec<String>,
    /// What was playing when the app closed, to resume from a cold start.
    pub last_context: Option<String>,
    pub last_track: Option<String>,
    pub last_position_ms: u32,
    /// Whether the listener had shuffle on, a mode that outlives contexts.
    pub shuffle_on: bool,
    /// Each table's chosen sort, by encoded page, restored at start.
    pub sorts: Vec<(String, crate::model::TableSort)>,
    /// Last window inner size, to restore on next launch.
    pub window_size: Option<[f32; 2]>,
    /// Last window outer position, to restore on next launch.
    pub window_pos: Option<[f32; 2]>,
    /// Whether the queue panel was open.
    pub queue_open: Option<bool>,
    /// Whether the Now Playing view was open.
    pub now_playing_open: Option<bool>,
    /// Last outer position of the Winamp window.
    pub winamp_pos: Option<[f32; 2]>,
}

impl SessionState {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string(self) {
            let _ = std::fs::write(path, text);
        }
    }
}

#[cfg(test)]
mod alternate_settings_tests {
    use super::*;

    #[test]
    fn old_settings_json_defaults_to_spotify_connect() {
        let parsed: Settings =
            serde_json::from_str(r#"{"device_name":"Desk","bitrate":160}"#).unwrap();
        assert_eq!(parsed.playback_backend, PlaybackBackend::Spotify);
        assert!(parsed.piped_api_base.is_empty());
        assert!(parsed.ytdlp_path.is_empty());
        assert!((parsed.alternate_min_score - 0.55).abs() < f32::EPSILON);
        assert!(parsed.alternate_skip_on_miss);
        assert_eq!(parsed.device_name, "Desk");
        assert_eq!(parsed.bitrate, 160);
    }

    #[test]
    fn default_settings_are_spotify_connect() {
        let settings = Settings::default();
        assert_eq!(settings.playback_backend, PlaybackBackend::Spotify);
        assert!((settings.alternate_min_score - 0.55).abs() < f32::EPSILON);
        assert!(settings.alternate_skip_on_miss);
    }

    #[test]
    fn alternate_backend_round_trips() {
        let settings = Settings {
            playback_backend: PlaybackBackend::Alternate,
            piped_api_base: "https://piped.example".into(),
            alternate_min_score: 0.7,
            alternate_skip_on_miss: false,
            ..Settings::default()
        };
        let text = serde_json::to_string(&settings).unwrap();
        let parsed: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.playback_backend, PlaybackBackend::Alternate);
        assert_eq!(parsed.piped_api_base, "https://piped.example");
        assert!((parsed.alternate_min_score - 0.7).abs() < f32::EPSILON);
        assert!(!parsed.alternate_skip_on_miss);
    }
    #[test]
    fn bom_settings_preserve_preferences_on_load_and_save() {
        let dir = std::env::temp_dir().join(format!("oxidify-settings-bom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            "\u{feff}{\"volume\":12345,\"device_name\":\"My speaker\"}",
        )
        .unwrap();
        let settings = Settings::load(&path);
        assert_eq!(settings.volume, 12345);
        assert_eq!(settings.device_name, "My speaker");
        settings.save(&path);
        assert_eq!(Settings::load(&path).volume, 12345);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
