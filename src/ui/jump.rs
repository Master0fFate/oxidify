//! Jump: a palette that finds anything already in the library, offline and
//! at once. `Ctrl+K` opens it over the page; typing narrows every song,
//! playlist, album, artist and podcast the app holds, plus a handful of
//! commands, with no request to Spotify. Enter plays a song or opens
//! anything else, `Ctrl+Enter` plays a playlist, album or artist straight
//! away, and the arrows move the choice.

use egui::{Align, Key, Layout, Modifiers, Rect, Sense, UiBuilder, pos2, vec2};

use crate::api::models::{PlayableItem, pick_image};
use crate::app::App;
use crate::model::{Action, Dialog, JumpEntry, JumpKind, Page};
use crate::theme::{self, Icon};

/// How many matches are shown; more than this is a narrower query away.
const SHOWN: usize = 40;
const ROW_HEIGHT: f32 = 52.0;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    if app.jump.entries.is_none() {
        app.jump.entries = Some(build(app));
        app.jump.ranked = None;
    }
    // Keys are taken before the field sees them, so the arrows move the
    // choice rather than the caret, and Enter does not leave the field.
    let (down, up, go, play_now, page_down, page_up) = ui.input_mut(|input| {
        (
            input.consume_key(Modifiers::NONE, Key::ArrowDown),
            input.consume_key(Modifiers::NONE, Key::ArrowUp),
            input.consume_key(Modifiers::NONE, Key::Enter),
            input.consume_key(Modifiers::COMMAND, Key::Enter),
            input.consume_key(Modifiers::NONE, Key::PageDown),
            input.consume_key(Modifiers::NONE, Key::PageUp),
        )
    });

    ui.horizontal(|ui| {
        theme::icon(ui, Icon::Search, 18.0, palette.secondary);
        let field = ui.add(
            egui::TextEdit::singleline(&mut app.jump.query)
                .id(egui::Id::new("jump-query"))
                .hint_text(
                    egui::RichText::new("Jump to a song, playlist, album, artist, or podcast")
                        .color(palette.dim),
                )
                .font(theme::regular(16.0))
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY),
        );
        if app.jump.focus_pending {
            app.jump.focus_pending = false;
            field.request_focus();
        }
    });
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    let query = app.jump.query.trim().to_lowercase();
    let stale = app
        .jump
        .ranked
        .as_ref()
        .is_none_or(|(for_query, _)| *for_query != query);
    if stale {
        let entries = app.jump.entries.as_deref().unwrap_or(&[]);
        let ranked = rank(entries, &query);
        app.jump.ranked = Some((query.clone(), ranked));
        app.jump.selected = 0;
    }
    let ranked: Vec<usize> = app
        .jump
        .ranked
        .as_ref()
        .map(|(_, ranked)| ranked.clone())
        .unwrap_or_default();
    let shown = ranked.len().min(SHOWN);
    if shown == 0 {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            let message = if query.is_empty() {
                "Nothing loaded yet. Open a page or two and come back."
            } else {
                "Nothing in your library matches that."
            };
            theme::text(ui, message, theme::regular(14.0), palette.secondary);
        });
        ui.add_space(24.0);
        hint_row(ui, &palette);
        return;
    }
    if down {
        app.jump.selected = (app.jump.selected + 1).min(shown - 1);
    }
    if up {
        app.jump.selected = app.jump.selected.saturating_sub(1);
    }
    if page_down {
        app.jump.selected = (app.jump.selected + 8).min(shown - 1);
    }
    if page_up {
        app.jump.selected = app.jump.selected.saturating_sub(8);
    }
    app.jump.selected = app.jump.selected.min(shown - 1);

    let mut chosen: Option<(usize, bool)> = None;
    if go {
        chosen = Some((app.jump.selected, false));
    } else if play_now {
        chosen = Some((app.jump.selected, true));
    }
    let selected = app.jump.selected;
    let list_height = (ROW_HEIGHT * shown as f32).min(ROW_HEIGHT * 8.5);
    egui::ScrollArea::vertical()
        .id_salt("jump-results")
        .max_height(list_height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.set_min_height(list_height.min(ROW_HEIGHT * shown as f32));
            ui.spacing_mut().item_spacing.y = 0.0;
            let width = ui.available_width();
            for (row, &index) in ranked.iter().take(shown).enumerate() {
                let Some(entry) = app
                    .jump
                    .entries
                    .as_ref()
                    .and_then(|entries| entries.get(index))
                    .cloned()
                else {
                    continue;
                };
                let (rect, response) =
                    ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::click());
                let is_selected = row == selected;
                if is_selected && (down || up || page_down || page_up || stale) {
                    ui.scroll_to_rect(rect, None);
                }
                if response.hovered() && ui.input(|input| input.pointer.is_moving()) {
                    app.jump.selected = row;
                }
                if ui.is_rect_visible(rect) {
                    if is_selected {
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(6),
                            palette.surface_hover,
                        );
                    }
                    let cover = Rect::from_min_size(
                        pos2(rect.left() + 8.0, rect.center().y - 18.0),
                        egui::Vec2::splat(36.0),
                    );
                    let round = entry.kind == JumpKind::Artist;
                    match entry.kind {
                        JumpKind::Liked => super::sidebar::liked_cover(ui, cover, 4.0),
                        JumpKind::Command => {
                            ui.painter().rect_filled(
                                cover,
                                egui::CornerRadius::same(4),
                                palette.surface,
                            );
                            let icon = entry.icon.unwrap_or(Icon::Sparkles);
                            icon.image(palette.text, 18.0).paint_at(
                                ui,
                                Rect::from_center_size(cover.center(), egui::Vec2::splat(18.0)),
                            );
                        }
                        _ => super::widgets::paint_cover(
                            ui,
                            &palette,
                            entry.image.as_deref(),
                            cover,
                            if round { 18.0 } else { 4.0 },
                            entry.kind.icon(),
                        ),
                    }
                    let kind = entry.kind.label();
                    let kind_width = 70.0;
                    let text_left = cover.right() + 12.0;
                    let text_rect = Rect::from_min_max(
                        pos2(text_left, rect.top() + 8.0),
                        pos2(rect.right() - kind_width - 12.0, rect.bottom() - 6.0),
                    );
                    let mut text_ui = ui.new_child(
                        UiBuilder::new()
                            .max_rect(text_rect)
                            .layout(Layout::top_down(Align::Min)),
                    );
                    text_ui.set_clip_rect(text_rect.intersect(ui.clip_rect()));
                    text_ui.spacing_mut().item_spacing.y = 1.0;
                    theme::text(&mut text_ui, &entry.name, theme::medium(14.0), palette.text);
                    if !entry.subtitle.is_empty() {
                        theme::text(
                            &mut text_ui,
                            &entry.subtitle,
                            theme::regular(12.0),
                            palette.secondary,
                        );
                    }
                    ui.painter().text(
                        pos2(rect.right() - 12.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        kind,
                        theme::regular(12.0),
                        palette.dim,
                    );
                }
                if response.clicked() {
                    chosen = Some((row, false));
                }
            }
        });
    ui.add_space(8.0);
    hint_row(ui, &palette);

    if let Some((row, play_now)) = chosen
        && let Some(entry) = ranked
            .get(row)
            .and_then(|&index| app.jump.entries.as_ref()?.get(index))
            .cloned()
    {
        let action = match (play_now, &entry.play) {
            (true, Some(play)) => play.clone(),
            _ => entry.primary.clone(),
        };
        app.actions.push(Action::CloseDialog);
        app.actions.push(action);
    }
}

