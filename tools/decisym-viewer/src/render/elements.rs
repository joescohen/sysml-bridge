use egui::{Color32, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Vec2};

use crate::model::{ElementId, ElementKind, Metatype, Model};
use crate::render::layout::Layout;
use crate::render::theme;

/// Draw an element (definition or usage) at its laid-out position.
///
/// All internal positioning is scaled by `zoom` so that the drawing
/// is consistent with the zoom-scaled layout sizes.
pub fn draw_element(
    painter: &egui::Painter,
    model: &Model,
    id: ElementId,
    layout: &Layout,
    selected: Option<ElementId>,
    font_id: &egui::FontId,
    zoom: f32,
) {
    let elem = match model.element(id) {
        Some(e) => e,
        None => return,
    };
    let pos = match layout.positions.get(&id) {
        Some(p) => *p,
        None => return,
    };
    let size = layout.sizes.get(&id).copied().unwrap_or(Vec2::new(
        theme::ELEMENT_MIN_WIDTH,
        theme::ELEMENT_MIN_HEIGHT,
    ));

    let rect = Rect::from_min_size(pos, size);
    let is_selected = selected == Some(id);

    // Corner radius: sharp for definitions, rounded for usages (scaled)
    let radius_f = if elem.kind.metatype() == Metatype::Definition {
        theme::DEFINITION_CORNER_RADIUS
    } else {
        theme::USAGE_CORNER_RADIUS
    } * zoom;
    let corner_radius = CornerRadius::same(radius_f as u8);

    let stroke_color = if is_selected {
        theme::COLOR_ELEMENT_SELECTED
    } else {
        theme::COLOR_ELEMENT_STROKE
    };
    let stroke_width = if is_selected {
        theme::ELEMENT_STROKE_WIDTH * theme::SELECTED_STROKE_MULTIPLIER
    } else {
        theme::ELEMENT_STROKE_WIDTH
    } * zoom;

    // Fill and stroke
    painter.rect(
        rect,
        corner_radius,
        theme::COLOR_ELEMENT_FILL,
        Stroke::new(stroke_width, stroke_color),
        StrokeKind::Outside,
    );

    // Clip all text to the element rectangle so nothing overflows.
    let clip = painter.clip_rect().intersect(rect);
    let clipped = egui::Painter::new(painter.ctx().clone(), painter.layer_id(), clip);

    // Scaled positioning constants
    let padding = theme::ELEMENT_PADDING * zoom;
    let header_height = theme::ELEMENT_HEADER_HEIGHT * zoom;
    let feature_row_height = theme::ELEMENT_FEATURE_ROW_HEIGHT * zoom;
    let keyword_name_gap = theme::KEYWORD_NAME_GAP * zoom;

    // Header: keyword + name
    let keyword = elem.kind.keyword();
    let name = elem.display_name();
    let type_str = elem
        .type_ref
        .as_deref()
        .map(|t| format!(": {t}"))
        .unwrap_or_default();

    let small_font = egui::FontId::proportional(font_id.size * theme::KEYWORD_FONT_SCALE);

    // Keyword line (small, gray)
    let keyword_pos = Pos2::new(pos.x + padding, pos.y + padding);
    clipped.text(
        keyword_pos,
        egui::Align2::LEFT_TOP,
        format!("<<{keyword}>>"),
        small_font,
        theme::COLOR_KEYWORD_TEXT,
    );

    // Name line
    let name_pos = Pos2::new(pos.x + padding, pos.y + padding + keyword_name_gap);
    clipped.text(
        name_pos,
        egui::Align2::LEFT_TOP,
        format!("{name}{type_str}"),
        font_id.clone(),
        theme::COLOR_HEADER_TEXT,
    );

    // Compartment separator
    let sep_y = pos.y + header_height;
    let features = visible_features(model, id);
    // Use effective ports (includes inherited from type definition)
    let ports = model.effective_ports(id);

    if !features.is_empty() || !ports.is_empty() {
        painter.line_segment(
            [Pos2::new(pos.x, sep_y), Pos2::new(pos.x + size.x, sep_y)],
            Stroke::new(theme::COMPARTMENT_LINE_WIDTH * zoom, stroke_color),
        );

        let feature_font = egui::FontId::proportional(font_id.size * theme::FEATURE_FONT_SCALE);
        let mut y = sep_y + theme::COMPARTMENT_TEXT_INSET * zoom;

        // Draw ports in feature compartment
        for &pid in &ports {
            if let Some(p) = model.element(pid) {
                let conj = if p.is_conjugated { "~" } else { "" };
                let tref = p
                    .type_ref
                    .as_deref()
                    .map(|t| format!(": {conj}{t}"))
                    .unwrap_or_default();
                let label = format!("port {}{}", p.display_name(), tref);
                clipped.text(
                    Pos2::new(pos.x + padding, y),
                    egui::Align2::LEFT_TOP,
                    label,
                    feature_font.clone(),
                    theme::COLOR_FEATURE_TEXT,
                );
                y += feature_row_height;
            }
        }

        // Draw other features
        for &fid in &features {
            if let Some(f) = model.element(fid) {
                let tref = f
                    .type_ref
                    .as_deref()
                    .map(|t| format!(": {t}"))
                    .unwrap_or_default();
                let label = format!("{} {}{}", f.kind.keyword(), f.display_name(), tref);
                clipped.text(
                    Pos2::new(pos.x + padding, y),
                    egui::Align2::LEFT_TOP,
                    label,
                    feature_font.clone(),
                    theme::COLOR_FEATURE_TEXT,
                );
                y += feature_row_height;
            }
        }
    }
}

