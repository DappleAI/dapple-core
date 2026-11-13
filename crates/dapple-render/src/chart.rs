//! `chart` fence recognition and SVG rendering.
//!
//! Reports carry quantitative material that a table alone does not make
//! readable. A fenced block whose info string is `chart` holds a JSON spec and
//! is rendered to an SVG chart by both emitters (Typst → PDF via
//! `image(bytes(…), format: "svg")`, HTML via an inlined `<svg>`).
//!
//! ````markdown
//! ```chart
//! { "type": "line", "title": "Revenue and net income",
//!   "x": ["FY2022", "FY2023", "FY2024"],
//!   "series": [{ "name": "Revenue", "data": [1.1, 1.4, 1.3] }],
//!   "unit": "usd" }
//! ```
//! ````
//!
//! Like [`crate::admonish`], the syntax rides on a plain code fence, so a
//! renderer that does not know it degrades to a visible code block. That
//! degradation is also the failure mode *here*: an unparseable info string,
//! malformed JSON, or a chart the backend refuses to draw returns `None` and
//! the caller falls through to its ordinary code-block path. **A chart must
//! never be able to fail a render** — the rendered document is the deliverable,
//! and a bad spec costs one figure, not the report.
//!
//! # The spec is ours, not the backend's
//!
//! The fence contract is deliberately a small, stable vocabulary rather than a
//! passthrough of the charting library's own JSON. Guides should not have to
//! know `charts-rs` field names, the theme has to be applied centrally, and
//! swapping the renderer later must not change a single guide.

use std::sync::OnceLock;

use serde_json::Value;

use crate::theme::{CHART_SERIES_COLORS, FONT_SANS, SLATE_200, SLATE_600, SLATE_800};

/// Liberation Sans, the same face the Typst world embeds for headings.
///
/// Load-bearing: Typst rasterizes SVG text against its own font book, which
/// holds only DejaVu Serif, Liberation Sans, and DejaVu Sans Mono
/// (`typeset/world.rs`). A chart labelled in any other family falls back
/// unpredictably in the PDF, so the chart backend is handed this exact face and
/// every spec names it. `charts-rs` strips the weight suffix when registering,
/// so the family resolves as "Liberation Sans".
const CHART_FONT: &[u8] = include_bytes!("../assets/fonts/LiberationSans-Regular.ttf");

/// Rendered chart width in SVG user units. The emitters scale to the text
/// column, so this fixes the aspect ratio and the label density, not the
/// printed size.
const CHART_WIDTH: u16 = 640;
const DEFAULT_CHART_HEIGHT: u16 = 320;

/// Which chart to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartKind {
    /// Trend over an ordered axis.
    Line,
    /// Grouped vertical bars, one group per x entry.
    Bar,
    /// Horizontal bars — the readable choice when category names are long.
    HBar,
    /// Vertical bars stacked per x entry (composition of a total).
    Stacked,
    /// Vertical bars crossing zero. Identical geometry to `Bar`; a distinct
    /// name because a signed series reads as a different thing.
    Diverging,
}

impl ChartKind {
    fn from_str(s: &str) -> Option<Self> {
        use ChartKind::*;
        Some(match s.to_ascii_lowercase().as_str() {
            "line" => Line,
            "bar" => Bar,
            "hbar" | "horizontal-bar" => HBar,
            "stacked" | "stacked-bar" => Stacked,
            "diverging" => Diverging,
            _ => return None,
        })
    }
}

/// Axis unit, which drives tick formatting only. Values are always passed in
/// already scaled by the caller: a report that thinks in millions sends 1.4,
/// not 1_400_000, and labels its own axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartUnit {
    Usd,
    Pct,
    Shares,
    Plain,
}

impl ChartUnit {
    fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "usd" | "dollars" => ChartUnit::Usd,
            "pct" | "percent" => ChartUnit::Pct,
            "shares" => ChartUnit::Shares,
            _ => ChartUnit::Plain,
        }
    }

    /// `charts-rs` axis formatter: `{c}` is the tick value.
    fn formatter(self) -> &'static str {
        match self {
            ChartUnit::Usd => "${c}",
            ChartUnit::Pct => "{c}%",
            ChartUnit::Shares | ChartUnit::Plain => "",
        }
    }
}

/// One named run of values, aligned to the spec's `x` entries.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartSeries {
    pub name: String,
    pub data: Vec<f32>,
    /// Pinned series color (`#rrggbb`). A document can keep one entity on one
    /// color across every chart it draws (e.g. a company's identity color);
    /// unset falls back to the theme palette cycle.
    pub color: Option<String>,
}

