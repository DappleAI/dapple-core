//! The `svg shapes` command — extract basic shape geometry from an SVG.
//!
//! Pure pipe mechanics: SVG text in, one record per `circle`/`ellipse`/`rect`
//! element out, in document order, normalized to 0..1 of the canvas (viewBox,
//! else width/height attributes). No semantics: what a shape MEANS — a mask
//! window, a layout mark — is the calling guide's policy. This exists so guide
//! tasks can compile their own composition geometry without any backend code
//! knowing their vocabulary.

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct SvgShapesCommand;

impl Command for SvgShapesCommand {
    fn name(&self) -> &str {
        "svg shapes"
    }

    fn signature(&self) -> Signature {
        Signature::build("svg shapes")
            .input_output_types(vec![(Type::String, Type::list(Type::record()))])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Extract circle/ellipse/rect geometry from SVG text, normalized to the canvas."
    }

    fn extra_description(&self) -> &str {
        r#"Reads every circle, ellipse, and rect element in document order and
returns one record per shape:

  kind  - "circle" | "ellipse" | "rect"
  id    - the element's id attribute, else positional "shape-N" (1-based)
  cx cy - shape centre, 0..1 of the canvas
  rx ry - half-extents, 0..1 of the canvas width/height
  x y   - bounding-box top-left, 0..1
  w h   - bounding-box size, 0..1

Zero-size shapes are skipped. An SVG with no shapes returns an empty list
(whether that is an error is the caller's policy). Fails on unparsable XML or
a document with no usable viewBox / width+height.

Example:
  asset text $mask_id | svg shapes | where kind != rect
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["svg", "shapes", "circle", "ellipse", "rect", "geometry"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let text = input.into_value(span)?.coerce_into_string()?;

        let shapes = parse_shapes(&text).map_err(|msg| ShellError::GenericError {
            error: "svg shapes: invalid SVG".into(),
            msg,
            span: Some(span),
            help: None,
            inner: vec![],
        })?;

        let vals = shapes.into_iter().map(|s| s.into_value(span)).collect();
        Ok(PipelineData::Value(Value::list(vals, span), None))
    }
}

/// One basic shape, normalized to 0..1 of the canvas.
#[derive(Debug, Clone, PartialEq)]
struct Shape {
    kind: &'static str,
    id: String,
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
}

impl Shape {
    fn into_value(self, span: Span) -> Value {
        Value::record(
            record! {
                "kind" => Value::string(self.kind, span),
                "id" => Value::string(self.id, span),
                "cx" => Value::float(self.cx, span),
                "cy" => Value::float(self.cy, span),
                "rx" => Value::float(self.rx, span),
                "ry" => Value::float(self.ry, span),
                "x" => Value::float(self.cx - self.rx, span),
                "y" => Value::float(self.cy - self.ry, span),
                "w" => Value::float(self.rx * 2.0, span),
                "h" => Value::float(self.ry * 2.0, span),
            },
            span,
        )
    }
}

/// The SVG canvas box (user units) — viewBox, else width/height attributes.
fn canvas_box(root: roxmltree::Node<'_, '_>) -> Result<(f64, f64, f64, f64), String> {
    if let Some(vb) = root.attribute("viewBox") {
        let parts: Vec<f64> = vb
            .split([' ', ','])
            .filter(|s| !s.is_empty())
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|e| format!("bad viewBox '{vb}': {e}"))?;
        if parts.len() != 4 || parts[2] <= 0.0 || parts[3] <= 0.0 {
            return Err(format!("bad viewBox '{vb}'"));
        }
        return Ok((parts[0], parts[1], parts[2], parts[3]));
    }
    let dim = |name: &str| -> Option<f64> {
        root.attribute(name)
            .map(|s| s.trim_end_matches("px"))
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| *v > 0.0)
    };
    match (dim("width"), dim("height")) {
        (Some(w), Some(h)) => Ok((0.0, 0.0, w, h)),
        _ => Err("no usable viewBox or width/height".into()),
    }
}

