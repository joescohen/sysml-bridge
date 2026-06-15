#![allow(dead_code)]
use egui::Color32;

// --- Corner radii ---
/// Definitions get sharp corners.
pub const DEFINITION_CORNER_RADIUS: f32 = 0.0;
/// Usages get rounded corners.
pub const USAGE_CORNER_RADIUS: f32 = 6.0;

// --- Port sizes ---
pub const PORT_SIZE: f32 = 12.0;
pub const PORT_HALF: f32 = PORT_SIZE / 2.0;

// --- Line widths ---
pub const COMPARTMENT_LINE_WIDTH: f32 = 1.0;
pub const ELEMENT_STROKE_WIDTH: f32 = 1.5;
/// Port border stroke width (spec: visible border around port square).
pub const PORT_STROKE_WIDTH: f32 = 1.0;
pub const CONNECTOR_LINE_WIDTH: f32 = 1.5;
pub const CONNECTOR_DASH_LENGTH: f32 = 6.0;
pub const CONNECTOR_DASH_GAP: f32 = 4.0;
pub const FRAME_STROKE_WIDTH: f32 = 2.0;

// --- Element sizing ---
pub const ELEMENT_MIN_WIDTH: f32 = 160.0;
/// Default minimum element height (header + minimal compartment).
pub const ELEMENT_MIN_HEIGHT: f32 = 60.0;
pub const ELEMENT_MAX_WIDTH: f32 = 400.0;
pub const ELEMENT_HEADER_HEIGHT: f32 = 40.0;
pub const ELEMENT_FEATURE_ROW_HEIGHT: f32 = 18.0;
pub const ELEMENT_PADDING: f32 = 8.0;
pub const ELEMENT_H_SPACING: f32 = 80.0;
pub const ELEMENT_V_SPACING: f32 = 30.0;

// --- Font ---
/// Base font size (px) at zoom 1.0.
pub const BASE_FONT_SIZE: f32 = 13.0;

// --- Font scales ---
/// Keyword text size relative to base font (e.g. `<<part def>>`).
pub const KEYWORD_FONT_SCALE: f32 = 0.75;
/// Feature compartment text size relative to base font.
pub const FEATURE_FONT_SCALE: f32 = 0.80;
/// Label/overview LOD text size relative to base font.
pub const LABEL_FONT_SCALE: f32 = 0.70;
/// Conjugation mark (~) font size relative to PORT_HALF.
pub const CONJUGATION_MARK_SCALE: f32 = 1.2;

// --- Spacing ---
/// Vertical gap between keyword line and name line in element header.
pub const KEYWORD_NAME_GAP: f32 = 16.0;
/// Vertical gap between keyword and name in overview package nodes.
pub const OVERVIEW_KEYWORD_NAME_GAP: f32 = 14.0;
/// Vertical inset below compartment separator before first feature line.
pub const COMPARTMENT_TEXT_INSET: f32 = 2.0;

// --- Selection ---
/// Stroke width multiplier when an element is selected.
pub const SELECTED_STROKE_MULTIPLIER: f32 = 1.5;

// --- Selection dot ---
/// Stroke width for the selected highlight circle at dot LOD.
pub const SELECTED_DOT_STROKE: f32 = 2.0;

// --- Arrowhead ---
pub const ARROWHEAD_LENGTH: f32 = 10.0;
pub const ARROWHEAD_WIDTH: f32 = 7.0;

// --- Activity control nodes (initial / final) ---
pub const CONTROL_NODE_RADIUS: f32 = 7.0;
pub const COLOR_CONTROL_NODE: Color32 = Color32::from_rgb(40, 40, 50);

// --- Frame ---
pub const FRAME_PADDING: f32 = 20.0;
pub const FRAME_TAB_HEIGHT: f32 = 24.0;
/// Max fraction of frame width used by the tab label.
pub const FRAME_TAB_MAX_FRACTION: f32 = 0.8;

