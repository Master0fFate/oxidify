---
title: Download
description: Get Oxidify for macOS, Windows, or Linux, with install instructions for each.
nav_order: 1
---

{% assign v = site.oxidify_version %}
{% assign base = "https://github.com/Master0fFate/oxidify/releases/download/v" | append: v %}

The current version is **v{{ v }}**. Every file below, with its SHA-256, is
listed in [checksums.txt]({{ base }}/checksums.txt); all versions live on
the [releases page](https://github.com/Master0fFate/oxidify/releases).

## macOS

One download for both Apple Silicon and Intel:

- [oxidify-v{{ v }}-macos-universal.dmg]({{ base }}/oxidify-v{{ v }}-macos-universal.dmg)

Open it and drag **Oxidify** to Applications.

### First open on macOS

This build is not notarized with Apple, so macOS blocks it the first
time. Recent macOS versions (Sequoia and later) no longer let you bypass
this with a right-click, so you open it once through Privacy & Security:

1. Double-click **Oxidify** in Applications. macOS says it cannot be
   opened because Apple cannot check it for malicious software. Click
   **Done** (do **not** click Move to Trash).
2. Open **System Settings**, then **Privacy & Security**.
3. Scroll down to the **Security** section, find *"Oxidify was blocked
   to protect your Mac"*, and click **Open Anyway**.
4. Authenticate, then click **Open Anyway** once more.

macOS remembers the choice, so later launches work with an ordinary
double-click.

## Windows

The installer adds Oxidify to the Start menu and needs no administrator
rights. Choose x86_64 for most PCs or aarch64 for Windows on ARM:

- [oxidify-v{{ v }}-x86_64-pc-windows-msvc-setup.exe]({{ base }}/oxidify-v{{ v }}-x86_64-pc-windows-msvc-setup.exe)
- [oxidify-v{{ v }}-aarch64-pc-windows-msvc-setup.exe]({{ base }}/oxidify-v{{ v }}-aarch64-pc-windows-msvc-setup.exe)

If you would rather not install anything, the same program comes as a zip:
unpack it and run `oxidify.exe`.

- [oxidify-v{{ v }}-x86_64-pc-windows-msvc.zip]({{ base }}/oxidify-v{{ v }}-x86_64-pc-windows-msvc.zip)
- [oxidify-v{{ v }}-aarch64-pc-windows-msvc.zip]({{ base }}/oxidify-v{{ v }}-aarch64-pc-windows-msvc.zip)

Either way, SmartScreen may warn about an unknown publisher on first run;
choose More info, then Run anyway.

## Linux

- [oxidify-v{{ v }}-x86_64-unknown-linux-gnu.tar.gz]({{ base }}/oxidify-v{{ v }}-x86_64-unknown-linux-gnu.tar.gz)
- [oxidify-v{{ v }}-aarch64-unknown-linux-gnu.tar.gz]({{ base }}/oxidify-v{{ v }}-aarch64-unknown-linux-gnu.tar.gz)

Unpack, put `oxidify` on your PATH, and copy the desktop entry and icon
from the bundled `packaging/` directory if you want it in your launcher.
Runtime needs are the ordinary desktop libraries: ALSA, PulseAudio or
PipeWire, and Wayland or X11.

Or build from source: see [Getting Started](/getting-started/).
