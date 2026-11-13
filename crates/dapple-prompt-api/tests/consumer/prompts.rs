use fake::Fake;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use crate::consumer::{Endpoint, ScenarioContext};
use dapple_data::model::prompts::Prompt;
use dapple_prompt_api::schema::prompts::{
    CreatePromptFluentMessageSchema, CreatePromptSchema, CreatePromptSetSchema, GetPromptSetSchema,
};
use reqwest::Url;
use uuid::Uuid;

use crate::consumer::error::ConsumerError;

// ============================================================================
// Prompt Endpoint (creates path atomically)
// ============================================================================

pub struct PromptEndpoint {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
}

impl PromptEndpoint {
    fn fake_create_schema_with_child_count(&self, count: usize) -> CreatePromptSchema {
        let fluent_messages: Vec<CreatePromptFluentMessageSchema> = {
            let ctx = self.context.lock().unwrap_or_else(|p| p.into_inner());
            ctx.fluent_message_batches
                .iter()
                .flatten()
                .take(count)
                .enumerate()
                .map(|(i, fm)| CreatePromptFluentMessageSchema {
                    fluent_message_id: fm.id,
                    role: Some(
                        match i % 3 {
                            0 => "system",
                            1 => "user",
                            _ => "assistant",
                        }
                        .to_string(),
                    ),
                })
                .collect()
        };

        CreatePromptSchema {
            name: Self::fake_name(),
            description: Some(Self::fake_description()),
            fluent_messages,
        }
    }
}

impl Endpoint for PromptEndpoint {
    type Object = Prompt;
    type GetSchema = GetPromptSetSchema;
    type GetSetSchema = GetPromptSetSchema;
    type CreateSchema = CreatePromptSchema;
    type CreateSetSchema = CreatePromptSetSchema;

    fn path(&self) -> &str {
        "prompts"
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
            assert_eq!(obj.name, fetched.name, "name mismatch");
            assert_eq!(
                obj.content_hash, fetched.content_hash,
                "content_hash mismatch"
            );
            // Verify fluent_messages are returned correctly
            assert_eq!(
                obj.fluent_messages.len(),
                fetched.fluent_messages.len(),
                "fluent_messages count mismatch"
            );
            for (created_fm, fetched_fm) in obj
                .fluent_messages
                .iter()
                .zip(fetched.fluent_messages.iter())
            {
                assert_eq!(
                    created_fm.fluent_message_id, fetched_fm.fluent_message_id,
                    "fluent_message_id mismatch"
                );
                assert_eq!(created_fm.role, fetched_fm.role, "role mismatch");
            }
        }
        Ok(())
    }

    async fn test_returned_post_set(
        &self,
        schema: &Self::CreateSetSchema,
        objs: &Vec<Self::Object>,
    ) -> Result<(), ConsumerError> {
        assert_eq!(schema.objs.len(), objs.len(), "created count mismatch");
        for s in schema.objs.iter() {
            let matching = objs.iter().find(|o| o.name == s.name);
            if let Some(o) = matching {
                assert_eq!(
                    s.fluent_messages.len(),
                    o.fluent_messages.len(),
                    "fluent_messages count mismatch for prompt {}",
                    o.id
                );
            }
        }
        self.test_returned_set(objs).await
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        self.fake_create_schema_with_child_count(2)
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = (size).fake::<usize>();

        CreatePromptSetSchema {
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
