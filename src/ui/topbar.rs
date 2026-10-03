//! The global bar across the top of the window: the mark and navigation on
//! the left, Home and search in the middle, the account on the right.

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, CornerRadius, Frame, Layout, Margin, Rect, Sense, UiBuilder, Vec2, pos2, vec2};

use crate::api::models::pick_image;
use crate::app::App;
use crate::backend::LocalPlayback;
use crate::model::{Action, Page};
use crate::settings::PlaybackBackend;
use crate::theme::{self, Icon, Palette};

pub(crate) const AVATAR_SIZE: f32 = 36.0;
const CONTROL_HIT: f32 = 31.0;
const CONTROL_GAP: f32 = 4.0;
const SPINNER_SIZE: f32 = 15.0;
const SOURCE_MIN: f32 = 48.0;
const UPDATE_WIDTH: f32 = 140.0;
/// The round Home button beside the search field.
const HOME_SIZE: f32 = 40.0;
/// How wide the search field grows on a wide window.
const SEARCH_MAX: f32 = 474.0;
/// Breathing room between the three clusters.
const CLUSTER_GAP: f32 = 16.0;
const NAV_SIZE: f32 = 32.0;
const MARK_SIZE: f32 = 32.0;
/// Where the Home-and-search cluster was drawn, for tests to read back.
const CLUSTER_RECT_ID: &str = "global-nav-cluster";

/// The width the navigation cluster takes: the mark, back, forward, and
/// the button that brings a hidden library back.
pub(crate) fn topbar_nav_width(sidebar_hidden: bool) -> f32 {
    let mut width = MARK_SIZE + 12.0 + NAV_SIZE + 8.0 + NAV_SIZE;
    if sidebar_hidden {
        width += 8.0 + NAV_SIZE;
    }
    width
}

/// Right-edge cluster: avatar, settings, refresh, and the spinner, update
/// notice, and playing-elsewhere chip when they show. Search yields before
/// this width is taken.
pub(crate) fn topbar_right_reserved(spinner: bool, update: bool, remote: bool) -> f32 {
    let mut width = AVATAR_SIZE + 2.0 * (CONTROL_GAP + CONTROL_HIT);
    if spinner {
        width += SPINNER_SIZE + 8.0;
    }
    if update {
        width += UPDATE_WIDTH + 8.0;
    }
    if remote {
        width += SOURCE_MIN + 8.0;
    }
    width
}

/// The search field's width given the room left between the navigation
/// and the right cluster, which the Home button shares.
pub(crate) fn topbar_search_width(room: f32) -> f32 {
    let cap = (room - HOME_SIZE - 8.0).max(0.0);
    (room * 0.5).clamp(160.0, SEARCH_MAX).min(cap)
}

fn nav_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    enabled: bool,
    tooltip: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::splat(NAV_SIZE),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if ui.is_rect_visible(rect) {
        let fill = if palette.dark {
            palette.panel
        } else {
            egui::Color32::from_black_alpha(20)
        };
        ui.painter().circle_filled(rect.center(), 16.0, fill);
        let color = if !enabled {
            palette.dim
        } else if response.hovered() {
            palette.text
        } else {
            palette.secondary
        };
        theme::paint_icon(ui, icon, rect, 20.0, color);
    }
    if enabled {
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tooltip)
    } else {
        response
    }
}

/// The round Home button, lit while Home is the page.
fn home_button(ui: &mut egui::Ui, palette: &Palette, active: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(HOME_SIZE), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let fill = if hovered {
            palette.surface_hover
        } else {
            palette.surface
        };
        ui.painter()
            .circle_filled(rect.center(), HOME_SIZE / 2.0, fill);
        let color = if active || hovered {
            palette.text
        } else {
            palette.secondary
        };
        theme::paint_icon(ui, Icon::House, rect, 22.0, color);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Home"));
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Home")
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    egui::Panel::top("global-nav")
        .exact_size(theme::TOP_BAR_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(palette.window).inner_margin(Margin {
            left: 12,
            right: 12,
            top: 6,
            bottom: 6,
        }))
        .show(ui, |ui| contents(app, ui));
}

