---
title: Everyday Use
description: Playing music, managing your library and playlists, devices, the queue, the tray, and every keyboard shortcut.
nav_order: 3
---

## Playing music

Playlist, album, artist, and podcast pages have play buttons. Cards show one
when you hover, and each row has its own. Double-click a row to start playback
from that song within its playlist or album. The shuffle button next to a
page's play button starts the page in shuffled order.

An unfinished podcast episode continues from the saved place shown in its
row. A finished or unstarted episode starts at the beginning. The active
episode keeps its live position, and episodes played from a playlist keep
that playlist's queue and displayed order. This applies to supported Spotify
playback; alternate local audio does not support podcast episodes.

The player bar shows what is playing locally or on another device. Click the
title to open its album, an artist name to open that artist, or the plus to
add the track to Liked Songs; a filled check means it is there already.
Drag the cover or title onto a playlist in Your Library, or between rows of
an open playlist, to add it. The arrow in the cover's corner, or the first
button at the right of the bar, opens the **Now Playing view**: the cover
large beside the page, the artist, and what is next in the queue. The
queue, the lyrics, and the Now Playing view share that column, one at a
time. The wheel over the volume slider
moves it five percent at a time. When Spotify reports that a device cannot
accept remote volume changes, volume and mute are disabled. Use that device's
own controls instead; keyboard and media-control volume commands are ignored
for the same device.

Local Spotify playback keeps sample-rate conversion continuous between audio
packets when the output uses a different rate, such as 48 kHz on Windows.

## Home

The chips above Home choose what it shows: everything, music only, or your
podcasts and saved episodes. The shortcuts under them are Liked Songs and
the playlists you played last. Home previews your most-played songs; select
**Your top songs**, **Show all**, or **Show more top songs** to open the
complete ranked list.

Track tables sort by their column headings: click **Title**, **Album**,
**Date added**, or the clock to sort by it, again to reverse, and a third
time to return to the list's own order. Sorted and filtered views are reused
until the list, filter, sort, or contributor names change, rather than sorted
again on every frame. The play button in Liked Songs follows the sorted view.

## Your Library

Pinned entries sit in a block right under Liked Songs: pin one from its
context menu, drag a row into the block to pin it where you drop it,
drag within the block to reorder it, and drag a pinned row below the
block to unpin it.

Below the pins, the sidebar starts out sorting playlists by when you
last played them. Drag one to a new place and the rest of the shelf
switches to your own order instead: rows stay exactly where you drop
them, and new playlists wait just under the pins until you place them.
Choose **Sort by recently played** from any playlist's context menu to
go back; dragging a row switches to your own order again.

The Albums, Artists, and Podcasts shelves pin the same way: drag into
the block, within it, or below it.

Use the chips to filter Your Library by Playlists, Artists, Albums, or
Podcasts, or use the magnifier to search it. The control at the right of
that row names the order the shelf is in, **Recents** or **Custom order**,
and switches it back to recently played or between full and compact rows.
Liked Songs stays at the top. The current page is highlighted, and the
playing playlist has a small speaker icon.

Click **Your Library** to fold the panel down to a rail of covers; each
still opens its page, takes a dropped song, and answers a right-click, and
the library icon opens it out again. `Ctrl+B` hides it altogether and
brings it back.

**Playlists you own** are fully editable: create one with the **Create**
button,
add songs from any row's menu (the list filters as you type) or by dragging
them onto a playlist in the sidebar or onto a gap in an open playlist page,
remove and reorder from the playlist page, and rename or delete from its
context menu. Reordering works by dragging a row to its new place, or from
its menu; while the table is sorted or filtered, rows keep their place.
**Remove from this playlist** remains available in those views, and playback
follows the visible songs in their displayed order. Clearing a destination
filter restores the Add to playlist menu's full scrollable height.
Adding a song that is already in the playlist asks first. Dropping a song on
Liked Songs saves it. Playlists you follow can be followed and unfollowed.

Settings → Appearance can show the library as names only and track tables as
one-line rows without covers. Date added is relative for the first month.
The date column is hidden when no real dates are available; rows with missing
dates stay aligned when other rows have dates.

## Search

Ctrl+F (or `/`) focuses search from anywhere. Results are grouped into top
result, songs, artists, albums, playlists, podcasts, and episodes. Use the
chips to show one type. The empty search page lists recent searches.
Clearing a previous query keeps your current page open. Typing the next query
opens Search again.

## Devices and the queue

The speaker icon in the player bar lists every Spotify Connect device on
your account; long lists scroll. Click one and the music moves there
mid-song; the same controls keep working. "Playing on …" in the top bar reminds you when sound
is coming out of something across the room. If you turned on alternate local
audio, this computer is listed as a local player with that limitation, not as
a Connect device, and the top bar names the match source. Playback starts as
soon as the audio headers are in; some M4A files wait until download finishes.
Network stalls and transient HTTP errors retry and resume. A terminal
transport or decode failure stops the track instead of skipping.

The queue lives behind the list icon, as a side panel or a full page. It
names the album or playlist the current song came from. Add anything to it
from a row's context menu. Right-click Home and Search cards for the same
actions as elsewhere.

### Receivers on the local network

A receiver running librespot or spotifyd, and some hardware speakers, appears
in Spotify's device list only after it has received an account credential.
Before then, the Web API cannot see it.

Oxidify searches the local network when you open the device picker. It
lists discovered receivers as *on your network*. Choose one to send it the
stored playback credential, encrypted so that only that receiver can read it.
Once connected, it appears as an ordinary Spotify Connect device and playback
moves to it.

This uses the credential stored for playing on this computer, so enable
playback here first (see [Getting Started](/getting-started/)). Receivers
that ask for a different kind of login are not connected this way yet.

