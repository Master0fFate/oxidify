//! The words of the playing track, in a side panel that follows the song.

use egui::{Align, Frame, Layout, Margin, Rect, Sense, UiBuilder, pos2, vec2};

use crate::app::{App, NowPlaying};
use crate::model::{Action, Loadable};
use crate::theme::{self, Icon};

use super::widgets;

const LINE_SIZE: f32 = 19.0;
const FULLSCREEN_LINE_SIZE: f32 = 34.0;
const LINE_GAP: f32 = 10.0;
const SUNG_LINE_AT: f32 = 0.2;
const LYRICS_BESIDE_WIDTH: f32 = 640.0;
const COVER_BESIDE_MIN_WIDTH: f32 = 900.0;
const COLUMN_GAP: f32 = 64.0;
const COVER_CAPTION: f32 = 90.0;
/// How long a line takes to light up or fade.
const LIGHT_UP_SECONDS: f32 = 0.22;

fn blend(from: egui::Color32, to: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from(egui::Rgba::from(from) * (1.0 - t) + egui::Rgba::from(to) * t)
}

pub fn side_panel(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let panel = egui::Panel::right("lyrics-panel")
        .resizable(true)
        .default_size(app.settings.lyrics_width)
        .size_range(280.0..=640.0)
        .show_separator_line(false)
        .frame(super::panel_frame(&palette, super::PanelSide::Right));
    let response = panel.show(ui, |ui| {
        super::panel_card(ui, &palette);
        Frame::new()
            .inner_margin(Margin::symmetric(12, 12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    theme::text(ui, "Lyrics", theme::bold(18.0), palette.text);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::icon_button(
                            ui,
                            Icon::X,
                            18.0,
                            palette.secondary,
                            palette.text,
                            "Close",
                        )
                        .clicked()
                        {
                            app.actions.push(Action::ToggleLyricsPanel);
                        }
                        if theme::icon_button(
                            ui,
                            Icon::Shrink,
                            18.0,
                            palette.secondary,
                            palette.text,
                            "Full screen",
                        )
                        .clicked()
                        {
                            app.actions.push(Action::ToggleFullscreenLyrics);
                        }
                        let loaded = matches!(&app.lyrics, Loadable::Loaded(Some(_)));
                        if loaded
                            && !app.lyrics_following
                            && theme::pill_button(ui, &palette, "Follow", false).clicked()
                        {
                            app.lyrics_following = true;
                            app.lyrics_line_shown = None;
                        }
                    });
                });
                ui.add_space(8.0);
                contents(app, ui, LINE_SIZE, false);
            });
    });
    let current_width = response.response.rect.width();
    if (app.settings.lyrics_width - current_width).abs() > 1.0 {
        app.settings.lyrics_width = current_width;
        app.actions.push(Action::SettingsChanged);
    }
}

pub fn fullscreen(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(palette.panel)
                .inner_margin(Margin::symmetric(28, 16)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(super::caption_width(ui.ctx()));
                    if theme::icon_button(
                        ui,
                        Icon::X,
                        18.0,
                        palette.secondary,
                        palette.text,
                        "Leave full screen",
                    )
                    .clicked()
                    {
                        app.actions.push(Action::ToggleFullscreenLyrics);
                    }
                });
            });
            match app.now_playing() {
                Some(now) if ui.available_width() >= COVER_BESIDE_MIN_WIDTH => {
                    with_cover(app, ui, &now);
                }
                _ => contents(app, ui, FULLSCREEN_LINE_SIZE, true),
            }
        });
}

fn with_cover(app: &mut App, ui: &mut egui::Ui, now: &NowPlaying) {
    let rest = ui.available_rect_before_wrap();
    let below = Rect::from_min_max(rest.min, pos2(rest.right(), rest.bottom() - 24.0));
    let words = matches!(&app.lyrics, Loadable::Loaded(Some(lyrics)) if !lyrics.instrumental);
    let mut span = below;
    if app.show_queue_panel {
        let width = super::queue::fullscreen_column_width(app.settings.queue_width);
        span.max.x -= width + COLUMN_GAP;
        let column = Rect::from_min_max(pos2(below.right() - width, below.top()), below.max);
        let mut queue_ui = ui.new_child(UiBuilder::new().max_rect(column));
        super::queue::fullscreen_column(app, &mut queue_ui);
    }
    if words {
        cover_beside_lyrics(app, ui, now, span);
    } else {
        cover_alone(app, ui, now, span);
    }
}