fn contents(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let bar = ui.max_rect();
    let sidebar_hidden = !app.settings.sidebar_visible;
    let spinner = app
        .backend
        .activity()
        .busy(std::time::Duration::from_millis(1000));
    let remote_label = app.now_playing().filter(|now| !now.local).map(|now| {
        format!(
            "Playing on {}",
            now.device_name.unwrap_or_else(|| "another device".into())
        )
    });

    // The three clusters are placed by hand: Home and search sit in the
    // middle of the window, sliding aside only when the navigation or the
    // account would otherwise be covered.
    let nav_width = topbar_nav_width(sidebar_hidden);
    let right_reserved =
        topbar_right_reserved(spinner, app.update.is_some(), remote_label.is_some());
    let room = bar.width() - nav_width - right_reserved - 2.0 * CLUSTER_GAP;
    let search_width = topbar_search_width(room);
    let cluster_width = HOME_SIZE + 8.0 + search_width;
    let min_left = bar.left() + nav_width + CLUSTER_GAP;
    let max_left = (bar.right() - right_reserved - CLUSTER_GAP - cluster_width).max(min_left);
    let cluster_left = (bar.center().x - cluster_width / 2.0).clamp(min_left, max_left);

    // Navigation.
    let nav_rect = Rect::from_min_size(bar.min, vec2(nav_width, bar.height()));
    let mut nav = ui.new_child(
        UiBuilder::new()
            .max_rect(nav_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    nav.spacing_mut().item_spacing.x = 8.0;
    let (mark_rect, mark) = nav.allocate_exact_size(Vec2::splat(MARK_SIZE), Sense::click());
    if nav.is_rect_visible(mark_rect) {
        theme::brand_logo(&nav, mark_rect.center(), MARK_SIZE);
    }
    mark.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Oxidify"));
    if mark
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Home")
        .clicked()
    {
        app.actions.push(Action::Open(Page::Home));
    }
    nav.add_space(4.0);
    if nav_button(
        &mut nav,
        &palette,
        Icon::ChevronLeft,
        app.can_go_back(),
        "Back",
    )
    .clicked()
    {
        app.actions.push(Action::Back);
    }
    if nav_button(
        &mut nav,
        &palette,
        Icon::ChevronRight,
        app.can_go_forward(),
        "Forward",
    )
    .clicked()
    {
        app.actions.push(Action::Forward);
    }
    if sidebar_hidden
        && nav_button(
            &mut nav,
            &palette,
            Icon::PanelLeft,
            true,
            "Show Your Library (Ctrl+B)",
        )
        .clicked()
    {
        app.actions.push(Action::ToggleSidebar);
    }

    // Home and search.
    let cluster_rect = Rect::from_min_size(
        pos2(cluster_left, bar.top()),
        vec2(cluster_width, bar.height()),
    );
    ui.data_mut(|data| data.insert_temp(egui::Id::new(CLUSTER_RECT_ID), cluster_rect));
    let mut middle = ui.new_child(
        UiBuilder::new()
            .max_rect(cluster_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    middle.spacing_mut().item_spacing.x = 8.0;
    if home_button(&mut middle, &palette, matches!(app.page(), Page::Home)).clicked() {
        app.actions.push(Action::Open(Page::Home));
    }
    let id = egui::Id::new("global-search");
    let before = app.search.query.clone();
    let response = super::widgets::search_field(
        &mut middle,
        &palette,
        id,
        &mut app.search.query,
        "What do you want to play?",
        search_width,
    );
    if app.search.focus_requested {
        app.search.focus_requested = false;
        response.request_focus();
    }
    // Clear empties the field and focuses it in the same frame.
    // Neither should navigate away from the current page.
    let cleared = app.search.query.is_empty() && !before.is_empty();
    if response.gained_focus() && !cleared && !matches!(app.page(), Page::Search) {
        app.actions.push(Action::Open(Page::Search));
    }
    if app.search.query != before {
        app.search.typed_at = Some(std::time::Instant::now());
        if !cleared && !matches!(app.page(), Page::Search) {
            app.actions.push(Action::Open(Page::Search));
        }
    }
    if response.lost_focus() && middle.input(|input| input.key_pressed(egui::Key::Enter)) {
        let query = app.search.query.clone();
        app.actions.push(Action::Search(query));
    }
    if response.has_focus() && middle.input(|input| input.key_pressed(egui::Key::Escape)) {
        response.surrender_focus();
    }

    // Account and the page's controls, from the right edge inwards.
    let right_rect =
        Rect::from_min_max(pos2(cluster_rect.right() + CLUSTER_GAP, bar.top()), bar.max);
    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(right_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    right.spacing_mut().item_spacing.x = CONTROL_GAP;
    account_menu(app, &mut right);
    right.add_space(4.0);
    if theme::icon_button(
        &mut right,
        Icon::Settings,
        19.0,
        palette.secondary,
        palette.text,
        "Settings",
    )
    .clicked()
    {
        app.actions.push(Action::Open(Page::Settings));
    }
    let can_refresh = !matches!(app.page(), Page::Settings);
    if right
        .add_enabled_ui(can_refresh, |ui| {
            theme::icon_button(
                ui,
                Icon::Refresh,
                19.0,
                palette.secondary,
                palette.text,
                "Refresh current page",
            )
        })
        .inner
        .clicked()
    {
        app.actions.push(Action::Reload(app.page().clone()));
    }
    // A quiet spinner once the app has been talking to Spotify for a
    // while, long enough that fast requests never flash it.
    if spinner {
        right.add_space(4.0);
        theme::spinner(&mut right, SPINNER_SIZE, palette.secondary)
            .on_hover_text("Talking to Spotify…");
    }
    if let Some(update) = &app.update {
        right.add_space(4.0);
        update_notice(&mut right, &palette, update, &mut app.actions);
    }
    if let Some(label) = remote_label {
        right.add_space(4.0);
        let max_w = right.available_width();
        if max_w >= SOURCE_MIN {
            source_chip(&mut right, &palette, &label, max_w, &mut app.actions);
        }
    }
}

fn account_menu(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let (name, avatar) = app
        .user
        .as_ref()
        .map(|user| {
            (
                user.name().to_string(),
                pick_image(&user.images, 64).map(str::to_string),
            )
        })
        .unwrap_or_default();
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(AVATAR_SIZE), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if response.hovered() {
            palette.surface_hover
        } else {
            palette.surface
        };
        ui.painter().circle_filled(rect.center(), 18.0, fill);
        let inner = Rect::from_center_size(rect.center(), Vec2::splat(28.0));
        match avatar.as_deref() {
            Some(url) => {
                super::widgets::paint_cover(ui, &palette, Some(url), inner, 14.0, Icon::User)
            }
            None => {
                let initial = name
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string();
                ui.painter()
                    .circle_filled(inner.center(), 14.0, palette.accent);
                ui.painter().text(
                    inner.center(),
                    egui::Align2::CENTER_CENTER,
                    initial,
                    theme::bold(13.0),
                    palette.on_accent,
                );
            }
        }
    }
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(&name);
    egui::Popup::menu(&response)
        .frame(super::widgets::menu_frame(&palette))
        .align(egui::RectAlign::BOTTOM_END)
        .show(|ui| {
            ui.set_width(220.0);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                theme::text(ui, &name, theme::semibold(14.0), palette.text);
            });
            let product = app
                .user
                .as_ref()
                .and_then(|user| user.product.clone())
                .map(|product| capitalize(&product));
            let source = profile_source_brief(app);
            if product.is_some() || source.is_some() {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    if let Some(product) = &product {
                        theme::text(ui, product, theme::regular(12.0), palette.secondary);
                    }
                    if let Some(source) = &source {
                        let prefix = if product.is_some() {
                            " · using "
                        } else {
                            "using "
                        };
                        theme::text(
                            ui,
                            format!("{prefix}{source}"),
                            theme::regular(12.0),
                            palette.secondary,
                        );
                    }
                });
            }
            super::widgets::menu_separator(ui, &palette);
            if super::widgets::menu_item(ui, &palette, Some(Icon::Settings), "Settings") {
                app.actions.push(Action::Open(Page::Settings));
            }
            if super::widgets::menu_item(ui, &palette, Some(Icon::Info), "Keyboard shortcuts") {
                app.actions
                    .push(Action::ShowDialog(crate::model::Dialog::Shortcuts));
            }
            let library_label = if app.settings.sidebar_visible {
                "Hide Your Library"
            } else {
                "Show Your Library"
            };
            if super::widgets::menu_item(ui, &palette, Some(Icon::PanelLeft), library_label) {
                app.actions.push(Action::ToggleSidebar);
            }
            super::widgets::menu_separator(ui, &palette);
            if super::widgets::menu_item(ui, &palette, Some(Icon::LogOut), "Sign out") {
                app.actions.push(Action::SignOut);
            }
        });
}

