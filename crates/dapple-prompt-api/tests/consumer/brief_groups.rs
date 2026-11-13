use fake::Fake;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use crate::consumer::{Endpoint, ScenarioContext};
use dapple_data::model::brief_groups::BriefGroup;
use dapple_prompt_api::schema::brief_groups::{
    CreateBriefGroupSchema, CreateBriefGroupSetSchema, GetBriefGroupSetSchema,
};
use reqwest::Url;
use uuid::Uuid;

use crate::consumer::error::ConsumerError;

// ============================================================================
// BriefGroup Endpoint (creates path atomically, references Briefs)
// ============================================================================

pub struct BriefGroupEndpoint {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
}

impl BriefGroupEndpoint {
    fn fake_create_schema_with_child_count(&self, count: usize) -> CreateBriefGroupSchema {
        let brief_ids: Vec<Uuid> = {
            let ctx = self.context.lock().unwrap_or_else(|p| p.into_inner());
            ctx.brief_batches
                .iter()
                .flat_map(|batch| batch.iter().map(|b| b.id))
                .take(count)
                .collect()
        };

        CreateBriefGroupSchema {
            name: Self::fake_name(),
            title: Self::fake_name(),
            description: Some(Self::fake_description()),
            brief_ids,
        }
    }
}

impl Endpoint for BriefGroupEndpoint {
    type Object = BriefGroup;
    type GetSchema = GetBriefGroupSetSchema;
    type GetSetSchema = GetBriefGroupSetSchema;
    type CreateSchema = CreateBriefGroupSchema;
    type CreateSetSchema = CreateBriefGroupSetSchema;

    fn path(&self) -> &str {
        "brief-groups"
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
                obj.briefs.len(),
                fetched.briefs.len(),
                "briefs count mismatch"
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
                    s.brief_ids.len(),
                    o.briefs.len(),
                    "briefs count mismatch for group {}",
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

        CreateBriefGroupSetSchema {
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