/// Get visible features (limited set for display).
fn visible_features(model: &Model, id: ElementId) -> Vec<ElementId> {
    let elem = match model.element(id) {
        Some(e) => e,
        None => return Vec::new(),
    };
    elem.features(model)
        .into_iter()
        .filter(|&fid| {
            model.element(fid).is_some_and(|e| {
                matches!(
                    e.kind,
                    crate::model::ElementKind::AttributeUsage
                        | crate::model::ElementKind::PartUsage
                        | crate::model::ElementKind::ActionUsage
                        | crate::model::ElementKind::StateUsage
                )
            })
        })
        .take(8)
        .collect()
}

/// Hit test: does the click position fall within this element?
pub fn hit_test(layout: &Layout, id: ElementId, click_pos: Pos2) -> bool {
    let pos = match layout.positions.get(&id) {
        Some(p) => *p,
        None => return false,
    };
    let size = layout.sizes.get(&id).copied().unwrap_or(Vec2::new(
        theme::ELEMENT_MIN_WIDTH,
        theme::ELEMENT_MIN_HEIGHT,
    ));
    Rect::from_min_size(pos, size).contains(click_pos)
}

/// Level-of-detail rendering for overview/zoom contexts.
/// Selects rendering based on element's screen-space height.
#[allow(clippy::too_many_arguments)]
pub fn draw_element_lod(
    painter: &egui::Painter,
    model: &Model,
    id: ElementId,
    layout: &Layout,
    selected: Option<ElementId>,
    font_id: &egui::FontId,
    zoom: f32,
    opacity: u8,
) {
    let size = match layout.sizes.get(&id).copied() {
        Some(s) => s,
        None => return,
    };
    let screen_height = size.y; // already zoom-scaled when used in canvas

    if opacity < 10 {
        return; // effectively invisible
    }

    match screen_height {
        h if h < theme::LOD_DOT_THRESHOLD => {
            draw_dot(painter, model, id, layout, selected, opacity)
        }
        h if h < theme::LOD_LABEL_THRESHOLD => {
            draw_label(painter, model, id, layout, selected, font_id, opacity)
        }
        h if h < theme::LOD_BOX_THRESHOLD => {
            draw_element_compact(painter, model, id, layout, selected, font_id, zoom, opacity)
        }
        _ => {
            if opacity == 255 {
                draw_element(painter, model, id, layout, selected, font_id, zoom);
            } else {
                draw_element_with_opacity(
                    painter, model, id, layout, selected, font_id, zoom, opacity,
                );
            }
        }
    }
}

/// Render element as a small colored dot.
fn draw_dot(
    painter: &egui::Painter,
    model: &Model,
    id: ElementId,
    layout: &Layout,
    selected: Option<ElementId>,
    opacity: u8,
) {
    let elem = match model.element(id) {
        Some(e) => e,
        None => return,
    };
    let pos = match layout.positions.get(&id) {
        Some(p) => *p,
        None => return,
    };
    let size = layout
        .sizes
        .get(&id)
        .copied()
        .unwrap_or(Vec2::splat(theme::PORT_SIZE));
    let center = Pos2::new(pos.x + size.x / 2.0, pos.y + size.y / 2.0);
    let radius = size.x.min(size.y) / 2.0;

    let base_color = if elem.kind.metatype() == Metatype::Definition {
        theme::COLOR_DEF_DOT
    } else {
        theme::COLOR_USAGE_DOT
    };
    let color = apply_opacity(base_color, opacity);

    painter.circle_filled(center, radius, color);
    if selected == Some(id) {
        painter.circle_stroke(
            center,
            radius + 1.0,
            Stroke::new(theme::SELECTED_DOT_STROKE, theme::COLOR_ELEMENT_SELECTED),
        );
    }
}

