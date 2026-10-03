//! The Home page.

use egui::{Align, CornerRadius, Layout, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::{PlayableItem, Playlist, pick_image};
use crate::app::App;
use crate::model::{Action, DISCOVER_TERMS, HomeFilter, Loadable, Page, RowContext};
use crate::theme::{self, Icon};

use super::widgets::{self, TrackRow};

/// The chips that choose what Home shows. Drawn above the page's scroll
/// area, so they stay in place while the shelves scroll under them.
pub fn filter_row(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(widgets::PAGE_PADDING);
        ui.spacing_mut().item_spacing.x = 8.0;
        for filter in HomeFilter::ALL {
            if theme::soft_button(
                ui,
                &palette,
                None,
                filter.label(),
                app.home.filter == filter,
            )
            .clicked()
            {
                app.actions.push(Action::SetHomeFilter(filter));
            }
        }
    });
    ui.add_space(2.0);
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    match app.home.filter {
        HomeFilter::All => {
            ui.add_space(4.0);
            quick_access(app, ui);
            ui.add_space(16.0);
            made_for_you(app, ui);
            recently_played(app, ui);
            top_artists(app, ui);
            your_podcasts(app, ui, false);
            top_tracks(app, ui);
            recommendations(app, ui);
        }
        HomeFilter::Music => {
            ui.add_space(4.0);
            quick_access(app, ui);
            ui.add_space(16.0);
            made_for_you(app, ui);
            recently_played(app, ui);
            top_artists(app, ui);
            top_tracks(app, ui);
            recommendations(app, ui);
        }
        HomeFilter::Podcasts => {
            ui.add_space(4.0);
            your_podcasts(app, ui, true);
            saved_episodes(app, ui);
        }
    }
}

struct Tile {
    image: Option<String>,
    name: String,
    page: Page,
    uri: Option<String>,
    liked: bool,
}

/// The grid of shortcuts at the top: Liked Songs and the playlists played
/// most recently, two rows of them on a wide page.
fn quick_access(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let mut tiles: Vec<Tile> = vec![Tile {
        image: None,
        name: "Liked Songs".to_string(),
        page: Page::LikedSongs,
        uri: app
            .user
            .as_ref()
            .map(|user| format!("spotify:user:{}:collection", user.id)),
        liked: true,
    }];
    if let Some(playlists) = app.library.playlists.get() {
        let rank = |uri: &str| {
            app.recent_contexts
                .iter()
                .position(|held| held == uri)
                .unwrap_or(usize::MAX)
        };
        let mut ordered: Vec<_> = playlists.iter().enumerate().collect();
        ordered.sort_by_key(|(index, playlist)| (rank(&playlist.uri), *index));
        for (_, playlist) in ordered.into_iter().take(7) {
            tiles.push(Tile {
                image: pick_image(&playlist.images, 64).map(str::to_string),
                name: playlist.name.clone(),
                page: Page::Playlist(playlist.id.clone()),
                uri: Some(playlist.uri.clone()),
                liked: false,
            });
        }
    }
    let available = ui.available_width();
    let columns = ((available / 300.0).floor() as usize).clamp(2, 4);
    let gap = 10.0;
    let tile_height = 64.0;
    let tile_width = (available - gap * (columns as f32 - 1.0)) / columns as f32;
    let rows = tiles.len().div_ceil(columns);
    for row in 0..rows {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for column in 0..columns {
                let Some(Tile {
                    image,
                    name,
                    page,
                    uri,
                    liked,
                }) = tiles.get(row * columns + column)
                else {
                    break;
                };
                let (rect, response) =
                    ui.allocate_exact_size(vec2(tile_width, tile_height), Sense::click());
                if ui.is_rect_visible(rect) {
                    let hovered = ui.rect_contains_pointer(rect);
                    let fill = if hovered {
                        palette.surface_hover
                    } else {
                        palette.surface
                    };
                    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
                    let cover = Rect::from_min_size(rect.min, Vec2::splat(tile_height));
                    if *liked {
                        super::sidebar::liked_cover(ui, cover, 6.0);
                    } else {
                        widgets::paint_cover(
                            ui,
                            &palette,
                            image.as_deref(),
                            cover,
                            6.0,
                            Icon::Music,
                        );
                    }
                    let play_room = if hovered && uri.is_some() { 52.0 } else { 12.0 };
                    let text_rect = Rect::from_min_max(
                        pos2(cover.right() + 12.0, rect.top()),
                        pos2(rect.right() - play_room, rect.bottom()),
                    );
                    let display = crate::bidi::display_text(name);
                    let is_rtl = crate::bidi::is_rtl(name);
                    let (pos, anchor) = if is_rtl {
                        (
                            pos2(text_rect.right(), rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                        )
                    } else {
                        (
                            pos2(text_rect.left(), rect.center().y),
                            egui::Align2::LEFT_CENTER,
                        )
                    };
                    ui.painter().with_clip_rect(text_rect).text(
                        pos,
                        anchor,
                        display.into_owned(),
                        theme::bold(14.5),
                        palette.text,
                    );
                    if hovered && let Some(uri) = uri {
                        let button = Rect::from_center_size(
                            pos2(rect.right() - 28.0, rect.center().y),
                            Vec2::splat(40.0),
                        );
                        let mut child =
                            ui.new_child(egui::UiBuilder::new().max_rect(button).layout(
                                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            ));
                        if theme::circle_button(
                            &mut child,
                            Icon::PlayFilled,
                            40.0,
                            palette.accent,
                            palette.accent_hover,
                            palette.on_accent,
                            "Play",
                        )
                        .clicked()
                        {
                            app.actions.push(Action::PlayContext {
                                uri: uri.clone(),
                                offset_uri: None,
                                offset_index: None,
                            });
                        }
                    }
                }
                let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                if let Some(uri) = uri {
                    egui::Popup::context_menu(&response)
                        .frame(widgets::menu_frame(&palette))
                        .show(|ui| {
                            widgets::context_menu_items(ui, app, uri, name, None);
                        });
                }
                if response.clicked() {
                    app.actions.push(Action::Open(page.clone()));
                }
            }
        });
    }
}

