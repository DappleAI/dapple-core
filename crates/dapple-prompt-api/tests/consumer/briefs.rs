use fake::Fake;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use crate::consumer::{Endpoint, ScenarioContext};
use dapple_data::model::briefs::Brief;
use dapple_prompt_api::schema::brief_units::{
    CreateBriefUnitInputSchema, CreateBriefUnitSchema, CreateBriefUnitSubtypeSchema,
};
use dapple_prompt_api::schema::briefs::{
    CreateBriefOutputSchema, CreateBriefSchema, CreateBriefSetSchema, GetBriefSetSchema,
};
use reqwest::Url;
use serde_json::json;
use uuid::Uuid;

use crate::consumer::error::ConsumerError;

// ============================================================================
// Brief Endpoint (creates path atomically, with inline units + outputs)
// ============================================================================

pub struct BriefEndpoint {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
}

impl BriefEndpoint {
    fn fake_create_schema_with_child_count(&self, count: usize) -> CreateBriefSchema {
        // Get value_slots from context to use as output targets (if available).
        // value_slots are an optional dependency for outputs — empty is OK,
        // we just don't emit any outputs in that case (zip below truncates).
        let value_slot_ids: Vec<Uuid> = {
            let ctx = self.context.lock().unwrap_or_else(|p| p.into_inner());
            ctx.value_slot_batches
                .iter()
                .flatten()
                .take(count)
                .map(|vs| vs.id)
                .collect()
        };

        // Build `count` units, each an input with a text control
        let units: Vec<CreateBriefUnitSchema> = (0..count)
            .map(|_| {
                let unit_key = format!("input-{}", Uuid::now_v7());
                CreateBriefUnitSchema {
                    unit_key,
                    layout: json!({}),
                    requires: json!({}),
                    extensions: json!({}),
                    subtype: CreateBriefUnitSubtypeSchema::Input(CreateBriefUnitInputSchema {
                        control_type: "text".to_string(),
                        label_text: Some("Test Input Label".to_string()),
                        label_fluent_message_id: None,
                        help_text: Some("Enter test data".to_string()),
                        help_fluent_message_id: None,
                        placeholder_text: Some("Type here...".to_string()),
                        placeholder_fluent_message_id: None,
                        control_spec: json!({}),
                        choices: vec![],
                    }),
                }
            })
            .collect();

        // Build `count` outputs (one per unit), only if we have enough value_slots
        let outputs: Vec<CreateBriefOutputSchema> = units
            .iter()
            .zip(value_slot_ids.iter())
            .map(|(unit, vs_id)| CreateBriefOutputSchema {
                unit_key: unit.unit_key.clone(),
                value_slot_id: *vs_id,
                source_kind: "input".to_string(),
                source_path: "/value".to_string(),
                transform: json!({}),
                display: None,
                reveal: false,
                title: None,
                group: None,
                format: None,
            })
            .collect();

        CreateBriefSchema {
            name: Self::fake_name(),
            title: Self::fake_name(),
            description: Some(Self::fake_description()),
            units,
            outputs,
        }
    }
}

impl Endpoint for BriefEndpoint {
    type Object = Brief;
    type GetSchema = GetBriefSetSchema;
    type GetSetSchema = GetBriefSetSchema;
    type CreateSchema = CreateBriefSchema;
    type CreateSetSchema = CreateBriefSetSchema;

    fn path(&self) -> &str {
        "briefs"
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
            assert_eq!(obj.id, fetched.id, "id mismatch");
            assert_eq!(obj.path_id, fetched.path_id, "path_id mismatch");
            assert_eq!(obj.title, fetched.title, "title mismatch");
            assert_eq!(
                obj.content_hash, fetched.content_hash,
                "content_hash mismatch"
            );
            // Verify units are returned if any were created
            assert_eq!(obj.units.len(), fetched.units.len(), "units count mismatch");
            // Verify outputs are returned if any were created
            assert_eq!(
                obj.outputs.len(),
                fetched.outputs.len(),
                "outputs count mismatch"
            );
        }
        Ok(())
    }

    async fn test_returned_post_set(
        &self,
        schema: &Self::CreateSetSchema,
        objs: &Vec<Self::Object>,
    ) -> Result<(), ConsumerError> {
        assert_eq!(schema.objs.len(), objs.len(), "created count mismatch");
        // Match schema items to response items by name (since id ordering may differ)
        for s in schema.objs.iter() {
            let matching = objs.iter().find(|o| o.name == s.name);
            if let Some(o) = matching {
                assert_eq!(
                    s.units.len(),
                    o.units.len(),
                    "units count mismatch for brief {}",
                    o.id
                );
                assert_eq!(
                    s.outputs.len(),
                    o.outputs.len(),
                    "outputs count mismatch for brief {}",
                    o.id
                );
            }
        }
        self.test_returned_set(objs).await
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        self.fake_create_schema_with_child_count(1)
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = (size).fake::<usize>();

        CreateBriefSetSchema {
            objs: (0..fakes)
                .map(|i| {
                    let count = if i == 0 { 1 } else { 3 };
                    self.fake_create_schema_with_child_count(count)
                })
                .collect(),
        }
    }

    fn fake_name() -> String {
        use fake::faker::name::en::Name;
        Name().fake()
    }
}