fn hint_row(ui: &mut egui::Ui, palette: &theme::Palette) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        for (keys, what) in [
            ("↑↓", "choose"),
            ("↵", "open or play"),
            ("Ctrl+↵", "play"),
            ("Esc", "close"),
        ] {
            theme::text(ui, keys, theme::semibold(11.5), palette.secondary);
            theme::text(ui, what, theme::regular(11.5), palette.dim);
        }
    });
}

/// Everything the app holds that can be jumped to, built once per opening.
pub fn build(app: &App) -> Vec<JumpEntry> {
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |entry: JumpEntry| {
        if seen.insert(entry.key.clone()) {
            entries.push(entry);
        }
    };
    if let Some(user) = &app.user {
        push(JumpEntry {
            key: "liked".into(),
            kind: JumpKind::Liked,
            name: "Liked Songs".into(),
            subtitle: match app.library.liked.total {
                Some(total) => format!("{total} songs"),
                None => "Playlist".into(),
            },
            image: None,
            icon: None,
            primary: Action::Open(Page::LikedSongs),
            play: Some(Action::PlayContext {
                uri: format!("spotify:user:{}:collection", user.id),
                offset_uri: None,
                offset_index: None,
            }),
        });
    }
    if let Some(playlists) = app.library.playlists.get() {
        for playlist in playlists {
            push(JumpEntry {
                key: playlist.uri.clone(),
                kind: JumpKind::Playlist,
                name: playlist.name.clone(),
                subtitle: format!("By {}", playlist.owner_name()),
                image: pick_image(&playlist.images, 64).map(str::to_string),
                icon: None,
                primary: Action::Open(Page::Playlist(playlist.id.clone())),
                play: Some(play_context(&playlist.uri)),
            });
        }
    }
    for saved in &app.library.albums.items {
        let album = &saved.album;
        push(JumpEntry {
            key: album.uri.clone(),
            kind: JumpKind::Album,
            name: album.name.clone(),
            subtitle: crate::api::models::join_names(
                album.artists.iter().map(|artist| artist.name.as_str()),
            ),
            image: pick_image(&album.images, 64).map(str::to_string),
            icon: None,
            primary: Action::Open(Page::Album(album.id.clone())),
            play: Some(play_context(&album.uri)),
        });
    }
    for artist in &app.library.artists.items {
        push(JumpEntry {
            key: artist.uri.clone(),
            kind: JumpKind::Artist,
            name: artist.name.clone(),
            subtitle: "Artist".into(),
            image: pick_image(&artist.images, 64).map(str::to_string),
            icon: None,
            primary: Action::Open(Page::Artist(artist.id.clone())),
            play: Some(play_context(&artist.uri)),
        });
    }
    for saved in &app.library.shows.items {
        let show = &saved.show;
        push(JumpEntry {
            key: show.uri.clone(),
            kind: JumpKind::Podcast,
            name: show.name.clone(),
            subtitle: show.publisher.clone(),
            image: pick_image(&show.images, 64).map(str::to_string),
            icon: None,
            primary: Action::Open(Page::Show(show.id.clone())),
            play: None,
        });
    }
    // Songs: Liked Songs first, then every playlist and album page that has
    // been opened, each song playing on from where it was found.
    for saved in &app.library.liked.items {
        let track = &saved.track;
        push(song_entry(
            track,
            "Liked Songs",
            app.user
                .as_ref()
                .map(|user| format!("spotify:user:{}:collection", user.id)),
        ));
    }
    let mut playlist_pages: Vec<_> = app.playlist_pages.iter().collect();
    playlist_pages.sort_by(|a, b| a.0.cmp(b.0));
    for (id, page) in playlist_pages {
        let (name, uri) = match page.playlist.get() {
            Some(playlist) => (playlist.name.clone(), playlist.uri.clone()),
            None => ("Playlist".into(), format!("spotify:playlist:{id}")),
        };
        for item in &page.items.items {
            if let Some(PlayableItem::Track(track)) = item.playable() {
                push(song_entry(track, &name, Some(uri.clone())));
            }
        }
    }
    let mut album_pages: Vec<_> = app.album_pages.iter().collect();
    album_pages.sort_by(|a, b| a.0.cmp(b.0));
    for (_, page) in album_pages {
        let Some(album) = page.album.get() else {
            continue;
        };
        for track in &page.tracks.items {
            push(song_entry(track, &album.name, Some(album.uri.clone())));
        }
    }
    for command in commands(app) {
        push(command);
    }
    entries
}

