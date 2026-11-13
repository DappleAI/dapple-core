//! The `document render` command — render piped markdown into a PDF **and** a
//! self-contained HTML page, both stored as `document` assets via the conduit
//! `POST /render-document` endpoint.
//!
//! This is how a guide task produces a report deliverable: the returned
//! `asset_id` (PDF) goes at a computed `:::brief-output`'s `source_path` with
//! `display: file` for a download card, and `html_asset_id` at another output
//! with `display: document` so the inline viewer shows the fully rendered,
//! anchor-linked report as the result page.

use crate::commands::prompt_send::{get_conduit_url, get_dapple_metadata};
use crate::conduit_client::{ConduitClient, ConduitClientError, RenderDocumentRequest};
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct DocumentRenderCommand;

impl Command for DocumentRenderCommand {
    fn name(&self) -> &str {
        "document render"
    }

    fn signature(&self) -> Signature {
        Signature::build("document render")
            .input_output_types(vec![(Type::String, Type::record())])
            .required(
                "title",
                SyntaxShape::String,
                "Document title (heading of page 1)",
            )
            .named(
                "subtitle",
                SyntaxShape::String,
                "Subtitle rendered under the title",
                None,
            )
            .named(
                "author",
                SyntaxShape::String,
                "Author shown on the byline",
                None,
            )
            .named(
                "date",
                SyntaxShape::String,
                "Preformatted date display string for the byline",
                None,
            )
            .named(
                "branding",
                SyntaxShape::String,
                "Branding style: consumer, technical, or internal (default technical)",
                None,
            )
            .switch(
                "toc",
                "Force a table-of-contents page (default: automatic when the document has 9+ h1-h3 headings)",
                None,
            )
            .switch("no-toc", "Suppress the table-of-contents page", None)
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Render piped markdown to PDF + HTML document assets."
    }