// --- Connector routing ---
// Design principles: frame containment, dedicated corridors, minimum clearances,
// parallel deconfliction. See CLAUDE.md "Connector Routing Design Principles".
/// Port exit distance before first bend.
pub const ROUTE_STUB_LENGTH: f32 = 20.0;
/// Min gap between route and element edge (was hardcoded 15.0).
pub const ROUTE_ELEMENT_CLEARANCE: f32 = 20.0;
/// Min y-separation between overlapping horizontal segments (PCB 3W rule: >=3× line width).
pub const ROUTE_PARALLEL_SPACING: f32 = 8.0;
/// Min distance from route waypoint to frame edge.
pub const ROUTE_FRAME_MARGIN: f32 = 30.0;
/// X-offset between parallel vertical channels in the same gap.
pub const ROUTE_CHANNEL_SPREAD: f32 = 8.0;
/// Threshold for merging adjacent element columns.
pub const ROUTE_COLUMN_MERGE: f32 = 10.0;
/// Margin outside first/last element column for gap placement.
pub const ROUTE_COLUMN_GAP_MARGIN: f32 = 25.0;
/// Threshold for matching a point to an element edge.
pub const ROUTE_EDGE_TOLERANCE: f32 = 2.0;
/// Inset for element-interior crossing checks.
pub const ROUTE_BOUNDARY_MARGIN: f32 = 1.0;

// --- Overview / LOD thresholds (screen-space height in px) ---
/// Below this: render as colored dot
pub const LOD_DOT_THRESHOLD: f32 = 30.0;
/// Below this: render as labeled rectangle
pub const LOD_LABEL_THRESHOLD: f32 = 80.0;
/// Below this: render as compact SysML box (keyword + name)
pub const LOD_BOX_THRESHOLD: f32 = 200.0;
// Above LOD_BOX_THRESHOLD: full SysML notation with compartments.

// --- Overview spacing ---
pub const OVERVIEW_PACKAGE_H_SPACING: f32 = 100.0;
pub const OVERVIEW_PACKAGE_V_SPACING: f32 = 40.0;
pub const OVERVIEW_PACKAGE_MIN_WIDTH: f32 = 120.0;
pub const OVERVIEW_PACKAGE_MIN_HEIGHT: f32 = 60.0;
/// Pixels per descendant for scaling package height
pub const OVERVIEW_WEIGHT_SCALE: f32 = 1.5;
/// Max package height in overview
pub const OVERVIEW_PACKAGE_MAX_HEIGHT: f32 = 300.0;
/// Extra horizontal padding for overview package text sizing.
pub const OVERVIEW_TEXT_PADDING: f32 = 20.0;

// --- Minimap ---
pub const MINIMAP_SIZE: f32 = 180.0;
pub const MINIMAP_MARGIN: f32 = 12.0;
pub const MINIMAP_BG_ALPHA: u8 = 220;

// --- Filter opacity ---
/// Opacity for elements that don't match search (0-255)
pub const FILTER_DIM_ALPHA: u8 = 50;
/// Opacity for filtered-out elements
pub const FILTER_HIDDEN_ALPHA: u8 = 25;

// --- Colors ---
pub const COLOR_ELEMENT_FILL: Color32 = Color32::from_rgb(245, 245, 250);
pub const COLOR_ELEMENT_STROKE: Color32 = Color32::from_rgb(60, 60, 80);
pub const COLOR_ELEMENT_SELECTED: Color32 = Color32::from_rgb(30, 100, 200);
pub const COLOR_HEADER_TEXT: Color32 = Color32::from_rgb(20, 20, 40);
pub const COLOR_KEYWORD_TEXT: Color32 = Color32::from_rgb(100, 100, 140);
pub const COLOR_FEATURE_TEXT: Color32 = Color32::from_rgb(60, 60, 80);
pub const COLOR_CONNECTOR: Color32 = Color32::from_rgb(80, 80, 100);
pub const COLOR_CONNECTOR_FLOW: Color32 = Color32::from_rgb(40, 120, 40);
pub const COLOR_CONNECTOR_BIND: Color32 = Color32::from_rgb(120, 80, 40);
pub const COLOR_PORT_FILL: Color32 = Color32::from_rgb(220, 220, 240);
pub const COLOR_PORT_STROKE: Color32 = Color32::from_rgb(60, 60, 80);
pub const COLOR_PORT_CONJUGATED: Color32 = Color32::from_rgb(200, 200, 220);
pub const COLOR_FRAME_STROKE: Color32 = Color32::from_rgb(40, 40, 60);
pub const COLOR_FRAME_TAB: Color32 = Color32::from_rgb(230, 230, 240);
pub const COLOR_CANVAS_BG: Color32 = Color32::from_rgb(255, 255, 255);