fn cover_beside_lyrics(app: &mut App, ui: &mut egui::Ui, now: &NowPlaying, span: Rect) {
    let side = (span.width() * 0.4)
        .min(span.height() - COVER_CAPTION)
        .clamp(160.0, 520.0);
    let lyrics_width = (span.width() - side - COLUMN_GAP).clamp(80.0, LYRICS_BESIDE_WIDTH);
    let left = span.center().x - (side + COLUMN_GAP + lyrics_width) / 2.0;
    let column = Rect::from_min_size(
        pos2(left, span.center().y - (side + COVER_CAPTION) / 2.0),
        vec2(side, side + COVER_CAPTION),
    );
    big_cover(app, ui, now, column, Align::Min);
    let lyrics = Rect::from_min_max(
        pos2(column.right() + COLUMN_GAP, span.top()),
        pos2(column.right() + COLUMN_GAP + lyrics_width, span.bottom()),
    );
    let mut content = ui.new_child(UiBuilder::new().max_rect(lyrics));
    let line_size = (lyrics_width * 0.046).clamp(26.0, FULLSCREEN_LINE_SIZE);
    contents(app, &mut content, line_size, false);
}

fn cover_alone(app: &mut App, ui: &mut egui::Ui, now: &NowPlaying, span: Rect) {
    let palette = app.palette;
    let side = (span.height() - 140.0)
        .min(span.width() * 0.5)
        .clamp(200.0, 560.0);
    let column = Rect::from_center_size(span.center(), vec2(side, side + COVER_CAPTION));
    big_cover(app, ui, now, column, Align::Center);
    let note = Rect::from_min_size(
        pos2(column.left(), column.bottom() + 8.0),
        vec2(column.width(), 120.0),
    );
    let mut note_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(note)
            .layout(Layout::top_down(Align::Center)),
    );
    note_ui.spacing_mut().item_spacing.y = 4.0;
    let ui = &mut note_ui;
    match &app.lyrics {
        Loadable::NotLoaded | Loadable::Loading => {
            theme::text(ui, "Loading…", theme::semibold(13.0), palette.secondary);
        }
        Loadable::Failed(error) => {
            let message = format!("Couldn't fetch the lyrics: {error}");
            ui.add(
                egui::Label::new(
                    egui::RichText::new(message)
                        .font(theme::regular(13.0))
                        .color(palette.secondary),
                )
                .wrap(),
            );
            ui.add_space(4.0);
            if theme::pill_button(ui, &palette, "Try again", false).clicked() {
                app.actions.push(Action::RetryLyrics);
            }
        }
        Loadable::Loaded(None) => {
            theme::text(ui, "No lyrics", theme::semibold(13.0), palette.text);
            theme::text(
                ui,
                "No lyrics found for this one.",
                theme::regular(13.0),
                palette.secondary,
            );
        }
        Loadable::Loaded(Some(_)) => {
            theme::text(ui, "Instrumental", theme::semibold(13.0), palette.text);
            theme::text(
                ui,
                "No words to follow on this one.",
                theme::regular(13.0),
                palette.secondary,
            );
        }
    }
}

fn big_cover(app: &App, ui: &mut egui::Ui, now: &NowPlaying, column: Rect, align: Align) {
    let palette = app.palette;
    let cover = Rect::from_min_size(column.min, vec2(column.width(), column.width()));
    let radius = 10.0;
    widgets::paint_shadow(ui, &palette, cover, radius);
    widgets::paint_cover(
        ui,
        &palette,
        now.art_url.as_deref().or(now.art_small.as_deref()),
        cover,
        radius,
        Icon::Music,
    );
    let caption = Rect::from_min_max(pos2(column.left(), cover.bottom() + 18.0), column.max);
    let mut text = ui.new_child(
        UiBuilder::new()
            .max_rect(caption)
            .layout(Layout::top_down(align)),
    );
    text.spacing_mut().item_spacing.y = 4.0;
    theme::text(&mut text, &now.title, theme::semibold(22.0), palette.text);
    theme::text(
        &mut text,
        &now.subtitle,
        theme::regular(14.0),
        palette.secondary,
    );
    if let Some(source) = &now.source_label {
        theme::text(&mut text, source, theme::regular(12.5), palette.dim);
    }
}

fn show_sung_line(ui: &egui::Ui, line: Rect) {
    let above = (ui.clip_rect().height() * SUNG_LINE_AT - line.height() / 2.0).max(0.0);
    let target = Rect::from_min_max(pos2(line.left(), line.top() - above), line.max);
    ui.scroll_to_rect(target, Some(Align::Min));
}

