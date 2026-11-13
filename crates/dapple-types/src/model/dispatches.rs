use crate::reference::StepReference;
use crate::{DataTransferObject, DateTime, JsonObject};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public DTO for dispatch task
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct DispatchTask {
    pub id: Uuid,
    pub dispatch_step: StepReference,
    pub task: crate::reference::TaskReference,
    pub created_at: DateTime,
    pub status: Option<String>,
}

/// Public DTO for dispatch step
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct DispatchStep {
    pub id: Uuid,
    pub dispatch_id: Uuid,
    pub step: StepReference,
    pub playbook_id: Uuid,
    #[schemars(with = "JsonObject")]
    pub params: serde_json::Value,
    pub created_at: DateTime,
    pub tasks: Vec<DispatchTask>,
}

/// Public DTO for dispatch stage
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct DispatchStage {
    pub id: Uuid,
    pub dispatch_id: Uuid,
    #[schemars(with = "JsonObject")]
    pub params: serde_json::Value,
    pub created_at: DateTime,
    pub steps: Vec<DispatchStep>,
}

/// Public DTO for dispatch
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct Dispatch {
    pub id: Uuid,
    pub journey_id: Uuid,
    #[schemars(with = "JsonObject")]
    pub intent: serde_json::Value,
    pub created_at: DateTime,
    pub stages: Vec<DispatchStage>,
}

impl DataTransferObject for DispatchTask {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for DispatchStep {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl DataTransferObject for Dispatch {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
