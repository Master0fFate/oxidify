//! Window layout: panels, overlays, keyboard shortcuts.
//!
//! The window is laid out the way Spotify's desktop client has been since
//! its global-navigation redesign: one bar across the top with the
//! navigation, Home, search, and the account; under it Your Library on the
//! left, the page in the middle, and an optional side panel (Now Playing,
//! the queue, or the lyrics) on the right, each a rounded panel floating on
//! the darker window with a gutter between them; and the player bar along
//! the bottom.

pub mod artist;
pub mod collection;
mod devices;
mod dialogs;
pub mod home;
mod keys;
pub mod library;
pub mod login;
mod lyrics;
pub mod now_playing;
pub mod player_bar;
pub mod queue;
pub mod search;
pub mod settings;
pub mod show;
pub mod sidebar;
pub mod topbar;
pub mod widgets;
pub mod winamp;
#[cfg(target_os = "windows")]
mod window_chrome;

use egui::{Align2, Color32, CornerRadius, Frame, Margin, Rect, Stroke, vec2};

use crate::api::models::pick_image;
use crate::app::App;
use crate::backend::AuthStatus;
use crate::model::{Action, Page, ToastKind};
use crate::theme::{self, Icon};

/// Which side of the window a panel sits on, for the gutters it owns: every
/// panel keeps the gap below it, the left panel the gap on its left, the
/// right panel the gap on its right, and the page both.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PanelSide {
    Left,
    Center,
    Right,
}

/// The frame a floating panel is drawn in: the window colour, with the
/// gutters this panel owns as margins. The rounded panel itself is painted
/// by [`panel_card`] once the frame has given the content its rectangle.
pub fn panel_frame(palette: &theme::Palette, side: PanelSide) -> Frame {
    let gap = theme::PANEL_GAP as i8;
    let (left, right) = match side {
        PanelSide::Left => (gap, 0),
        PanelSide::Center => (gap, gap),
        PanelSide::Right => (0, gap),
    };
    Frame::new().fill(palette.window).inner_margin(Margin {
        left,
        right,
        top: 0,
        bottom: gap,
    })
}

/// Paints the rounded panel behind a panel's content and clips the content
/// to it, so scrolled rows end at the rounded edge instead of the window's.
pub fn panel_card(ui: &mut egui::Ui, palette: &theme::Palette) -> Rect {
    let card = ui.max_rect();
    ui.painter()
        .rect_filled(card, CornerRadius::same(theme::PANEL_RADIUS), palette.panel);
    ui.set_clip_rect(card.intersect(ui.clip_rect()));
    card
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let ctx = &ctx;
    #[cfg(target_os = "windows")]
    window_chrome::show(app, ui);
    keys::handle(app, ctx);
    for path in winamp::dropped_skins(ctx) {
        app.actions.push(Action::InstallSkin(path));
    }
    let signed_in = app.is_connected() && app.user.is_some();
    let connecting = matches!(app.auth, AuthStatus::Connecting | AuthStatus::Starting)
        || (app.is_connected() && app.user.is_none());
    if !signed_in {
        login::show(app, ui, connecting);
        toasts(app, ctx, 20.0);
        return;
    }
    player_bar::show(app, ui);
    topbar::show(app, ui);
    if app.settings.sidebar_visible {
        sidebar::show(app, ui);
    }
    // One side panel at a time, the way the official client keeps its
    // right column: Now Playing, the queue, or the lyrics.
    if app.show_now_playing_panel {
        now_playing::side_panel(app, ui);
    } else if app.show_queue_panel {
        queue::side_panel(app, ui);
    } else if app.show_lyrics_panel {
        lyrics::side_panel(app, ui);
    }
    central(app, ui);
    devices::popup(app, ctx);
    dialogs::show(app, ctx);
    widgets::drag_ghost(ctx, &app.palette);
    toasts(app, ctx, theme::PLAYER_BAR_HEIGHT + 16.0);
}

