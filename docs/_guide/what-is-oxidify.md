---
title: What is Oxidify?
description: Why Oxidify exists, what it supports, and its current limitations.
nav_order: 0
---

## Why Oxidify

The official Spotify client includes a browser engine and can use a
significant amount of memory. Most lightweight alternatives use a terminal
interface. Oxidify provides a small graphical client instead.

Oxidify is a native music player written in Rust with
[egui](https://github.com/emilk/egui), playing music through
[librespot](https://github.com/librespot-org/librespot). It is a single native
binary with no embedded browser engine. It starts in well under a second and
uses a layout similar to Spotify's desktop client.

![Oxidify showing a playlist with the queue open and a track playing](/screenshot.png)

## What it does

- **Plays music on this computer.** With Spotify Premium, Oxidify is a
  Spotify Connect device: pick it from your phone, or press play here.
  Gapless, up to 320 kbps, with optional volume normalisation and an on-disk
  audio cache.
- **Plays without Premium.** Free and unconfirmed accounts are routed to
  alternate local playback. Oxidify races native YouTube and optional Piped
  search, then applies one match score across all results. yt-dlp is the final
  compatibility fallback, not the normal path. Those matches are not Spotify
  audio; the interface labels their actual route, and you are responsible for
  using this lawfully under the terms of your account and of the providers you
  configure.
- **Controls other devices.** Move playback to a speaker, a phone, or
  another computer from the device picker, and keep controlling it: play,
  pause, skip, seek, shuffle, repeat, volume.
- **Library access.** Playlists, Liked Songs, saved albums, followed
  artists, podcasts, and saved episodes, filterable in the sidebar and as
  full pages. Playlists you own can be created, edited, and reordered.
- **Search** across songs, artists, albums, playlists, podcasts, and
  episodes, with artist pages, discographies, and related artists.
- **Stays out of the way.** Closing the window keeps the music playing from
  the system tray. MPRIS means your media keys, your bar, and `playerctl`
  see it like any other player. Every common action has a keyboard shortcut.
- **Looks the part.** Pages and the player bar can take a tint from the
  album art of whatever you are looking at or listening to; light and dark
  themes, or follow the system.

## What it does not do

Oxidify deliberately has a limited scope:

- **Spotify Connect on this computer needs Spotify Premium**, as with every
  librespot-based client. Browsing, search, and remote control work on any
  account. Free or unconfirmed accounts use alternate local audio for
  third-party matches; those matches are not Spotify audio.
- Initial setup has two sign-ins because Spotify grants Web API and streaming
  access separately. [How it connects](/how-it-connects/) explains why.
- Local playback tops out at 320 kbps. Spotify protects its lossless streams
  with DRM that librespot does not support, and Oxidify will not circumvent
  it. This can change if [lawful support lands upstream](https://github.com/librespot-org/librespot/issues/1583).
- Alternate playback does not support podcasts, and Spotify Lossless is not
  available through it either.
- No video podcasts or social features.
- Playlist reordering is a menu action, not drag-and-drop.
- Oxidify is an **unofficial** client built on Spotify's public Web API
  and librespot. Spotify changes these from time to time; when they do,
  features can break until the client catches up.

If something misbehaves, [an issue](https://github.com/Master0fFate/oxidify/issues)
should include the output of `oxidify -v`, what happened, and what you
expected.

## Account safety

We are not aware of a Spotify account being suspended for using Oxidify
or another librespot player with Premium. Sign-in happens on Spotify's own
pages, audio uses the quality included with Premium, DRM stays intact, and
Oxidify does not rip tracks or block ads.

Reported suspensions usually involve modded apps that remove ads from free
accounts, track ripping, or stream manipulation. Oxidify does none of
those things, and its contribution rules prohibit them.

Alternate local playback is different: it does not stream from Spotify at
all, so Spotify's streaming terms do not describe it. Whether matching and
playing third-party audio is acceptable under the terms of your account and
of the services you point it at is your responsibility. Oxidify makes no
promise that any provider approves of it.

## Prior art

Oxidify is derived from
[Fastpotify](https://github.com/crmne/fastpotify) by Carmine Paolino, an MIT
licensed project, and keeps its architecture and much of its code. Fastpotify
itself uses [librespot](https://github.com/librespot-org/librespot) for
Spotify playback. [spotify-tui](https://github.com/Rigellute/spotify-tui),
[spotify-player](https://github.com/aome510/spotify-player), and
[ncspot](https://github.com/hrkfdn/ncspot) demonstrated the scope possible in
a small client.

Oxidify is an independent project, not affiliated with or endorsed by
Spotify AB or by the Fastpotify author. Spotify is a trademark of
Spotify AB.
