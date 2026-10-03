//! The Now Playing view: what is playing, large, in a panel beside the
//! page, with the artist and what comes next, the way the official
//! client's right column shows it.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Rect, Sense, Vec2, vec2};

use crate::api::models::{PlayableItem, pick_image};
use crate::app::{App, NowPlaying};
use crate::model::{Action, Loadable, Page, RowContext};
use crate::theme::{self, Icon};

use super::widgets::{self, TrackRow};

/// How much of the panel's top the playing cover's colour washes.
const TINT_HEIGHT: f32 = 280.0;

pub fn side_panel(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let panel = egui::Panel::right("now-playing-panel")
        .resizable(true)
        .default_size(app.settings.now_playing_width)
        .size_range(280.0..=520.0)
        .show_separator_line(false)
        .frame(super::panel_frame(&palette, super::PanelSide::Right));
    let response = panel.show(ui, |ui| {
        let card = super::panel_card(ui, &palette);
        if let Some(tint) = app.now_playing_tint() {
            let header = Rect::from_min_size(card.min, vec2(card.width(), TINT_HEIGHT));
            widgets::paint_rounded_gradient(
                ui,
                header,
                f32::from(theme::PANEL_RADIUS),
                super::blend(palette.panel, tint, 0.55),
                palette.panel,
            );
        }
        Frame::new()
            .inner_margin(Margin::symmetric(16, 12))
            .show(ui, |ui| {
                header(app, ui);
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .id_salt("now-playing-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| contents(app, ui));
            });
    });
    let width = response.response.rect.width();
    if (width - app.settings.now_playing_width).abs() > 1.0 {
        app.settings.now_playing_width = width;
        app.actions.push(Action::SettingsChanged);
    }
}

fn header(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        // The panel is titled by what the music comes from, as the
        // official client titles it; the title opens that page.
        match app.playing_context_heading() {
            Some((name, page)) => {
                let response = theme::link(ui, name, theme::bold(16.0), palette.text);
                if response.clicked() {
                    app.actions.push(Action::Open(page));
                }
            }
            None => {
                theme::text(ui, "Now playing", theme::bold(16.0), palette.text);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::icon_button(ui, Icon::X, 18.0, palette.secondary, palette.text, "Close")
                .clicked()
            {
                app.actions.push(Action::ToggleNowPlayingPanel);
            }
            if let Some(item) = app.now_playing_item() {
                let more = theme::icon_button(
                    ui,
                    Icon::Ellipsis,
                    18.0,
                    palette.secondary,
                    palette.text,
                    "More",
                );
                egui::Popup::menu(&more)
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| widgets::item_menu(ui, app, &item, None, None));
            }
        });
    });
}

fn contents(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let Some(now) = app.now_playing() else {
        widgets::empty_state(
            ui,
            &palette,
            Icon::SquarePlay,
            "Nothing playing",
            "Play something and it shows up here, large.",
        );
        return;
    };
    let width = ui.available_width();
    ui.add_space(4.0);

    // The cover, as wide as the panel.
    let (cover, _) = ui.allocate_exact_size(Vec2::splat(width), Sense::hover());
    widgets::paint_shadow(ui, &palette, cover, 8.0);
    widgets::paint_cover(
        ui,
        &palette,
        now.art_url.as_deref().or(now.art_small.as_deref()),
        cover,
        8.0,
        if now.is_episode {
            Icon::Mic
        } else {
            Icon::Music
        },
    );
    ui.add_space(14.0);

    title_block(app, ui, &now, width);

    if !now.local {
        ui.add_space(12.0);
        let label = format!(
            "Playing on {}",
            now.device_name
                .clone()
                .unwrap_or_else(|| "another device".into())
        );
        let response = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                theme::icon(ui, Icon::Speaker, 16.0, palette.accent);
                theme::text(ui, label, theme::medium(13.0), palette.accent)
            })
            .inner;
        if response
            .interact(Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            app.actions.push(Action::ToggleDevicesPopup);
        }
    }

    ui.add_space(20.0);
    about_the_artist(app, ui, &now);
    next_in_queue(app, ui, &now);
    ui.add_space(24.0);
}