/// Render element as a labeled rectangle (name + kind icon).
fn draw_label(
    painter: &egui::Painter,
    model: &Model,
    id: ElementId,
    layout: &Layout,
    selected: Option<ElementId>,
    font_id: &egui::FontId,
    opacity: u8,
) {
    let elem = match model.element(id) {
        Some(e) => e,
        None => return,
    };
    let pos = match layout.positions.get(&id) {
        Some(p) => *p,
        None => return,
    };
    let size = layout.sizes.get(&id).copied().unwrap_or(Vec2::new(
        theme::ELEMENT_MIN_HEIGHT,
        theme::LOD_DOT_THRESHOLD,
    ));
    let rect = Rect::from_min_size(pos, size);

    // Spec: definitions = sharp corners, usages = rounded corners.
    // At label LOD, scale USAGE_CORNER_RADIUS proportionally to element height.
    let corner_radius = if elem.kind.metatype() == Metatype::Definition {
        CornerRadius::ZERO
    } else {
        let r = (theme::USAGE_CORNER_RADIUS * size.y / theme::LOD_LABEL_THRESHOLD)
            .min(theme::USAGE_CORNER_RADIUS);
        CornerRadius::same(r as u8)
    };

    let fill = apply_opacity(theme::COLOR_ELEMENT_FILL, opacity);
    let stroke_color = if selected == Some(id) {
        theme::COLOR_ELEMENT_SELECTED
    } else {
        apply_opacity(theme::COLOR_ELEMENT_STROKE, opacity)
    };

    // Note: draw_label does not receive zoom directly — the layout is already
    // zoom-scaled when this LOD path runs. The font_id.size already incorporates
    // zoom. Derive stroke scale from element height ratio to base LOD threshold.
    let stroke_scale = size.y / theme::LOD_LABEL_THRESHOLD;
    painter.rect(
        rect,
        corner_radius,
        fill,
        Stroke::new(theme::ELEMENT_STROKE_WIDTH * stroke_scale, stroke_color),
        StrokeKind::Outside,
    );

    // Label: element name, clipped to rect
    let clip = painter.clip_rect().intersect(rect);
    let clipped = egui::Painter::new(painter.ctx().clone(), painter.layer_id(), clip);
    let small_font = egui::FontId::proportional(font_id.size * theme::LABEL_FONT_SCALE);
    let text_color = apply_opacity(theme::COLOR_HEADER_TEXT, opacity);
    clipped.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        elem.display_name(),
        small_font,
        text_color,
    );
}

/// Render element as compact SysML box (keyword header + name, no compartments).
#[allow(clippy::too_many_arguments)]
fn draw_element_compact(
    painter: &egui::Painter,
    model: &Model,
    id: ElementId,
    layout: &Layout,
    selected: Option<ElementId>,
    font_id: &egui::FontId,
    zoom: f32,
    opacity: u8,
) {
    let elem = match model.element(id) {
        Some(e) => e,
        None => return,
    };
    let pos = match layout.positions.get(&id) {
        Some(p) => *p,
        None => return,
    };
    let size = layout.sizes.get(&id).copied().unwrap_or(Vec2::new(
        theme::ELEMENT_MIN_WIDTH,
        theme::ELEMENT_MIN_HEIGHT,
    ));
    let rect = Rect::from_min_size(pos, size);

    let radius_f = if elem.kind.metatype() == Metatype::Definition {
        theme::DEFINITION_CORNER_RADIUS
    } else {
        theme::USAGE_CORNER_RADIUS
    } * zoom;
    let corner_radius = CornerRadius::same(radius_f as u8);

    let fill = apply_opacity(theme::COLOR_ELEMENT_FILL, opacity);
    let stroke_color = if selected == Some(id) {
        theme::COLOR_ELEMENT_SELECTED
    } else {
        apply_opacity(theme::COLOR_ELEMENT_STROKE, opacity)
    };
    let stroke_width = if selected == Some(id) {
        theme::ELEMENT_STROKE_WIDTH * theme::SELECTED_STROKE_MULTIPLIER
    } else {
        theme::ELEMENT_STROKE_WIDTH
    } * zoom;

    painter.rect(
        rect,
        corner_radius,
        fill,
        Stroke::new(stroke_width, stroke_color),
        StrokeKind::Outside,
    );

    let clip = painter.clip_rect().intersect(rect);
    let clipped = egui::Painter::new(painter.ctx().clone(), painter.layer_id(), clip);

    let padding = theme::ELEMENT_PADDING * zoom;
    let small_font = egui::FontId::proportional(font_id.size * theme::KEYWORD_FONT_SCALE);
    let keyword_color = apply_opacity(theme::COLOR_KEYWORD_TEXT, opacity);
    let header_color = apply_opacity(theme::COLOR_HEADER_TEXT, opacity);

    // Keyword line
    let keyword_pos = Pos2::new(pos.x + padding, pos.y + padding);
    clipped.text(
        keyword_pos,
        egui::Align2::LEFT_TOP,
        format!("<<{}>>", elem.kind.keyword()),
        small_font,
        keyword_color,
    );

    // Name line
    let name = elem.display_name();
    let type_str = elem
        .type_ref
        .as_deref()
        .map(|t| format!(": {t}"))
        .unwrap_or_default();
    let name_pos = Pos2::new(
        pos.x + padding,
        pos.y + padding + theme::KEYWORD_NAME_GAP * zoom,
    );
    clipped.text(
        name_pos,
        egui::Align2::LEFT_TOP,
        format!("{name}{type_str}"),
        font_id.clone(),
        header_color,
    );
}