// --- Overview colors ---
pub const COLOR_PACKAGE_FILL: Color32 = Color32::from_rgb(235, 240, 248);
pub const COLOR_PACKAGE_STROKE: Color32 = Color32::from_rgb(70, 80, 110);
pub const COLOR_DEF_DOT: Color32 = Color32::from_rgb(80, 120, 200);
pub const COLOR_USAGE_DOT: Color32 = Color32::from_rgb(60, 160, 80);
pub const COLOR_SPECIALIZATION_EDGE: Color32 = Color32::from_rgb(140, 100, 180);
pub const COLOR_TYPE_REF_EDGE: Color32 = Color32::from_rgb(160, 160, 180);
pub const COLOR_TOOLTIP_BG: Color32 = Color32::from_rgb(50, 50, 65);
pub const COLOR_TOOLTIP_TEXT: Color32 = Color32::from_rgb(240, 240, 245);
pub const COLOR_ANNOTATION_BG: Color32 = Color32::from_rgb(255, 255, 255);
pub const COLOR_ANNOTATION_STROKE: Color32 = Color32::from_rgb(80, 80, 120);
pub const COLOR_SEARCH_HIGHLIGHT: Color32 = Color32::from_rgb(255, 200, 50);
pub const COLOR_SEARCH_CURRENT: Color32 = Color32::from_rgb(255, 140, 30);
pub const COLOR_MINIMAP_BG: Color32 = Color32::from_rgb(245, 245, 250);
pub const COLOR_MINIMAP_VIEWPORT: Color32 = Color32::from_rgb(30, 100, 200);
pub const COLOR_FILTER_CHIP_BG: Color32 = Color32::from_rgb(230, 235, 245);
pub const COLOR_FILTER_CHIP_ACTIVE: Color32 = Color32::from_rgb(200, 220, 255);
/// Highlight color for current context element in minimap
pub const COLOR_MINIMAP_HIGHLIGHT: Color32 = Color32::from_rgb(255, 120, 40);
// --- Canvas scrollbars ---
/// Width/height of scrollbar tracks at canvas edges.
pub const SCROLLBAR_WIDTH: f32 = 10.0;
/// Minimum thumb length so it remains clickable.
pub const SCROLLBAR_MIN_THUMB: f32 = 20.0;
/// Padding around content bounds for scrollbar range.
pub const SCROLLBAR_CONTENT_MARGIN: f32 = 100.0;
/// Scrollbar track background.
pub const COLOR_SCROLLBAR_TRACK: Color32 = Color32::from_rgba_premultiplied(200, 200, 210, 80);
/// Scrollbar thumb (normal).
pub const COLOR_SCROLLBAR_THUMB: Color32 = Color32::from_rgba_premultiplied(140, 140, 160, 160);
/// Scrollbar thumb (hovered/dragged).
pub const COLOR_SCROLLBAR_THUMB_ACTIVE: Color32 =
    Color32::from_rgba_premultiplied(100, 100, 130, 200);

// --- Library window ---
/// Default width for the Element Library window.
pub const LIBRARY_PANEL_WIDTH: f32 = 300.0;

/// Breadcrumb separator and styling
pub const COLOR_BREADCRUMB_TEXT: Color32 = Color32::from_rgb(50, 50, 70);
pub const COLOR_BREADCRUMB_LINK: Color32 = Color32::from_rgb(30, 100, 200);
pub const COLOR_BREADCRUMB_BG: Color32 = Color32::from_rgb(245, 245, 250);
pub const BREADCRUMB_HEIGHT: f32 = 28.0;