fn page_tint(app: &mut App) -> Option<Color32> {
    let page = app.page().clone();
    let image = match &page {
        Page::Playlist(id) => app
            .playlist_pages
            .get(id)
            .and_then(|page| page.playlist.get())
            .and_then(|playlist| pick_image(&playlist.images, 300))
            .map(str::to_string),
        Page::Album(id) => app
            .album_pages
            .get(id)
            .and_then(|page| page.album.get())
            .and_then(|album| pick_image(&album.images, 300))
            .map(str::to_string),
        Page::Artist(id) => app
            .artist_pages
            .get(id)
            .and_then(|page| page.artist.get())
            .and_then(|artist| pick_image(&artist.images, 300))
            .map(str::to_string),
        Page::Show(id) => app
            .show_pages
            .get(id)
            .and_then(|page| page.show.get())
            .and_then(|show| pick_image(&show.images, 300))
            .map(str::to_string),
        Page::LikedSongs => return Some(Color32::from_rgb(0x2b, 0x4c, 0x73)),
        _ => None,
    };
    if !app.settings.accent_from_art && image.is_some() {
        return None;
    }
    match image {
        Some(url) => app.tint_for(Some(&url)),
        None => app.now_playing_tint(),
    }
}

fn central(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let tint = page_tint(app);
    egui::CentralPanel::default()
        .frame(panel_frame(&palette, PanelSide::Center))
        .show(ui, |ui| {
            let card = panel_card(ui, &palette);
            if let Some(tint) = tint {
                let strength = if matches!(
                    app.page(),
                    Page::Home | Page::Search | Page::Settings | Page::Queue
                ) {
                    0.45
                } else {
                    0.85
                };
                let top = blend(palette.panel, tint, strength);
                let header = Rect::from_min_size(card.min, vec2(card.width(), 340.0));
                widgets::paint_rounded_gradient(
                    ui,
                    header,
                    f32::from(theme::PANEL_RADIUS),
                    top,
                    palette.panel,
                );
            }
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            let page = app.page().clone();
            // Home's filter chips stay put while the shelves scroll under
            // them, as the official client's do.
            if page == Page::Home {
                home::filter_row(app, ui);
            }
            egui::ScrollArea::vertical()
                .id_salt(("page", page.encode()))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    Frame::new()
                        .inner_margin(Margin {
                            left: widgets::PAGE_PADDING as i8,
                            right: widgets::PAGE_PADDING as i8,
                            top: 4,
                            bottom: 48,
                        })
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            match page {
                                Page::Home => home::show(app, ui),
                                Page::TopSongs => collection::top_songs(app, ui),
                                Page::Search => search::show(app, ui),
                                Page::LikedSongs => collection::liked(app, ui),
                                Page::Albums | Page::Artists | Page::Podcasts | Page::Episodes => {
                                    library::show(app, ui, page)
                                }
                                Page::Playlist(id) => collection::playlist(app, ui, &id),
                                Page::Album(id) => collection::album(app, ui, &id),
                                Page::Artist(id) => artist::show(app, ui, &id),
                                Page::Show(id) => show::show(app, ui, &id),
                                Page::Queue => queue::page(app, ui),
                                Page::Settings => settings::show(app, ui),
                            }
                        });
                });
        });
}

pub fn blend(base: Color32, tint: Color32, amount: f32) -> Color32 {
    let a = egui::Rgba::from(base);
    let b = egui::Rgba::from(tint);
    let mixed = a * (1.0 - amount) + b * amount;
    let mut color = Color32::from(mixed);
    color[3] = 255;
    color
}

fn toasts(app: &mut App, ctx: &egui::Context, bottom_offset: f32) {
    if app.toasts.is_empty() {
        return;
    }
    let palette = app.palette;
    egui::Area::new(egui::Id::new("toasts"))
        .anchor(Align2::RIGHT_BOTTOM, vec2(-20.0, -bottom_offset))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for toast in &app.toasts {
                let age = toast.created.elapsed().as_secs_f32();
                let alpha = if age < 0.15 {
                    age / 0.15
                } else if age > 2.8 {
                    ((3.2 - age) / 0.4).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                ui.set_opacity(alpha);
                Frame::new()
                    .fill(palette.overlay)
                    .stroke(Stroke::new(1.0, palette.outline))
                    .corner_radius(CornerRadius::same(theme::RADIUS))
                    .inner_margin(Margin::symmetric(14, 10))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: palette.shadow,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (icon, color) = match toast.kind {
                                ToastKind::Info => (Icon::CircleCheck, palette.accent),
                                ToastKind::Error => (Icon::CircleAlert, palette.danger),
                            };
                            theme::icon(ui, icon, 16.0, color);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&toast.message)
                                        .font(theme::medium(13.5))
                                        .color(palette.text),
                                )
                                .wrap(),
                            );
                        });
                    });
            }
        });
}
