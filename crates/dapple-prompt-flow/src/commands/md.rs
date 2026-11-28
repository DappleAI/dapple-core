//! Parent command for markdown emission.
//!
//! Usage:
//!   md table <headers>            - Build a GFM table from piped rows
//!   md admonish <kind> [title]    - Wrap piped markdown in an admonition callout

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct MdCommand;

impl Command for MdCommand {
    fn name(&self) -> &str {
        "md"
    }

    fn signature(&self) -> Signature {
        Signature::build("md")
            .input_output_types(vec![(Type::Nothing, Type::String)])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Build the markdown furniture a guide's report is assembled from."
    }

    fn extra_description(&self) -> &str {
        r#"Guide scripts cannot import each other, so shared markdown emission
lives here rather than being copy-pasted between guides. These produce the
markdown `dapple-render` reads: GFM tables and mdbook-admonish callouts.

Subcommands:
  md table <headers>          - GFM table from piped rows; drops all-empty columns
  md admonish <kind> [title]  - admonition callout around piped markdown
  md src <source>             - tag a value with its source ([text](src:source))
  md fact <source> <display>  - tag an `edgar facts` row, keeping its value as filed
  md attribute <id>           - attribute a passage or chart to one source
  md provenance               - declare the sources (registry fence)

The last four are provenance tags: invisible in the PDF and HTML, read by
`document render` into a JSON provenance manifest beside the PDF and HTML.

Both follow the same doctrine: NOTHING TO DRAW RENDERS AS "". A caller tests the
result and omits its own heading with it, so a prop can never cost the report.

  let t = ($rows | md table ["Year" "Filing"])
  if ($t | is-not-empty) { $out = $out + $heading + $nl + $nl + $t }

Not here on purpose:
  - slug minting. The guides mint explicit {#tl-<id>} anchors and their minter
    deliberately differs from the renderer's GFM slugger; converging them would
    break anchor uniqueness.
  - `band` as a heading grouper. One guide's `band` emits an `#### label` section
    rather than a callout; only the callout half is shared, as
    `md admonish --fence 4`.
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["md", "markdown", "table", "admonish", "report", "dapple"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string("Run 'md --help' for usage information", call.head),
            None,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(MdCommand.name(), "md");
    }

    #[test]
    fn test_command_description() {
        assert!(!MdCommand.description().is_empty());
    }
}
