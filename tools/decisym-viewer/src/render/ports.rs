use egui::{CornerRadius, Rect, Stroke, StrokeKind};

use crate::model::{ElementId, Model};
use crate::render::layout::Layout;
use crate::render::theme;

/// Draw a port as a small rectangle straddling the parent element boundary.
pub fn draw_port(
    painter: &egui::Painter,
    model: &Model,
    id: ElementId,
    layout: &Layout,
    zoom: f32,
) {
    let elem = match model.element(id) {
        Some(e) => e,
        None => return,
    };
    let center = match layout.port_positions.get(&id) {
        Some(p) => *p,
        None => return,
    };

    let port_size = theme::PORT_SIZE * zoom;
    let rect = Rect::from_center_size(center, egui::vec2(port_size, port_size));

    let fill = if elem.is_conjugated {
        theme::COLOR_PORT_CONJUGATED
    } else {
        theme::COLOR_PORT_FILL
    };

    painter.rect(
        rect,
        CornerRadius::ZERO,
        fill,
        Stroke::new(theme::PORT_STROKE_WIDTH * zoom, theme::COLOR_PORT_STROKE),
        StrokeKind::Outside,
    );

    // Draw conjugation mark (~) inside conjugated ports
    if elem.is_conjugated {
        let font =
            egui::FontId::proportional(theme::PORT_HALF * theme::CONJUGATION_MARK_SCALE * zoom);
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            "~",
            font,
            theme::COLOR_PORT_STROKE,
        );
    }
}

/// Draw all ports for elements in the layout.
pub fn draw_all_ports(painter: &egui::Painter, model: &Model, layout: &Layout, zoom: f32) {
    for &port_id in layout.port_positions.keys() {
        draw_port(painter, model, port_id, layout, zoom);
    }
}
