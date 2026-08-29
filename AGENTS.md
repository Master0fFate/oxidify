# Oxidify agent guide

Follow `CONTRIBUTING.md`; it is the canonical product and contribution policy.
These instructions add implementation constraints for coding agents.

## Product boundaries

- Keep Oxidify a small native music player for a Spotify library, with its
  alternate local playback mode. Do not add a browser engine, telemetry, or a
  hosted backend.
- Spotify playback capabilities come from librespot and require confirmed
  Premium. Do not advertise or implement a capability merely because its name
  appears in a protobuf or enum. In particular, do not pursue Spotify Lossless
  or DRM circumvention unless lawful support first lands upstream.
- Alternate playback serves accounts that cannot use Spotify Connect. Its
  audio comes from third-party matches, it is labelled as such in the
  interface, and Spotify tokens never reach alternate providers. Preserve
  those warnings and that separation.
- Do not broaden a task into adjacent features or a general refactor. Preserve
  existing user behaviour unless the task explicitly changes it.

## Architecture

- `src/ui/` draws views and emits `Action`s. Apply actions after drawing in
  `src/app.rs`; do not mutate application state from inside a borrowed view.
- Network and playback work belongs on the runtime in `src/backend.rs`, in
  the player engine in `src/player.rs`, or in `src/alternate/`, never as
  blocking work on the UI thread.
- Keep platform integrations behind target-specific modules or `cfg` blocks.
  A fix for one platform must keep the other two targets compiling.
- Settings and state files must remain readable, backward compatible, and
  atomically written. Never log credentials or authorization responses.
- Prefer existing dependencies. Explain any new crate in `Cargo.toml` next to
  the dependency when the reason is not obvious.

Read `docs/_reference/how-it-connects.md` before changing authentication,
Spotify requests, Connect, credential storage, or network behaviour. Read the
nearby module tests before changing a state machine or API fallback.

## Branches

Work on `main`. Commit there directly, one topic per commit, each
compiling and passing the checks on its own. Feature branches and pull
requests are for outside contributors; the maintainer's own work, and
work done with the maintainer, does not go through them.

## Definition of done

- Add focused regression tests for changed behaviour. Use the `demo` feature
  for deterministic UI coverage and screenshots.
- Update the README and docs when user-visible behaviour, settings, files, or
  network access changes.
- Run the full checks from `CONTRIBUTING.md`. Do not weaken a lint, delete a
  test, or add an `allow` merely to make CI green without explaining why the
  underlying rule does not apply.
- Report platform coverage honestly. Do not claim a platform was tested when
  it was only compiled or reasoned about.

## Releases

A release is not the tag alone. Every one of these moves together:

- `Cargo.toml` version (and the lockfile via a build), committed before
  the tag so the binaries report the right version.
- The `v*` tag, which triggers the release workflow; replace its
  generated notes with written ones.
- The Homebrew cask in the maintainer's tap and the AUR package, both
  from the release's `checksums.txt`, when they exist.

Missing any of these ships a release that lies somewhere; the dropdown
was forgotten once already.
