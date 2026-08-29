//! Oxidify-owned window chrome for the undecorated Windows window.

use egui::{
    Color32, CornerRadius, CursorIcon, Frame, Id, Margin, Order, PointerButton, Pos2, Rect,
    ResizeDirection, Sense, Stroke, Vec2, ViewportCommand, pos2, vec2,
};

use crate::app::App;
use crate::theme::{self, Icon, Palette};

const TITLE_HEIGHT: f32 = 40.0;
const CONTROL_WIDTH: f32 = 46.0;
const RESIZE_EDGE: f32 = 5.0;
const RESIZE_CORNER: f32 = 10.0;
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
    for (index, (rect, direction)) in resize_regions(viewport).into_iter().enumerate() {
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

fn resize_regions(rect: Rect) -> [(Rect, ResizeDirection); 8] {
    let top_left = Rect::from_min_size(rect.min, Vec2::splat(RESIZE_CORNER));
    let top_right = Rect::from_min_size(
        pos2(rect.right() - RESIZE_CORNER, rect.top()),
        Vec2::splat(RESIZE_CORNER),
    );
    let bottom_left = Rect::from_min_size(
        pos2(rect.left(), rect.bottom() - RESIZE_CORNER),
        Vec2::splat(RESIZE_CORNER),
    );
    let bottom_right = Rect::from_min_size(
        pos2(rect.right() - RESIZE_CORNER, rect.bottom() - RESIZE_CORNER),
        Vec2::splat(RESIZE_CORNER),
    );
    let horizontal = (rect.width() - RESIZE_CORNER * 2.0).max(0.0);
    let vertical = (rect.height() - RESIZE_CORNER * 2.0).max(0.0);
    [
        (top_left, ResizeDirection::NorthWest),
        (top_right, ResizeDirection::NorthEast),
        (bottom_left, ResizeDirection::SouthWest),
        (bottom_right, ResizeDirection::SouthEast),
        (
            Rect::from_min_size(
                pos2(rect.left() + RESIZE_CORNER, rect.top()),
                vec2(horizontal, RESIZE_EDGE),
            ),
            ResizeDirection::North,
        ),
        (
            Rect::from_min_size(
                pos2(rect.left() + RESIZE_CORNER, rect.bottom() - RESIZE_EDGE),
                vec2(horizontal, RESIZE_EDGE),
            ),
            ResizeDirection::South,
        ),
        (
            Rect::from_min_size(
                pos2(rect.left(), rect.top() + RESIZE_CORNER),
                vec2(RESIZE_EDGE, vertical),
            ),
            ResizeDirection::West,
        ),
        (
            Rect::from_min_size(
                pos2(rect.right() - RESIZE_EDGE, rect.top() + RESIZE_CORNER),
                vec2(RESIZE_EDGE, vertical),
            ),
            ResizeDirection::East,
        ),
    ]
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

    #[test]
    fn titlebar_controls_keep_windows_order_and_size() {
        let title = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, TITLE_HEIGHT));
        let minimize = control_rect(title, 0);
        let maximize = control_rect(title, 1);
        let close = control_rect(title, 2);
        assert_eq!(minimize.width(), CONTROL_WIDTH);
        assert_eq!(maximize.left(), minimize.right());
        assert_eq!(close.left(), maximize.right());
        assert_eq!(close.right(), title.right());
    }

    #[test]
    fn resize_regions_cover_each_direction_without_owning_the_interior() {
        let viewport = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let regions = resize_regions(viewport);
        assert_eq!(regions.len(), 8);
        for (_, direction) in regions {
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
        assert!(
            resize_regions(viewport)
                .iter()
                .all(|(region, _)| !region.contains(viewport.center()))
        );
    }
}
