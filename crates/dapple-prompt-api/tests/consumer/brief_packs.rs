use fake::Fake;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use crate::consumer::{Endpoint, ScenarioContext};
use dapple_data::model::brief_packs::BriefPack;
use dapple_prompt_api::schema::brief_packs::{
    BriefGroupReferenceSchema, CreateBriefPackSchema, CreateBriefPackSetSchema,
    GetBriefPackSetSchema,
};
use reqwest::Url;
use uuid::Uuid;

use crate::consumer::error::ConsumerError;

// ============================================================================
// BriefPack Endpoint (creates path atomically, references BriefGroups)
// ============================================================================

pub struct BriefPackEndpoint {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
}

impl BriefPackEndpoint {
    fn fake_create_schema_with_child_count(&self, count: usize) -> CreateBriefPackSchema {
        let group_refs: Vec<BriefGroupReferenceSchema> = {
            let ctx = self.context.lock().unwrap_or_else(|p| p.into_inner());
            ctx.brief_group_batches
                .iter()
                .flat_map(|batch| batch.iter().map(|g| g.id))
                .take(count)
                .map(|id| BriefGroupReferenceSchema {
                    id,
                    ui_overrides: serde_json::json!({}),
                })
                .collect()
        };

        CreateBriefPackSchema {
            name: Self::fake_name(),
            title: Self::fake_name(),
            description: Some(Self::fake_description()),
            pack_spec: serde_json::json!({"mode": "test"}),
            groups: group_refs,
        }
    }
}

impl Endpoint for BriefPackEndpoint {
    type Object = BriefPack;
    type GetSchema = GetBriefPackSetSchema;
    type GetSetSchema = GetBriefPackSetSchema;
    type CreateSchema = CreateBriefPackSchema;
    type CreateSetSchema = CreateBriefPackSetSchema;

    fn path(&self) -> &str {
        "brief-packs"
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
            assert_eq!(
                obj.groups.len(),
                fetched.groups.len(),
                "groups count mismatch"
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
        for s in schema.objs.iter() {
            let matching = objs.iter().find(|o| o.name == s.name);
            if let Some(o) = matching {
                assert_eq!(
                    s.groups.len(),
                    o.groups.len(),
                    "groups count mismatch for pack {}",
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

        CreateBriefPackSetSchema {
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