fn made_for_you(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let mut playlists: Vec<Playlist> = Vec::new();
    let mut loading = false;
    let mut failed = false;
    for term in DISCOVER_TERMS {
        match app.home.discover.get(*term) {
            Some(Loadable::Loaded(list)) => {
                for playlist in list {
                    let duplicate = playlists.iter().any(|existing| {
                        existing.id == playlist.id
                            || existing.name.eq_ignore_ascii_case(&playlist.name)
                    });
                    if !duplicate {
                        playlists.push(playlist.clone());
                    }
                }
            }
            Some(Loadable::Loading) => loading = true,
            Some(Loadable::Failed(_)) => failed = true,
            _ => {}
        }
    }
    if playlists.is_empty() && !loading && !failed {
        return;
    }
    widgets::shelf(ui, &palette, "made-for-you", "Made for you", |ui| {
        if playlists.is_empty() && loading {
            widgets::loading_row(ui, &palette);
        } else if playlists.is_empty() && failed {
            widgets::error_row(ui, app, "Couldn't load this shelf", Some(Page::Home));
        }
        for playlist in &playlists {
            let subtitle = playlist
                .description
                .as_deref()
                .map(crate::util::strip_html)
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| format!("By {}", playlist.owner_name()));
            let card = widgets::card(
                ui,
                app,
                pick_image(&playlist.images, 300),
                &playlist.name,
                &subtitle,
                false,
                true,
            );
            widgets::card_context_menu(app, &card, &playlist.uri, &playlist.name);
            if card.play {
                app.actions.push(Action::PlayContext {
                    uri: playlist.uri.clone(),
                    offset_uri: None,
                    offset_index: None,
                });
            }
            if card.clicked {
                app.actions
                    .push(Action::Open(Page::Playlist(playlist.id.clone())));
            }
        }
    });
}

