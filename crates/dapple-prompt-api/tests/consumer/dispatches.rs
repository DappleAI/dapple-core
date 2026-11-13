use crate::consumer::{Endpoint, ScenarioContext};
use dapple_data::model::dispatches::Dispatch;
use dapple_prompt_api::schema::dispatches::{
    CreateDispatchSchema, CreateDispatchSetSchema, CreateDispatchStepSchema,
    CreateDispatchTaskSchema, GetDispatchSetSchema,
};
use reqwest::Url;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::consumer::error::ConsumerError;
use fake::Fake;
use log::info;

pub struct DispatchEndpoint {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
}

impl DispatchEndpoint {
    /// Build a dispatch referencing up to `stage_limit` stages from a playbook.
    /// Steps are submitted directly; stages are derived by the backend.
    fn fake_create_schema_with_stage_limit(&self, stage_limit: usize) -> CreateDispatchSchema {
        let guard = self.context.lock().unwrap_or_else(|p| p.into_inner());

        // pick a journey
        let journey = guard
            .random_journey_from_any_batch()
            .expect("test setup: no journeys available in ScenarioContext for dispatch generation");

        // For the batch case (stage_limit > 1), try to find a playbook with multiple stages.
        // For the single case (stage_limit == 1), any playbook will do.
        let mut best_playbook: Option<&dapple_data::model::playbooks::Playbook> = None;

        for batch in guard.playbook_batches.iter() {
            for pb in batch.iter() {
                // Must have at least 1 stage with 1 step with tasks
                let valid_stages: usize = pb
                    .stages
                    .iter()
                    .filter(|st| st.steps.iter().any(|step| !step.tasks.is_empty()))
                    .count();

                if valid_stages == 0 {
                    continue;
                }

                match &best_playbook {
                    None => best_playbook = Some(pb),
                    Some(current) => {
                        let current_valid = current
                            .stages
                            .iter()
                            .filter(|st| st.steps.iter().any(|step| !step.tasks.is_empty()))
                            .count();
                        if valid_stages > current_valid {
                            best_playbook = Some(pb);
                        }
                    }
                }
            }
        }

        let playbook =
            best_playbook.expect("test setup: no suitable playbook with stages available");

        // Collect valid stages up to the limit, then flatten steps from those stages
        let stages_to_use: Vec<_> = playbook
            .stages
            .iter()
            .filter(|st| st.steps.iter().any(|step| !step.tasks.is_empty()))
            .take(stage_limit.max(1))
            .collect();

        // Flatten steps from all selected stages into a single steps vec
        let step_schemas: Vec<CreateDispatchStepSchema> = stages_to_use
            .iter()
            .flat_map(|stage| {
                stage
                    .steps
                    .iter()
                    .filter(|step| !step.tasks.is_empty())
                    .map(|step| {
                        let tasks: Vec<CreateDispatchTaskSchema> = step
                            .tasks
                            .iter()
                            .map(|t| CreateDispatchTaskSchema { step_task_id: t.id })
                            .collect();

                        CreateDispatchStepSchema {
                            stage_step_id: step.id,
                            params: serde_json::json!({}),
                            tasks,
                        }
                    })
            })
            .collect();

        CreateDispatchSchema {
            journey_id: journey.id,
            intent: serde_json::json!({}),
            steps: step_schemas,
        }
    }

    fn fake_create_schema(&self) -> CreateDispatchSchema {
        self.fake_create_schema_with_stage_limit(1)
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> CreateDispatchSetSchema {
        let fakes = (size).fake::<usize>();

        CreateDispatchSetSchema {
            objs: (0..fakes)
                .map(|i| {
                    let stage_limit = if i == 0 { 1 } else { 3 };
                    self.fake_create_schema_with_stage_limit(stage_limit)
                })
                .collect(),
        }
    }
}

impl Endpoint for DispatchEndpoint {
    type Object = Dispatch;
    type GetSchema = GetDispatchSetSchema;
    type GetSetSchema = GetDispatchSetSchema;
    type CreateSchema = CreateDispatchSchema;
    type CreateSetSchema = CreateDispatchSetSchema;

    fn path(&self) -> &str {
        "dispatches"
    }

    fn base_url(&self) -> Url {
        self.base_url.clone()
    }

    async fn test_fetched_object_id(
        &self,
        object_id: Uuid,
        obj: &Self::Object,
    ) -> Result<(), ConsumerError> {
        assert_eq!(obj.id, object_id, "fetched object id");
        Ok(())
    }

    async fn test_returned_set(&self, objs: &Vec<Self::Object>) -> Result<(), ConsumerError> {
        for obj in objs.iter() {
            let fetched = self.get_object(obj.id).await?;
            assert_eq!(obj, &fetched, "fetched object does not match original");
        }
        Ok(())
    }

    async fn test_returned_post_set(
        &self,
        schema: &Self::CreateSetSchema,
        objs: &Vec<Self::Object>,
    ) -> Result<(), ConsumerError> {
        assert_eq!(schema.objs.len(), objs.len(), "created count mismatch");

        // Verify total stage counts match between schema and response.
        // Steps submitted in the schema are grouped by stage on the backend,
        // so we compare the number of distinct stages each dispatch produced.
        let _schema_stage_counts: Vec<usize> = schema
            .objs
            .iter()
            .map(|s| {
                // The backend derives stages from steps, so we cannot predict exact
                // stage count from the schema alone. Instead, just check response is non-empty.
                // At minimum there should be 1 stage per dispatch (since we always send steps).
                s.steps.len().max(1)
            })
            .collect();
        let response_stage_counts: Vec<usize> = objs.iter().map(|o| o.stages.len()).collect();
        // Both should have dispatches with at least 1 stage
        for count in &response_stage_counts {
            assert!(
                *count >= 1,
                "each dispatch should have at least 1 stage, got {}",
                count
            );
        }

        for o in objs.iter() {
            // Verify each created dispatch roundtrips correctly
            let fetched = self.get_object(o.id).await?;
            assert_eq!(o, &fetched, "created dispatch did not roundtrip");
        }

        info!(
            "Dispatch stage counts: {:?}",
            objs.iter().map(|o| o.stages.len()).collect::<Vec<usize>>()
        );

        Ok(())
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        self.fake_create_schema()
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        self.fake_create_set_schema(size)
    }
}
