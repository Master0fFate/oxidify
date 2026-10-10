//! The left panel: Your Library.
//!
//! Open, it is the library the official client shows: a header with the
//! Create button, chips for what to list, a search and a sort control, and
//! the rows. Folded, it is a rail of covers that still opens, plays, and
//! takes drops.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::pick_image;
use crate::app::App;
use crate::model::{Action, Dialog, DragEntry, DragTrack, Loadable, Page};
use crate::theme::{self, Icon, Palette};

const ROW_HEIGHT: f32 = 60.0;
/// A rail row: a 48px cover with a little air around it.
const RAIL_ROW_HEIGHT: f32 = 56.0;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Filter {
    #[default]
    Playlists,
    Artists,
    Albums,
    Podcasts,
}

struct Entry {
    image: Option<String>,
    name: String,
    subtitle: String,
    page: Page,
    uri: String,
    round: bool,
    liked: bool,
    owned: bool,
    playlist_index: Option<usize>,
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let collapsed = app.settings.sidebar_collapsed;
    let panel = if collapsed {
        egui::Panel::left("sidebar-rail")
            .resizable(false)
            .exact_size(theme::RAIL_WIDTH + theme::PANEL_GAP)
    } else {
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(app.settings.sidebar_width)
            .size_range(240.0..=480.0)
    };
    let panel = panel
        .show_separator_line(false)
        .frame(super::panel_frame(&palette, super::PanelSide::Left));
    let response = panel.show(ui, |ui| {
        super::panel_card(ui, &palette);
        let margin = if collapsed {
            Margin {
                left: 8,
                right: 8,
                top: 10,
                bottom: 8,
            }
        } else {
            Margin {
                left: 12,
                right: 8,
                top: 10,
                bottom: 8,
            }
        };
        Frame::new().inner_margin(margin).show(ui, |ui| {
            if collapsed {
                rail_header(app, ui);
            } else {
                header(app, ui);
            }
            list(app, ui, collapsed);
        });
    });
    let width = response.response.rect.width();
    if !collapsed && (width - app.settings.sidebar_width).abs() > 1.0 {
        app.settings.sidebar_width = width;
        app.actions.push(Action::SettingsChanged);
    }
}

fn filter_id() -> egui::Id {
    egui::Id::new("sidebar-filter")
}

fn current_filter(ui: &egui::Ui) -> Filter {
    ui.data(|data| data.get_temp::<Filter>(filter_id()))
        .unwrap_or_default()
}

/// The library's title, which folds the panel down when clicked: the icon
/// turns into an arrow on hover to say so.
fn library_title(ui: &mut egui::Ui, palette: &Palette) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap("Your Library".to_string(), theme::bold(15.0), palette.text);
    let size = vec2(26.0 + 6.0 + galley.size().x, 32.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let color = if hovered {
            palette.text
        } else {
            palette.secondary
        };
        let icon = if hovered {
            Icon::ArrowLeft
        } else {
            Icon::Library
        };
        let icon_rect =
            Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), Vec2::splat(22.0));
        icon.image(color, 22.0).paint_at(ui, icon_rect);
        ui.painter().galley(
            pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0),
            galley,
            palette.text,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Collapse Your Library")
    });
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Collapse Your Library")
}

fn create_playlist(app: &mut App) {
    app.actions.push(Action::ShowDialog(Dialog::CreatePlaylist {
        name: String::new(),
        public: false,
        add_uris: Vec::new(),
    }));
}

fn header(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let mut filter = current_filter(ui);
    let show_search_id = egui::Id::new("sidebar-show-search");
    let mut show_search = ui
        .data(|data| data.get_temp::<bool>(show_search_id))
        .unwrap_or(false);

    ui.horizontal(|ui| {
        ui.add_space(2.0);
        if library_title(ui, &palette).clicked() {
            app.actions.push(Action::ToggleSidebarCollapsed);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(2.0);
            // Creating is the one thing to make here, so the button says so
            // and does it, no menu in between.
            if theme::soft_button(ui, &palette, Some(Icon::Plus), "Create", false)
                .on_hover_text("Create a playlist")
                .clicked()
            {
                create_playlist(app);
            }
        });
    });
    ui.add_space(8.0);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (value, label) in [
            (Filter::Playlists, "Playlists"),
            (Filter::Artists, "Artists"),
            (Filter::Albums, "Albums"),
            (Filter::Podcasts, "Podcasts"),
        ] {
            if theme::soft_button(ui, &palette, None, label, filter == value).clicked() {
                filter = value;
            }
        }
    });
    ui.add_space(6.0);

    // Search on the left, the order on the right, as the official client
    // arranges them.
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        if theme::icon_button(
            ui,
            Icon::Search,
            16.0,
            palette.secondary,
            palette.text,
            "Search in Your Library",
        )
        .clicked()
        {
            show_search = !show_search;
            if show_search {
                ui.memory_mut(|memory| memory.request_focus(egui::Id::new("sidebar-search")));
            } else {
                app.library.filter.clear();
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(4.0);
            order_menu(app, ui, filter);
        });
    });
    ui.data_mut(|data| {
        data.insert_temp(filter_id(), filter);
        data.insert_temp(show_search_id, show_search);
    });
    if show_search {
        ui.add_space(2.0);
        super::widgets::search_field(
            ui,
            &palette,
            egui::Id::new("sidebar-search"),
            &mut app.library.filter,
            "Search in Your Library",
            ui.available_width() - 4.0,
        );
    }
    ui.add_space(4.0);
}

