use fake::Fake;
use std::ops::Range;

use crate::consumer::Endpoint;
use dapple_data::model::value_slots::ValueSlot;
use dapple_prompt_api::schema::value_slots::{
    CreateValueSlotSchema, CreateValueSlotSetSchema, GetValueSlotSetSchema,
};
use reqwest::Url;
use uuid::Uuid;

use crate::consumer::error::ConsumerError;

// ============================================================================
// ValueSlot Endpoint (creates path atomically)
// ============================================================================

pub struct ValueSlotEndpoint {
    pub base_url: Url,
}

impl Endpoint for ValueSlotEndpoint {
    type Object = ValueSlot;
    type GetSchema = GetValueSlotSetSchema;
    type GetSetSchema = GetValueSlotSetSchema;
    type CreateSchema = CreateValueSlotSchema;
    type CreateSetSchema = CreateValueSlotSetSchema;

    fn path(&self) -> &str {
        "value-slots"
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
        let value_kinds = ["text", "int", "number", "bool", "json", "asset"];
        let kind_idx = (0..value_kinds.len()).fake::<usize>();

        CreateValueSlotSchema {
            name: Self::fake_name(),
            value_kind: value_kinds[kind_idx].to_string(),
            title: Self::fake_name(),
            description: Some(Self::fake_description()),
            constraints: serde_json::json!({}),
            cardinality: "single".to_string(),
            min_selections: None,
            max_selections: None,
        }
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = (size).fake::<usize>();

        CreateValueSlotSetSchema {
            objs: (0..fakes).map(|_| self.fake_create_schema()).collect(),
        }
    }

    fn fake_name() -> String {
        use fake::faker::name::en::Name;
        Name().fake()
    }
}
