---
title: Getting Started
description: Install Oxidify, sign in through your browser, and enable playback on this computer.
nav_order: 2
---

## Install

The [Download page](/download/) has installers and archives for macOS,
Windows, and Linux.

Or build from source with [Rust](https://rustup.rs) 1.95 or newer:

```sh
git clone https://github.com/Master0fFate/oxidify
cd oxidify
cargo install --path .
```

On Linux the GUI needs the development packages any egui application does,
plus audio. On Arch:

```sh
sudo pacman -S --needed alsa-lib libpulse libxkbcommon wayland
```

On Debian or Ubuntu:

```sh
sudo apt install libasound2-dev libpulse-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev
```

Oxidify uses system fonts for scripts that its interface font does not
cover, including Chinese, Japanese, Korean, Arabic, Hebrew, Thai, and Indic
scripts. macOS and Windows include fonts for the common cases. On Linux,
install `noto-fonts` and `noto-fonts-cjk` (Arch) or `fonts-noto` and
`fonts-noto-cjk` (Debian or Ubuntu) if titles appear as empty boxes.

![Japanese, Chinese, and Korean titles in a playlist](/assets/images/scripts.png)

A desktop entry ships in `packaging/applications/oxidify.desktop`.

## Sign in

Start the app and press **Sign in with Spotify**. Your browser opens
Spotify's own consent page; your password never touches Oxidify. When
Spotify redirects back, your library loads and you can search, browse, and
control your other devices immediately.

Oxidify stores a refresh token in your platform's state directory
(`~/.local/state/oxidify` on Linux). You normally need the browser only
once per machine.

Coming from Fastpotify? The first run imports your settings, sign-ins,
skins, and playback credentials from Fastpotify's directories, once, and
never writes back to them.

## Enable playback on this computer

Playing music *on this machine* requires a second browser approval because
Spotify treats streaming as a separate grant
([why](/how-it-connects/)). Take it from the device menu (the speaker icon
in the player bar, then **Play here, set up once**) or from Settings.
It needs Spotify Premium. Oxidify saves the resulting playback credential
for later sessions.

After that, this computer shows up as a Spotify Connect device named
**Oxidify** (rename it in Settings), visible from your phone like any
speaker.

## Alternate local audio (optional)

Spotify Connect on this computer is the default and needs Premium. Settings
also has **Alternate local audio**. Oxidify selects it for a Free account,
or until Spotify confirms Premium; Premium users can select it in Settings.
The app still uses the Spotify Web API for your library and search, then looks
up a third-party match. Native YouTube is ready without extra setup. You can
also add a Piped endpoint in Settings. Those providers search concurrently and
the existing match score chooses the strongest result. Search and stream
lookups use bounded memory caches. yt-dlp runs only as the compatibility
fallback.

That mode is not Spotify Connect, not Spotify audio, and not a way to bypass
DRM. Oxidify does not ship a Piped instance. It does embed an official pinned
yt-dlp build in each supported release binary, extracts it into the local state
directory, and never downloads yt-dlp at runtime. A yt-dlp you installed is
used only when its version is strictly newer than that pin. You are responsible
for provider endpoints, binaries, and terms. Podcasts are not supported. A weak match is never played; you
can choose to skip to the next track instead. Playback starts after a short
buffer. An M4A file with metadata at the end may wait until download
finishes. Network stalls and transient HTTP errors retry and resume from
received ranges. A terminal transport or decode failure stops instead of
skipping.

## A few things worth knowing on day one

- **Closing the window does not stop the music.** Oxidify keeps playing
  from the system tray; reopen it from the tray icon and quit from the tray
  menu or Ctrl+Q. On macOS you can also reopen it from the Dock. Settings can
  turn this off.
- **Play requests show their progress.** A pressed play button spins until
  Spotify responds.
- **Common actions have shortcuts.** Space plays and pauses, Ctrl+F or `/`
  searches, and `Q` opens the queue. Ctrl+/ shows the full list.
- **Rows and cards have context menus.** Right-click a song, playlist, album,
  or artist to see actions such as queue, save, add to playlist, and copy link.