/// The sort and view control: names the order the shelf is in and offers
/// the other, and switches the rows between the two densities.
fn order_menu(app: &mut App, ui: &mut egui::Ui, filter: Filter) {
    let palette = app.palette;
    let custom = filter == Filter::Playlists && !app.settings.sidebar_order.is_empty();
    let label = if custom { "Custom order" } else { "Recents" };
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), theme::medium(13.0), palette.secondary);
    let size = vec2(galley.size().x + 22.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let color = if response.hovered() {
            palette.text
        } else {
            palette.secondary
        };
        ui.painter().galley(
            pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
        let icon_rect =
            Rect::from_center_size(pos2(rect.right() - 9.0, rect.center().y), Vec2::splat(16.0));
        Icon::ListMusic.image(color, 16.0).paint_at(ui, icon_rect);
    }
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Sort and view");
    egui::Popup::menu(&response)
        .frame(super::widgets::menu_frame(&palette))
        .show(|ui| {
            ui.set_width(200.0);
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                theme::text(ui, "Sort by", theme::semibold(12.5), palette.secondary);
            });
            let recents = super::widgets::menu_item(
                ui,
                &palette,
                Some(if custom { None } else { Some(Icon::Check) }.unwrap_or(Icon::Clock)),
                "Recently played",
            );
            if recents && custom {
                // Dragging a row brings the listener's own order back, so
                // this asks no confirmation.
                app.settings.sidebar_order.clear();
                app.mark_settings_dirty();
            }
            if filter == Filter::Playlists {
                super::widgets::menu_item_enabled(
                    ui,
                    &palette,
                    Some(if custom {
                        Icon::Check
                    } else {
                        Icon::GripVertical
                    }),
                    "Custom order (drag rows)",
                    false,
                );
            }
            super::widgets::menu_separator(ui, &palette);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                theme::text(ui, "View as", theme::semibold(12.5), palette.secondary);
            });
            let grid = app.settings.sidebar_grid;
            let compact = app.settings.compact_library && !grid;
            let list = !grid && !compact;
            let mark = |on: bool, icon: Icon| if on { Icon::Check } else { icon };
            if super::widgets::menu_item(
                ui,
                &palette,
                Some(mark(compact, Icon::ListMusic)),
                "Compact",
            ) && !compact
            {
                app.settings.compact_library = true;
                app.settings.sidebar_grid = false;
                app.mark_settings_dirty();
            }
            if super::widgets::menu_item(ui, &palette, Some(mark(list, Icon::ListVideo)), "List")
                && !list
            {
                app.settings.compact_library = false;
                app.settings.sidebar_grid = false;
                app.mark_settings_dirty();
            }
            if super::widgets::menu_item(ui, &palette, Some(mark(grid, Icon::LayoutGrid)), "Grid")
                && !grid
            {
                app.settings.sidebar_grid = true;
                app.mark_settings_dirty();
            }
        });
}

/// The folded library's header: the icon that opens it out, and Create.
fn rail_header(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let expand = theme::icon_button(
            ui,
            Icon::Library,
            22.0,
            palette.secondary,
            palette.text,
            "Expand Your Library",
        );
        if expand.clicked() {
            app.actions.push(Action::ToggleSidebarCollapsed);
        }
        if theme::icon_button(
            ui,
            Icon::Plus,
            20.0,
            palette.secondary,
            palette.text,
            "Create a playlist",
        )
        .clicked()
        {
            create_playlist(app);
        }
    });
    ui.add_space(6.0);
}

