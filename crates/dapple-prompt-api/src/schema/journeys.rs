use crate::schema::{ParamDoc, QueryParamDocs, param_schema_from};
use dapple_data::FilterOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateJourneySetSchema {
    pub objs: Vec<CreateJourneySchema>,
}

/// PATCH /api/journeys/{id} body. Updates the viewing principal's display
/// name override; the underlying `rev.journeys.name` is never mutated.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct UpdateJourneySchema {
    pub name: String,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct CreateJourneySchema {
    pub name: String,
    pub playbook_id: Uuid,
    /// Ordered list of brief pack IDs to attach (optional)
    #[serde(default)]
    pub brief_pack_ids: Vec<Uuid>,
    /// Ordered list of inference profile IDs to attach (optional)
    #[serde(default)]
    pub inference_profile_ids: Vec<Uuid>,
    /// Journey-chain primitive (Migration 44). When set, this new journey
    /// is chained from the named predecessor and the effective-binding
    /// reader walks back through that link for slot pre-fill. Must be a
    /// journey the caller owns (same `principal_id`/`owner_company_id`)
    /// and its `guide_id` must match the `guide_id` below.
    #[serde(default)]
    pub predecessor_journey_id: Option<Uuid>,
    /// Stage-level fork point (Migration 74). Optional; requires
    /// `predecessor_journey_id`. Must be a `rev.playbook_stages.id`
    /// belonging to the PREDECESSOR's playbook. When set, this journey
    /// inherits only the predecessor bindings produced by stages strictly
    /// BEFORE the named stage ("fork at stage N" = redo N) — the fork
    /// opens with earlier panes done and stage N onward clean. When
    /// omitted, chaining inherits the predecessor's whole binding pool
    /// (pre-74 behavior).
    #[serde(default)]
    pub fork_stage_id: Option<Uuid>,
    /// The specific `rev.guides.id` version this journey is pinned to.
    /// REQUIRED: the caller supplies the exact guide object it navigated to
    /// (browse is canonical-pin-aware); the backend validates the guide
    /// exposes `playbook_id` and never resolves "latest"/by-path. For chained
    /// journeys it must additionally equal the predecessor's `guide_id`.
    pub guide_id: Uuid,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum GetJourneySetSchema {
    ByObjectIds {
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    },
    ByPlaybook {
        playbook_id: Uuid,
        options: Option<FilterOptions>,
    },
    ByUser {
        user_id: Uuid,
        options: Option<FilterOptions>,
    },
}

impl QueryParamDocs for GetJourneySetSchema {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[
            ParamDoc {
                name: "ids",
                description: "UUID specific to the object.",
                required: false,
                schema: || param_schema_from::<Vec<Uuid>>(),
            },
            ParamDoc {
                name: "options",
                description: "Query filter options (e.g. `limit`, `page`). Optional — when omitted a default set of filter options will be applied by the server.",
                required: false,
                schema: || param_schema_from::<FilterOptions>(),
            },
        ]
    }
}

/// Query parameters for `GET /api/journeys/chains` (Migration 44 Phase 2).
/// Scopes the chain listing to one `rev.guides.id` (specific version) so
/// concurrent essays across different guides don't bleed into each
/// other's tiles.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct ChainsIndexQuery {
    pub guide_id: Uuid,
}

impl QueryParamDocs for ChainsIndexQuery {
    fn extra_params() -> &'static [ParamDoc<'static>] {
        &[ParamDoc {
            name: "guide_id",
            description: "Specific `rev.guides.id` (version UUID) to scope the chain listing to.",
            required: true,
            schema: || param_schema_from::<Uuid>(),
        }]
    }
}