fn update_notice(
    ui: &mut egui::Ui,
    palette: &Palette,
    update: &crate::updates::Release,
    actions: &mut Vec<Action>,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(UPDATE_WIDTH, 30.0), Sense::click());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(14),
        palette.accent.gamma_multiply(0.16),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Update available",
        theme::medium(12.5),
        palette.accent,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Update available")
    });
    if response.clicked() {
        actions.push(Action::OpenUrl(update.url.clone()));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn source_chip(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    max_width: f32,
    actions: &mut Vec<Action>,
) {
    let text_max = (max_width - 28.0).max(8.0);
    let mut job = LayoutJob::single_section(
        label.to_string(),
        TextFormat {
            font_id: theme::medium(12.5),
            color: palette.accent,
            ..Default::default()
        },
    );
    job.wrap.max_width = text_max;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
    let size = vec2(
        (galley.size().x + 28.0).min(max_width),
        galley.size().y + 12.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(14),
        palette.accent.gamma_multiply(0.16),
    );
    let icon_rect =
        egui::Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(13.0));
    Icon::Speaker
        .image(palette.accent, 13.0)
        .paint_at(ui, icon_rect);
    ui.painter().galley(
        pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.accent,
    );
    if response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        actions.push(Action::ToggleDevicesPopup);
    }
}