fn list(app: &mut App, ui: &mut egui::Ui, collapsed: bool) {
    let palette = app.palette;
    let filter = current_filter(ui);

    // Make sure the selected shelf is loading.
    match filter {
        Filter::Playlists => {}
        Filter::Albums => {
            if !app.library.albums.loaded_once && !app.library.albums.loading {
                app.actions.push(Action::LoadMore(Page::Albums));
            }
        }
        Filter::Artists => {
            if !app.library.artists.loaded_once && !app.library.artists.loading {
                app.actions.push(Action::LoadMore(Page::Artists));
            }
        }
        Filter::Podcasts => {
            if !app.library.shows.loaded_once && !app.library.shows.loading {
                app.actions.push(Action::LoadMore(Page::Podcasts));
            }
        }
    }

    let needle = if collapsed {
        String::new()
    } else {
        app.library.filter.trim().to_lowercase()
    };
    let user_id = app.user_id().unwrap_or("").to_string();
    let mut entries: Vec<Entry> = Vec::new();
    let mut loading = false;
    let mut error: Option<String> = None;
    let mut more_page: Option<Page> = None;
    match filter {
        Filter::Playlists => {
            if needle.is_empty() || "liked songs".contains(&needle) {
                entries.push(Entry {
                    image: None,
                    name: "Liked Songs".into(),
                    subtitle: match app.library.liked.total {
                        Some(total) => format!("Playlist • {total} songs"),
                        None => "Playlist".into(),
                    },
                    page: Page::LikedSongs,
                    uri: String::new(),
                    round: false,
                    liked: true,
                    owned: false,
                    playlist_index: None,
                });
            }
            match &app.library.playlists {
                Loadable::Loaded(playlists) => {
                    // Recently played first, the way Spotify orders its own
                    // sidebar; the rest keep the library's order. The ranks
                    // are looked up once each: searching the recents for
                    // every comparison made a large library slow to scroll.
                    let recent = rank_of(&app.recent_contexts);
                    let rank = |uri: &str| recent.get(uri).copied().unwrap_or(usize::MAX);
                    let mut ordered: Vec<_> = playlists.iter().enumerate().collect();
                    ordered.sort_by_cached_key(|(index, playlist)| (rank(&playlist.uri), *index));
                    for (index, playlist) in ordered {
                        if !needle.is_empty() && !playlist.name.to_lowercase().contains(&needle) {
                            continue;
                        }
                        let owned = playlist.owned_by(&user_id);
                        entries.push(Entry {
                            image: pick_image(&playlist.images, 64).map(str::to_string),
                            name: playlist.name.clone(),
                            subtitle: format!("Playlist • {}", playlist.owner_name()),
                            page: Page::Playlist(playlist.id.clone()),
                            uri: playlist.uri.clone(),
                            round: false,
                            liked: false,
                            owned,
                            playlist_index: Some(index),
                        });
                    }
                }
                Loadable::Loading | Loadable::NotLoaded => loading = true,
                Loadable::Failed(message) => error = Some(message.clone()),
            }
        }
        Filter::Albums => {
            for saved in &app.library.albums.items {
                let album = &saved.album;
                if !needle.is_empty()
                    && !album.name.to_lowercase().contains(&needle)
                    && !album
                        .artists
                        .iter()
                        .any(|a| a.name.to_lowercase().contains(&needle))
                {
                    continue;
                }
                entries.push(Entry {
                    image: pick_image(&album.images, 64).map(str::to_string),
                    name: album.name.clone(),
                    subtitle: format!(
                        "{} • {}",
                        album.kind_label(),
                        crate::api::models::join_names(
                            album.artists.iter().map(|a| a.name.as_str())
                        )
                    ),
                    page: Page::Album(album.id.clone()),
                    uri: album.uri.clone(),
                    round: false,
                    liked: false,
                    owned: false,
                    playlist_index: None,
                });
            }
            loading = app.library.albums.loading && app.library.albums.items.is_empty();
            error = app.library.albums.error.clone();
            if app.library.albums.can_load_more() {
                more_page = Some(Page::Albums);
            }
        }
        Filter::Artists => {
            for artist in &app.library.artists.items {
                if !needle.is_empty() && !artist.name.to_lowercase().contains(&needle) {
                    continue;
                }
                entries.push(Entry {
                    image: pick_image(&artist.images, 64).map(str::to_string),
                    name: artist.name.clone(),
                    subtitle: "Artist".into(),
                    page: Page::Artist(artist.id.clone()),
                    uri: artist.uri.clone(),
                    round: true,
                    liked: false,
                    owned: false,
                    playlist_index: None,
                });
            }
            loading = app.library.artists.loading && app.library.artists.items.is_empty();
            error = app.library.artists.error.clone();
            if app.library.artists.can_load_more() {
                more_page = Some(Page::Artists);
            }
        }
        Filter::Podcasts => {
            for saved in &app.library.shows.items {
                let show = &saved.show;
                if !needle.is_empty() && !show.name.to_lowercase().contains(&needle) {
                    continue;
                }
                entries.push(Entry {
                    image: pick_image(&show.images, 64).map(str::to_string),
                    name: show.name.clone(),
                    subtitle: format!("Podcast • {}", show.publisher),
                    page: Page::Show(show.id.clone()),
                    uri: show.uri.clone(),
                    round: false,
                    liked: false,
                    owned: false,
                    playlist_index: None,
                });
            }
            loading = app.library.shows.loading && app.library.shows.items.is_empty();
            error = app.library.shows.error.clone();
            if app.library.shows.can_load_more() {
                more_page = Some(Page::Podcasts);
            }
        }
    }

    // Pinned entries sit on top, in the order they were pinned; Liked
    // Songs stays above them, and everyone else keeps their order. Once
    // the playlists shelf has an order of its own, that order wins there:
    // rows sit where they were dropped, and playlists the saved order has
    // not met yet, the newly created and followed, wait at the top.
    let pinned = rank_of(&app.settings.pinned_contexts);
    let pin_rank = |uri: &str| pinned.get(uri).copied().unwrap_or(usize::MAX);
    let custom_order = filter == Filter::Playlists && !app.settings.sidebar_order.is_empty();
    let saved = rank_of(&app.settings.sidebar_order);
    let saved_rank = |uri: &str| saved.get(uri).copied();
    // Pins are pins, whatever orders the rest: Liked Songs, then the
    // pinned block, then everyone else by the listener's own order or,
    // failing one, by recency.
    entries.sort_by_cached_key(|entry| {
        if entry.liked {
            (0, 0)
        } else {
            match pin_rank(&entry.uri) {
                usize::MAX if custom_order => match saved_rank(&entry.uri) {
                    Some(rank) => (3, rank),
                    None => (2, entry.playlist_index.unwrap_or(0)),
                },
                usize::MAX => (2, 0),
                rank => (1, rank),
            }
        }
    });
    // The zones a dragged row can land in: everything sits below Liked
    // Songs, and the pinned entries form one block right after it.
    let liked_rows = entries.iter().take_while(|entry| entry.liked).count();
    let pinned_rows = entries
        .iter()
        .filter(|entry| !entry.liked && pin_rank(&entry.uri) != usize::MAX)
        .count();
    let playing_context = app.playing_context_uri();
    let context_playing = app.believed_playing();
    let current_page = app.page().clone();

    egui::ScrollArea::vertical()
        .id_salt("sidebar-list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if loading {
                if collapsed {
                    ui.vertical_centered(|ui| theme::spinner(ui, 18.0, palette.accent));
                } else {
                    super::widgets::loading_row(ui, &palette);
                }
            }
            if let Some(error) = &error {
                if collapsed {
                    ui.vertical_centered(|ui| {
                        theme::icon(ui, Icon::CircleAlert, 16.0, palette.danger)
                            .on_hover_text(error);
                    });
                } else {
                    super::widgets::error_row(ui, app, error, None);
                }
            }
            if entries.is_empty() && !loading && error.is_none() && !collapsed {
                ui.add_space(12.0);
                theme::subtle(
                    ui,
                    &palette,
                    if needle.is_empty() {
                        "Nothing here yet."
                    } else {
                        "No matches."
                    },
                );
            }
            // While something is in hand, find where it hangs up front:
            // neighbours shift before that row draws, so the spot cannot
            // be discovered row by row. The fixed row height makes it
            // arithmetic.
            if app.settings.sidebar_grid && !collapsed {
                grid_tiles(
                    app,
                    ui,
                    &entries,
                    playing_context.as_deref(),
                    context_playing,
                    &current_page,
                    custom_order,
                    more_page.clone(),
                );
                return;
            }
            let compact = app.settings.compact_library;
            let row_height = if collapsed {
                RAIL_ROW_HEIGHT
            } else if compact {
                44.0
            } else {
                ROW_HEIGHT
            };
            let list_top = ui.cursor().top();
            let pointer = ui
                .ctx()
                .pointer_latest_pos()
                .filter(|pos| ui.clip_rect().contains(*pos));
            // A song in hand lands on a row, when that row can take one.
            let dragging_song = egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx());
            let drop_target = dragging_song
                .then_some(pointer)
                .flatten()
                .map(|pos| ((pos.y - list_top) / row_height).floor())
                .filter(|row| *row >= 0.0 && *row < entries.len() as f32)
                .map(|row| row as usize)
                .filter(|row| entries[*row].liked || entries[*row].owned);
            // A sidebar row in hand lands between rows: the slot nearest
            // the pointer, never above Liked Songs.
            let reordering = egui::DragAndDrop::has_payload_of_type::<DragEntry>(ui.ctx());
            let reorder_slot = reordering.then_some(pointer).flatten().map(|pos| {
                (((pos.y - list_top) / row_height).round().max(0.0) as usize)
                    .clamp(liked_rows, entries.len())
            });
            super::widgets::virtual_rows(ui, entries.len(), row_height, |ui, index| {
                let entry = &entries[index];
                let droppable = entry.liked || entry.owned;
                let drop_hover = drop_target == Some(index);
                let active = entry.page == current_page;
                let playing = context_playing
                    && !entry.uri.is_empty()
                    && playing_context.as_deref() == Some(entry.uri.as_str());
                let pinned =
                    !entry.uri.is_empty() && app.settings.pinned_contexts.contains(&entry.uri);
                let (rect, response) = ui.allocate_exact_size(
                    vec2(ui.available_width(), row_height),
                    Sense::click_and_drag(),
                );
                // Past the drag threshold the row itself is in hand, to be
                // pinned into place; clicks and the context menu keep their
                // meaning. Liked Songs stays where it is.
                if !entry.liked
                    && !entry.uri.is_empty()
                    && response.drag_started_by(egui::PointerButton::Primary)
                {
                    egui::DragAndDrop::set_payload(
                        ui.ctx(),
                        DragEntry {
                            uri: entry.uri.clone(),
                            title: entry.name.clone(),
                            image: entry.image.clone(),
                        },
                    );
                }
                // Neighbours ease apart around the row that would take a
                // song, macOS style, and part at the slot a dragged row
                // would land in. Each row keeps one animated offset, which
                // also eases everything back after the drag ends.
                let shift = ui.ctx().animate_value_with_time(
                    ui.id().with(("drop-shift", index)),
                    if let Some(slot) = reorder_slot {
                        if index < slot { -4.0 } else { 4.0 }
                    } else {
                        match drop_target {
                            Some(target) if index < target => -4.0,
                            Some(target) if index > target => 4.0,
                            _ => 0.0,
                        }
                    },
                    0.12,
                );
                let rect = rect.translate(vec2(0.0, shift));
                if ui.is_rect_visible(rect) {
                    if active {
                        ui.painter()
                            .rect_filled(rect, CornerRadius::same(6), palette.surface);
                    } else if response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            palette.surface_hover.gamma_multiply(0.6),
                        );
                    }
                    if drop_hover {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            palette.accent.gamma_multiply(0.18),
                        );
                        ui.painter().rect_stroke(
                            rect,
                            CornerRadius::same(6),
                            egui::Stroke::new(1.5, palette.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let cover_size = if collapsed {
                        48.0
                    } else if compact {
                        28.0
                    } else {
                        44.0
                    };
                    let cover_center = if collapsed {
                        rect.center()
                    } else {
                        pos2(rect.left() + 8.0 + cover_size / 2.0, rect.center().y)
                    };
                    let cover_rect = Rect::from_center_size(cover_center, Vec2::splat(cover_size));
                    if entry.liked {
                        liked_cover(ui, cover_rect, 6.0);
                    } else {
                        super::widgets::paint_cover(
                            ui,
                            &palette,
                            entry.image.as_deref(),
                            cover_rect,
                            if entry.round { cover_size / 2.0 } else { 6.0 },
                            if entry.round { Icon::User } else { Icon::Music },
                        );
                    }
                    let name_color = if playing {
                        palette.accent
                    } else {
                        palette.text
                    };
                    if !collapsed {
                        let text_left = cover_rect.right() + 12.0;
                        let text_right = rect.right() - if playing || pinned { 28.0 } else { 8.0 };
                        let painter = ui.painter().with_clip_rect(Rect::from_min_max(
                            pos2(text_left, rect.top()),
                            pos2(text_right, rect.bottom()),
                        ));
                        if compact {
                            painter.text(
                                pos2(text_left, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                &entry.name,
                                theme::medium(13.5),
                                name_color,
                            );
                        } else {
                            painter.text(
                                pos2(text_left, rect.center().y - 9.0),
                                egui::Align2::LEFT_CENTER,
                                &entry.name,
                                theme::medium(14.0),
                                name_color,
                            );
                            painter.text(
                                pos2(text_left, rect.center().y + 10.0),
                                egui::Align2::LEFT_CENTER,
                                &entry.subtitle,
                                theme::regular(12.5),
                                palette.secondary,
                            );
                        }
                        if playing {
                            let icon_rect = Rect::from_center_size(
                                pos2(rect.right() - 16.0, rect.center().y),
                                Vec2::splat(16.0),
                            );
                            Icon::Volume2
                                .image(palette.accent, 16.0)
                                .paint_at(ui, icon_rect);
                        } else if pinned {
                            let icon_rect = Rect::from_center_size(
                                pos2(rect.right() - 16.0, rect.center().y),
                                Vec2::splat(13.0),
                            );
                            Icon::Pin
                                .image(palette.secondary, 13.0)
                                .paint_at(ui, icon_rect);
                        }
                    } else if playing {
                        // On the rail the speaker rides the cover's corner,
                        // on a disc of panel so it reads over any art.
                        let badge = pos2(cover_rect.right() - 6.0, cover_rect.bottom() - 6.0);
                        ui.painter().circle_filled(badge, 9.0, palette.panel);
                        Icon::Volume2
                            .image(palette.accent, 12.0)
                            .paint_at(ui, Rect::from_center_size(badge, Vec2::splat(12.0)));
                    }
                    // Hovering the art offers to play right from here.
                    let can_play = (!entry.uri.is_empty() || entry.liked) && !collapsed;
                    let play_response = can_play.then(|| {
                        ui.interact(
                            cover_rect,
                            ui.id().with(("sidebar-play", index)),
                            Sense::click(),
                        )
                    });
                    let play_hover = play_response.as_ref().is_some_and(|play| play.hovered());
                    if play_hover || (response.hovered() && can_play) {
                        ui.painter().rect_filled(
                            cover_rect,
                            CornerRadius::same(if entry.round { 22 } else { 6 }),
                            egui::Color32::from_black_alpha(120),
                        );
                        Icon::PlayFilled
                            .image(
                                if play_hover {
                                    palette.accent
                                } else {
                                    egui::Color32::WHITE
                                },
                                18.0,
                            )
                            .paint_at(
                                ui,
                                Rect::from_center_size(
                                    cover_rect.center()
                                        + theme::play_glyph_offset(Icon::PlayFilled, 18.0),
                                    Vec2::splat(18.0),
                                ),
                            );
                        if let Some(play) = &play_response {
                            play.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
                        }
                    }
                    if play_response.is_some_and(|play| play.clicked()) {
                        let uri = if entry.liked {
                            app.user
                                .as_ref()
                                .map(|user| format!("spotify:user:{}:collection", user.id))
                        } else {
                            Some(entry.uri.clone())
                        };
                        if let Some(uri) = uri {
                            app.actions.push(Action::PlayContext {
                                uri,
                                offset_uri: None,
                                offset_index: None,
                            });
                        }
                    }
                    // Rows that cannot take the song step back a little.
                    if dragging_song && !droppable {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            palette.panel.gamma_multiply(0.5),
                        );
                    }
                }
                if dragging_song
                    && droppable
                    && let Some(track) = response.dnd_release_payload::<DragTrack>()
                {
                    if entry.liked {
                        // Dropping on Liked Songs saves; a song already
                        // saved is left alone.
                        if app.is_saved(&track.uri) != Some(true) {
                            app.actions.push(Action::ToggleSaved(track.uri.clone()));
                        }
                    } else if let Page::Playlist(id) = &entry.page {
                        app.actions.push(Action::AddToPlaylist {
                            playlist_id: id.clone(),
                            playlist_name: entry.name.clone(),
                            uris: vec![track.uri.clone()],
                            position: None,
                            confirmed: false,
                        });
                    }
                }
                if response.clicked() {
                    app.actions.push(Action::Open(entry.page.clone()));
                }
                entry_menu(app, &response, entry, custom_order);
                let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                if collapsed {
                    // The rail has no words; the tooltip carries them.
                    response.on_hover_text(format!("{}\n{}", entry.name, entry.subtitle));
                }
            });
            if let Some(slot) = reorder_slot {
                // A line in the gap the rows opened, so the eye lands
                // where the row will.
                let y = list_top + slot as f32 * row_height;
                ui.painter().hline(
                    ui.max_rect().x_range().shrink(6.0),
                    y,
                    egui::Stroke::new(2.0, palette.accent),
                );
                if ui.input(|input| input.pointer.any_released())
                    && let Some(drag) = egui::DragAndDrop::take_payload::<DragEntry>(ui.ctx())
                {
                    if filter == Filter::Playlists {
                        drop_playlist_row(app, &entries, liked_rows, pinned_rows, slot, &drag.uri);
                    } else {
                        drop_row(app, &entries, liked_rows, pinned_rows, slot, &drag.uri);
                    }
                }
            }
            if let Some(page) = more_page {
                super::widgets::load_more_when_near_end(ui, app, page, true);
            }
        });
}

