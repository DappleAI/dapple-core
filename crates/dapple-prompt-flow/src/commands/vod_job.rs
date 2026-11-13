//! `vod job` — poll a media-gateway job by id (for `--async` follow-up).

use nu_engine::command_prelude::*;
use uuid::Uuid;

use crate::commands::media_gateway::{poll_job, to_pipeline};

#[derive(Clone)]
pub struct VodJobCommand;

impl Command for VodJobCommand {
    fn name(&self) -> &str {
        "vod job"
    }

    fn signature(&self) -> Signature {
        Signature::build("vod job")
            .required("job_id", SyntaxShape::String, "The job id to poll")
            .input_output_types(vec![(Type::Any, Type::record())])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Poll a media-gateway job by id (status + result when done)."
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let id_str: String = call.req(engine_state, stack, 0)?;
        let job_id = Uuid::parse_str(id_str.trim()).map_err(|e| ShellError::GenericError {
            error: "Invalid job id".into(),
            msg: format!("'{id_str}' is not a valid UUID: {e}"),
            span: Some(span),
            help: None,
            inner: vec![],
        })?;
        let result = poll_job(job_id, span)?;
        Ok(to_pipeline(&result))
    }
}
