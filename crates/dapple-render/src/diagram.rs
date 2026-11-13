//! Diagram fences — the third figure family beside charts.
//!
//! ````markdown
//! ```diagram
//! { "type": "venn2", "a": { "label": "ZIP", ... }, ... }
//! ```
//! ````
//!
//! Like charts, the spec vocabulary is OURS, not a backend's: a small stable
//! set of archetypes a guide can rely on across renderer changes. Unlike
//! charts there is no drawing backend at all — each archetype hand-emits its
//! SVG, the same way the wordmark is inlined. The failure contract is
//! identical to charts: any parse or layout problem returns `None` and the
//! fence degrades to a visible code block; a diagram must never be able to
//! fail a render.
//!
//! Because there is no text-measurement engine, every archetype defends with
//! hard caps and clipping at PARSE time (list lengths, label widths, lane and
//! grid counts) rather than trusting callers to be tasteful. Text uses
//! Liberation Sans only — Typst rasterizes SVG text against its own font book
//! (DejaVu Serif / Liberation Sans / DejaVu Sans Mono), so any other family
//! falls back unpredictably in the PDF.

use serde_json::Value;

use crate::chart::RenderedFigure;
use crate::theme::{SLATE_200, SLATE_500, SLATE_600, SLATE_800};

/// Fixed drawing width in user units, matching charts.
const DIAGRAM_WIDTH: f32 = 640.0;
/// The one font family every diagram names (see module docs).
const FONT: &str = "Liberation Sans";

/// Is this fence a diagram fence? Mirrors the standalone-keyword rule of
/// [`crate::chart::is_chart_info`]: `diagrams` is an ordinary code block.
pub fn is_diagram_info(info: &str) -> bool {
    match info.trim().strip_prefix("diagram") {
        Some(rest) => rest.is_empty() || rest.starts_with(char::is_whitespace),
        None => false,
    }
}

/// Fence body → figure, or `None` to degrade. The one entry point both
/// emitters use.
pub fn render_diagram_fence(body: &str) -> Option<RenderedFigure> {
    let value: Value = match serde_json::from_str(body.trim()) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("diagram fence body is not valid JSON, rendering as code block: {e}");
            return None;
        }
    };
    let caption = str_field(&value, "caption");
    let svg = match value.get("type").and_then(Value::as_str).unwrap_or("") {
        "venn2" => render_venn2(&value),
        "lanes" => render_lanes(&value),
        "flow" => render_flow(&value),
        other => {
            log::warn!("unknown diagram type '{other}', rendering as code block");
            return None;
        }
    }?;
    Some(RenderedFigure { svg, caption })
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn str_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Clip a label to `max` characters, appending an ellipsis when cut. Works on
/// character boundaries so multi-byte text never splits mid-glyph.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn escape_svg(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Exactly `#` plus six hex digits, or the fallback.
fn color_or(value: &Value, key: &str, fallback: &str) -> String {
    let c = str_field(value, key);
    if c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit()) {
        c.to_ascii_lowercase()
    } else {
        if !c.is_empty() {
            log::warn!("diagram color '{c}' is not #rrggbb, using default");
        }
        fallback.to_string()
    }
}

fn svg_open(height: f32) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" \
         viewBox=\"0 0 {w} {h}\">",
        w = DIAGRAM_WIDTH,
        h = height,
    )
}

fn text_el(
    x: f32,
    y: f32,
    size: f32,
    fill: &str,
    anchor: &str,
    weight: &str,
    text: &str,
) -> String {
    format!(
        "<text x=\"{x}\" y=\"{y}\" font-family=\"{FONT}\" font-size=\"{size}\" fill=\"{fill}\" \
         text-anchor=\"{anchor}\" font-weight=\"{weight}\">{}</text>",
        escape_svg(text)
    )
}

/// A text element with a white halo (`paint-order: stroke`), for labels that
/// may cross lines or other labels. Renderers without paint-order support
/// degrade to plain text.
fn haloed_text_el(
    x: f32,
    y: f32,
    size: f32,
    fill: &str,
    anchor: &str,
    weight: &str,
    text: &str,
) -> String {
    format!(
        "<text x=\"{x}\" y=\"{y}\" font-family=\"{FONT}\" font-size=\"{size}\" fill=\"{fill}\" \
         text-anchor=\"{anchor}\" font-weight=\"{weight}\" stroke=\"#ffffff\" stroke-width=\"3\" \
         paint-order=\"stroke\">{}</text>",
        escape_svg(text)
    )
}

