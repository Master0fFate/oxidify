---
title: Settings & Files
description: Where Oxidify keeps configuration, credentials, and caches, and what is safe to delete.
nav_order: 0
---

## Where things live

Oxidify follows each platform's conventions. On Linux:

| What | Where | Safe to delete? |
| --- | --- | --- |
| Settings | `~/.config/oxidify/settings.json` | Yes, you lose preferences |
| Winamp skins | `~/.config/oxidify/skins/` | Yes, you add them again |
| Shared Web API sign-in | `~/.local/state/oxidify/shared_web_api_token.json` | Yes, you sign in again |
| Personal Web API sign-in | `~/.local/state/oxidify/personal_web_api_token.json` | Yes, personal acceleration is removed |
| Playback credential | `~/.local/state/oxidify/credentials/` | Yes, you approve playback again |
| Last session | `~/.local/state/oxidify/session.json` | Yes |
| Audio cache | `~/.cache/oxidify/audio/` | Always |
| Artwork cache | `~/.cache/oxidify/art/` | Always |
| Lyrics cache | `~/.cache/oxidify/lyrics/` | Always |
| Account-tagged Top songs cache (six-hour lifetime, at most 1 MiB) | `~/.cache/oxidify/top-tracks.json` | Always |
| Account-scoped playlist cache | `~/.cache/oxidify/playlists/<account-id>/` | Always |
| Last run's log | `~/.local/state/oxidify/oxidify.log` | Always |
| Crash log | `~/.local/state/oxidify/panic.log` | Always |
| Bundled yt-dlp | `~/.local/state/oxidify/bin/` | Yes; the app extracts it again |

Clearing caches never signs you out; credentials live in *state*, not
*cache*. Web API token files are written with owner-only permissions.
Signing out from Settings deletes both Web API grants and the separate
playback credential.

On macOS, settings, state, and the logs are in
`~/Library/Application Support/me.master0ffate.oxidify` and the caches in
`~/Library/Caches/me.master0ffate.oxidify`. On Windows, settings are in
`%APPDATA%\master0ffate\oxidify\config`, state and the logs in
`%LOCALAPPDATA%\master0ffate\oxidify\data`, and the caches in
`%LOCALAPPDATA%\master0ffate\oxidify\cache`.

Fastpotify, the project Oxidify is derived from, kept its files in
`fastpotify` directories next to these. The first run after switching
imports settings, sign-ins, skins, and playback credentials from there,
once; the old directories are left untouched.

## settings.json

Settings are stored in one readable JSON file and written atomically. UTF-8
files saved with a leading byte order mark by a Windows editor are accepted
without resetting preferences. Its main fields are:

| Field | Default | Meaning |
| --- | --- | --- |
| `device_name` | `Oxidify` | Name on Spotify Connect |
| `bitrate` | `320` | 96, 160, or 320 kbps |
| `normalisation` | `false` | Volume normalisation |
| `autoplay` | `true` | Keep playing similar music at the end |
| `gapless` | `true` | Gapless playback |
| `audio_backend` | platform | `pulseaudio` or `rodio` on Linux |
| `audio_cache_mb` | `1024` | On-disk audio cache budget |
| `theme` | `dark` | `dark`, `light`, or `system` |
| `accent_from_art` | `true` | Tint pages with album art |
| `winamp_window` | `false` | The window is the Winamp mini player |
| `skin` | none | A file or folder name in the skins folder; the built-in skin when absent |
| `skin_scale` | by display | Screen pixels per skin pixel, 1 to 4 |
| `winamp_on_top` | `false` | Keep the mini player above other windows |
| `vis` | `bars` | The mini player's visualiser: `bars`, `scope`, or `off` |
| `playlist_open` | `false` | The playlist window is open under the mini player |
| `playlist_height` | `174` | The playlist window's height in skin pixels |
| `eq_open` | `false` | The equalizer window is open under the mini player |
| `eq_on` | `false` | The equalizer shapes local playback |
| `eq_preamp_db` | `0` | The preamp, in decibels, never above zero |
| `eq_bands_db` | ten zeros | The bands from 60 Hz to 16 kHz, in decibels, -12 to 12 |
| `balance` | `0` | Left to right, -1 to 1, for local playback |
| `mono` | `false` | Play both channels the same |
| `playlist_shaded` | `false` | The playlist window is rolled up to its title bar |
| `winamp_shaded` | `false` | The main window is rolled up to its title bar |
| `keep_playing_in_background` | `true` | Close to tray |
| `check_for_updates` | `true` | Ask GitHub once a day for a newer release |
| `web_client_id` | none | Optional personal Spotify app id used alongside shared coverage |
| `playback_backend` | `spotify` | `spotify` (Connect / librespot) or `alternate` |
| `piped_api_base` | empty | Optional Piped-compatible YouTube fallback |
| `ytdlp_path` | empty | Last-resort user yt-dlp; used only if strictly newer than the official pin |
| `alternate_min_score` | `0.55` | Minimum match score; weaker hits are never played |
| `alternate_skip_on_miss` | `true` | Skip forward when no match meets the score. Transient network errors retry; terminal transport and decode failures stop instead of skipping. |

## Command line

```
oxidify [OPTIONS]

  --device-name <NAME>  Spotify Connect name for this session
  -v, --verbose         More logs from librespot and the API client
```

`oxidify.log` in the state directory is what to attach to a bug report:
it contains the last run's output, including the additional lines printed by
`oxidify -v`. If the app crashed, attach `panic.log` from the same directory
as well.

## Demo mode

Builds made with `cargo build --features demo` accept `--demo`, which fills
the interface with sample data, useful for screenshots, theming, and
interface work. Demo mode never writes settings.

`--demo-page` opens a page, such as `home`, `playlist:pl1`, or `artist:art0`,
and `--demo-show` adds surfaces on top of it: a comma separated list of
`queue`, `now-playing`, `collapsed`, `podcasts`, `devices`, `shortcuts`,
`create`, `light`, `focus`, `login`, `winamp`, `playlist`, and `eq`.

`--demo-shot <PATH>` writes the window to a PNG and exits, which is how the
screenshots in these pages are made:

```
cargo run --release --features demo -- \
  --demo-shot docs/screenshot.png --demo-page playlist:pl1 --demo-show now-playing
```

The shot is the window's own frame buffer, so it comes out at whatever size
the window is. `--demo-shot-delay <MS>` sets how long cover art has to arrive
before the frame is taken.