fn recently_played(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let history = match app.home.recently_played.clone() {
        Loadable::Loaded(history) => history,
        Loadable::Loading | Loadable::NotLoaded => {
            widgets::shelf(ui, &palette, "recent", "Recently played", |ui| {
                widgets::loading_row(ui, &palette)
            });
            return;
        }
        Loadable::Failed(message) => {
            widgets::shelf(ui, &palette, "recent", "Recently played", |ui| {
                widgets::error_row(ui, app, &message, Some(Page::Home));
            });
            return;
        }
    };
    let mut seen = std::collections::HashSet::new();
    let tracks: Vec<_> = history
        .into_iter()
        .filter(|entry| {
            entry
                .track
                .id
                .as_ref()
                .is_some_and(|id| seen.insert(id.clone()))
        })
        .take(16)
        .collect();
    if tracks.is_empty() {
        return;
    }
    widgets::shelf(ui, &palette, "recent", "Recently played", |ui| {
        for entry in &tracks {
            let track = &entry.track;
            let card = widgets::card(
                ui,
                app,
                track.image(300),
                &track.name,
                &track.artist_names(),
                false,
                true,
            );
            widgets::card_context_menu(app, &card, &track.uri, &track.name);
            if card.play {
                app.remember_track(track);
                app.actions.push(Action::PlayUris {
                    uris: vec![track.uri.clone()],
                    index: 0,
                });
            }
            if card.clicked
                && let Some(album) = &track.album
                && !album.id.is_empty()
            {
                app.actions
                    .push(Action::Open(Page::Album(album.id.clone())));
            }
        }
    });
}

fn top_artists(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let artists = match app.home.top_artists.clone() {
        Loadable::Loaded(artists) => artists,
        Loadable::Loading | Loadable::NotLoaded => {
            widgets::shelf(ui, &palette, "top-artists", "Your top artists", |ui| {
                widgets::loading_row(ui, &palette)
            });
            return;
        }
        Loadable::Failed(message) => {
            widgets::shelf(ui, &palette, "top-artists", "Your top artists", |ui| {
                widgets::error_row(ui, app, &message, Some(Page::Home));
            });
            return;
        }
    };
    if artists.is_empty() {
        return;
    }
    widgets::shelf(ui, &palette, "top-artists", "Your top artists", |ui| {
        for artist in &artists {
            let card = widgets::card(
                ui,
                app,
                pick_image(&artist.images, 300),
                &artist.name,
                "Artist",
                true,
                true,
            );
            widgets::card_context_menu(app, &card, &artist.uri, &artist.name);
            if card.play {
                app.actions.push(Action::PlayContext {
                    uri: artist.uri.clone(),
                    offset_uri: None,
                    offset_index: None,
                });
            }
            if card.clicked {
                app.actions
                    .push(Action::Open(Page::Artist(artist.id.clone())));
            }
        }
    });
}

/// The podcasts in the library as a shelf. Under the Podcasts chip the
/// shelf asks for them; under All it shows them only once they are here,
/// so opening Home asks Spotify for nothing extra.
fn your_podcasts(app: &mut App, ui: &mut egui::Ui, load: bool) {
    let palette = app.palette;
    let shows = &app.library.shows;
    if load && !shows.loaded_once && !shows.loading {
        app.actions.push(Action::LoadMore(Page::Podcasts));
    }
    let loading = shows.loading && shows.items.is_empty();
    let error = shows.error.clone();
    let shows: Vec<_> = shows.items.iter().map(|saved| saved.show.clone()).collect();
    if shows.is_empty() && !load {
        return;
    }
    if shows.is_empty() && !loading && error.is_none() {
        if app.library.shows.loaded_once {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Mic,
                "No podcasts yet",
                "Podcasts you follow will appear here.",
            );
        }
        return;
    }
    let mut show_all = false;
    widgets::shelf_with_all(
        ui,
        &palette,
        "home-podcasts",
        "Your podcasts",
        &mut show_all,
        |ui| {
            if loading {
                widgets::loading_row(ui, &palette);
            }
            if let Some(error) = &error {
                widgets::error_row(ui, app, error, Some(Page::Podcasts));
            }
            for show in &shows {
                let card = widgets::card(
                    ui,
                    app,
                    pick_image(&show.images, 300),
                    &show.name,
                    &show.publisher,
                    false,
                    false,
                );
                widgets::card_context_menu(app, &card, &show.uri, &show.name);
                if card.clicked {
                    app.actions.push(Action::Open(Page::Show(show.id.clone())));
                }
            }
        },
    );
    if show_all {
        app.actions.push(Action::Open(Page::Podcasts));
    }
}