/// A parsed, validated chart spec.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartSpec {
    pub kind: ChartKind,
    pub title: String,
    pub subtitle: String,
    pub x: Vec<String>,
    pub series: Vec<ChartSeries>,
    pub unit: ChartUnit,
    pub height: u16,
    /// Force whole-number axis ticks. Counts are the common case: left to its own
    /// devices the backend divides the range into six, so "4 flags" gets an axis
    /// labelled 0.9, 1.8, 2.7 — ticks that cannot exist in the data.
    pub int_axis: bool,
    /// Fine print rendered by the EMITTERS below the chart (never inside the
    /// SVG — `subtitle` draws in the plot band and feeds the legend-margin
    /// math). The place for data caveats: basis, scale choices, what the
    /// numbers cannot say.
    pub caption: String,
}

/// Is this fence a chart fence?
///
/// Mirrors [`crate::admonish::parse_admonish_info`]'s standalone-keyword rule:
/// `charts` and `chartjs` are ordinary code blocks, `chart` is ours. Trailing
/// tokens are tolerated and ignored — every setting lives in the body, so an
/// info string carrying more than the keyword is a guide-side mistake worth
/// rendering anyway rather than a reason to drop the chart.
pub fn is_chart_info(info: &str) -> bool {
    match info.trim().strip_prefix("chart") {
        Some(rest) => rest.is_empty() || rest.starts_with(char::is_whitespace),
        None => false,
    }
}

/// Parse a chart fence body. `None` when the body is not a usable spec; the
/// reason is logged and the caller degrades to a code block.
pub fn parse_chart_spec(body: &str) -> Option<ChartSpec> {
    let value: Value = match serde_json::from_str(body.trim()) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("chart fence body is not valid JSON, rendering as code block: {e}");
            return None;
        }
    };

    let kind_str = value.get("type").and_then(Value::as_str).unwrap_or("bar");
    let Some(kind) = ChartKind::from_str(kind_str) else {
        log::warn!("unknown chart type '{kind_str}', rendering as code block");
        return None;
    };

    let x: Vec<String> = value
        .get("x")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(json_label).collect())
        .unwrap_or_default();

    let raw_series = value.get("series").and_then(Value::as_array);
    let Some(raw_series) = raw_series else {
        log::warn!("chart spec has no series array, rendering as code block");
        return None;
    };

    let mut series = Vec::new();
    for (i, s) in raw_series.iter().enumerate() {
        let name = s
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let data: Vec<f32> = s
            .get("data")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(json_number).collect())
            .unwrap_or_default();
        if data.is_empty() {
            log::warn!("chart series {i} ('{name}') has no data, dropping it");
            continue;
        }
        let color = s
            .get("color")
            .and_then(Value::as_str)
            .map(str::trim)
            .and_then(|c| {
                if is_hex_color(c) {
                    Some(c.to_ascii_lowercase())
                } else {
                    log::warn!("chart series '{name}' color '{c}' is not #rrggbb, ignoring it");
                    None
                }
            });
        series.push(ChartSeries { name, data, color });
    }
    if series.is_empty() {
        log::warn!("chart spec has no series with data, rendering as code block");
        return None;
    }

    // Align every series to the axis. A short series is padded with zeros and a
    // long one is truncated, so a mismatched spec draws a slightly wrong chart
    // instead of panicking inside the backend or silently dropping a category.
    let x = if x.is_empty() {
        (1..=series.iter().map(|s| s.data.len()).max().unwrap_or(0))
            .map(|i| i.to_string())
            .collect()
    } else {
        x
    };
    for s in &mut series {
        if s.data.len() != x.len() {
            log::warn!(
                "chart series '{}' has {} points against {} axis entries, aligning",
                s.name,
                s.data.len(),
                x.len()
            );
            s.data.resize(x.len(), 0.0);
        }
    }

    let height = value
        .get("height")
        .and_then(Value::as_u64)
        .map(|h| h.clamp(160, 900) as u16)
        .unwrap_or(DEFAULT_CHART_HEIGHT);

    Some(ChartSpec {
        kind,
        title: value
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
        subtitle: value
            .get("subtitle")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
        x,
        series,
        unit: ChartUnit::from_str(value.get("unit").and_then(Value::as_str).unwrap_or("")),
        height,
        int_axis: value
            .get("int_axis")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        caption: value
            .get("caption")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
    })
}

