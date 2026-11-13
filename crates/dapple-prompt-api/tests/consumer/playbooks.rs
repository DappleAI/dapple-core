use crate::consumer::{Endpoint, ScenarioContext};
use dapple_data::model::playbooks::Playbook;
use dapple_prompt_api::schema::playbooks::{
    CreatePlaybookSchema, CreatePlaybookSetSchema, CreatePlaybookStageSchema,
    CreatePlaybookStageStepSchema, CreatePlaybookTaskSchema, GetPlaybookSchema,
    GetPlaybookSetSchema,
};
use reqwest::Url;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use fake::Fake;

use crate::consumer::error::ConsumerError;
use log::info;

pub struct PlaybookEndpoint {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
}

impl PlaybookEndpoint {
    fn fake_create_playbook_stages_schema(&self, count: usize) -> Vec<CreatePlaybookStageSchema> {
        let mut out = Vec::with_capacity(count);
        {
            let guard = self.context.lock().unwrap_or_else(|p| p.into_inner());

            for _ in 0..count {
                // fail early if there are no steps available in the scenario context
                let s = guard.random_step_from_any_batch()
                    .expect("test setup: no steps available in ScenarioContext for playbook step generation");

                let tasks = s
                    .tasks
                    .iter()
                    .map(|t| CreatePlaybookTaskSchema { task_id: t.task.id })
                    .collect();

                let step = CreatePlaybookStageStepSchema {
                    step_id: s.id,
                    params: serde_json::json!({}),
                    tasks,
                };

                out.push(CreatePlaybookStageSchema { steps: vec![step] });
            }
        }
        out
    }
}

impl Endpoint for PlaybookEndpoint {
    type Object = Playbook;
    type GetSchema = GetPlaybookSchema;
    type GetSetSchema = GetPlaybookSetSchema;
    type CreateSchema = CreatePlaybookSchema;
    type CreateSetSchema = CreatePlaybookSetSchema;

    fn path(&self) -> &str {
        "playbooks"
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
        for obj in objs {
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
        assert_eq!(schema.objs.len(), objs.len());

        for s in schema.objs.iter() {
            let o = objs
                .iter()
                .find(|o| o.name == s.name)
                .expect("response missing playbook with matching name");
            assert_eq!(s.description, o.description, "description");
            assert_eq!(
                s.stages.len(),
                o.stages.len(),
                "stages count mismatch: schema had {} stages but response had {}",
                s.stages.len(),
                o.stages.len()
            );
        }

        info!("requested count: {}", schema.objs.len());
        info!(
            "POSTed set of {} playbooks successfully verified",
            objs.len()
        );
        info!("response results size: {}", objs.len());
        info!(
            "Created playbooks IDs: {:?}",
            objs.iter().map(|o| o.id).collect::<Vec<Uuid>>()
        );
        info!(
            "Playbook stage counts: {:?}",
            objs.iter().map(|o| o.stages.len()).collect::<Vec<usize>>()
        );

        Ok(())
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        // Create a minimal playbook schema; stages will be attached by fake_create_set_schema
        CreatePlaybookSchema {
            name: Self::fake_name(),
            description: Self::fake_description(),
            stages: vec![],
            categories: vec![],
        }
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = (size).fake::<usize>();

        // build categories from ScenarioContext `categories-init` batch; pick a random number of categories between 0 and 5 for each playbook
        use rand::Rng;
        let mut rng = rand::thread_rng();

        let mut categories_pool: Vec<uuid::Uuid> = Vec::new();
        {
            let guard = self.context.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(batch_handle) = guard.category_named_batches.get("categories-init") {
                // collect all categories in that batch
                if let Some(batch) = guard.category_batches.get(batch_handle.index) {
                    categories_pool = batch.iter().map(|c| c.id).collect();
                }
            }
        }

        Self::CreateSetSchema {
            objs: (0..fakes)
                .map(|i| {
                    let mut schema = self.fake_create_schema();
                    // First playbook: 1 stage, subsequent: 3 stages
                    let stage_count = if i == 0 { 1 } else { 3 };
                    schema.stages = self.fake_create_playbook_stages_schema(stage_count);

                    // choose a random number of categories up to 6
                    let pick = rng.gen_range(0..6);
                    if !categories_pool.is_empty() && pick > 0 {
                        let pick = std::cmp::min(pick, categories_pool.len());
                        use rand::seq::SliceRandom;
                        schema.categories = categories_pool
                            .choose_multiple(&mut rng, pick)
                            .cloned()
                            .collect();
                    }

                    schema
                })
                .collect(),
        }
    }
}
