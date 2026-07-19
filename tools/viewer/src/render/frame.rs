use egui::{CornerRadius, Pos2, Rect, Stroke, StrokeKind};

use crate::render::layout::Layout;
use crate::render::theme;

/// Draw the diagram frame with a tab label.
///
/// All dimensions are scaled by `zoom` to stay consistent with the
/// zoom-scaled layout.
pub fn draw_frame(
    painter: &egui::Painter,
    layout: &Layout,
    label: &str,
    font_id: &egui::FontId,
    zoom: f32,
) {
    // Use content_bounds (Principle 1): frame encompasses elements, ports, AND
    // connector waypoints. This guarantees detoured routes are never outside
    // or flush against the frame border.
    let content = match layout.content_bounds() {
        Some(r) => r,
        None => return,
    };
    let min_x = content.min.x;
    let min_y = content.min.y;
    let max_x = content.max.x;
    let max_y = content.max.y;

    let frame_padding = theme::FRAME_PADDING * zoom;
    let tab_height = theme::FRAME_TAB_HEIGHT * zoom;
    let text_padding = theme::ELEMENT_PADDING * zoom;
    let stroke_width = theme::FRAME_STROKE_WIDTH * zoom;

    // Measure tab label width deterministically
    let tab_text_width = painter.ctx().fonts_mut(|f| {
        f.layout_no_wrap(label.to_string(), font_id.clone(), egui::Color32::BLACK)
            .rect
            .width()
    });
    let tab_content_width = tab_text_width + text_padding * 2.0;

    // Add padding around elements
    let frame_rect = Rect::from_min_max(
        Pos2::new(min_x - frame_padding, min_y - frame_padding - tab_height),
        Pos2::new(max_x + frame_padding, max_y + frame_padding),
    );

    let stroke = Stroke::new(stroke_width, theme::COLOR_FRAME_STROKE);
    painter.rect_stroke(frame_rect, CornerRadius::ZERO, stroke, StrokeKind::Outside);

    // Tab in top-left corner: sized to fit the label text
    let tab_width = tab_content_width.min(frame_rect.width() * theme::FRAME_TAB_MAX_FRACTION);
    let tab_rect = Rect::from_min_size(frame_rect.min, egui::vec2(tab_width, tab_height));
    painter.rect(
        tab_rect,
        CornerRadius::ZERO,
        theme::COLOR_FRAME_TAB,
        stroke,
        StrokeKind::Outside,
    );

    // Clip tab label text to the tab rectangle
    let tab_clip = painter.clip_rect().intersect(tab_rect);
    let clipped = egui::Painter::new(painter.ctx().clone(), painter.layer_id(), tab_clip);

    let text_pos = Pos2::new(tab_rect.min.x + text_padding, tab_rect.center().y);
    clipped.text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        label,
        font_id.clone(),
        theme::COLOR_FRAME_STROKE,
    );
}