/// Exactly `#` plus six hex digits — the one color form the spec accepts.
fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// A JSON array entry as an axis label: strings verbatim, numbers stringified,
/// anything else blank rather than `null`.
fn json_label(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// A JSON array entry as a chart value. Non-numeric and non-finite entries
/// become 0.0: a hole in a series must not move the axis to infinity.
fn json_number(v: &Value) -> f32 {
    let n = match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    };
    if n.is_finite() { n as f32 } else { 0.0 }
}

/// Register the document's own font with the chart backend, once.
///
/// `charts-rs` keeps its font map in a `OnceLock`, so the first call wins for
/// the life of the process; this crate is the only caller.
fn init_font() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        if let Err(e) = charts_rs::get_or_try_init_fonts(Some(vec![CHART_FONT])) {
            log::warn!("chart font registration failed, charts fall back to the default face: {e}");
        }
    });
}

/// Translate a spec into the chart backend's own JSON, theme applied.
///
/// Split out from [`render_chart_svg`] so the layout contract is assertable
/// without parsing an SVG back apart.
fn build_chart_json(spec: &ChartSpec) -> Value {
    let stacked = spec.kind == ChartKind::Stacked;
    let series_json: Vec<Value> = spec
        .series
        .iter()
        .map(|s| {
            let mut o = serde_json::Map::new();
            o.insert("name".into(), Value::String(s.name.clone()));
            o.insert(
                "data".into(),
                Value::Array(
                    s.data
                        .iter()
                        .map(|v| {
                            serde_json::Number::from_f64(*v as f64)
                                .map(Value::Number)
                                .unwrap_or(Value::Number(0.into()))
                        })
                        .collect(),
                ),
            );
            if stacked {
                // Series sharing a stack name are stacked on one another.
                o.insert("stack".into(), Value::String("total".into()));
            }
            Value::Object(o)
        })
        .collect();

    let mut root = serde_json::Map::new();
    root.insert("width".into(), Value::from(CHART_WIDTH));
    root.insert("height".into(), Value::from(spec.height));
    root.insert("font_family".into(), Value::String(FONT_SANS.to_string()));
    root.insert("title_text".into(), Value::String(spec.title.clone()));
    root.insert(
        "sub_title_text".into(),
        Value::String(spec.subtitle.clone()),
    );
    root.insert("title_font_color".into(), Value::String(SLATE_800.into()));
    root.insert(
        "sub_title_font_color".into(),
        Value::String(SLATE_600.into()),
    );
    root.insert("legend_font_color".into(), Value::String(SLATE_600.into()));
    root.insert("x_axis_font_color".into(), Value::String(SLATE_600.into()));
    root.insert(
        "x_axis_stroke_color".into(),
        Value::String(SLATE_200.into()),
    );
    root.insert("grid_stroke_color".into(), Value::String(SLATE_200.into()));
    // charts-rs assigns series_colors positionally, so a per-index list makes
    // a spec-pinned color a per-series override with no backend changes.
    root.insert(
        "series_colors".into(),
        Value::Array(
            spec.series
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    Value::String(match &s.color {
                        Some(c) => c.clone(),
                        None => CHART_SERIES_COLORS[i % CHART_SERIES_COLORS.len()].to_string(),
                    })
                })
                .collect(),
        ),
    );
    let mut y_axis = serde_json::json!({
        "axis_font_color": SLATE_600,
        "axis_stroke_color": SLATE_200,
        "axis_formatter": spec.unit.formatter(),
    });
    if spec.int_axis {
        // Round the top of the axis up to a clean multiple of the split count, so every
        // tick lands on a whole number. Stacked series share an x position, so the peak
        // is the column TOTAL, not the largest single value.
        let peak = if stacked {
            (0..spec.x.len())
                .map(|i| {
                    spec.series
                        .iter()
                        .map(|s| s.data.get(i).copied().unwrap_or(0.0).max(0.0))
                        .sum::<f32>()
                })
                .fold(0.0f32, f32::max)
        } else {
            spec.series
                .iter()
                .flat_map(|s| s.data.iter())
                .fold(0.0f32, |a, b| a.max(*b))
        };
        let peak = peak.ceil().max(1.0) as u32;
        let splits = peak.clamp(1, 6);
        let top = peak.div_ceil(splits) * splits;
        y_axis["axis_max"] = Value::from(top);
        y_axis["axis_min"] = Value::from(0);
        y_axis["axis_split_number"] = Value::from(splits);
    }
    root.insert("y_axis_configs".into(), Value::Array(vec![y_axis]));
    root.insert(
        "x_axis_data".into(),
        Value::Array(spec.x.iter().map(|s| Value::String(s.clone())).collect()),
    );
    root.insert("series_list".into(), Value::Array(series_json));
    // A single unnamed series has nothing to distinguish, so the legend is
    // noise; the title already says what is plotted.
    if spec.series.len() < 2 {
        root.insert("legend_show".into(), Value::Bool(false));
    } else {
        // The backend draws the title and the legend in the same top band and
        // does NOT reserve space for both, so an unset margin overprints the
        // legend across the title. Every upstream example sets this by hand;
        // clear the title band (30) plus the subtitle band (20) when present.
        let top = 5.0
            + if spec.title.is_empty() { 0.0 } else { 30.0 }
            + if spec.subtitle.is_empty() { 0.0 } else { 20.0 };
        root.insert(
            "legend_margin".into(),
            serde_json::json!({ "top": top, "bottom": 10.0 }),
        );
    }
    if spec.kind == ChartKind::Line {
        root.insert("series_smooth".into(), Value::Bool(false));
    }
    Value::Object(root)
}

