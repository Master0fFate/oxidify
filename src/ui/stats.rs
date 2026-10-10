//! Your listening: the plays counted on this computer, summed up by
//! week, month, year and all time, with the songs, artists and albums
//! played most, a chart of the days or months, and a streak. Nothing on
//! this page comes from or goes to Spotify.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Rect, Sense, UiBuilder, pos2, vec2};

use crate::app::App;
use crate::history::{Period, Summary};
use crate::model::{Action, Page};
use crate::theme::{self, Icon};
use crate::util;

use super::widgets;

const ROW_HEIGHT: f32 = 52.0;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.add_space(20.0);
    theme::text(ui, "Your listening", theme::bold(30.0), palette.text);
    ui.add_space(4.0);
    theme::text(
        ui,
        "Counted on this computer once a song has played for half a minute. Nothing leaves it.",
        theme::regular(13.5),
        palette.secondary,
    );
    ui.add_space(14.0);
    let options: Vec<(Period, &str)> = Period::ALL
        .iter()
        .map(|period| (*period, period.label()))
        .collect();
    if let Some(period) = widgets::chips(ui, &palette, &options, app.stats_period) {
        app.actions.push(Action::SetStatsPeriod(period));
    }
    ui.add_space(18.0);

    let summary = app.stats_summary();
    if summary.plays == 0 {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            theme::icon(ui, Icon::TrendingUp, 36.0, palette.dim);
            ui.add_space(10.0);
            theme::text(
                ui,
                "Nothing counted yet",
                theme::semibold(16.0),
                palette.text,
            );
            ui.add_space(4.0);
            theme::text(
                ui,
                "Play something for half a minute and it shows up here.",
                theme::regular(13.5),
                palette.secondary,
            );
        });
        return;
    }

    tiles(ui, &palette, &summary);
    ui.add_space(22.0);
    chart(ui, &palette, &summary, app.stats_period);
    ui.add_space(26.0);

    let wide = ui.available_width() >= 900.0;
    if wide {
        ui.columns(2, |columns| {
            top_songs(app, &mut columns[0], &summary);
            top_artists(app, &mut columns[1], &summary);
            columns[1].add_space(20.0);
            top_albums(app, &mut columns[1], &summary);
        });
    } else {
        top_songs(app, ui, &summary);
        ui.add_space(20.0);
        top_artists(app, ui, &summary);
        ui.add_space(20.0);
        top_albums(app, ui, &summary);
    }

    ui.add_space(30.0);
    let confirm_id = egui::Id::new("stats-clear-confirm");
    let confirming = ui
        .data(|data| data.get_temp::<bool>(confirm_id))
        .unwrap_or(false);
    ui.horizontal(|ui| {
        if confirming {
            theme::text(
                ui,
                "This forgets every play counted so far.",
                theme::regular(13.0),
                palette.secondary,
            );
            if theme::pill_button(ui, &palette, "Clear history", true).clicked() {
                ui.data_mut(|data| data.remove::<bool>(confirm_id));
                app.actions.push(Action::ClearHistory);
            }
            if theme::pill_button(ui, &palette, "Keep it", false).clicked() {
                ui.data_mut(|data| data.remove::<bool>(confirm_id));
            }
        } else if theme::pill_button(ui, &palette, "Clear history", false).clicked() {
            ui.data_mut(|data| data.insert_temp(confirm_id, true));
        }
    });
}