/// The right-click menu of a library entry: the context's own actions,
/// pinning, and the way back to the automatic order.
fn entry_menu(app: &mut App, response: &egui::Response, entry: &Entry, custom_order: bool) {
    let palette = app.palette;
    if !entry.uri.is_empty() {
        let owned_playlist = entry
            .owned
            .then_some(entry.playlist_index)
            .flatten()
            .and_then(|index| {
                app.library
                    .playlists
                    .get()
                    .and_then(|list| list.get(index))
                    .cloned()
            });
        egui::Popup::context_menu(response)
            .frame(super::widgets::menu_frame(&palette))
            .show(|ui| {
                super::widgets::context_menu_items(
                    ui,
                    app,
                    &entry.uri,
                    &entry.name,
                    owned_playlist.as_ref(),
                );
                let pinned = app.settings.pinned_contexts.contains(&entry.uri);
                if super::widgets::menu_item(
                    ui,
                    &palette,
                    Some(if pinned { Icon::PinOff } else { Icon::Pin }),
                    if pinned { "Unpin" } else { "Pin to top" },
                ) {
                    if pinned {
                        app.settings
                            .pinned_contexts
                            .retain(|held| held != &entry.uri);
                    } else {
                        app.settings.pinned_contexts.push(entry.uri.clone());
                        app.settings.sidebar_order.retain(|held| held != &entry.uri);
                    }
                    app.mark_settings_dirty();
                }
                if custom_order
                    && super::widgets::menu_item(
                        ui,
                        &palette,
                        Some(Icon::Clock),
                        "Sort by recently played",
                    )
                {
                    // Dragging a row brings the listener's own
                    // order back, so this asks no confirmation.
                    app.settings.sidebar_order.clear();
                    app.mark_settings_dirty();
                }
            });
    } else if entry.liked {
        egui::Popup::context_menu(response)
            .frame(super::widgets::menu_frame(&palette))
            .show(|ui| {
                if super::widgets::menu_item(ui, &palette, Some(Icon::Play), "Play")
                    && let Some(user) = &app.user
                {
                    app.actions.push(Action::PlayContext {
                        uri: format!("spotify:user:{}:collection", user.id),
                        offset_uri: None,
                        offset_index: None,
                    });
                }
            });
    }
}