/// Render a parsed spec to an SVG document. `None` when the backend refuses,
/// which the caller degrades on.
pub fn render_chart_svg(spec: &ChartSpec) -> Option<String> {
    init_font();
    let json = build_chart_json(spec).to_string();
    let result = match spec.kind {
        ChartKind::Line => charts_rs::LineChart::from_json(&json).and_then(|c| c.svg()),
        ChartKind::HBar => charts_rs::HorizontalBarChart::from_json(&json).and_then(|c| c.svg()),
        ChartKind::Bar | ChartKind::Stacked | ChartKind::Diverging => {
            charts_rs::BarChart::from_json(&json).and_then(|c| c.svg())
        }
    };

    match result {
        Ok(svg) => Some(svg),
        Err(e) => {
            log::warn!("chart rendering failed, falling back to a code block: {e}");
            None
        }
    }
}

/// A rendered figure ready for an emitter: the SVG document plus the fine
/// print to set below it (empty = no caption). Shared by charts and diagrams
/// so both take the same caption path through the emitters.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderedFigure {
    pub svg: String,
    pub caption: String,
}

/// Fence body → figure, or `None` to degrade. The one entry point both
/// emitters use. A caption problem can never fail a render: the caption is
/// carried as data and the emitters escape it themselves.
pub fn render_chart_fence(body: &str) -> Option<RenderedFigure> {
    let spec = parse_chart_spec(body)?;
    let svg = render_chart_svg(&spec)?;
    Some(RenderedFigure {
        svg,
        caption: spec.caption,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE_SPEC: &str = r#"{
        "type": "line",
        "title": "Revenue",
        "x": ["FY2022", "FY2023"],
        "series": [{ "name": "Revenue", "data": [1.1, 1.4] }],
        "unit": "usd"
    }"#;

    #[test]
    fn chart_keyword_must_stand_alone() {
        assert!(is_chart_info("chart"));
        assert!(is_chart_info("  chart  "));
        assert!(is_chart_info("chart bar"));
        assert!(!is_chart_info("charts"));
        assert!(!is_chart_info("chartjs"));
        assert!(!is_chart_info("rust"));
        assert!(!is_chart_info(""));
    }

    #[test]
    fn parses_a_full_spec() {
        let spec = parse_chart_spec(LINE_SPEC).unwrap();
        assert_eq!(spec.kind, ChartKind::Line);
        assert_eq!(spec.title, "Revenue");
        assert_eq!(spec.unit, ChartUnit::Usd);
        assert_eq!(spec.x, vec!["FY2022", "FY2023"]);
        assert_eq!(spec.series.len(), 1);
        assert_eq!(spec.series[0].data, vec![1.1, 1.4]);
        assert_eq!(spec.height, DEFAULT_CHART_HEIGHT);
    }

    #[test]
    fn type_defaults_to_bar_and_aliases_resolve() {
        let spec = parse_chart_spec(r#"{"series":[{"name":"a","data":[1]}]}"#).unwrap();
        assert_eq!(spec.kind, ChartKind::Bar);
        for (token, want) in [
            ("hbar", ChartKind::HBar),
            ("horizontal-bar", ChartKind::HBar),
            ("stacked", ChartKind::Stacked),
            ("stacked-bar", ChartKind::Stacked),
            ("diverging", ChartKind::Diverging),
        ] {
            let body = format!(r#"{{"type":"{token}","series":[{{"name":"a","data":[1]}}]}}"#);
            assert_eq!(parse_chart_spec(&body).unwrap().kind, want, "{token}");
        }
    }

    #[test]
    fn malformed_specs_degrade() {
        // Not JSON at all.
        assert!(parse_chart_spec("not json").is_none());
        // Unknown chart type.
        assert!(
            parse_chart_spec(r#"{"type":"sankey","series":[{"name":"a","data":[1]}]}"#).is_none()
        );
        // No series key.
        assert!(parse_chart_spec(r#"{"type":"bar","x":["a"]}"#).is_none());
        // Series present but every one is empty.
        assert!(parse_chart_spec(r#"{"series":[{"name":"a","data":[]}]}"#).is_none());
    }

    #[test]
    fn series_are_aligned_to_the_axis() {
        // Short series pads, long series truncates: never a panic, never a
        // silently dropped category.
        let spec = parse_chart_spec(
            r#"{"x":["a","b","c"],"series":[
                {"name":"short","data":[1]},
                {"name":"long","data":[1,2,3,4,5]}
            ]}"#,
        )
        .unwrap();
        assert_eq!(spec.series[0].data, vec![1.0, 0.0, 0.0]);
        assert_eq!(spec.series[1].data, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn axis_is_synthesized_when_absent() {
        let spec = parse_chart_spec(r#"{"series":[{"name":"a","data":[5,6,7]}]}"#).unwrap();
        assert_eq!(spec.x, vec!["1", "2", "3"]);
    }

    #[test]
    fn non_numeric_and_non_finite_values_become_zero() {
        let spec =
            parse_chart_spec(r#"{"x":["a","b","c"],"series":[{"name":"n","data":[1,null,"2"]}]}"#)
                .unwrap();
        assert_eq!(spec.series[0].data, vec![1.0, 0.0, 2.0]);
    }

    #[test]
    fn height_is_clamped() {
        let tall =
            parse_chart_spec(r#"{"height":99999,"series":[{"name":"a","data":[1]}]}"#).unwrap();
        assert_eq!(tall.height, 900);
        let short = parse_chart_spec(r#"{"height":1,"series":[{"name":"a","data":[1]}]}"#).unwrap();
        assert_eq!(short.height, 160);
    }

    #[test]
    fn int_axis_rounds_ticks_to_whole_numbers() {
        // Counts must not get an axis labelled 0.9 / 1.8 / 2.7.
        let spec = parse_chart_spec(
            r#"{"type":"stacked","int_axis":true,"x":["a","b"],
                "series":[{"name":"one","data":[1,2]},{"name":"two","data":[2,2]}]}"#,
        )
        .unwrap();
        assert!(spec.int_axis);
        let y = &build_chart_json(&spec)["y_axis_configs"][0];
        // Stacked peak is the column total (2+2), not the largest single value.
        assert_eq!(y["axis_max"], 4);
        assert_eq!(y["axis_split_number"], 4);
        assert_eq!(y["axis_min"], 0);
        // Off by default, so dollar charts keep the backend's own scaling.
        let plain = parse_chart_spec(LINE_SPEC).unwrap();
        assert!(!plain.int_axis);
        assert!(
            build_chart_json(&plain)["y_axis_configs"][0]
                .get("axis_max")
                .is_none()
        );
    }

    #[test]
    fn unit_drives_the_axis_formatter() {
        assert_eq!(ChartUnit::from_str("usd").formatter(), "${c}");
        assert_eq!(ChartUnit::from_str("pct").formatter(), "{c}%");
        assert_eq!(ChartUnit::from_str("shares").formatter(), "");
        assert_eq!(ChartUnit::from_str("nonsense"), ChartUnit::Plain);
    }

    #[test]
    fn legend_clears_the_title_and_subtitle_bands() {
        // Regression: the backend draws title and legend in the same top band
        // and reserves space for neither, so without this margin the legend
        // overprints the title. 5 gap + 30 title + 20 subtitle.
        let spec = parse_chart_spec(
            r#"{"title":"T","subtitle":"S","x":["a"],
                "series":[{"name":"one","data":[1]},{"name":"two","data":[2]}]}"#,
        )
        .unwrap();
        let json = build_chart_json(&spec);
        assert_eq!(json["legend_margin"]["top"], 55.0);

        // Title only: no subtitle band to clear.
        let spec = parse_chart_spec(
            r#"{"title":"T","x":["a"],
                "series":[{"name":"one","data":[1]},{"name":"two","data":[2]}]}"#,
        )
        .unwrap();
        assert_eq!(build_chart_json(&spec)["legend_margin"]["top"], 35.0);
    }

    #[test]
    fn a_lone_series_drops_the_legend() {
        let spec = parse_chart_spec(LINE_SPEC).unwrap();
        let json = build_chart_json(&spec);
        assert_eq!(json["legend_show"], false);
        assert!(json.get("legend_margin").is_none());
    }

    #[test]
    fn stacking_is_applied_only_to_the_stacked_kind() {
        let body = |kind: &str| {
            format!(
                r#"{{"type":"{kind}","x":["a"],
                    "series":[{{"name":"one","data":[1]}},{{"name":"two","data":[2]}}]}}"#
            )
        };
        let stacked = build_chart_json(&parse_chart_spec(&body("stacked")).unwrap());
        assert_eq!(stacked["series_list"][0]["stack"], "total");
        assert_eq!(stacked["series_list"][1]["stack"], "total");
        let grouped = build_chart_json(&parse_chart_spec(&body("bar")).unwrap());
        assert!(grouped["series_list"][0].get("stack").is_none());
    }

    #[test]
    fn renders_svg_naming_the_embedded_font() {
        let svg = render_chart_fence(LINE_SPEC).unwrap().svg;
        assert!(svg.starts_with("<svg"), "not an svg document: {svg:.80}");
        assert!(svg.contains("xmlns"));
        assert!(svg.contains("viewBox"));
        // The PDF path rasterizes SVG text against Typst's own font book, which
        // has Liberation Sans and not Roboto.
        assert!(svg.contains(FONT_SANS), "chart does not name {FONT_SANS}");
        assert!(!svg.contains("Roboto"));
        assert!(svg.contains("Revenue"));
    }

    #[test]
    fn every_kind_renders() {
        for kind in ["line", "bar", "hbar", "stacked", "diverging"] {
            let body = format!(
                r#"{{"type":"{kind}","title":"T","x":["a","b"],
                    "series":[{{"name":"one","data":[1,-2]}},{{"name":"two","data":[3,4]}}]}}"#
            );
            let figure =
                render_chart_fence(&body).unwrap_or_else(|| panic!("{kind} did not render"));
            assert!(figure.svg.starts_with("<svg"), "{kind}");
        }
    }

    #[test]
    fn caption_and_pinned_series_colors_ride_the_spec() {
        let body = r##"{
            "type": "line", "x": ["FY2022", "FY2023"],
            "series": [
                { "name": "ZIP", "data": [100, 140], "color": "#EA580C" },
                { "name": "NOW", "data": [100, 122] }
            ],
            "caption": "Indexed to 100; scales differ."
        }"##;
        let spec = parse_chart_spec(body).unwrap();
        assert_eq!(spec.caption, "Indexed to 100; scales differ.");
        assert_eq!(spec.series[0].color.as_deref(), Some("#ea580c"));
        assert_eq!(spec.series[1].color, None);
        let json = build_chart_json(&spec);
        // Pinned color at its own index; the unpinned series falls back to the
        // theme cycle at ITS index (not the next unused color).
        assert_eq!(json["series_colors"][0], "#ea580c");
        assert_eq!(json["series_colors"][1], CHART_SERIES_COLORS[1]);
        let figure = render_chart_fence(body).unwrap();
        assert_eq!(figure.caption, "Indexed to 100; scales differ.");
        // charts-rs normalizes hex to uppercase in the emitted SVG.
        assert!(
            figure.svg.to_ascii_lowercase().contains("ea580c"),
            "pinned color must reach the svg"
        );
    }

    #[test]
    fn a_bad_series_color_is_ignored_not_fatal() {
        let body = r#"{
            "type": "bar", "x": ["a"],
            "series": [ { "name": "one", "data": [1], "color": "tomato" } ]
        }"#;
        let spec = parse_chart_spec(body).expect("bad color must not drop the chart");
        assert_eq!(spec.series[0].color, None);
        assert!(render_chart_fence(body).is_some());
    }

    #[test]
    fn a_bad_fence_body_never_panics_and_yields_none() {
        assert!(render_chart_fence("").is_none());
        assert!(render_chart_fence("{").is_none());
        assert!(render_chart_fence("[]").is_none());
        assert!(render_chart_fence("null").is_none());
    }
}
