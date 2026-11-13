pub mod api_tokens;
pub mod auth_token;
pub mod companies;
pub mod dispatches;
pub mod external_service_tokens;
pub mod service_token;

pub mod dispatch_task_attempts;

pub mod playbooks;

pub mod categories;
pub mod tasks;

pub mod user_details;

pub mod journey_asset_uploads;
pub mod journeys;

pub mod steps;

pub mod hats;

pub mod stages;

pub mod fluent_messages;
pub mod guide_ingest;
pub mod guide_render;
pub mod guide_search;
pub mod guides;

pub mod assets;
pub mod brief_groups;
pub mod brief_packs;
pub mod brief_units;
pub mod briefs;
pub mod canonical;
pub mod deprecations;
pub mod directives;
pub mod dispatch_intent_bindings;
pub mod inference_profiles;

pub mod prompts;
pub mod user_favorite_guides;
pub mod user_favorite_journey_chains;
pub mod value_slots;
pub mod versioned_objects;

use aide::openapi::{
    Parameter, ParameterData, ParameterSchemaOrContent, ReferenceOr, SchemaObject,
};
use aide::transform::TransformOperation;
use schemars::JsonSchema;
use serde::Serialize;

fn param_schema_from<T: JsonSchema>() -> ParameterSchemaOrContent {
    let root = schemars::schema_for!(T);

    let schema_obj = SchemaObject {
        json_schema: root, // this is the actual JSON Schema for T
        external_docs: None,
        example: None,
    };

    ParameterSchemaOrContent::Schema(schema_obj)
}

#[derive(Clone)]
pub struct ParamDoc<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub required: bool,
    pub schema: fn() -> ParameterSchemaOrContent,
}

impl From<ParamDoc<'_>> for Parameter {
    fn from(val: ParamDoc<'_>) -> Self {
        Parameter::Query {
            parameter_data: ParameterData {
                name: val.name.into(),
                description: Some(val.description.into()),
                required: val.required,
                deprecated: None,
                format: (val.schema)(),
                example: None,
                examples: Default::default(),
                explode: None,
                extensions: Default::default(),
            },
            allow_reserved: false,
            style: Default::default(), // QueryStyle::Form
            allow_empty_value: None,
        }
    }
}

/// Trait for "this enum drives a query param + related params in docs".
pub trait QueryParamDocs: JsonSchema + Serialize + Sized {
    /// Extra query params that go alongside the mode param.
    fn extra_params() -> &'static [ParamDoc<'static>];

    /// Attach all query-parameter documentation to this operation.
    fn apply_query_param_docs(mut op: TransformOperation) -> TransformOperation {
        let operation = op.inner_mut();

        // 1) The `query` (mode) parameter
        {
            let pd = ParamDoc {
                name: "query",
                description: "A JSON object containing all needed query parameters.`).",
                required: false,
                schema: || param_schema_from::<Self>(),
            };

            operation.parameters.push(ReferenceOr::Item(pd.into()));
        }

        // 2) Extra parameters (`ids`, `options`, `playbook_id`, ...).
        // Note: there is no documented `query_name` parameter — the
        // `QsQuery` extractor reads the `query` tag declared above and
        // nothing else. A previous `ParamDoc { name: "query_name", ... }`
        // was removed because it advertised a parameter the extractor
        // never consumed; a downstream consumer (dapple-ui) wrote
        // `?query_name=by_ids&...` against that documentation and the
        // request silently 400'd, masquerading as a working architecture.
        for pdoc in Self::extra_params() {
            operation
                .parameters
                .push(ReferenceOr::Item(pdoc.clone().into()));
        }

        op
    }
}