/// The saved episodes, a handful of them, with the page for the rest.
fn saved_episodes(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let list = &app.library.episodes;
    if !list.loaded_once && !list.loading {
        app.actions.push(Action::LoadMore(Page::Episodes));
    }
    let loading = list.loading && list.items.is_empty();
    let error = list.error.clone();
    let episodes: Vec<_> = list
        .items
        .iter()
        .take(6)
        .map(|saved| saved.episode.clone())
        .collect();
    let more = list.items.len() > episodes.len() || list.can_load_more();
    if episodes.is_empty() && !loading && error.is_none() {
        return;
    }
    ui.add_space(8.0);
    let mut show_all = false;
    section_heading(ui, &palette, "Saved episodes", more, &mut show_all);
    if show_all {
        app.actions.push(Action::Open(Page::Episodes));
    }
    ui.add_space(4.0);
    if loading {
        widgets::loading_row(ui, &palette);
    }
    if let Some(error) = &error {
        widgets::error_row(ui, app, error, Some(Page::Episodes));
    }
    for episode in &episodes {
        super::show::episode_row(app, ui, episode, None);
    }
    ui.add_space(12.0);
}

/// A section title with, when there is somewhere to go, a Show all link
/// at the right edge.
fn section_heading(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    title: &str,
    link: bool,
    show_all: &mut bool,
) {
    ui.horizontal(|ui| {
        theme::section_title(ui, palette, title);
        if link {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::link(ui, "Show all", theme::semibold(13.0), palette.secondary).clicked() {
                    *show_all = true;
                }
            });
        }
    });
}

fn track_list(
    app: &mut App,
    ui: &mut egui::Ui,
    title: &str,
    tracks: Loadable<Vec<crate::api::models::Track>>,
    limit: usize,
    title_page: Option<Page>,
    more_label: Option<&str>,
) {
    let palette = app.palette;
    let tracks = match tracks {
        Loadable::Loaded(tracks) => tracks,
        Loadable::Loading | Loadable::NotLoaded => {
            let mut show_all = false;
            section_heading(ui, &palette, title, title_page.is_some(), &mut show_all);
            if show_all && let Some(page) = title_page {
                app.actions.push(Action::Open(page));
            }
            widgets::loading_row(ui, &palette);
            ui.add_space(12.0);
            return;
        }
        Loadable::Failed(message) => {
            theme::section_title(ui, &palette, title);
            widgets::error_row(ui, app, &message, Some(title_page.unwrap_or(Page::Home)));
            ui.add_space(12.0);
            return;
        }
    };
    if tracks.is_empty() {
        return;
    }
    let mut show_all = false;
    section_heading(ui, &palette, title, title_page.is_some(), &mut show_all);
    if show_all && let Some(page) = title_page.clone() {
        app.actions.push(Action::Open(page));
    }
    ui.add_space(4.0);
    let uris: Vec<String> = tracks.iter().map(|track| track.uri.clone()).collect();
    let context = RowContext::Uris(uris);
    let items: Vec<PlayableItem> = tracks.into_iter().map(PlayableItem::Track).collect();
    for (index, item) in items.iter().take(limit).enumerate() {
        widgets::track_row(
            ui,
            app,
            TrackRow {
                index,
                number: None,
                item,
                context: &context,
                show_cover: true,
                show_album: true,
                added_at: None,
                added_by: None,
                show_added_by: false,
                compact: app.settings.compact_tracks,
                shift: 0.0,
            },
        );
    }
    if let Some(label) = more_label
        && items.len() > limit
        && theme::link(ui, label, theme::semibold(14.0), palette.secondary).clicked()
    {
        app.actions.push(Action::Open(Page::TopSongs));
    }
    ui.add_space(16.0);
}

fn top_tracks(app: &mut App, ui: &mut egui::Ui) {
    let tracks = app.home.top_tracks.clone();
    track_list(
        app,
        ui,
        "Your top songs",
        tracks,
        10,
        Some(Page::TopSongs),
        Some("Show more top songs"),
    );
}

fn recommendations(app: &mut App, ui: &mut egui::Ui) {
    let tracks = app.home.recommendations.clone();
    track_list(app, ui, "Recommended for you", tracks, 20, None, None);
}
