use crate::consumer::Endpoint;
use dapple_data::model::inference_profile::InferenceProfile;
use dapple_prompt_api::schema::inference_profiles::{
    CreateInferenceProfileSchema, CreateInferenceProfileSetSchema, GetInferenceProfileSetSchema,
};
use reqwest::Url;
use std::ops::Range;
use uuid::Uuid;

use fake::Fake;

use crate::consumer::error::ConsumerError;
use log::info;

pub struct InferenceProfileEndpoint {
    pub base_url: Url,
}

/// Placeholder GET schema (single object fetched by path param, not query)
#[derive(schemars::JsonSchema, serde::Serialize, serde::Deserialize, Debug)]
pub struct GetInferenceProfileSchema {
    id: Uuid,
}

impl Endpoint for InferenceProfileEndpoint {
    type Object = InferenceProfile;
    type GetSchema = GetInferenceProfileSchema;
    type GetSetSchema = GetInferenceProfileSetSchema;
    type CreateSchema = CreateInferenceProfileSchema;
    type CreateSetSchema = CreateInferenceProfileSetSchema;

    fn path(&self) -> &str {
        "inference-profiles"
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

        schema.objs.iter().zip(objs.iter()).for_each(|(s, o)| {
            assert_eq!(s.name, o.name, "name");
            assert_eq!(s.description, o.description, "description");
        });

        info!("requested count: {}", schema.objs.len());
        info!(
            "POSTed set of {} inference profiles successfully verified",
            objs.len()
        );
        info!(
            "Created inference profile IDs: {:?}",
            objs.iter().map(|o| o.id).collect::<Vec<Uuid>>()
        );

        Ok(())
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        // Inference profiles can be created without configs for basic testing.
        // Configs reference model_releases from seed data which may not exist
        // in the test DB, so we skip them for the basic lifecycle test.
        CreateInferenceProfileSchema {
            name: Self::fake_name(),
            description: Some(Self::fake_description()),
            configs: Vec::new(),
        }
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = (size).fake::<usize>();

        Self::CreateSetSchema {
            objs: (0..fakes).map(|_| self.fake_create_schema()).collect(),
        }
    }
}
