//! Keyboard shortcuts.

use egui::{Key, Modifiers};

use crate::app::App;
use crate::model::{Action, Dialog, Page};

pub fn handle(app: &mut App, ctx: &egui::Context) {
    let typing = ctx.memory(|memory| memory.focused().is_some());
    // Text fields own caret-navigation arrows; other focused widgets still
    // allow player and page-navigation shortcuts.
    let editing_text = ctx.text_edit_focused();
    let mut actions = Vec::new();
    ctx.input_mut(|input| {
        let mut key = |modifiers: Modifiers, key: Key, action: Action| {
            if input.consume_key(modifiers, key) {
                actions.push(action);
            }
        };
        // egui's plain-key matcher accepts an extra Shift. Consume the
        // more specific chords first, especially Queue before Quit.
        key(
            Modifiers::COMMAND | Modifiers::SHIFT,
            Key::A,
            Action::OpenUri("artist".into()),
        );
        key(
            Modifiers::COMMAND | Modifiers::SHIFT,
            Key::B,
            Action::OpenUri("album".into()),
        );
        // Cmd+Shift+Q is Log Out, taken by the window server.
        if cfg!(target_os = "macos") {
            key(Modifiers::COMMAND, Key::U, Action::ToggleQueuePanel);
        } else {
            key(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::Q,
                Action::ToggleQueuePanel,
            );
        }
        key(Modifiers::COMMAND, Key::F, Action::FocusSearch);
        key(Modifiers::COMMAND, Key::B, Action::ToggleSidebar);
        key(Modifiers::COMMAND, Key::Comma, Action::Open(Page::Settings));
        key(Modifiers::COMMAND, Key::Q, Action::Quit);
        // winit installs its own macOS app menu, whose Hide item owns Cmd+H
        // before the window is offered the key.
        if cfg!(target_os = "macos") {
            key(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::H,
                Action::Open(Page::Home),
            );
        } else {
            key(Modifiers::COMMAND, Key::H, Action::Open(Page::Home));
        }
        key(Modifiers::COMMAND, Key::L, Action::Open(Page::LikedSongs));
        // Cmd+M minimises on macOS.
        if cfg!(target_os = "macos") {
            key(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::M,
                Action::ToggleWinampWindow,
            );
        } else {
            key(Modifiers::COMMAND, Key::M, Action::ToggleWinampWindow);
        }
        key(
            Modifiers::COMMAND,
            Key::Slash,
            Action::ShowDialog(Dialog::Shortcuts),
        );
        if !editing_text {
            key(Modifiers::ALT, Key::ArrowLeft, Action::Back);
            key(Modifiers::ALT, Key::ArrowRight, Action::Forward);
            key(Modifiers::COMMAND, Key::ArrowLeft, Action::Previous);
            key(Modifiers::COMMAND, Key::ArrowRight, Action::Next);
            key(Modifiers::COMMAND, Key::ArrowUp, Action::VolumeBy(5));
            key(Modifiers::COMMAND, Key::ArrowDown, Action::VolumeBy(-5));
        }
        if !typing {
            key(
                Modifiers::NONE,
                Key::Questionmark,
                Action::ShowDialog(Dialog::Shortcuts),
            );
            key(
                Modifiers::SHIFT,
                Key::Questionmark,
                Action::ShowDialog(Dialog::Shortcuts),
            );
            key(Modifiers::SHIFT, Key::ArrowLeft, Action::SeekBy(-10_000));
            key(Modifiers::SHIFT, Key::ArrowRight, Action::SeekBy(10_000));
            key(Modifiers::NONE, Key::Space, Action::TogglePlay);
            key(Modifiers::NONE, Key::M, Action::ToggleMute);
            key(Modifiers::NONE, Key::S, Action::ToggleShuffle);
            key(Modifiers::NONE, Key::R, Action::CycleRepeat);
            key(Modifiers::NONE, Key::Q, Action::ToggleQueuePanel);
            key(Modifiers::NONE, Key::L, Action::ToggleLyricsPanel);
            key(Modifiers::NONE, Key::Slash, Action::FocusSearch);
        }
    });
    if !typing
        && ctx.input_mut(|input| {
            let pressed = input.events.iter().any(|event| {
                matches!(event, egui::Event::Key {
                    key: Key::B,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } if *modifiers == Modifiers::NONE)
            });
            pressed && input.consume_key(Modifiers::NONE, Key::B)
        })
        && let Some(now) = app.now_playing().filter(|now| !now.is_episode)
    {
        actions.push(Action::ToggleSaved(now.uri));
    }
    // Resolve the "open current artist/album" placeholders.
    for action in actions {
        match action {
            Action::OpenUri(kind) if kind == "artist" => {
                if let Some(id) = app
                    .now_playing()
                    .and_then(|now| now.artists.first().and_then(|artist| artist.id.clone()))
                {
                    app.actions.push(Action::Open(Page::Artist(id)));
                }
            }
            Action::OpenUri(kind) if kind == "album" => {
                if let Some(now) = app.now_playing() {
                    if let Some(id) = now.album_id {
                        app.actions.push(Action::Open(Page::Album(id)));
                    } else if let Some(id) = now.show_id {
                        app.actions.push(Action::Open(Page::Show(id)));
                    }
                }
            }
            other => app.actions.push(other),
        }
    }
    let (back, forward) = ctx.input(|input| {
        (
            input.pointer.button_pressed(egui::PointerButton::Extra1),
            input.pointer.button_pressed(egui::PointerButton::Extra2),
        )
    });
    if back {
        app.actions.push(Action::Back);
    }
    if forward {
        app.actions.push(Action::Forward);
    }
    if ctx.input(|input| input.key_pressed(Key::Escape)) {
        if app.dialog.is_some() {
            app.actions.push(Action::CloseDialog);
        } else if app.show_devices {
            app.show_devices = false;
        }
    }
}

pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Space", "Play or pause"),
    ("Ctrl+←  /  Ctrl+→", "Previous or next"),
    ("Shift+←  /  Shift+→", "Seek 10 seconds"),
    ("Ctrl+↑  /  Ctrl+↓", "Volume up or down"),
    ("M", "Mute or unmute"),
    ("B", "Like or unlike the playing song"),
    ("S", "Toggle shuffle"),
    ("R", "Cycle repeat"),
    ("Q", "Show the queue"),
    ("L", "Show the lyrics"),
    ("Ctrl+F  or  /", "Search"),
    ("Ctrl+B", "Show or hide the sidebar"),
    ("Alt+←  /  Alt+→", "Back or forward"),
    (
        if cfg!(target_os = "macos") {
            "Ctrl+Shift+H"
        } else {
            "Ctrl+H"
        },
        "Home",
    ),
    ("Ctrl+L", "Liked Songs"),
    ("Ctrl+Shift+A", "Go to the playing artist"),
    ("Ctrl+Shift+B", "Go to the playing album"),
    (
        if cfg!(target_os = "macos") {
            "Ctrl+Shift+M"
        } else {
            "Ctrl+M"
        },
        "Winamp mini player",
    ),
    ("Ctrl+,", "Settings"),
    ("Ctrl+/ or ?", "Keyboard shortcuts"),
    ("Ctrl+Q", "Quit"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppOptions;
    use crate::paths::AppDirs;
    use crate::settings::Settings;

    fn with_app(name: &str, check: impl FnOnce(&mut App)) {
        let root = std::env::temp_dir().join(format!("oxidify-keys-{name}-{}", std::process::id()));
        let mut app = App::new(
            &crate::backend::Waker::default(),
            AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            Settings::default(),
            AppOptions {
                media_controls: false,
                tray: false,
            },
        );
        crate::demo::populate(&mut app);
        check(&mut app);
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    fn key_event(key: Key, modifiers: Modifiers, repeat: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers,
        }
    }

    fn dispatch(app: &mut App, events: Vec<egui::Event>, typing: bool) {
        app.actions.clear();
        let ctx = egui::Context::default();
        if events
            .iter()
            .any(|event| matches!(event, egui::Event::Key { repeat: true, .. }))
        {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events: vec![key_event(Key::B, Modifiers::NONE, false)],
                    ..Default::default()
                },
                |_| {},
            );
            output.textures_delta.clear();
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                if typing {
                    ui.text_edit_singleline(&mut String::new()).request_focus();
                }
                handle(app, &ctx);
            },
        );
        output.textures_delta.clear();
    }

    fn command_modifiers() -> Modifiers {
        if cfg!(target_os = "macos") {
            Modifiers::MAC_CMD | Modifiers::COMMAND
        } else {
            Modifiers::CTRL | Modifiers::COMMAND
        }
    }

    #[test]
    fn shift_shortcuts_are_not_consumed_by_the_plain_shortcuts() {
        with_app("shift", |app| {
            let command = command_modifiers();
            let shifted = command | Modifiers::SHIFT;
            if !cfg!(target_os = "macos") {
                dispatch(app, vec![key_event(Key::Q, shifted, false)], false);
                assert!(matches!(app.actions.as_slice(), [Action::ToggleQueuePanel]));
            }
            dispatch(app, vec![key_event(Key::Q, command, false)], false);
            assert!(matches!(app.actions.as_slice(), [Action::Quit]));
            let album = app.now_playing().unwrap().album_id.unwrap();
            dispatch(app, vec![key_event(Key::B, shifted, false)], false);
            assert!(
                matches!(app.actions.as_slice(), [Action::Open(Page::Album(id))] if id == &album)
            );
            dispatch(app, vec![key_event(Key::B, command, false)], false);
            assert!(matches!(app.actions.as_slice(), [Action::ToggleSidebar]));
        });
    }

    #[test]
    fn text_fields_keep_caret_arrows_but_other_focus_keeps_player_shortcuts() {
        with_app("caret", |app| {
            let ctx = egui::Context::default();
            let field = egui::Id::new("caret-field");
            let row = egui::Id::new("caret-row");
            let command = command_modifiers();
            let mut text = String::from("find this song");
            let end = text.chars().count();
            // The app dispatches shortcuts before drawing its text fields.
            let frame = |app: &mut App, text: &mut String, events| {
                app.actions.clear();
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        handle(app, ui.ctx());
                        ui.add(egui::TextEdit::singleline(text).id(field));
                        ui.interact(
                            egui::Rect::from_min_size(
                                egui::pos2(0.0, 100.0),
                                egui::vec2(200.0, 28.0),
                            ),
                            row,
                            egui::Sense::click(),
                        );
                    },
                );
                output.textures_delta.clear();
            };
            frame(app, &mut text, vec![]);
            ctx.memory_mut(|memory| memory.request_focus(field));
            frame(app, &mut text, vec![]);
            let command_left = if cfg!(target_os = "macos") { 0 } else { 10 };
            for (key, modifiers, expected) in [
                (Key::ArrowLeft, command, command_left),
                (Key::ArrowLeft, Modifiers::ALT, 10),
                (Key::ArrowUp, command, 0),
            ] {
                let mut state = egui::TextEdit::load_state(&ctx, field).unwrap();
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(
                        egui::text::CCursor::new(end),
                    )));
                state.store(&ctx, field);
                frame(app, &mut text, vec![key_event(key, modifiers, false)]);
                assert!(
                    app.actions.is_empty(),
                    "{key:?} with {modifiers:?} triggered a shortcut"
                );
                let caret = egui::TextEdit::load_state(&ctx, field)
                    .and_then(|state| state.cursor.char_range())
                    .map(|range| range.primary.index);
                assert_eq!(caret, Some(egui::text::CCursor::new(expected).index));
            }
            assert_eq!(text, "find this song");
            ctx.memory_mut(|memory| memory.surrender_focus(field));
            frame(app, &mut text, vec![]);
            frame(
                app,
                &mut text,
                vec![key_event(Key::ArrowRight, command, false)],
            );
            assert!(matches!(app.actions.as_slice(), [Action::Next]));
            frame(
                app,
                &mut text,
                vec![key_event(Key::ArrowLeft, Modifiers::ALT, false)],
            );
            assert!(matches!(app.actions.as_slice(), [Action::Back]));
            ctx.memory_mut(|memory| memory.request_focus(row));
            frame(app, &mut text, vec![]);
            assert!(!ctx.text_edit_focused());
            frame(
                app,
                &mut text,
                vec![key_event(Key::ArrowRight, command, false)],
            );
            assert!(matches!(app.actions.as_slice(), [Action::Next]));
        });
    }

    #[test]
    fn b_likes_the_playing_track_but_not_typing_or_key_repeat() {
        with_app("like", |app| {
            dispatch(app, vec![key_event(Key::B, Modifiers::NONE, false)], false);
            assert!(matches!(
                app.actions.as_slice(),
                [Action::ToggleSaved(uri)] if uri == "spotify:track:trk0"
            ));
            dispatch(app, vec![key_event(Key::B, Modifiers::NONE, false)], true);
            assert!(app.actions.is_empty());
            dispatch(app, vec![key_event(Key::B, Modifiers::NONE, true)], false);
            assert!(app.actions.is_empty());
            dispatch(
                app,
                vec![key_event(Key::B, Modifiers::COMMAND, false)],
                false,
            );
            assert!(matches!(app.actions.as_slice(), [Action::ToggleSidebar]));
        });
    }

    #[test]
    fn b_does_not_like_episodes_or_an_empty_player() {
        with_app("episode", |app| {
            app.remote.as_mut().unwrap().state.item = Some(
                crate::api::models::PlayableItem::Episode(crate::api::models::Episode {
                    uri: "spotify:episode:test".into(),
                    ..Default::default()
                }),
            );
            dispatch(app, vec![key_event(Key::B, Modifiers::NONE, false)], false);
            assert!(app.actions.is_empty());
            app.remote = None;
            dispatch(app, vec![key_event(Key::B, Modifiers::NONE, false)], false);
            assert!(app.actions.is_empty());
        });
    }

    #[test]
    fn mouse_buttons_navigate_on_press_not_release() {
        with_app("mouse", |app| {
            for button in [egui::PointerButton::Extra1, egui::PointerButton::Extra2] {
                for pressed in [true, false] {
                    dispatch(
                        app,
                        vec![egui::Event::PointerButton {
                            pos: egui::Pos2::ZERO,
                            button,
                            pressed,
                            modifiers: Modifiers::NONE,
                        }],
                        false,
                    );
                    if !pressed {
                        assert!(app.actions.is_empty());
                    } else if button == egui::PointerButton::Extra1 {
                        assert!(matches!(app.actions.as_slice(), [Action::Back]));
                    } else {
                        assert!(matches!(app.actions.as_slice(), [Action::Forward]));
                    }
                }
            }
        });
    }
}