fn tiles(ui: &mut egui::Ui, palette: &theme::Palette, summary: &Summary) {
    let streak = match summary.streak_days {
        0 => "No streak".to_string(),
        1 => "1 day in a row".to_string(),
        days => format!("{days} days in a row"),
    };
    let peak = summary
        .peak_hour
        .map(|hour| format!("Most often around {hour:02}:00"))
        .unwrap_or_default();
    let tiles = [
        (
            util::format_total_ms(summary.listened_ms),
            "listened".to_string(),
        ),
        (util::format_count(u64::from(summary.plays)), "plays".into()),
        (
            util::format_count(u64::from(summary.distinct_songs)),
            "different songs".into(),
        ),
        (
            util::format_count(u64::from(summary.distinct_artists)),
            "artists".into(),
        ),
    ];
    let gap = 12.0;
    let width = ui.available_width();
    let columns = if width >= 760.0 { 4 } else { 2 };
    let tile_width = (width - gap * (columns as f32 - 1.0)) / columns as f32;
    let rows: Vec<&[(String, String)]> = tiles.chunks(columns).collect();
    for row in rows {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (value, what) in row {
                Frame::new()
                    .fill(palette.surface)
                    .corner_radius(CornerRadius::same(theme::RADIUS))
                    .inner_margin(Margin::symmetric(16, 14))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(tile_width - 32.0);
                            theme::text(ui, value, theme::bold(24.0), palette.text);
                            theme::text(ui, what, theme::regular(12.5), palette.secondary);
                        });
                    });
            }
        });
        ui.add_space(gap);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 18.0;
        theme::icon(ui, Icon::Sparkles, 14.0, palette.accent);
        theme::text(ui, streak, theme::medium(13.0), palette.text);
        if !peak.is_empty() {
            theme::icon(ui, Icon::Clock, 14.0, palette.secondary);
            theme::text(ui, peak, theme::regular(13.0), palette.secondary);
        }
    });
}

/// Plays per day or per month, as bars drawn straight onto the page.
fn chart(ui: &mut egui::Ui, palette: &theme::Palette, summary: &Summary, period: Period) {
    let title = match period {
        Period::Week => "Plays each day",
        Period::Month => "Plays each day, last thirty",
        Period::Year | Period::All => "Plays each month",
    };
    theme::text(ui, title, theme::semibold(16.0), palette.text);
    ui.add_space(8.0);
    let height = 140.0;
    let label_height = 18.0;
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), height + label_height),
        Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let bars = &summary.bars;
    let most = bars.iter().map(|bar| bar.plays).max().unwrap_or(0).max(1);
    let count = bars.len().max(1) as f32;
    let gap = if bars.len() > 12 { 3.0 } else { 8.0 };
    let bar_width = ((rect.width() - gap * (count - 1.0)) / count).max(2.0);
    let baseline = rect.top() + height;
    ui.painter().hline(
        rect.x_range(),
        baseline,
        egui::Stroke::new(1.0, palette.outline),
    );
    let label_every = if bars.len() > 12 { 5 } else { 1 };
    for (index, bar) in bars.iter().enumerate() {
        let left = rect.left() + index as f32 * (bar_width + gap);
        let full = height - 6.0;
        let bar_height =
            (bar.plays as f32 / most as f32 * full).max(if bar.plays > 0 { 3.0 } else { 0.0 });
        let bar_rect = Rect::from_min_max(
            pos2(left, baseline - bar_height),
            pos2(left + bar_width, baseline),
        );
        let last = index + 1 == bars.len();
        let fill = if last {
            palette.accent
        } else {
            palette.accent_hover
        };
        if bar.plays > 0 {
            ui.painter()
                .rect_filled(bar_rect, CornerRadius::same(3), fill);
        }
        let hover = Rect::from_min_max(pos2(left, rect.top()), pos2(left + bar_width, baseline));
        if ui.rect_contains_pointer(hover) {
            let text = format!(
                "{}: {} {}",
                bar.label,
                bar.plays,
                if bar.plays == 1 { "play" } else { "plays" }
            );
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                egui::Id::new(("stats-bar", index)),
                egui::PopupAnchor::Pointer,
            )
            .show(|ui| {
                ui.label(text);
            });
        }
        if index % label_every == 0 || last {
            ui.painter().text(
                pos2(left + bar_width / 2.0, baseline + 4.0),
                egui::Align2::CENTER_TOP,
                &bar.label,
                theme::regular(11.0),
                palette.secondary,
            );
        }
    }
}