/// Your Library as a grid of covers, the view the official client offers
/// beside the list: as many tiles across as the panel is wide, each
/// opening, taking a dropped song, and answering a right-click like its
/// row. Rows cannot be dragged into an order here; pins still work from
/// the menu.
#[allow(clippy::too_many_arguments)]
fn grid_tiles(
    app: &mut App,
    ui: &mut egui::Ui,
    entries: &[Entry],
    playing_context: Option<&str>,
    context_playing: bool,
    current_page: &Page,
    custom_order: bool,
    more_page: Option<Page>,
) {
    let palette = app.palette;
    let available = ui.available_width();
    let gap = 8.0;
    let columns = (((available + gap) / (96.0 + gap)).floor() as usize).max(2);
    let tile = ((available - gap * (columns as f32 - 1.0)) / columns as f32).max(40.0);
    let row_height = tile + 30.0;
    let rows = entries.len().div_ceil(columns);
    let dragging_song = egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx());
    super::widgets::virtual_rows(ui, rows, row_height, |ui, row| {
        let (band, _) = ui.allocate_exact_size(vec2(available, row_height), Sense::hover());
        for column in 0..columns {
            let index = row * columns + column;
            let Some(entry) = entries.get(index) else {
                break;
            };
            let left = band.left() + column as f32 * (tile + gap);
            let tile_rect =
                Rect::from_min_size(pos2(left, band.top()), vec2(tile, row_height - 6.0));
            let cover_rect = Rect::from_min_size(pos2(left, band.top()), Vec2::splat(tile));
            let response = ui.interact(
                tile_rect,
                ui.id().with(("library-tile", index)),
                Sense::click(),
            );
            let hovered = response.hovered();
            let active = entry.page == *current_page;
            let playing = context_playing
                && !entry.uri.is_empty()
                && playing_context == Some(entry.uri.as_str());
            let droppable = entry.liked || entry.owned;
            let drop_hover = dragging_song && droppable && ui.rect_contains_pointer(tile_rect);
            if ui.is_rect_visible(tile_rect) {
                if active || hovered {
                    ui.painter().rect_filled(
                        tile_rect.expand(4.0),
                        CornerRadius::same(8),
                        if active {
                            palette.surface
                        } else {
                            palette.surface_hover.gamma_multiply(0.6)
                        },
                    );
                }
                let radius = if entry.round { tile / 2.0 } else { 6.0 };
                if entry.liked {
                    liked_cover(ui, cover_rect, radius);
                } else {
                    super::widgets::paint_cover(
                        ui,
                        &palette,
                        entry.image.as_deref(),
                        cover_rect,
                        radius,
                        if entry.round { Icon::User } else { Icon::Music },
                    );
                }
                if drop_hover {
                    ui.painter().rect_stroke(
                        cover_rect,
                        CornerRadius::same(radius.min(127.0) as u8),
                        egui::Stroke::new(2.0, palette.accent),
                        egui::StrokeKind::Inside,
                    );
                }
                if dragging_song && !droppable {
                    ui.painter().rect_filled(
                        tile_rect,
                        CornerRadius::same(6),
                        palette.panel.gamma_multiply(0.5),
                    );
                }
                let name_color = if playing {
                    palette.accent
                } else {
                    palette.text
                };
                let painter = ui.painter().with_clip_rect(Rect::from_min_max(
                    pos2(tile_rect.left() + 2.0, cover_rect.bottom()),
                    tile_rect.max,
                ));
                painter.text(
                    pos2(tile_rect.left() + 2.0, cover_rect.bottom() + 14.0),
                    egui::Align2::LEFT_CENTER,
                    &entry.name,
                    theme::semibold(12.5),
                    name_color,
                );
                if playing {
                    let badge = pos2(cover_rect.right() - 12.0, cover_rect.bottom() - 12.0);
                    ui.painter().circle_filled(badge, 11.0, palette.panel);
                    Icon::Volume2
                        .image(palette.accent, 14.0)
                        .paint_at(ui, Rect::from_center_size(badge, Vec2::splat(14.0)));
                } else if !entry.uri.is_empty() && app.settings.pinned_contexts.contains(&entry.uri)
                {
                    let badge = pos2(cover_rect.right() - 11.0, cover_rect.top() + 11.0);
                    ui.painter().circle_filled(badge, 9.0, palette.panel);
                    Icon::Pin
                        .image(palette.secondary, 11.0)
                        .paint_at(ui, Rect::from_center_size(badge, Vec2::splat(11.0)));
                }
            }
            if dragging_song
                && droppable
                && let Some(track) = response.dnd_release_payload::<DragTrack>()
            {
                if entry.liked {
                    if app.is_saved(&track.uri) != Some(true) {
                        app.actions.push(Action::ToggleSaved(track.uri.clone()));
                    }
                } else if let Page::Playlist(id) = &entry.page {
                    app.actions.push(Action::AddToPlaylist {
                        playlist_id: id.clone(),
                        playlist_name: entry.name.clone(),
                        uris: vec![track.uri.clone()],
                        position: None,
                        confirmed: false,
                    });
                }
            }
            if response.clicked() {
                app.actions.push(Action::Open(entry.page.clone()));
            }
            entry_menu(app, &response, entry, custom_order);
            response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(format!("{}\n{}", entry.name, entry.subtitle));
        }
    });
    if let Some(page) = more_page {
        super::widgets::load_more_when_near_end(ui, app, page, true);
    }
}

