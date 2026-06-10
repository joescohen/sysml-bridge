use egui::{Pos2, Stroke};

use crate::model::{Model, RelationshipKind};
use crate::render::layout::{ConnectorRoute, Layout};
use crate::render::theme;

/// Draw a connector (relationship line) along its route.
pub fn draw_connector(
    painter: &egui::Painter,
    model: &Model,
    route: &ConnectorRoute,
    _layout: &Layout,
    zoom: f32,
) {
    if route.points.len() < 2 {
        return;
    }

    let rel = match model.relationships.get(route.rel_index) {
        Some(r) => r,
        None => return,
    };

    let (color, dashed) = match rel.kind {
        RelationshipKind::Connect | RelationshipKind::Interface => (theme::COLOR_CONNECTOR, false),
        RelationshipKind::Bind => (theme::COLOR_CONNECTOR_BIND, true),
        RelationshipKind::Flow => (theme::COLOR_CONNECTOR_FLOW, false),
        RelationshipKind::Allocate => (theme::COLOR_CONNECTOR, true),
        _ => (theme::COLOR_CONNECTOR, false),
    };

    let stroke = Stroke::new(theme::CONNECTOR_LINE_WIDTH * zoom, color);

    if dashed {
        draw_dashed_polyline(painter, &route.points, stroke, zoom);
    } else {
        for window in route.points.windows(2) {
            painter.line_segment([window[0], window[1]], stroke);
        }
    }

    // Draw arrowhead for flows
    if rel.kind == RelationshipKind::Flow
        && let (Some(&prev), Some(&tip)) = (
            route
                .points
                .len()
                .checked_sub(2)
                .and_then(|i| route.points.get(i)),
            route.points.last(),
        ) {
            draw_arrowhead(painter, prev, tip, color, zoom);
        }
}

/// Draw all connectors in the layout.
pub fn draw_all_connectors(painter: &egui::Painter, model: &Model, layout: &Layout, zoom: f32) {
    for route in &layout.connector_routes {
        draw_connector(painter, model, route, layout, zoom);
    }
}

/// Draw a dashed polyline.
fn draw_dashed_polyline(painter: &egui::Painter, points: &[Pos2], stroke: Stroke, zoom: f32) {
    for window in points.windows(2) {
        draw_dashed_line(painter, window[0], window[1], stroke, zoom);
    }
}

fn draw_dashed_line(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke, zoom: f32) {
    let dir = to - from;
    let len = dir.length();
    if len < 0.1 {
        return;
    }
    let unit = dir / len;
    let dash = theme::CONNECTOR_DASH_LENGTH * zoom;
    let gap = theme::CONNECTOR_DASH_GAP * zoom;
    let cycle = dash + gap;
    let mut t = 0.0;
    while t < len {
        let seg_end = (t + dash).min(len);
        let p0 = from + unit * t;
        let p1 = from + unit * seg_end;
        painter.line_segment([p0, p1], stroke);
        t += cycle;
    }
}

/// Draw a filled arrowhead.
fn draw_arrowhead(painter: &egui::Painter, from: Pos2, tip: Pos2, color: egui::Color32, zoom: f32) {
    let dir = tip - from;
    let len = dir.length();
    if len < 0.1 {
        return;
    }
    let unit = dir / len;
    let perp = egui::vec2(-unit.y, unit.x);

    let arrow_len = theme::ARROWHEAD_LENGTH * zoom;
    let arrow_width = theme::ARROWHEAD_WIDTH * zoom;

    let base = tip - unit * arrow_len;
    let left = base + perp * arrow_width * 0.5;
    let right = base - perp * arrow_width * 0.5;

    painter.add(egui::Shape::convex_polygon(
        vec![tip, left, right],
        color,
        Stroke::NONE,
    ));
}
