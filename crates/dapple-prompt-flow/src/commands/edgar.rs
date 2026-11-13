//! The `edgar` parent command (see `edgar filings`, `edgar facts`).

use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct EdgarCommand;

impl Command for EdgarCommand {
    fn name(&self) -> &str {
        "edgar"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar").category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "SEC EDGAR access (filings sectioned by Item, XBRL facts, fund holdings)."
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::Value(
            Value::string(
                "Subcommands: `edgar filings`, `edgar facts`, `edgar exhibits`, `edgar holders`, \
                 `edgar insiders`, `edgar segments`, `edgar fund`, `edgar nport`, `edgar ncen`, \
                 `edgar nmfp`, `edgar nmfp-universe`, `edgar fund-sections`, `edgar resolve`, `edgar company-cik`, \
                 `edgar search`, `edgar adv`, `edgar advised`, `edgar portfolio`, `edgar bdc`, `edgar book`, `edgar manager`, `edgar security`. \
                 Run any with --help. Requires DAPPLE_EDGAR_USER_AGENT.",
                call.head,
            ),
            None,
        ))
    }
}
