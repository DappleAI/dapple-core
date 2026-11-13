// filepath: /home/kenny/src/dapple/dapple-prompt/crates/dapple-prompt-api/tests/consumer/hats.rs
use crate::consumer::{Endpoint, ScenarioContext};
use dapple_data::model::hats::Hat;
use dapple_prompt_api::schema::hats::{
    CreateHatSchema, CreateHatSetSchema, GetHatSchema, GetHatSetSchema,
};
use reqwest::Url;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::consumer::error::ConsumerError;
use log::info;

pub struct HatEndpoint {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
}

impl Endpoint for HatEndpoint {
    type Object = Hat;
    type GetSchema = GetHatSchema;
    type GetSetSchema = GetHatSetSchema;
    type CreateSchema = CreateHatSchema;
    type CreateSetSchema = CreateHatSetSchema;

    fn path(&self) -> &str {
        "hats"
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

        for (s, o) in schema.objs.iter().zip(objs.iter()) {
            assert_eq!(s.name, o.name, "name");
            assert_eq!(s.description, o.description, "description");
            // categories may be empty
            if s.categories.is_empty() {
                assert!(o.categories.is_empty());
            } else {
                // all requested categories should be present on the returned object
                for c in s.categories.iter() {
                    // `o.categories` is Vec<CategoryReference>; check presence by id
                    assert!(o.categories.iter().any(|r| &r.id == c));
                }
            }
        }

        info!("POSTed set of {} hats successfully verified", objs.len());
        Ok(())
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        // pick up categories from ScenarioContext if available
        let guard = self.context.lock().unwrap_or_else(|p| p.into_inner());
        let mut categories_pool: Vec<uuid::Uuid> = Vec::new();
        if let Some(batch_handle) = guard.category_named_batches.get("categories-init")
            && let Some(batch) = guard.category_batches.get(batch_handle.index)
        {
            categories_pool = batch.iter().map(|c| c.id).collect();
        }

        // choose 0 or some categories deterministically
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let pick = if categories_pool.is_empty() {
            0
        } else {
            rng.gen_range(0..=categories_pool.len())
        };
        let cats = if pick == 0 {
            vec![]
        } else {
            categories_pool.into_iter().take(pick).collect()
        };

        CreateHatSchema {
            name: Self::fake_name(),
            description: Self::fake_description(),
            categories: cats,
        }
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = size.start;

        Self::CreateSetSchema {
            objs: (0..fakes).map(|_| self.fake_create_schema()).collect(),
        }
    }
}