fn contents(app: &mut App, ui: &mut egui::Ui, line_size: f32, centered: bool) {
    let palette = app.palette;
    let Some(now) = app.now_playing() else {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Mic,
            "Nothing playing",
            "Play something and its words show up here.",
        );
        return;
    };
    let lyrics = match &app.lyrics {
        Loadable::NotLoaded | Loadable::Loading => {
            widgets::loading_row(ui, &palette);
            return;
        }
        Loadable::Failed(error) => {
            let message = format!("Couldn't fetch the lyrics: {error}");
            ui.add_space(8.0);
            theme::text(ui, message, theme::regular(13.0), palette.secondary);
            ui.add_space(8.0);
            if theme::pill_button(ui, &palette, "Try again", false).clicked() {
                app.actions.push(Action::RetryLyrics);
            }
            return;
        }
        Loadable::Loaded(None) => {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Mic,
                "No lyrics",
                "No lyrics found for this one.",
            );
            return;
        }
        Loadable::Loaded(Some(lyrics)) if lyrics.instrumental => {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Music,
                "Instrumental",
                "No words to follow on this one.",
            );
            return;
        }
        Loadable::Loaded(Some(lyrics)) => lyrics.clone(),
    };

    let active = lyrics.active_line(now.position_ms);
    let follow = app.lyrics_following && app.lyrics_line_shown != Some(active);
    // The line being sung is bold and in the accent colour; every other
    // line is quiet, regular text, the same before and after it has been
    // sung. A line takes 220 ms to light up or fade, as in omarchy-lyrics.
    let quiet = palette.text.gamma_multiply(0.45);
    let body = if centered {
        Layout::top_down(Align::Center)
    } else {
        Layout::default()
    };
    let scroll = egui::ScrollArea::vertical()
        .id_salt("lyrics-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.with_layout(body, |ui| {
                // Before the first line there is nothing to highlight, so the
                // panel sits at the top rather than wherever it was left.
                if follow && lyrics.synced && active.is_none() {
                    let top = ui.cursor().min;
                    ui.scroll_to_rect(
                        egui::Rect::from_min_size(top, egui::vec2(1.0, 1.0)),
                        Some(Align::Min),
                    );
                }
                let height = ui.clip_rect().height();
                let (above, below) = if lyrics.synced {
                    (
                        (height * SUNG_LINE_AT - line_size).max(12.0),
                        (height * (1.0 - SUNG_LINE_AT)).max(60.0),
                    )
                } else {
                    (12.0, 60.0)
                };
                ui.add_space(above);
                for (index, line) in lyrics.lines.iter().enumerate() {
                    let is_active = active == Some(index);
                    let lit = ui.ctx().animate_bool_with_time(
                        egui::Id::new("lyric-line").with(index),
                        is_active,
                        crate::motion::seconds(ui.ctx(), LIGHT_UP_SECONDS),
                    );
                    let color = blend(quiet, palette.accent, lit);
                    let font = if lit > 0.5 {
                        theme::bold(line_size)
                    } else {
                        theme::regular(line_size)
                    };
                    // A timed line with no words is the band playing on.
                    let text = if line.text.is_empty() && lyrics.synced {
                        "\u{266a}"
                    } else {
                        line.text.as_str()
                    };
                    let sense = if lyrics.synced {
                        Sense::click()
                    } else {
                        Sense::hover()
                    };
                    let display = crate::bidi::display_text(text);
                    let halign = if centered {
                        Align::Center
                    } else {
                        crate::bidi::halign_for(text)
                    };
                    let response = ui.add(
                        egui::Label::new(
                            egui::RichText::new(display.into_owned())
                                .font(font)
                                .color(color),
                        )
                        .halign(halign)
                        .sense(sense),
                    );
                    let rect = response.rect;
                    if lyrics.synced {
                        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.clicked()
                            && let Some(at_ms) = line.at_ms
                        {
                            app.actions.push(Action::Seek(at_ms));
                            app.lyrics_following = true;
                        }
                    }
                    if is_active && follow {
                        show_sung_line(ui, rect);
                    }
                    ui.add_space(LINE_GAP);
                }
                // Words without timing can only be followed by the clock: sit
                // at the part of the text the song is probably at.
                if app.lyrics_following && !lyrics.synced && now.duration_ms > 0 {
                    let fraction =
                        (f64::from(now.position_ms) / f64::from(now.duration_ms)).clamp(0.0, 1.0);
                    let content = ui.min_rect();
                    let y = content.top() + content.height() * fraction as f32;
                    ui.scroll_to_rect(
                        egui::Rect::from_min_max(
                            egui::pos2(content.left(), y),
                            egui::pos2(content.right(), y + 1.0),
                        ),
                        Some(Align::Center),
                    );
                }
                ui.add_space(below);
            });
        });
    // Scrolling by hand means the reader wants to look elsewhere; the
    // Follow button in the header picks the song back up.
    if ui.rect_contains_pointer(scroll.inner_rect)
        && ui.input(|input| input.smooth_scroll_delta.y != 0.0)
    {
        app.lyrics_following = false;
    }
    app.lyrics_line_shown = Some(active);
}