fn play_context(uri: &str) -> Action {
    Action::PlayContext {
        uri: uri.to_string(),
        offset_uri: None,
        offset_index: None,
    }
}

fn song_entry(track: &crate::api::models::Track, from: &str, context: Option<String>) -> JumpEntry {
    let play = match context {
        Some(uri) => Action::PlayContext {
            uri,
            offset_uri: Some(track.uri.clone()),
            offset_index: None,
        },
        None => Action::PlayUris {
            uris: vec![track.uri.clone()],
            index: 0,
        },
    };
    JumpEntry {
        key: track.uri.clone(),
        kind: JumpKind::Song,
        name: track.name.clone(),
        subtitle: format!("{} • {}", track.artist_names(), from),
        image: track.image(64).map(str::to_string),
        icon: None,
        primary: play.clone(),
        play: Some(play),
    }
}

fn commands(app: &App) -> Vec<JumpEntry> {
    let command = |name: &str, what: &str, icon: Icon, action: Action| JumpEntry {
        key: format!("command:{name}"),
        kind: JumpKind::Command,
        name: name.into(),
        subtitle: what.into(),
        image: None,
        icon: Some(icon),
        primary: action,
        play: None,
    };
    let mut list = vec![
        command("Home", "Go to Home", Icon::House, Action::Open(Page::Home)),
        command(
            "Queue",
            "Show or hide the queue",
            Icon::ListMusic,
            Action::ToggleQueuePanel,
        ),
        command(
            "Lyrics",
            "Show or hide the lyrics",
            Icon::Mic,
            Action::ToggleLyricsPanel,
        ),
        command(
            "Shuffle",
            if app.now_playing().is_some_and(|now| now.shuffle) {
                "Turn shuffle off"
            } else {
                "Turn shuffle on"
            },
            Icon::Shuffle,
            Action::ToggleShuffle,
        ),
        command("Repeat", "Cycle repeat", Icon::Repeat, Action::CycleRepeat),
        command(
            "Mini player",
            "Open or close the Winamp window",
            Icon::SquarePlay,
            Action::ToggleWinampWindow,
        ),
        command(
            "Refresh",
            "Reload the current page",
            Icon::Refresh,
            Action::Reload(app.page().clone()),
        ),
        command(
            "Your listening",
            "The songs, artists and albums you played most",
            Icon::TrendingUp,
            Action::Open(Page::Stats),
        ),
        command(
            "Settings",
            "Open Settings",
            Icon::Settings,
            Action::Open(Page::Settings),
        ),
        command(
            "Keyboard shortcuts",
            "Every key the app answers to",
            Icon::Info,
            Action::ShowDialog(Dialog::Shortcuts),
        ),
    ];
    if app.now_playing().is_some() {
        list.insert(
            0,
            command(
                "Play or pause",
                "Toggle playback",
                Icon::PlayFilled,
                Action::TogglePlay,
            ),
        );
    }
    list
}