fn profile_source_brief(app: &App) -> Option<String> {
    if let Some(now) = app.now_playing() {
        if !now.local {
            return None;
        }
        if let Some(label) = now.source_label.as_deref() {
            return Some(alternate_source_brief(label));
        }
    }
    let alternate = app.settings.playback_backend == PlaybackBackend::Alternate
        || matches!(app.local_playback, LocalPlayback::AlternateReady { .. });
    alternate.then_some("yt-dlp".to_string())
}

fn alternate_source_brief(label: &str) -> String {
    let lower = label.to_ascii_lowercase();
    if lower.contains("yt-dlp") {
        "yt-dlp".into()
    } else if lower.contains("piped") {
        "Piped".into()
    } else if lower.contains("youtube") {
        "YouTube".into()
    } else {
        "alternate audio".into()
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_global_search_preserves_the_page_and_focus_until_typing() {
        use crate::app::AppOptions;
        use crate::paths::AppDirs;
        use crate::settings::Settings;

        let root =
            std::env::temp_dir().join(format!("oxidify-clear-search-{}", std::process::id()));
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
        app.open(Page::Home);
        app.search.query = "Bonobo".into();
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let frame = |app: &mut App, events| {
            app.actions.clear();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(1280.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| show(app, ui),
            );
            output.textures_delta.clear();
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        let field = egui::Id::new("global-search");
        let field_rect = ctx.read_response(field).unwrap().rect;
        // The Clear control is immediately beside the text field, inside
        // the rounded search pill.
        let clear = pos2(field_rect.right() + 13.0, field_rect.center().y);
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(clear),
                    egui::Event::PointerButton {
                        pos: clear,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            assert!(app.actions.is_empty(), "clearing must not navigate");
        }
        assert!(app.search.query.is_empty());
        for _ in 0..3 {
            frame(&mut app, vec![]);
            assert!(
                app.actions.is_empty(),
                "focus after clearing must not navigate"
            );
        }
        assert!(matches!(app.page(), Page::Home));
        assert!(ctx.memory(|memory| memory.has_focus(field)));
        frame(&mut app, vec![egui::Event::Text("Rework".into())]);
        assert_eq!(app.search.query, "Rework");
        assert!(matches!(
            app.actions.as_slice(),
            [Action::Open(Page::Search)]
        ));
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn home_and_search_sit_in_the_middle_of_a_wide_bar() {
        use crate::app::AppOptions;
        use crate::paths::AppDirs;
        use crate::settings::Settings;

        let root = std::env::temp_dir().join(format!("oxidify-topbar-mid-{}", std::process::id()));
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
        app.open(Page::Home);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let width = 1400.0;
        for _ in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(width, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| show(&mut app, ui),
            );
            output.textures_delta.clear();
        }
        // The cluster (Home button, gap, search pill) is centred on the
        // window, and the pill has its full width.
        let cluster = ctx
            .data(|data| data.get_temp::<Rect>(egui::Id::new(CLUSTER_RECT_ID)))
            .expect("the bar records where Home and search sit");
        let cluster_center = cluster.center().x;
        assert!(
            (cluster_center - width / 2.0).abs() < 1.0,
            "cluster centre {cluster_center} is off the window centre"
        );
        assert!((cluster.width() - HOME_SIZE - 8.0 - SEARCH_MAX).abs() < 0.5);
        let field = ctx
            .read_response(egui::Id::new("global-search"))
            .unwrap()
            .rect;
        assert!(cluster.contains_rect(field));
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn update_notice_opens_the_release_page_only_when_clicked() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let release = crate::updates::Release {
            version: "1.2.3".into(),
            url: "https://github.com/Master0fFate/oxidify/releases/tag/v1.2.3".into(),
        };
        let mut actions = Vec::new();
        let mut rect = egui::Rect::NOTHING;
        let mut frame = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    rect = update_notice(ui, &Palette::dark(), &release, &mut actions).rect;
                },
            );
            output.textures_delta.clear();
            rect.center()
        };
        let pos = frame(Vec::new());
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        assert!(matches!(actions.as_slice(), [Action::OpenUrl(url)] if url == &release.url));
    }

    #[test]
    fn search_yields_before_the_navigation_and_account_clusters() {
        for total in [500.0, 760.0, 1000.0, 1600.0] {
            for (spinner, update, remote) in [(false, false, false), (true, true, true)] {
                let nav = topbar_nav_width(true);
                let right = topbar_right_reserved(spinner, update, remote);
                let room = total - nav - right - 2.0 * CLUSTER_GAP;
                let search = topbar_search_width(room);
                if room >= HOME_SIZE + 8.0 {
                    assert!(
                        nav + CLUSTER_GAP + HOME_SIZE + 8.0 + search + CLUSTER_GAP + right
                            <= total + 0.5,
                        "at {total} the clusters overlap: search={search} right={right}"
                    );
                } else {
                    // Nothing fits between the clusters: the field gives
                    // up its width entirely rather than going negative.
                    assert_eq!(search, 0.0);
                }
                assert!(search <= SEARCH_MAX);
            }
        }
        // A wide window gets the full field.
        let room = 1600.0 - topbar_nav_width(false) - topbar_right_reserved(false, false, false);
        assert_eq!(topbar_search_width(room), SEARCH_MAX);
        // A narrow one shrinks the field instead of the account.
        assert!(topbar_search_width(300.0) < 200.0);
        assert!(topbar_right_reserved(false, false, false) >= AVATAR_SIZE);
        assert!(
            topbar_right_reserved(true, true, true) > topbar_right_reserved(false, false, false)
        );
    }

    #[test]
    fn alternate_source_brief_names_the_real_route() {
        assert_eq!(
            alternate_source_brief("yt-dlp YouTube match · not Spotify audio"),
            "yt-dlp"
        );
        assert_eq!(
            alternate_source_brief("Piped match · not Spotify audio"),
            "Piped"
        );
    }
}
