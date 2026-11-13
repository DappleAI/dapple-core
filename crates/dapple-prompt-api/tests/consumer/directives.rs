use fake::Fake;
use serde_json::json;
use std::ops::Range;

use crate::consumer::Endpoint;
use dapple_data::model::directives::Directive;
use dapple_prompt_api::schema::directives::{
    CreateDirectiveSchema, CreateDirectiveSetSchema, GetDirectiveSetSchema,
};
use reqwest::Url;
use uuid::Uuid;

use crate::consumer::error::ConsumerError;

// ============================================================================
// Directive Endpoint (creates path atomically)
// ============================================================================

pub struct DirectiveEndpoint {
    pub base_url: Url,
}

impl Endpoint for DirectiveEndpoint {
    type Object = Directive;
    type GetSchema = GetDirectiveSetSchema;
    type GetSetSchema = GetDirectiveSetSchema;
    type CreateSchema = CreateDirectiveSchema;
    type CreateSetSchema = CreateDirectiveSetSchema;

    fn path(&self) -> &str {
        "directives"
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
            assert_eq!(
                obj.max_attempts, fetched.max_attempts,
                "max_attempts mismatch"
            );
        }
        Ok(())
    }

    async fn test_returned_post_set(
        &self,
        _schema: &Self::CreateSetSchema,
        objs: &Vec<Self::Object>,
    ) -> Result<(), ConsumerError> {
        self.test_returned_set(objs).await
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        // Create a simple "run and proceed on success, fail on error" directive
        let logic = json!({
            "if": [
                { "==": [{ "var": { "run": [] } }, { "status": "ok" }] },
                "proceed",
                "fail"
            ]
        });

        CreateDirectiveSchema {
            name: Self::fake_name(),
            logic,
            max_attempts: Some(1),
            retry_delay_ms: None,
            description: Some(Self::fake_description()),
        }
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = (size).fake::<usize>();

        CreateDirectiveSetSchema {
            objs: (0..fakes).map(|_| self.fake_create_schema()).collect(),
        }
    }

    fn fake_name() -> String {
        use fake::faker::name::en::Name;
        Name().fake()
    }
}