/// The entries that match `query`, best first. An empty query lists what
/// was played most recently, then the commands.
pub fn rank(entries: &[JumpEntry], query: &str) -> Vec<usize> {
    if query.is_empty() {
        let mut recent: Vec<(usize, usize)> = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            match entry.kind {
                JumpKind::Command => recent.push((1_000 + index, index)),
                JumpKind::Song => {}
                _ => recent.push((index, index)),
            }
        }
        recent.sort();
        return recent.into_iter().map(|(_, index)| index).collect();
    }
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut scored: Vec<(i32, u8, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            // Every word has to be found in the name or the subtitle; a
            // match in the name counts for more.
            let mut total = 0;
            for word in &words {
                let name = score(word, &entry.name);
                let subtitle = score(word, &entry.subtitle).map(|s| s / 2);
                total += match (name, subtitle) {
                    (None, None) => return None,
                    (name, subtitle) => name.unwrap_or(0).max(subtitle.unwrap_or(0)),
                };
            }
            Some((total, entry.kind.rank(), index))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    scored.into_iter().map(|(_, _, index)| index).collect()
}

/// How well `word` matches `text`: a whole match best, then a prefix, a
/// word starting with it, anything containing it, and last the letters
/// in order with gaps between them. `None` is no match at all.
pub fn score(word: &str, text: &str) -> Option<i32> {
    let text = text.to_lowercase();
    if text == word {
        return Some(100);
    }
    if text.starts_with(word) {
        return Some(90 - (text.len() - word.len()).min(20) as i32);
    }
    if text
        .split(|c: char| !c.is_alphanumeric())
        .any(|part| part.starts_with(word))
    {
        return Some(70);
    }
    if text.contains(word) {
        return Some(50);
    }
    if word.chars().count() < 3 {
        return None;
    }
    let mut chars = text.chars();
    let mut gaps = 0;
    for needle in word.chars() {
        let mut gap = 0;
        loop {
            let next = chars.next()?;
            if next == needle {
                break;
            }
            gap += 1;
        }
        gaps += gap;
    }
    Some(30 - gaps.min(25))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: JumpKind, name: &str, subtitle: &str) -> JumpEntry {
        JumpEntry {
            key: format!("{name}:{subtitle}"),
            kind,
            name: name.into(),
            subtitle: subtitle.into(),
            image: None,
            icon: None,
            primary: Action::Open(Page::Home),
            play: None,
        }
    }

    /// Whole and prefix matches come before matches inside a word, and the
    /// letters in order with gaps come last; two letters never match by
    /// scattered letters, which would light up half the library.
    #[test]
    fn scores_rank_whole_prefix_word_and_scattered_matches() {
        assert_eq!(score("night", "Night"), Some(100));
        assert!(score("night", "Night drive").unwrap() > score("night", "Late night").unwrap());
        assert!(score("night", "Late night").unwrap() > score("night", "Midnights").unwrap());
        assert!(score("night", "Midnights").unwrap() > score("nght", "Night drive").unwrap());
        assert_eq!(score("zz", "Jazz"), Some(50));
        assert_eq!(score("zq", "Jazz quartet"), None);
        assert_eq!(score("xyz", "Night"), None);
    }

    /// Every word has to match, the best match leads, and with nothing
    /// typed the list is what can be opened, then the commands, no songs.
    #[test]
    fn ranking_needs_every_word_and_leads_with_the_best_match() {
        let entries = vec![
            entry(JumpKind::Song, "Night Owl", "Bonobo • Liked Songs"),
            entry(JumpKind::Playlist, "Late night focus", "By Carmine"),
            entry(JumpKind::Album, "Nightfall", "Khruangbin"),
            entry(JumpKind::Command, "Settings", "Open Settings"),
        ];
        let names = |query: &str| -> Vec<&str> {
            rank(&entries, query)
                .into_iter()
                .map(|index| entries[index].name.as_str())
                .collect()
        };
        assert_eq!(
            names("night"),
            vec!["Night Owl", "Nightfall", "Late night focus"]
        );
        assert_eq!(names("night focus"), vec!["Late night focus"]);
        assert_eq!(names("bonobo"), vec!["Night Owl"]);
        assert_eq!(names("sett"), vec!["Settings"]);
        assert_eq!(names(""), vec!["Late night focus", "Nightfall", "Settings"]);
        assert!(names("zzzz").is_empty());
    }
}
