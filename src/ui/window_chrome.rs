//! Oxidify-owned window chrome for the undecorated Windows window.

use egui::{
    Color32, CornerRadius, CursorIcon, Frame, Id, Margin, Order, PointerButton, Pos2, Rect,
    ResizeDirection, Sense, Stroke, ViewportCommand, pos2, vec2,
};

use crate::app::App;
use crate::theme::{self, Icon, Palette};

const TITLE_HEIGHT: f32 = 40.0;
const CONTROL_WIDTH: f32 = 46.0;
/// The width of every resize hit target, border and corner alike.
const RESIZE_EDGE: f32 = 5.0;
const FIELD: Color32 = Color32::from_rgb(0x91, 0xc4, 0xff);
const INK: Color32 = Color32::from_rgb(0x0d, 0x3a, 0x73);

/// Draws the title bar and installs resize hit targets around the viewport.
pub fn show(app: &App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let palette = app.palette;
    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
    let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));

    if !fullscreen {
        egui::Panel::top("oxidify-window-titlebar")
            .exact_size(TITLE_HEIGHT)
            .frame(Frame::new().fill(palette.panel).inner_margin(Margin::ZERO))
            .show(ui, |ui| title_bar(ui, &palette, maximized));
    }

    if !maximized && !fullscreen {
        resize_handles(&ctx);
    }
}