/// Draw element with opacity applied (for filtered state).
#[allow(clippy::too_many_arguments)]
fn draw_element_with_opacity(
    painter: &egui::Painter,
    model: &Model,
    id: ElementId,
    layout: &Layout,
    selected: Option<ElementId>,
    font_id: &egui::FontId,
    zoom: f32,
    opacity: u8,
) {
    // For simplicity, draw the compact version when dimmed
    draw_element_compact(painter, model, id, layout, selected, font_id, zoom, opacity);
}

/// Apply opacity to a color.
fn apply_opacity(color: Color32, opacity: u8) -> Color32 {
    if opacity == 255 {
        return color;
    }
    let a = (color.a() as u16 * opacity as u16 / 255) as u8;
    Color32::from_rgba_premultiplied(
        (color.r() as u16 * a as u16 / 255) as u8,
        (color.g() as u16 * a as u16 / 255) as u8,
        (color.b() as u16 * a as u16 / 255) as u8,
        a,
    )
}

/// Draw an overview package node (larger, with child count).
#[allow(clippy::too_many_arguments)]
pub fn draw_overview_package(
    painter: &egui::Painter,
    model: &Model,
    id: ElementId,
    layout: &Layout,
    selected: Option<ElementId>,
    font_id: &egui::FontId,
    zoom: f32,
    opacity: u8,
) {
    let elem = match model.element(id) {
        Some(e) => e,
        None => return,
    };
    let pos = match layout.positions.get(&id) {
        Some(p) => *p,
        None => return,
    };
    let size = layout.sizes.get(&id).copied().unwrap_or(Vec2::new(
        theme::OVERVIEW_PACKAGE_MIN_WIDTH,
        theme::OVERVIEW_PACKAGE_MIN_HEIGHT,
    ));
    let rect = Rect::from_min_size(pos, size);

    let is_package = elem.kind == ElementKind::Package;
    // Spec: definitions (including packages) = sharp corners, usages = rounded.
    let corner_radius = if is_package || elem.kind.metatype() == Metatype::Definition {
        CornerRadius::same((theme::DEFINITION_CORNER_RADIUS * zoom) as u8)
    } else {
        CornerRadius::same((theme::USAGE_CORNER_RADIUS * zoom) as u8)
    };

    let fill = apply_opacity(
        if is_package {
            theme::COLOR_PACKAGE_FILL
        } else {
            theme::COLOR_ELEMENT_FILL
        },
        opacity,
    );
    let stroke_color = if selected == Some(id) {
        theme::COLOR_ELEMENT_SELECTED
    } else {
        apply_opacity(
            if is_package {
                theme::COLOR_PACKAGE_STROKE
            } else {
                theme::COLOR_ELEMENT_STROKE
            },
            opacity,
        )
    };
    let stroke_width = if selected == Some(id) {
        theme::ELEMENT_STROKE_WIDTH * theme::SELECTED_STROKE_MULTIPLIER
    } else {
        theme::ELEMENT_STROKE_WIDTH
    } * zoom;

    painter.rect(
        rect,
        corner_radius,
        fill,
        Stroke::new(stroke_width, stroke_color),
        StrokeKind::Outside,
    );

    let clip = painter.clip_rect().intersect(rect);
    let clipped = egui::Painter::new(painter.ctx().clone(), painter.layer_id(), clip);

    let padding = theme::ELEMENT_PADDING * zoom;
    let small_font = egui::FontId::proportional(font_id.size * theme::LABEL_FONT_SCALE);
    let text_color = apply_opacity(theme::COLOR_HEADER_TEXT, opacity);
    let keyword_color = apply_opacity(theme::COLOR_KEYWORD_TEXT, opacity);

    // Keyword
    let keyword_pos = Pos2::new(pos.x + padding, pos.y + padding);
    clipped.text(
        keyword_pos,
        egui::Align2::LEFT_TOP,
        format!("<<{}>>", elem.kind.keyword()),
        small_font.clone(),
        keyword_color,
    );

    // Name
    let name_pos = Pos2::new(
        pos.x + padding,
        pos.y + padding + theme::OVERVIEW_KEYWORD_NAME_GAP * zoom,
    );
    clipped.text(
        name_pos,
        egui::Align2::LEFT_TOP,
        elem.display_name(),
        font_id.clone(),
        text_color,
    );
}