/// A dropped playlist row lands in one of two worlds. Inside the pinned
/// block it pins, or reorders the pins, exactly where it fell. Below the
/// block it orders the rest: the first such drop snapshots the order on
/// screen so nothing jumps, and rows then sit where they are put. A
/// pinned row dropped below the block is unpinned.
fn drop_playlist_row(
    app: &mut App,
    entries: &[Entry],
    liked_rows: usize,
    pinned_rows: usize,
    slot: usize,
    uri: &str,
) {
    let section_end = liked_rows + pinned_rows;
    let was_pinned = app.settings.pinned_contexts.iter().any(|held| held == uri);
    if pinned_rows > 0 && slot < section_end {
        // Into the pinned block: the pinned entry the drop lands in front
        // of anchors the new pin position.
        let anchor = entries[liked_rows..section_end]
            .iter()
            .skip(slot.saturating_sub(liked_rows))
            .map(|entry| entry.uri.as_str())
            .find(|held| *held != uri)
            .map(str::to_string);
        let mut pinned = app.settings.pinned_contexts.clone();
        pinned.retain(|held| held != uri);
        let at = anchor
            .and_then(|anchor| pinned.iter().position(|held| *held == anchor))
            .unwrap_or(pinned.len());
        pinned.insert(at, uri.to_string());
        if pinned != app.settings.pinned_contexts {
            app.settings.pinned_contexts = pinned;
            app.settings.sidebar_order.retain(|held| held != uri);
            app.mark_settings_dirty();
        }
        return;
    }
    // Below the block: the rest of the shelf takes the listener's own
    // order, and a pinned row dropped here stops being pinned.
    if was_pinned {
        app.settings.pinned_contexts.retain(|held| held != uri);
        app.mark_settings_dirty();
        if app.settings.sidebar_order.is_empty() {
            // The automatic order stays automatic: the row returns to
            // living by recency.
            return;
        }
    }
    let mut order = full_playlist_order(app);
    let anchor = entries
        .iter()
        .skip(slot)
        .filter(|entry| !entry.liked)
        .map(|entry| entry.uri.as_str())
        .find(|held| *held != uri)
        .map(str::to_string);
    order.retain(|held| held != uri);
    let at = anchor
        .and_then(|anchor| order.iter().position(|held| *held == anchor))
        .unwrap_or(order.len());
    order.insert(at, uri.to_string());
    if order != app.settings.sidebar_order {
        app.settings.sidebar_order = order;
        app.mark_settings_dirty();
    }
}