fn plays_label(plays: u32) -> String {
    if plays == 1 {
        "1 play".into()
    } else {
        format!("{plays} plays")
    }
}

fn top_songs(app: &mut App, ui: &mut egui::Ui, summary: &Summary) {
    let palette = app.palette;
    theme::text(
        ui,
        "Songs you played most",
        theme::semibold(16.0),
        palette.text,
    );
    ui.add_space(6.0);
    for (rank, song) in summary.top_songs.iter().enumerate() {
        let response = ranked_row(
            ui,
            &palette,
            rank,
            song.image.as_deref(),
            Icon::Music,
            false,
            &song.name,
            &song.artists.join(", "),
            &plays_label(song.plays),
        );
        if response.clicked() {
            app.actions.push(Action::PlayUris {
                uris: vec![song.uri.clone()],
                index: 0,
            });
        }
    }
}

fn top_artists(app: &mut App, ui: &mut egui::Ui, summary: &Summary) {
    let palette = app.palette;
    theme::text(
        ui,
        "Artists you played most",
        theme::semibold(16.0),
        palette.text,
    );
    ui.add_space(6.0);
    for (rank, artist) in summary.top_artists.iter().enumerate() {
        let response = ranked_row(
            ui,
            &palette,
            rank,
            None,
            Icon::User,
            true,
            &artist.name,
            "",
            &plays_label(artist.plays),
        );
        if response.clicked()
            && let Some(id) = &artist.id
        {
            app.actions.push(Action::Open(Page::Artist(id.clone())));
        }
    }
}

fn top_albums(app: &mut App, ui: &mut egui::Ui, summary: &Summary) {
    let palette = app.palette;
    theme::text(
        ui,
        "Albums you played most",
        theme::semibold(16.0),
        palette.text,
    );
    ui.add_space(6.0);
    for (rank, album) in summary.top_albums.iter().enumerate() {
        let response = ranked_row(
            ui,
            &palette,
            rank,
            album.image.as_deref(),
            Icon::Disc,
            false,
            &album.name,
            &album.artist,
            &plays_label(album.plays),
        );
        if response.clicked()
            && let Some(id) = &album.id
        {
            app.actions.push(Action::Open(Page::Album(id.clone())));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn ranked_row(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    rank: usize,
    image: Option<&str>,
    fallback: Icon,
    round: bool,
    name: &str,
    subtitle: &str,
    trailing: &str,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(6), palette.surface_hover);
        }
        ui.painter().text(
            pos2(rect.left() + 20.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            (rank + 1).to_string(),
            theme::medium(13.0),
            palette.secondary,
        );
        let cover = Rect::from_min_size(
            pos2(rect.left() + 40.0, rect.center().y - 18.0),
            egui::Vec2::splat(36.0),
        );
        widgets::paint_cover(
            ui,
            palette,
            image,
            cover,
            if round { 18.0 } else { 4.0 },
            fallback,
        );
        let trailing_galley = ui.painter().layout_no_wrap(
            trailing.to_string(),
            theme::regular(12.5),
            palette.secondary,
        );
        let trailing_width = trailing_galley.size().x;
        ui.painter().galley(
            pos2(
                rect.right() - 12.0 - trailing_width,
                rect.center().y - trailing_galley.size().y / 2.0,
            ),
            trailing_galley,
            palette.secondary,
        );
        let text_rect = Rect::from_min_max(
            pos2(cover.right() + 12.0, rect.top() + 8.0),
            pos2(rect.right() - trailing_width - 24.0, rect.bottom() - 6.0),
        );
        let mut text_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(text_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        text_ui.set_clip_rect(text_rect.intersect(ui.clip_rect()));
        text_ui.spacing_mut().item_spacing.y = 1.0;
        theme::text(&mut text_ui, name, theme::medium(14.0), palette.text);
        if !subtitle.is_empty() {
            theme::text(
                &mut text_ui,
                subtitle,
                theme::regular(12.0),
                palette.secondary,
            );
        }
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