/// The title and who made it, with the add-to-Liked-Songs control beside.
fn title_block(app: &mut App, ui: &mut egui::Ui, now: &NowPlaying, width: f32) {
    let palette = app.palette;
    let liked_room = if now.is_episode { 0.0 } else { 44.0 };
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.vertical(|ui| {
            ui.set_width((width - liked_room).max(40.0));
            ui.spacing_mut().item_spacing.y = 3.0;
            let title = theme::link(ui, &now.title, theme::bold(22.0), palette.text);
            if title.clicked() {
                if let Some(id) = &now.album_id {
                    app.actions.push(Action::Open(Page::Album(id.clone())));
                } else if let Some(id) = &now.show_id {
                    app.actions.push(Action::Open(Page::Show(id.clone())));
                }
            }
            if now.is_episode {
                let response =
                    theme::link(ui, &now.subtitle, theme::regular(14.0), palette.secondary);
                if response.clicked()
                    && let Some(id) = &now.show_id
                {
                    app.actions.push(Action::Open(Page::Show(id.clone())));
                }
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (index, artist) in now.artists.iter().enumerate() {
                        if index > 0 {
                            theme::text(ui, ", ", theme::regular(14.0), palette.secondary);
                        }
                        let response = theme::link(
                            ui,
                            artist.name.clone(),
                            theme::regular(14.0),
                            palette.secondary,
                        );
                        if response.clicked()
                            && let Some(id) = artist.id.clone()
                        {
                            app.actions.push(Action::Open(Page::Artist(id)));
                        }
                    }
                });
            }
        });
        if !now.is_episode {
            ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                let saved = app.is_saved(&now.uri).unwrap_or(false);
                if theme::liked_button(ui, &palette, saved, 20.0, palette.secondary).clicked() {
                    app.actions.push(Action::ToggleSaved(now.uri.clone()));
                }
            });
        }
    });
}

/// A card for the first credited artist: their picture and following when
/// Spotify has told us, their name regardless.
fn about_the_artist(app: &mut App, ui: &mut egui::Ui, now: &NowPlaying) {
    let palette = app.palette;
    let Some(artist) = now.artists.first().filter(|_| !now.is_episode) else {
        return;
    };
    let Some(id) = artist.id.clone() else {
        return;
    };
    let loaded = app
        .artist_pages
        .get(&id)
        .and_then(|page| page.artist.get())
        .cloned();
    let image = loaded
        .as_ref()
        .and_then(|artist| pick_image(&artist.images, 300).map(str::to_string));
    let followers = loaded
        .as_ref()
        .and_then(|artist| artist.followers.as_ref())
        .map(|followers| format!("{} followers", with_thousands(followers.total)));
    let name = artist.name.clone();
    let response = card(ui, &palette, |ui| {
        theme::text(ui, "About the artist", theme::semibold(14.0), palette.text);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            widgets::cover(ui, &palette, image.as_deref(), 64.0, 32.0, Icon::User);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                theme::text(ui, &name, theme::semibold(15.0), palette.text);
                match &followers {
                    Some(followers) => {
                        theme::text(ui, followers, theme::regular(12.5), palette.secondary);
                    }
                    None if loaded.is_none() => {
                        theme::text(ui, "Artist", theme::regular(12.5), palette.secondary);
                    }
                    None => {}
                }
            });
        });
    });
    if response.clicked() {
        app.actions.push(Action::Open(Page::Artist(id)));
    }
}

/// What plays after this, with a way into the whole queue.
fn next_in_queue(app: &mut App, ui: &mut egui::Ui, now: &NowPlaying) {
    let palette = app.palette;
    let next: Option<PlayableItem> = match &app.queue {
        Loadable::Loaded(queue) => queue
            .queue
            .iter()
            .find(|item| item.uri() != now.uri)
            .cloned(),
        _ => None,
    };
    let Some(next) = next else {
        return;
    };
    ui.add_space(12.0);
    Frame::new()
        .fill(
            palette
                .surface
                .gamma_multiply(if palette.dark { 0.8 } else { 1.0 }),
        )
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin::symmetric(8, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                theme::text(ui, "Next in queue", theme::semibold(14.0), palette.text);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(8.0);
                    if theme::link(ui, "Open queue", theme::medium(12.5), palette.secondary)
                        .clicked()
                    {
                        app.actions.push(Action::ToggleQueuePanel);
                    }
                });
            });
            ui.add_space(4.0);
            let context = RowContext::Uris(vec![next.uri().to_string()]);
            widgets::track_row(
                ui,
                app,
                TrackRow {
                    index: 0,
                    number: None,
                    item: &next,
                    context: &context,
                    show_cover: true,
                    show_album: false,
                    added_at: None,
                    added_by: None,
                    show_added_by: false,
                    compact: true,
                    shift: 0.0,
                },
            );
        });
}

/// A raised, clickable card, the whole of it one target.
fn card(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let response = Frame::new()
        .fill(
            palette
                .surface
                .gamma_multiply(if palette.dark { 0.8 } else { 1.0 }),
        )
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        })
        .response;
    let response = response.interact(Sense::click());
    if response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            CornerRadius::same(theme::RADIUS),
            egui::Stroke::new(1.0, palette.outline),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `1284930` as `1,284,930`.
fn with_thousands(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follower_counts_group_thousands() {
        assert_eq!(with_thousands(0), "0");
        assert_eq!(with_thousands(999), "999");
        assert_eq!(with_thousands(1_000), "1,000");
        assert_eq!(with_thousands(1_284_930), "1,284,930");
    }
}