/// Every loaded playlist in the order the shelf presents them when no
/// filter narrows the view: the saved order once one exists, with the
/// playlists it has not met yet first, otherwise the pinned block and
/// then recency. The saved order is rewritten from this, so it covers
/// the whole library rather than the rows that happened to be visible.
fn full_playlist_order(app: &App) -> Vec<String> {
    let Some(playlists) = app.library.playlists.get() else {
        return Vec::new();
    };
    let mut ordered: Vec<_> = playlists.iter().enumerate().collect();
    if app.settings.sidebar_order.is_empty() {
        let recents = rank_of(&app.recent_contexts);
        let recent = |uri: &str| recents.get(uri).copied().unwrap_or(usize::MAX);
        let pins = rank_of(&app.settings.pinned_contexts);
        let pinned = |uri: &str| pins.get(uri).copied();
        ordered.sort_by_cached_key(|(index, playlist)| match pinned(&playlist.uri) {
            Some(rank) => (0, rank, 0),
            None => (1, recent(&playlist.uri), *index),
        });
    } else {
        let saved_order = rank_of(&app.settings.sidebar_order);
        let saved = |uri: &str| saved_order.get(uri).copied();
        ordered.sort_by_cached_key(|(index, playlist)| match saved(&playlist.uri) {
            Some(rank) => (1, rank, 0),
            None => (0, *index, 0),
        });
    }
    ordered
        .into_iter()
        .map(|(_, playlist)| playlist.uri.clone())
        // Pins live in their own list; the saved order holds the rest.
        .filter(|uri| !app.settings.pinned_contexts.contains(uri))
        .collect()
}