fn title_bar(ui: &mut egui::Ui, palette: &Palette, maximized: bool) {
    let rect = ui.max_rect();
    let controls_left = rect.right() - CONTROL_WIDTH * 3.0;
    let mark_rect =
        Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(20.0, 20.0));
    paint_mark(ui.painter(), mark_rect);
    ui.painter().text(
        pos2(mark_rect.right() + 9.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Oxidify",
        theme::semibold(13.5),
        palette.text,
    );

    let drag_left = mark_rect.right() + 76.0;
    let drag_rect = Rect::from_min_max(
        pos2(drag_left.min(controls_left), rect.top()),
        pos2(controls_left, rect.bottom()),
    );
    let drag = ui.interact(
        drag_rect,
        Id::new("oxidify-titlebar-drag"),
        Sense::click_and_drag(),
    );
    if drag.double_clicked_by(PointerButton::Primary) {
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if drag.drag_started_by(PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }

    let minimize = control_rect(rect, 0);
    let maximize = control_rect(rect, 1);
    let close = control_rect(rect, 2);
    if chrome_button(ui, minimize, Icon::Minus, palette, "Minimize", false) {
        ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
    }
    let maximize_icon = if maximized {
        Icon::Shrink
    } else {
        Icon::Square
    };
    let maximize_label = if maximized { "Restore" } else { "Maximize" };
    if chrome_button(ui, maximize, maximize_icon, palette, maximize_label, false) {
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }
    if chrome_button(ui, close, Icon::X, palette, "Close", true) {
        // This follows the same close-request path as the native button, so
        // close-to-tray can still cancel it in App::tick.
        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
    }
}

fn control_rect(title: Rect, index: usize) -> Rect {
    let left = title.right() - CONTROL_WIDTH * (3 - index) as f32;
    Rect::from_min_size(pos2(left, title.top()), vec2(CONTROL_WIDTH, TITLE_HEIGHT))
}

fn chrome_button(
    ui: &mut egui::Ui,
    rect: Rect,
    icon: Icon,
    palette: &Palette,
    label: &str,
    destructive: bool,
) -> bool {
    let response = ui
        .interact(
            rect,
            Id::new(("oxidify-window-control", label)),
            Sense::click(),
        )
        .on_hover_text(label);
    // The label doubles as the widget's accessible name; the drawing
    // itself is glyphs, which a screen reader cannot read.
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if response.hovered() {
        let fill = if destructive {
            Color32::from_rgb(0xc4, 0x2b, 0x3b)
        } else {
            palette.surface_hover
        };
        ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
    }
    let color = if destructive && response.hovered() {
        Color32::WHITE
    } else {
        palette.secondary
    };
    theme::paint_icon(ui, icon, rect, 15.0, color);
    response.clicked()
}

fn paint_mark(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, CornerRadius::same(5), FIELD);
    let center = rect.center();
    let radius = rect.width() * 0.33;
    let points: Vec<Pos2> = (0..6)
        .map(|step| {
            let angle = std::f32::consts::TAU * step as f32 / 6.0 - std::f32::consts::FRAC_PI_2;
            center + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    painter.add(egui::Shape::closed_line(points, Stroke::new(1.7, INK)));
    painter.circle_filled(center, rect.width() * 0.055, INK);
}

fn resize_handles(ctx: &egui::Context) {
    let viewport = ctx.viewport_rect();
    // The caption buttons keep their clicks: the bands stop at their
    // strip, and the corner that would sit on it is theirs, the way
    // native Windows gives the close button the top-right corner.
    let controls = Rect::from_min_size(
        pos2(viewport.right() - CONTROL_WIDTH * 3.0, viewport.top()),
        vec2(CONTROL_WIDTH * 3.0, TITLE_HEIGHT),
    );
    for (index, (rect, direction)) in resize_regions(viewport, controls).into_iter().enumerate() {
        // One tiny foreground area per edge keeps the window resizable without
        // putting an invisible full-window interaction layer over the app.
        egui::Area::new(Id::new(("oxidify-window-resize", index)))
            .order(Order::Foreground)
            .fixed_pos(rect.min)
            .interactable(true)
            .show(ctx, |ui| {
                let (_, response) = ui.allocate_exact_size(rect.size(), Sense::drag());
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(cursor_for(direction));
                }
                if response.drag_started_by(PointerButton::Primary) {
                    ui.ctx()
                        .send_viewport_cmd(ViewportCommand::BeginResize(direction));
                }
            });
    }
}

/// The resize hit targets: a 5px frame of edge bands and corner squares
/// covering every [`ResizeDirection`]. `reserved` is the caption-button
/// strip: the top band ends at its left edge, the right band starts below
/// it, and the corner square that would land on it is left out entirely so
/// the buttons answer their own clicks.
fn resize_regions(window: Rect, reserved: Rect) -> Vec<(Rect, ResizeDirection)> {
    let edge = RESIZE_EDGE;
    let min = window.min;
    let max = window.max;
    // An empty reservation leaves the frame whole.
    let keep_left = if reserved.is_positive() {
        reserved.min.x
    } else {
        f32::INFINITY
    };
    let keep_below = if reserved.is_positive() {
        reserved.max.y
    } else {
        f32::NEG_INFINITY
    };
    let mut regions = vec![
        (
            Rect::from_min_max(min, pos2(min.x + edge, min.y + edge)),
            ResizeDirection::NorthWest,
        ),
        (
            Rect::from_min_max(pos2(min.x, max.y - edge), pos2(min.x + edge, max.y)),
            ResizeDirection::SouthWest,
        ),
        (
            Rect::from_min_max(pos2(max.x - edge, max.y - edge), max),
            ResizeDirection::SouthEast,
        ),
        (
            Rect::from_min_max(
                pos2(min.x + edge, min.y),
                pos2(keep_left.min(max.x - edge), min.y + edge),
            ),
            ResizeDirection::North,
        ),
        (
            Rect::from_min_max(pos2(min.x + edge, max.y - edge), pos2(max.x - edge, max.y)),
            ResizeDirection::South,
        ),
        (
            Rect::from_min_max(pos2(min.x, min.y + edge), pos2(min.x + edge, max.y - edge)),
            ResizeDirection::West,
        ),
        (
            Rect::from_min_max(
                pos2(max.x - edge, keep_below.max(min.y + edge)),
                pos2(max.x, max.y - edge),
            ),
            ResizeDirection::East,
        ),
    ];
    let north_east = Rect::from_min_max(pos2(max.x - edge, min.y), pos2(max.x, min.y + edge));
    if !reserved.intersects(north_east) {
        regions.push((north_east, ResizeDirection::NorthEast));
    }
    regions.retain(|(rect, _)| rect.is_positive());
    regions
}

fn cursor_for(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The direction a point grabs, read straight off the hit targets.
    fn direction_at(point: Pos2, regions: &[(Rect, ResizeDirection)]) -> Option<ResizeDirection> {
        regions
            .iter()
            .find(|(rect, _)| rect.contains(point))
            .map(|(_, direction)| *direction)
    }

    /// Where the caption buttons sit in a window.
    fn controls(window: Rect) -> Rect {
        Rect::from_min_size(
            pos2(window.right() - CONTROL_WIDTH * 3.0, window.top()),
            vec2(CONTROL_WIDTH * 3.0, TITLE_HEIGHT),
        )
    }

    #[test]
    fn titlebar_controls_keep_windows_order_and_size() {
        let title = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, TITLE_HEIGHT));
        let minimize = control_rect(title, 0);
        let maximize = control_rect(title, 1);
        let close = control_rect(title, 2);
        assert_eq!(minimize.width(), CONTROL_WIDTH);
        assert_eq!(minimize.height(), TITLE_HEIGHT);
        assert_eq!(maximize.left(), minimize.right());
        assert_eq!(close.left(), maximize.right());
        assert_eq!(close.right(), title.right());
        assert_eq!(close.top(), title.top());
    }

    #[test]
    fn resize_directions_at_corners_edges_and_interior() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let regions = resize_regions(window, controls(window));
        let grab = |x: f32, y: f32| direction_at(pos2(x, y), &regions);
        // Corners pick the diagonals they sit on.
        assert_eq!(grab(2.0, 2.0), Some(ResizeDirection::NorthWest));
        assert_eq!(grab(2.0, 598.0), Some(ResizeDirection::SouthWest));
        assert_eq!(grab(798.0, 598.0), Some(ResizeDirection::SouthEast));
        // Edge midpoints pick their own side.
        assert_eq!(grab(400.0, 2.0), Some(ResizeDirection::North));
        assert_eq!(grab(400.0, 598.0), Some(ResizeDirection::South));
        assert_eq!(grab(2.0, 300.0), Some(ResizeDirection::West));
        assert_eq!(grab(798.0, 300.0), Some(ResizeDirection::East));
        // Past the frame, the window belongs to the app.
        assert_eq!(grab(400.0, 300.0), None);
        assert_eq!(grab(6.0, 6.0), None);
        assert_eq!(grab(794.0, 300.0), None);
    }

    #[test]
    fn resize_never_takes_the_caption_buttons() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let reserved = controls(window);
        let regions = resize_regions(window, reserved);
        for (rect, _) in &regions {
            assert!(
                !rect.intersect(reserved).is_positive(),
                "{rect:?} covers the buttons"
            );
        }
        // No resize answers inside the button strip, corner and edges
        // included, so the controls answer their own clicks.
        for point in [
            pos2(799.0, 1.0),
            pos2(799.0, 20.0),
            pos2(700.0, 2.0),
            pos2(700.0, 39.0),
        ] {
            assert_eq!(direction_at(point, &regions), None, "at {point:?}");
        }
    }

    #[test]
    fn every_direction_has_its_cursor() {
        for direction in [
            ResizeDirection::North,
            ResizeDirection::South,
            ResizeDirection::East,
            ResizeDirection::West,
            ResizeDirection::NorthEast,
            ResizeDirection::SouthEast,
            ResizeDirection::NorthWest,
            ResizeDirection::SouthWest,
        ] {
            assert_eq!(
                cursor_for(direction),
                match direction {
                    ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
                    ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
                    ResizeDirection::NorthWest | ResizeDirection::SouthEast =>
                        CursorIcon::ResizeNwSe,
                    ResizeDirection::NorthEast | ResizeDirection::SouthWest =>
                        CursorIcon::ResizeNeSw,
                }
            );
        }
        // With nothing reserved, all eight directions get a target, the
        // north-east corner included.
        let window = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let regions = resize_regions(window, Rect::NOTHING);
        assert_eq!(regions.len(), 8);
        assert_eq!(
            direction_at(pos2(798.0, 2.0), &regions),
            Some(ResizeDirection::NorthEast)
        );
    }
}