fn title_el(title: &str) -> String {
    if title.is_empty() {
        String::new()
    } else {
        text_el(
            DIAGRAM_WIDTH / 2.0,
            22.0,
            16.0,
            SLATE_800,
            "middle",
            "bold",
            title,
        )
    }
}

fn title_offset(title: &str) -> f32 {
    if title.is_empty() { 8.0 } else { 36.0 }
}

// ---------------------------------------------------------------------------
// venn2 — two overlapping circles with three item columns below
// ---------------------------------------------------------------------------

const VENN_ITEM_CAP: usize = 5;
const VENN_LABEL_CAP: usize = 28;

struct VennSide {
    label: String,
    color: String,
    count: Option<i64>,
    items: Vec<String>,
}

fn venn_side(value: &Value, key: &str, fallback_label: &str, fallback_color: &str) -> VennSide {
    let side = value.get(key).cloned().unwrap_or(Value::Null);
    let label = {
        let l = str_field(&side, "label");
        if l.is_empty() {
            fallback_label.to_string()
        } else {
            clip(&l, VENN_LABEL_CAP)
        }
    };
    VennSide {
        label,
        color: color_or(&side, "color", fallback_color),
        count: side.get("count").and_then(Value::as_i64),
        items: side
            .get("items")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(|s| clip(s.trim(), VENN_LABEL_CAP))
                    .filter(|s| !s.is_empty())
                    .take(VENN_ITEM_CAP)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn render_venn2(value: &Value) -> Option<String> {
    let a = venn_side(value, "a", "A", SLATE_500);
    let b = venn_side(value, "b", "B", SLATE_600);
    let shared = venn_side(value, "shared", "Both", SLATE_800);
    let title = str_field(value, "title");

    let top = title_offset(&title);
    // Circle band: two r=88 circles whose centers sit 150 apart, so the lens
    // in the middle is wide enough for a count.
    let r = 88.0;
    let cy = top + 14.0 + r;
    let (cxa, cxb) = (DIAGRAM_WIDTH / 2.0 - 75.0, DIAGRAM_WIDTH / 2.0 + 75.0);
    let items_top = cy + r + 30.0;
    let max_items = a.items.len().max(b.items.len()).max(shared.items.len());
    let items_height = if max_items == 0 {
        0.0
    } else {
        18.0 + max_items as f32 * 16.0
    };
    let height = items_top + items_height + 6.0;

    let mut svg = svg_open(height);
    svg.push_str(&title_el(&title));
    for (cx, side) in [(cxa, &a), (cxb, &b)] {
        svg.push_str(&format!(
            "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{c}\" fill-opacity=\"0.15\" \
             stroke=\"{c}\" stroke-width=\"2\"/>",
            c = side.color,
        ));
    }
    // Side labels above the circles, pushed outward so they clear the lens.
    svg.push_str(&text_el(
        cxa - r / 2.0,
        cy - r - 8.0,
        13.0,
        &a.color,
        "middle",
        "bold",
        &a.label,
    ));
    svg.push_str(&text_el(
        cxb + r / 2.0,
        cy - r - 8.0,
        13.0,
        &b.color,
        "middle",
        "bold",
        &b.label,
    ));
    // Counts: one per exclusive region, one in the lens. Counts always render
    // when given — they are the diagram's payload.
    if let Some(n) = a.count {
        svg.push_str(&text_el(
            cxa - 40.0,
            cy + 6.0,
            20.0,
            &a.color,
            "middle",
            "bold",
            &n.to_string(),
        ));
    }
    if let Some(n) = b.count {
        svg.push_str(&text_el(
            cxb + 40.0,
            cy + 6.0,
            20.0,
            &b.color,
            "middle",
            "bold",
            &n.to_string(),
        ));
    }
    if let Some(n) = shared.count {
        svg.push_str(&text_el(
            DIAGRAM_WIDTH / 2.0,
            cy + 6.0,
            20.0,
            SLATE_800,
            "middle",
            "bold",
            &n.to_string(),
        ));
    }
    svg.push_str(&text_el(
        DIAGRAM_WIDTH / 2.0,
        cy + r + 20.0,
        12.0,
        SLATE_600,
        "middle",
        "normal",
        &shared.label,
    ));

    // Item columns below the circles: only-A, shared, only-B. Text lines, not
    // in-circle labels — there is no wrapping engine to trust.
    if max_items > 0 {
        let cols = [
            (16.0, "start", &a.color, &a.items),
            (
                DIAGRAM_WIDTH / 2.0,
                "middle",
                &SLATE_600.to_string(),
                &shared.items,
            ),
            (DIAGRAM_WIDTH - 16.0, "end", &b.color, &b.items),
        ];
        for (x, anchor, color, items) in cols {
            for (i, item) in items.iter().enumerate() {
                svg.push_str(&text_el(
                    x,
                    items_top + 12.0 + i as f32 * 16.0,
                    11.5,
                    color,
                    anchor,
                    "normal",
                    item,
                ));
            }
        }
    }
    svg.push_str("</svg>");
    Some(svg)
}

// ---------------------------------------------------------------------------
// lanes — up to three horizontal lanes of dated events on one year axis
// ---------------------------------------------------------------------------

/// Five, not three, because the vocabularies callers actually lane by are
/// five-valued and a three-way cut of one is never neutral. A risk register
/// classifies each year as new / escalated / softened / dropped (plus an
/// unchanged that draws no dot): fold escalated with softened and the DIRECTION
/// of the change is gone, drop the exits and a risk leaving the register never
/// appears. Both are the "read it identically whichever way it went" rule
/// failing in the furniture rather than in the prose.
const LANE_CAP: usize = 5;
const LANE_EVENT_CAP: usize = 10;
const LANE_LABEL_CAP: usize = 24;
/// One month as a fraction of a year, for the month-scale axis below.
const MONTH: f32 = 1.0 / 12.0;
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

struct LaneEvent {
    /// Fractional year, e.g. 2023.5 for 2023-07-01.
    at: f32,
    text: String,
}

struct Lane {
    label: String,
    color: String,
    events: Vec<LaneEvent>,
}

/// `YYYY` or `YYYY-MM-DD` → fractional year. Anything else is `None` and the
/// event is dropped (with a warning), never a panic.
fn parse_when(s: &str) -> Option<f32> {
    let s = s.trim();
    let year_part: &str = s.split('-').next().unwrap_or("");
    if year_part.len() != 4 {
        return None;
    }
    let year: f32 = year_part.parse::<i32>().ok()? as f32;
    let mut parts = s.split('-').skip(1);
    let month: f32 = parts
        .next()
        .and_then(|m| m.parse::<u32>().ok())
        .filter(|m| (1..=12).contains(m))
        .map(|m| (m - 1) as f32 / 12.0)
        .unwrap_or(0.0);
    Some(year + month)
}

fn render_lanes(value: &Value) -> Option<String> {
    let raw_lanes = value.get("lanes").and_then(Value::as_array)?;
    let mut lanes: Vec<Lane> = Vec::new();
    for (i, l) in raw_lanes.iter().take(LANE_CAP).enumerate() {
        let mut events: Vec<LaneEvent> = l
            .get("events")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        let text = clip(&str_field(e, "text"), LANE_LABEL_CAP);
                        let when = str_field(e, "when");
                        match parse_when(&when) {
                            Some(at) if !text.is_empty() => Some(LaneEvent { at, text }),
                            _ => {
                                log::warn!("lanes event with bad when '{when}' dropped");
                                None
                            }
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        events.sort_by(|x, y| x.at.total_cmp(&y.at));
        events.truncate(LANE_EVENT_CAP);
        if events.is_empty() {
            continue;
        }
        lanes.push(Lane {
            label: {
                let lab = clip(&str_field(l, "label"), LANE_LABEL_CAP);
                if lab.is_empty() {
                    format!("Lane {}", i + 1)
                } else {
                    lab
                }
            },
            color: color_or(l, "color", SLATE_600),
            events,
        });
    }
    if lanes.is_empty() {
        log::warn!("lanes diagram has no lane with datable events, rendering as code block");
        return None;
    }

    let title = str_field(value, "title");
    let top = title_offset(&title);
    let lane_height = 94.0;
    let axis_height = 26.0;
    let height = top + lanes.len() as f32 * lane_height + axis_height;
    let (x0, x1) = (86.0, DIAGRAM_WIDTH - 24.0);

    let lo = lanes
        .iter()
        .flat_map(|l| l.events.iter().map(|e| e.at))
        .fold(f32::INFINITY, f32::min);
    let hi = lanes
        .iter()
        .flat_map(|l| l.events.iter().map(|e| e.at))
        .fold(f32::NEG_INFINITY, f32::max);
    // MONTH SCALE IS CHOSEN BY THE DATA, NOT BY THE SPAN. Any event carrying a
    // month makes the set month-granular; a set of bare `YYYY` events stays on
    // the year scale it has always had however short its range, so every
    // year-dated diagram lays out byte-identically to before.
    //
    // It matters because the year-scale pad below is wrong at month resolution:
    // a ten-month filing trail has `hi - lo` under 1.0, so it would be padded
    // out to a full two-year window, squeezed into some 40% of the canvas, and
    // labelled with one or two ticks — the shape of the traffic, which is the
    // whole reason to draw it, disappears into the padding.
    let month_scale = lanes
        .iter()
        .flat_map(|l| l.events.iter())
        .any(|e| e.at.fract() > f32::EPSILON)
        && (hi - lo) <= 2.0;
    // A degenerate range (single instant) still lays out: pad around it.
    let (lo, hi) = if month_scale {
        (lo - MONTH, hi + MONTH)
    } else if (hi - lo) < 1.0 {
        (lo - 0.5, lo + 1.5)
    } else {
        (lo - 0.4, hi + 0.6)
    };
    let x_of = |at: f32| x0 + (at - lo) / (hi - lo) * (x1 - x0);

    let mut svg = svg_open(height);
    svg.push_str(&title_el(&title));

    for (li, lane) in lanes.iter().enumerate() {
        let ly = top + li as f32 * lane_height + lane_height / 2.0 + 6.0;
        svg.push_str(&text_el(
            x0 - 12.0,
            ly + 4.0,
            12.5,
            &lane.color,
            "end",
            "bold",
            &lane.label,
        ));
        svg.push_str(&format!(
            "<line x1=\"{x0}\" y1=\"{ly}\" x2=\"{x1}\" y2=\"{ly}\" stroke=\"{}\" stroke-width=\"2\"/>",
            lane.color
        ));
        for (ei, e) in lane.events.iter().enumerate() {
            let ex = x_of(e.at);
            svg.push_str(&format!(
                "<circle cx=\"{ex}\" cy=\"{ly}\" r=\"4.5\" fill=\"{}\"/>",
                lane.color
            ));
            // One line per label ("YYYY · text"), placed on a four-slot offset
            // cycle CONTAINED within this lane's band (±38 < lane_height/2), so
            // neighboring events share a level only when four apart and one
            // lane's labels can never bleed into the next lane's space. A
            // white halo keeps a label legible if it still crosses something —
            // there is no text-measurement engine to do better.
            let dy = [-36.0, 36.0, -18.0, 18.0][ei % 4];
            let ty = ly + dy + if dy > 0.0 { 8.0 } else { 0.0 };
            let (c1, c2) = if dy > 0.0 {
                (ly + 5.0, ly + dy - 2.0)
            } else {
                (ly + dy + 4.0, ly - 5.0)
            };
            svg.push_str(&format!(
                "<line x1=\"{ex}\" y1=\"{c1}\" x2=\"{ex}\" y2=\"{c2}\" stroke=\"{SLATE_200}\" \
                 stroke-width=\"1\" stroke-dasharray=\"2,2\"/>",
            ));
            // Keep the label inside the canvas whatever its anchor point.
            let anchor = if ex > x1 - 90.0 {
                "end"
            } else if ex < x0 + 60.0 {
                "start"
            } else {
                "middle"
            };
            // The stamp carries what the POSITION cannot. On a year axis that is
            // the year; on a month axis the year is the same for nearly every
            // event and already on the ticks, so the month is the informative
            // half and "2026 · " on all ten labels is just noise.
            let stamp = if month_scale {
                MONTH_NAMES[((e.at.fract() * 12.0).round() as i32).rem_euclid(12) as usize]
                    .to_string()
            } else {
                (e.at.floor() as i32).to_string()
            };
            let label = format!("{stamp} · {}", e.text);
            svg.push_str(&haloed_text_el(
                ex, ty, 10.5, SLATE_800, anchor, "normal", &label,
            ));
        }
    }

    // Axis ticks, thinned so at most ~8 label: whole years, or whole months when
    // the events carry one.
    let axis_y = height - axis_height + 8.0;
    let ticks: Vec<(f32, String)> = if month_scale {
        // Counted in MONTH INDICES so the arithmetic stays integral: index `m`
        // is year `m / 12`, month `m % 12`. An f32 fractional year holds a month
        // to about four decimal places, which is far inside half a month, so the
        // index round-trips exactly.
        let m_lo = (lo * 12.0).ceil() as i32;
        let m_hi = (hi * 12.0).floor() as i32;
        let step = (((m_hi - m_lo).max(1) as f32 / 8.0).ceil() as i32).max(1);
        (m_lo..=m_hi)
            .step_by(step as usize)
            .enumerate()
            .map(|(i, m)| {
                let name = MONTH_NAMES[m.rem_euclid(12) as usize];
                // The year rides the first tick and every January, so a month
                // axis can never be read a year out.
                let label = if i == 0 || m.rem_euclid(12) == 0 {
                    format!("{name} {}", m.div_euclid(12))
                } else {
                    name.to_string()
                };
                (m as f32 / 12.0, label)
            })
            .collect()
    } else {
        let first_year = lo.ceil() as i32;
        let last_year = hi.floor() as i32;
        let step = (((last_year - first_year).max(1) as f32 / 8.0).ceil() as i32).max(1);
        (first_year..=last_year)
            .step_by(step as usize)
            .map(|y| (y as f32, y.to_string()))
            .collect()
    };
    for (at, label) in ticks {
        let x = x_of(at);
        svg.push_str(&format!(
            "<line x1=\"{x}\" y1=\"{}\" x2=\"{x}\" y2=\"{}\" stroke=\"{SLATE_200}\" stroke-width=\"1\"/>",
            axis_y - 6.0,
            axis_y - 2.0
        ));
        svg.push_str(&text_el(
            x,
            axis_y + 10.0,
            10.0,
            SLATE_500,
            "middle",
            "normal",
            &label,
        ));
    }
    svg.push_str("</svg>");
    Some(svg)
}

// ---------------------------------------------------------------------------
// flow — boxes on an author-specified grid with straight labeled arrows
// ---------------------------------------------------------------------------

/// The grid BOUND, not a usable area — read the next paragraph before filling it.
/// Boxes are placed at the caller's own `col`/`row` and the canvas is sized from
/// the maxima ACTUALLY used, so raising this only admits more; it can never move
/// an existing box.
///
/// ARROWS ARE STRAIGHT AND BOXES PAINT OVER THEM, so a long arrow passes BEHIND
/// any box between its ends. Measured on the 3-column layout (two subjects on row
/// 0, a stack of shared third parties in the middle column): a node two rows down
/// still trims horizontally and routes clear, and a node three or more rows down
/// flips to a vertical trim and crosses every box between. So a middle stack is
/// legible at two and not at three, whatever this constant says. A caller wanting
/// more should draw fewer relations, not a taller grid.
const FLOW_GRID: i64 = 5;
const FLOW_BOX_LABEL_CAP: usize = 18;
const FLOW_ARROW_LABEL_CAP: usize = 24;

struct FlowBox {
    id: String,
    label: String,
    color: String,
    col: i64,
    row: i64,
}

fn render_flow(value: &Value) -> Option<String> {
    let raw_boxes = value.get("boxes").and_then(Value::as_array)?;
    let mut boxes: Vec<FlowBox> = Vec::new();
    for b in raw_boxes {
        let id = str_field(b, "id");
        let label = clip(&str_field(b, "label"), FLOW_BOX_LABEL_CAP);
        let col = b.get("col").and_then(Value::as_i64).unwrap_or(-1);
        let row = b.get("row").and_then(Value::as_i64).unwrap_or(-1);
        if id.is_empty() || label.is_empty() {
            log::warn!("flow box without id/label dropped");
            continue;
        }
        if !(0..FLOW_GRID).contains(&col) || !(0..FLOW_GRID).contains(&row) {
            log::warn!("flow box '{id}' outside the {FLOW_GRID}x{FLOW_GRID} grid, dropped");
            continue;
        }
        boxes.push(FlowBox {
            id,
            label,
            color: color_or(b, "color", SLATE_600),
            col,
            row,
        });
    }
    if boxes.is_empty() {
        log::warn!("flow diagram has no placeable boxes, rendering as code block");
        return None;
    }

    let title = str_field(value, "title");
    let top = title_offset(&title);
    let cols_used = boxes.iter().map(|b| b.col).max().unwrap_or(0) + 1;
    let rows_used = boxes.iter().map(|b| b.row).max().unwrap_or(0) + 1;
    let cell_w = DIAGRAM_WIDTH / cols_used as f32;
    let cell_h = 86.0;
    let (box_w, box_h) = ((cell_w - 40.0).min(190.0), 48.0);
    let height = top + rows_used as f32 * cell_h + 8.0;

    let center = |b: &FlowBox| -> (f32, f32) {
        (
            b.col as f32 * cell_w + cell_w / 2.0,
            top + b.row as f32 * cell_h + cell_h / 2.0,
        )
    };

    let mut svg = svg_open(height);
    svg.push_str(
        "<defs><marker id=\"dg-arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" \
         markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\">\
         <path d=\"M 0 0 L 10 5 L 0 10 z\" fill=\"#62748e\"/></marker></defs>",
    );
    svg.push_str(&title_el(&title));

    // Arrows first so boxes paint over their tails.
    if let Some(arrows) = value.get("arrows").and_then(Value::as_array) {
        for a in arrows {
            let from = str_field(a, "from");
            let to = str_field(a, "to");
            let (Some(fb), Some(tb)) = (
                boxes.iter().find(|b| b.id == from),
                boxes.iter().find(|b| b.id == to),
            ) else {
                log::warn!("flow arrow references unknown box ('{from}' -> '{to}'), dropped");
                continue;
            };
            if from == to {
                continue;
            }
            let (x1, y1) = center(fb);
            let (x2, y2) = center(tb);
            // Trim the line back to the box edges along the dominant axis so
            // the arrowhead lands on the border, not the center.
            let (sx, sy, ex, ey) = if (x2 - x1).abs() >= (y2 - y1).abs() {
                let dir = if x2 > x1 { 1.0 } else { -1.0 };
                (
                    x1 + dir * box_w / 2.0,
                    y1,
                    x2 - dir * (box_w / 2.0 + 2.0),
                    y2,
                )
            } else {
                let dir = if y2 > y1 { 1.0 } else { -1.0 };
                (
                    x1,
                    y1 + dir * box_h / 2.0,
                    x2,
                    y2 - dir * (box_h / 2.0 + 2.0),
                )
            };
            svg.push_str(&format!(
                "<line x1=\"{sx}\" y1=\"{sy}\" x2=\"{ex}\" y2=\"{ey}\" stroke=\"{SLATE_500}\" \
                 stroke-width=\"1.5\" marker-end=\"url(#dg-arrow)\"/>",
            ));
            let label = clip(&str_field(a, "label"), FLOW_ARROW_LABEL_CAP);
            if !label.is_empty() {
                svg.push_str(&haloed_text_el(
                    (sx + ex) / 2.0,
                    (sy + ey) / 2.0 - 4.0,
                    10.0,
                    SLATE_500,
                    "middle",
                    "normal",
                    &label,
                ));
            }
        }
    }

    for b in &boxes {
        let (cx, cy) = center(b);
        svg.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{box_w}\" height=\"{box_h}\" rx=\"6\" \
             fill=\"{c}\" fill-opacity=\"0.12\" stroke=\"{c}\" stroke-width=\"1.5\"/>",
            cx - box_w / 2.0,
            cy - box_h / 2.0,
            c = b.color,
        ));
        svg.push_str(&text_el(
            cx,
            cy + 4.0,
            12.0,
            SLATE_800,
            "middle",
            "bold",
            &b.label,
        ));
    }
    svg.push_str("</svg>");
    Some(svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagram_keyword_must_stand_alone() {
        assert!(is_diagram_info("diagram"));
        assert!(is_diagram_info("  diagram  "));
        assert!(!is_diagram_info("diagrams"));
        assert!(!is_diagram_info("chart"));
        assert!(!is_diagram_info(""));
    }

    #[test]
    fn venn2_renders_with_counts_items_and_font() {
        let fig = render_diagram_fence(
            r##"{ "type": "venn2", "title": "Coverage",
                 "a": { "label": "ZIP", "color": "#ea580c", "count": 60, "items": ["Segments"] },
                 "b": { "label": "NOW", "color": "#0d9488", "count": 96, "items": ["Backlog", "RPO"] },
                 "shared": { "count": 163, "items": ["Revenue"] },
                 "caption": "Counts are record dimensions." }"##,
        )
        .expect("venn2 should render");
        assert!(fig.svg.starts_with("<svg"));
        assert!(fig.svg.contains("Liberation Sans"));
        assert!(fig.svg.contains("#ea580c") && fig.svg.contains("#0d9488"));
        assert!(fig.svg.contains(">163<") && fig.svg.contains(">60<"));
        assert_eq!(fig.caption, "Counts are record dimensions.");
    }

    #[test]
    fn venn2_clips_and_caps_items() {
        let long = "x".repeat(60);
        let items: Vec<String> = (0..9).map(|i| format!("{long}{i}")).collect();
        let spec = serde_json::json!({
            "type": "venn2",
            "a": { "label": long, "items": items },
            "b": {}, "shared": {}
        });
        let fig = render_diagram_fence(&spec.to_string()).expect("should render");
        // Clipped labels carry an ellipsis: the side label plus the capped
        // five items — the other four items never render.
        assert_eq!(fig.svg.matches('…').count(), VENN_ITEM_CAP + 1);
    }

    #[test]
    fn lanes_renders_and_orders_events() {
        let fig = render_diagram_fence(
            r##"{ "type": "lanes", "title": "Two histories",
                 "lanes": [
                   { "label": "ZIP", "color": "#ea580c",
                     "events": [ { "when": "2022", "text": "Buyback" }, { "when": "2019-06-01", "text": "IPO" } ] },
                   { "label": "NOW", "color": "#0d9488",
                     "events": [ { "when": "2004", "text": "Founded" } ] } ] }"##,
        )
        .expect("lanes should render");
        assert!(fig.svg.contains("IPO") && fig.svg.contains("Founded"));
        assert!(fig.svg.contains("Liberation Sans"));
    }

    #[test]
    fn five_lanes_render_and_a_sixth_drops() {
        let labels = [
            "New",
            "Escalated",
            "Softened",
            "Dropped",
            "Restated",
            "Sixth",
        ];
        let lanes: Vec<_> = labels
            .iter()
            .enumerate()
            .map(|(i, l)| {
                serde_json::json!({
                    "label": l,
                    "events": [ { "when": (2021 + i).to_string(), "text": l } ]
                })
            })
            .collect();
        let fig = render_diagram_fence(
            &serde_json::json!({ "type": "lanes", "lanes": lanes }).to_string(),
        )
        .expect("lanes should render");
        for l in &labels[..LANE_CAP] {
            assert!(fig.svg.contains(l), "lane {l} missing");
        }
        assert!(!fig.svg.contains("Sixth"), "LANE_CAP must still bite");
    }

    #[test]
    fn month_dated_lanes_get_a_month_axis() {
        let fig = render_diagram_fence(
            r##"{ "type": "lanes", "title": "Since the last annual report",
                 "lanes": [
                   { "label": "Contradicts", "color": "#ea580c", "events": [
                       { "when": "2026-02-11", "text": "Auditor change" },
                       { "when": "2026-05-02", "text": "Guidance pulled" } ] },
                   { "label": "Extends", "color": "#0d9488", "events": [
                       { "when": "2026-03-19", "text": "New facility" },
                       { "when": "2026-09-30", "text": "Buyback raised" } ] } ] }"##,
        )
        .expect("lanes should render");
        // The stamp on each label is the MONTH, never the year every event shares.
        assert!(fig.svg.contains("Feb · Auditor change"), "{}", fig.svg);
        assert!(fig.svg.contains("Sep · Buyback raised"));
        assert!(
            !fig.svg.contains("2026 ·"),
            "a year stamp here is pure noise"
        );
        // The first tick carries the year; July is a tick nothing happened in,
        // so its presence proves the axis is months and not two padded years.
        assert!(fig.svg.contains("Jan 2026"), "{}", fig.svg);
        assert!(fig.svg.contains(">Jul</text>"), "{}", fig.svg);
    }

    #[test]
    fn year_dated_lanes_keep_the_year_axis() {
        let fig = render_diagram_fence(
            r##"{ "type": "lanes", "lanes": [ { "label": "New", "events": [
                 { "when": "2021", "text": "Opened" }, { "when": "2025", "text": "Closed" } ] } ] }"##,
        )
        .expect("lanes should render");
        assert!(fig.svg.contains("2021 · Opened"));
        assert!(
            MONTH_NAMES.iter().all(|m| !fig.svg.contains(m)),
            "bare years must not reach the month scale"
        );
    }

    #[test]
    fn lanes_with_no_datable_events_degrades() {
        assert!(
            render_diagram_fence(
                r#"{ "type": "lanes", "lanes": [ { "label": "A", "events": [ { "when": "someday", "text": "?" } ] } ] }"#,
            )
            .is_none()
        );
    }

    #[test]
    fn flow_renders_boxes_and_arrows() {
        let fig = render_diagram_fence(
            r##"{ "type": "flow", "title": "Who names whom",
                 "boxes": [ { "id": "a", "label": "ZIP", "color": "#ea580c", "col": 0, "row": 0 },
                            { "id": "b", "label": "NOW", "color": "#0d9488", "col": 2, "row": 0 } ],
                 "arrows": [ { "from": "a", "to": "b", "label": "names as competitor" } ] }"##,
        )
        .expect("flow should render");
        assert!(fig.svg.contains("marker-end"));
        assert!(fig.svg.contains("names as competitor"));
    }

    #[test]
    fn flow_fills_a_five_row_grid_and_still_bounds_it() {
        let fig = render_diagram_fence(
            r##"{ "type": "flow",
                 "boxes": [ { "id": "a", "label": "ZIP", "col": 0, "row": 0 },
                            { "id": "b", "label": "NOW", "col": 2, "row": 0 },
                            { "id": "aud", "label": "Auditor", "col": 1, "row": 1 },
                            { "id": "ppl", "label": "2 directors", "col": 1, "row": 2 },
                            { "id": "cus", "label": "3 customers", "col": 1, "row": 3 },
                            { "id": "hld", "label": "9 holders", "col": 1, "row": 4 } ] }"##,
        )
        .expect("flow should render");
        for l in ["Auditor", "2 directors", "3 customers", "9 holders"] {
            assert!(fig.svg.contains(l), "box {l} missing");
        }
        // One past the grid still drops, so the bound is raised and not removed.
        let over = render_diagram_fence(
            r#"{ "type": "flow", "boxes": [ { "id": "a", "label": "In", "col": 0, "row": 0 },
                                            { "id": "b", "label": "Out", "col": 0, "row": 5 } ] }"#,
        )
        .expect("the in-grid box alone should render");
        assert!(over.svg.contains("In") && !over.svg.contains("Out"));
    }

    #[test]
    fn flow_rejects_unknown_arrow_targets_but_still_renders() {
        let fig = render_diagram_fence(
            r#"{ "type": "flow",
                 "boxes": [ { "id": "a", "label": "A", "col": 0, "row": 0 } ],
                 "arrows": [ { "from": "a", "to": "ghost" } ] }"#,
        )
        .expect("boxes alone should render");
        assert!(!fig.svg.contains("marker-end"), "dangling arrow must drop");
    }

    #[test]
    fn every_malformed_spec_degrades_never_panics() {
        for body in [
            "",
            "not json",
            "{}",
            r#"{ "type": "mystery" }"#,
            r#"{ "type": "venn2" }"#,
            r#"{ "type": "lanes" }"#,
            r#"{ "type": "lanes", "lanes": [] }"#,
            r#"{ "type": "flow" }"#,
            r#"{ "type": "flow", "boxes": [ { "id": "a", "label": "A", "col": 9, "row": 0 } ] }"#,
            r#"{ "type": "venn2", "a": 5, "b": [], "shared": "x" }"#,
        ] {
            // venn2 with no data still draws two empty circles — that is a
            // usable (if bare) diagram, so only the truly unusable specs are
            // asserted to degrade.
            let _ = render_diagram_fence(body);
        }
        assert!(render_diagram_fence("not json").is_none());
        assert!(render_diagram_fence(r#"{ "type": "mystery" }"#).is_none());
        assert!(render_diagram_fence(r#"{ "type": "lanes", "lanes": [] }"#).is_none());
        assert!(render_diagram_fence(r#"{ "type": "flow" }"#).is_none());
    }

    #[test]
    fn escaped_text_cannot_break_the_svg() {
        let fig = render_diagram_fence(
            r#"{ "type": "venn2", "a": { "label": "<script>\"&'" }, "b": {}, "shared": {} }"#,
        )
        .expect("should render");
        assert!(!fig.svg.contains("<script>"));
        assert!(fig.svg.contains("&lt;script&gt;"));
    }
}