    fn extra_description(&self) -> &str {
        r#"Calls the conduit /render-document endpoint. Rendering is deterministic
and local (no model): the markdown body becomes a Dapple-branded PDF and a
self-contained HTML page, both persisted as assets (asset_kind `document`;
mime `application/pdf` and `text/html`).

Markdown features beyond CommonMark + GFM tables:
- Heading anchors: every heading gets an id (GFM slugger: lowercase, spaces
  to dashes, -1/-2 on duplicates). `### Heading {#custom-id}` sets the id
  explicitly (the {#...} never renders). `[text](#id)` becomes a clickable
  internal link in the PDF and the HTML.
- Admonition callouts (mdbook-admonish syntax): fence a block with
  ```admonish <type> "Optional title"  — types: note, info, tip, warning,
  danger, success, question, failure, bug, example, quote, abstract. The body
  is markdown.
- Charts: fence a block with ```chart holding a JSON spec. Rendered to SVG and
  embedded in both outputs (no external assets, no network).
    { "type": "line" | "bar" | "hbar" | "stacked" | "diverging",
      "title": "...", "subtitle": "...",          // both optional
      "x": ["FY2022", "FY2023"],                  // optional, else 1..n
      "series": [ { "name": "Revenue", "data": [1.1, 1.4] } ],
      "unit": "usd" | "pct" | "shares" | "plain", // optional, tick labels only
      "height": 320 }                             // optional, clamped 160..900
  Pass values already scaled (1.4, not 1400000) and say the scale in the
  title. Series align to `x`: short pads with zeros, long truncates. A spec
  that will not parse renders as a plain code block instead, so a bad chart
  costs one figure and never the document.
- The PDF gets an outline (bookmarks) from headings, page numbers, a running
  header, and an optional linked TOC page (--toc / --no-toc override the
  9-heading auto threshold).

Returns a record with both assets:
- asset_id, asset_kind, mime_type, content_length   (the PDF)
- html_asset_id, html_content_length                 (the HTML page)

Wire the deliverable via computed `:::brief-output`s on `asset`-kind value
slots: `html_asset_id` with `display: document` shows the rendered report
inline as the result page; `asset_id` with `display: file` adds a PDF
download card.

Environment:
- DAPPLE_PROMPT_CONDUIT_URL: URL of the conduit service (required)
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["document", "pdf", "report", "render", "markdown", "asset"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Render report markdown to a PDF document asset",
                example: r#"$report_markdown | document render "Year over Year Review""#,
                result: None,
            },
            Example {
                description: "Render with a byline and consumer branding",
                example: r#"$body | document render "Trip Plan" --author "dapple" --date "July 2026" --branding consumer"#,
                result: None,
            },
        ]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;

        let title: String = call.req(engine_state, stack, 0)?;
        if title.trim().is_empty() {
            return Err(ShellError::GenericError {
                error: "Empty document title".into(),
                msg: "The title positional argument must be a non-empty string".into(),
                span: Some(span),
                help: None,
                inner: vec![],
            });
        }

        let markdown =
            input
                .into_value(span)?
                .coerce_into_string()
                .map_err(|_| ShellError::GenericError {
                    error: "Invalid document body".into(),
                    msg: "Pipe the markdown body into `document render` as a string".into(),
                    span: Some(span),
                    help: Some(
                        "e.g. $report_markdown | document render \"Year over Year Review\"".into(),
                    ),
                    inner: vec![],
                })?;
        if markdown.trim().is_empty() {
            return Err(ShellError::GenericError {
                error: "Empty document body".into(),
                msg: "The piped markdown body is empty".into(),
                span: Some(span),
                help: Some("Build the report markdown before piping it in".into()),
                inner: vec![],
            });
        }

        let subtitle: Option<String> = call.get_flag(engine_state, stack, "subtitle")?;
        let author: Option<String> = call.get_flag(engine_state, stack, "author")?;
        let date: Option<String> = call.get_flag(engine_state, stack, "date")?;
        let branding: Option<String> = call.get_flag(engine_state, stack, "branding")?;
        let branding = branding.unwrap_or_else(|| "technical".to_string());

        let force_toc = call.has_flag(engine_state, stack, "toc")?;
        let no_toc = call.has_flag(engine_state, stack, "no-toc")?;
        let toc = match (force_toc, no_toc) {
            (true, true) => {
                return Err(ShellError::GenericError {
                    error: "Conflicting TOC flags".into(),
                    msg: "--toc and --no-toc cannot be combined".into(),
                    span: Some(span),
                    help: Some(
                        "Pass at most one; omitting both uses the automatic threshold".into(),
                    ),
                    inner: vec![],
                });
            }
            (true, false) => Some(true),
            (false, true) => Some(false),
            (false, false) => None,
        };

        let metadata = get_dapple_metadata(engine_state, stack);
        let conduit_url = get_conduit_url(&metadata);
        let client = ConduitClient::new(&conduit_url);

        let request = RenderDocumentRequest {
            title,
            markdown,
            subtitle,
            author,
            date,
            branding,
            toc,
        };

        let response = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { client.render_document(request).await })
        });

        match response {
            Ok(resp) => {
                let record = record! {
                    "asset_id" => Value::string(resp.asset_id.to_string(), span),
                    "asset_kind" => Value::string(resp.asset_kind, span),
                    "mime_type" => Value::string(resp.mime_type, span),
                    "content_length" => Value::int(resp.content_length, span),
                    "html_asset_id" => Value::string(resp.html_asset_id.to_string(), span),
                    "html_content_length" => Value::int(resp.html_content_length, span),
                };
                Ok(PipelineData::Value(Value::record(record, span), None))
            }
            Err(e) => Err(render_error_to_shell_error(e, span)),
        }
    }
}

/// Map a conduit client error into a shaped command error. The conduit puts
/// the real render/persist cause in the response body (internal surface), so
/// pass it through verbatim — this is the only place the author sees it.
fn render_error_to_shell_error(e: ConduitClientError, span: Span) -> ShellError {
    let (error, msg, help) = match &e {
        ConduitClientError::Http(inner) => (
            "Conduit unreachable".to_string(),
            format!("document render request failed: {inner}"),
            Some("Check DAPPLE_PROMPT_CONDUIT_URL and that the conduit pod is running".to_string()),
        ),
        ConduitClientError::DocumentRenderError { code, message } => (
            "Document render failed".to_string(),
            format!("conduit returned {code}: {message}"),
            None,
        ),
        other => (
            "Document render failed".to_string(),
            other.to_string(),
            None,
        ),
    };
    ShellError::GenericError {
        error,
        msg,
        span: Some(span),
        help,
        inner: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(DocumentRenderCommand.name(), "document render");
    }

    #[test]
    fn test_command_description() {
        assert!(!DocumentRenderCommand.description().is_empty());
    }

    #[test]
    fn test_signature_has_flags() {
        let sig = DocumentRenderCommand.signature();
        let flag_names: Vec<&str> = sig.named.iter().map(|f| f.long.as_str()).collect();
        for expected in ["subtitle", "author", "date", "branding", "toc", "no-toc"] {
            assert!(flag_names.contains(&expected), "missing --{expected}");
        }
        assert_eq!(sig.required_positional.len(), 1);
        assert_eq!(sig.required_positional[0].name, "title");
    }
}