## Lyrics

The microphone button in the player bar (or `L`) opens lyrics for the playing
track beside the page. For timed lyrics, the current line is
highlighted and the panel scrolls automatically; click a line to seek to it.
Manual scrolling pauses automatic following, and **Follow** resumes it.
Oxidify requests lyrics from Spotify when local playback is authorized.
Otherwise, or when Spotify has no lyrics for a track, it uses
[LRCLIB](https://lrclib.net), an open database that needs no account. Podcasts
and tracks without a transcription show an unavailable message.

![The lyrics panel beside a playlist, following the song](/assets/images/lyrics.png)

## The Winamp mini player

Ctrl+M (Cmd+Shift+M on macOS), the shrink button beside the settings gear,
or **Switch to it** in Settings turns Oxidify into a small player that
wears classic Winamp skins: the `.wsz` files of the Winamp 2 era, of which
the [Winamp Skin Museum](https://skins.webamp.org) keeps tens of thousands.
There is one window at a time; the logo in the skin's corner, Eject, or
Ctrl+M again brings the big window back where it was.

![The mini player wearing the built-in skin](/assets/images/winamp.png)

Drop a `.wsz` on either window and Oxidify copies it into its skins
folder and puts it on. Settings lists every skin in that folder, with the
built-in one first, and has a button to open the folder.

The window is drawn at a whole number of screen pixels per skin pixel, so
the pixels stay crisp at any size. Pick 1x to 4x from the menu behind a
right-click on the title bar (or the **O** at the display's edge), where
always-on-top lives too; **D** toggles double size and **A** always on top,
as they did. Drag the title bar to move it; it reopens where you left it,
and the keyboard shortcuts work there too.

The buttons do what they say, with a few translations. **Stop** pauses and
rewinds, **I** opens the playing album in the big window, and repeat is on
or off. **PL** opens the playlist window under the player, in the skin's
own frame and colours and the small unsmoothed lettering of the time: what
is playing, then the queue; double-click a
song to play from there, Ctrl-click to select several, drag the corner to
make it taller, and its X or PL again closes it. Its buttons do what
Spotify allows of what Winamp's did: ADD finds music, SEL picks rows,
MISC opens the song's pages, LIST OPTS plays one of your playlists or
saves the queue as a new one, and REM only explains that no app can take
from Spotify's queue. Notices that the big window shows as toasts scroll
through the marquee here. **EQ** opens the equalizer between the player and the playlist: Winamp's
ten bands and its presets, shaping the music played on this computer (a
speaker across the room plays what Spotify sends it). The preamp only
turns down, and AUTO, which loaded a preset per song, stays off. The same
equalizer is in Settings with its curve drawn from the same digital filters
used for local Spotify playback. The bands compensate for their overlap;
alternate local audio does not yet use this equalizer. The X and both logos
of the main window bring back the big window; its shade button, or a
double-click on the title bar, rolls it up to a bar with the time, a small
transport, and a seek bar, as Winamp's shade mode did. Skins that are not
rectangles keep their shape: whatever their `region.txt` leaves out is
see-through. Quitting is in the right-click menu and Ctrl+Q. Oxidify has no balance
control, so that slider is drawn but does nothing.
Click the time to count down instead of up. The balance slider moves the
sound between the speakers and the MONO and STEREO lamps are a switch,
both for music played on this computer. The playlist's own shade button
rolls it up to a title bar and down again. The display's left box is the
spectrum analyser, peaks and all, in the skin's own colours; click it, or
**V**, for the oscilloscope, and again for nothing. It shows the sound
leaving this computer, so a device across the room leaves it flat. Modern
(Winamp 3 and 5) skins are a different format and are not supported.

## The tray

Closing the window keeps the music playing: Oxidify stays in the system
tray with play, pause, skip, and quit in its menu, and clicking the icon
brings the window back. On Linux it is a standard status-notifier item, so
it works in any bar that shows tray icons, and MPRIS keeps `playerctl`,
media keys, and your desktop's players widget working the whole time.

## One window, one instance

Starting Oxidify while it is already running brings the existing window
forward instead of opening a second instance. This avoids duplicate Spotify
Connect devices and conflicting media-key handlers.

## Keyboard shortcuts

| Shortcut | What it does |
| --- | --- |
| `Space` | Play or pause |
| `Ctrl+←` / `Ctrl+→` | Previous or next |
| `Shift+←` / `Shift+→` | Seek 10 seconds |
| `Ctrl+↑` / `Ctrl+↓` | Volume |
| `M` | Mute |
| `B` | Like or unlike the playing song |
| `S` / `R` | Shuffle / cycle repeat |
| `Q` | Queue panel |
| `Ctrl+F` or `/` | Search |
| `Ctrl+B` | Show or hide Your Library |
| `Alt+←` / `Alt+→`, or mouse side buttons | Back or forward |
| `Ctrl+H` / `Ctrl+L` | Home / Liked Songs |
| `Ctrl+Shift+A` / `Ctrl+Shift+B` | Playing artist / album |
| `Ctrl+M` | Winamp mini player |
| `Ctrl+,` | Settings |
| `Ctrl+/` or `?` | All shortcuts |
| `Ctrl+Q` | Quit |

On macOS, `Cmd` replaces `Ctrl`. Text fields retain arrow-key editing and
selection; Shift shortcuts do not trigger their unshifted counterparts.

## Settings

Settings (Ctrl+,) includes the Connect device name, audio quality up to
320 kbps, volume normalisation, autoplay, gapless playback, the audio backend
on Linux, the audio cache size, the equalizer, themes, album-art tinting,
the mini player's skin and size, and close-to-tray behaviour. Applying playback settings restarts the local player. Other
settings take effect immediately.