/// Each uri's place in an ordered list, for lookups while sorting: the
/// first occurrence counts, as `position` did.
fn rank_of(order: &[String]) -> std::collections::HashMap<&str, usize> {
    let mut ranks = std::collections::HashMap::with_capacity(order.len());
    for (index, uri) in order.iter().enumerate() {
        ranks.entry(uri.as_str()).or_insert(index);
    }
    ranks
}

/// A dropped album, artist, or podcast row lands in the pinned block:
/// within the block the drop position is its new pin order, and below the
/// block the row goes back to living by recency, so it simply stops being
/// pinned. Liked Songs is not part of the pinned list and never moves.
fn drop_row(
    app: &mut App,
    entries: &[Entry],
    liked_rows: usize,
    pinned_rows: usize,
    slot: usize,
    uri: &str,
) {
    let mut pinned = app.settings.pinned_contexts.clone();
    let section_end = liked_rows + pinned_rows;
    if slot <= section_end {
        // The pinned entry the drop lands in front of anchors the new
        // position, so entries pinned from another shelf keep theirs.
        let anchor = entries[liked_rows..section_end]
            .iter()
            .skip(slot - liked_rows)
            .map(|entry| entry.uri.as_str())
            .find(|held| *held != uri)
            .map(str::to_string);
        pinned.retain(|held| held != uri);
        let at = anchor
            .and_then(|anchor| pinned.iter().position(|held| *held == anchor))
            .unwrap_or(pinned.len());
        pinned.insert(at, uri.to_string());
    } else {
        pinned.retain(|held| held != uri);
    }
    if pinned != app.settings.pinned_contexts {
        app.settings.pinned_contexts = pinned;
        app.mark_settings_dirty();
    }
}

/// The Liked Songs tile: the violet-to-mint gradient Spotify gives it,
/// with a white heart, whatever the accent. The gradient is a two-by-two
/// texture the GPU interpolates, so it takes the tile's rounded corners.
pub fn liked_cover(ui: &egui::Ui, rect: Rect, radius: f32) {
    let id = egui::Id::new("liked-songs-gradient");
    // Loading a texture takes the context's own lock, so the data store is
    // released before it; holding both at once deadlocks.
    let cached = ui
        .ctx()
        .data(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = cached.unwrap_or_else(|| {
        let image = egui::ColorImage {
            size: [2, 2],
            source_size: egui::vec2(2.0, 2.0),
            pixels: vec![
                egui::Color32::from_rgb(0x45, 0x0a, 0xf5),
                egui::Color32::from_rgb(0x7a, 0x5c, 0xe8),
                egui::Color32::from_rgb(0x7a, 0x5c, 0xe8),
                egui::Color32::from_rgb(0xc4, 0xef, 0xd9),
            ],
        };
        let handle =
            ui.ctx()
                .load_texture("liked-songs-gradient", image, egui::TextureOptions::LINEAR);
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, handle.clone()));
        handle
    });
    egui::Image::new(egui::load::SizedTexture::new(texture.id(), rect.size()))
        .corner_radius(CornerRadius::same(radius.min(127.0) as u8))
        .paint_at(ui, rect);
    let size = rect.width() * 0.45;
    let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(size));
    Icon::HeartFilled
        .image(egui::Color32::WHITE, size)
        .paint_at(ui, icon_rect);
}