/// Parse every circle/ellipse/rect in document order, normalized 0..1.
fn parse_shapes(text: &str) -> Result<Vec<Shape>, String> {
    let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    let (min_x, min_y, width, height) = canvas_box(root)?;

    let attr = |n: roxmltree::Node<'_, '_>, name: &str| -> Option<f64> {
        n.attribute(name).and_then(|s| s.trim().parse::<f64>().ok())
    };

    let mut shapes: Vec<Shape> = Vec::new();
    for node in root.descendants().filter(roxmltree::Node::is_element) {
        let (kind, cx, cy, rx, ry) = match node.tag_name().name() {
            "circle" => {
                let r = match attr(node, "r") {
                    Some(r) if r > 0.0 => r,
                    _ => continue,
                };
                (
                    "circle",
                    attr(node, "cx").unwrap_or(0.0),
                    attr(node, "cy").unwrap_or(0.0),
                    r,
                    r,
                )
            }
            "ellipse" => {
                let (rx, ry) = match (attr(node, "rx"), attr(node, "ry")) {
                    (Some(rx), Some(ry)) if rx > 0.0 && ry > 0.0 => (rx, ry),
                    _ => continue,
                };
                (
                    "ellipse",
                    attr(node, "cx").unwrap_or(0.0),
                    attr(node, "cy").unwrap_or(0.0),
                    rx,
                    ry,
                )
            }
            "rect" => {
                let (w, h) = match (attr(node, "width"), attr(node, "height")) {
                    (Some(w), Some(h)) if w > 0.0 && h > 0.0 => (w, h),
                    _ => continue,
                };
                let x = attr(node, "x").unwrap_or(0.0);
                let y = attr(node, "y").unwrap_or(0.0);
                ("rect", x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0)
            }
            _ => continue,
        };
        let id = node
            .attribute("id")
            .map_or_else(|| format!("shape-{}", shapes.len() + 1), str::to_string);
        shapes.push(Shape {
            kind,
            id,
            cx: (cx - min_x) / width,
            cy: (cy - min_y) / height,
            rx: rx / width,
            ry: ry / height,
        });
    }
    Ok(shapes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100">
      <rect id="frame" x="10" y="10" width="20" height="20" fill="none"/>
      <circle id="alpha" cx="50" cy="50" r="20" fill="#123456"/>
      <circle cx="150" cy="50" r="10" fill="black"/>
      <ellipse id="wide" cx="100" cy="25" rx="30" ry="10" fill="red"/>
    </svg>"##;

    #[test]
    fn parses_shapes_normalized_in_document_order() {
        let shapes = parse_shapes(SVG).unwrap();
        assert_eq!(shapes.len(), 4);

        assert_eq!(shapes[0].kind, "rect");
        assert_eq!(shapes[0].id, "frame");
        assert!((shapes[0].cx - 0.1).abs() < 1e-9); // (10+10)/200
        assert!((shapes[0].cy - 0.2).abs() < 1e-9); // (10+10)/100
        assert!((shapes[0].rx - 0.05).abs() < 1e-9); // 10/200
        assert!((shapes[0].ry - 0.1).abs() < 1e-9); // 10/100

        assert_eq!(shapes[1].kind, "circle");
        assert_eq!(shapes[1].id, "alpha");
        assert!((shapes[1].cx - 0.25).abs() < 1e-9);
        assert!((shapes[1].cy - 0.5).abs() < 1e-9);
        assert!((shapes[1].rx - 0.1).abs() < 1e-9); // 20/200
        assert!((shapes[1].ry - 0.2).abs() < 1e-9); // 20/100

        // No id → positional fallback counts ALL shapes, 1-based.
        assert_eq!(shapes[2].id, "shape-3");
        assert!((shapes[2].cx - 0.75).abs() < 1e-9);

        assert_eq!(shapes[3].kind, "ellipse");
        assert_eq!(shapes[3].id, "wide");
        assert!((shapes[3].rx - 0.15).abs() < 1e-9); // 30/200
        assert!((shapes[3].ry - 0.1).abs() < 1e-9); // 10/100
    }

    #[test]
    fn empty_svg_returns_empty_list() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><g/></svg>"#;
        assert_eq!(parse_shapes(svg).unwrap(), vec![]);
    }

    #[test]
    fn zero_size_shapes_are_skipped() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
          <circle cx="1" cy="1" r="0"/><rect x="1" y="1" width="0" height="5"/>
        </svg>"#;
        assert_eq!(parse_shapes(svg).unwrap(), vec![]);
    }

    #[test]
    fn width_height_fallback_strips_px() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100px" height="50px">
          <circle cx="50" cy="25" r="5"/>
        </svg>"#;
        let shapes = parse_shapes(svg).unwrap();
        assert!((shapes[0].cx - 0.5).abs() < 1e-9);
        assert!((shapes[0].cy - 0.5).abs() < 1e-9);
    }

    #[test]
    fn rejects_garbage_and_missing_canvas() {
        assert!(parse_shapes("not svg").is_err());
        let no_box =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><circle cx="1" cy="1" r="1"/></svg>"#;
        assert!(parse_shapes(no_box).is_err());
    }
}
