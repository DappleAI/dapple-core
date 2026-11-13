// ...existing code...
use crate::consumer::Endpoint;
use dapple_data::model::categories::Category;
use dapple_prompt_api::schema::categories::{
    CreateCategorySchema, CreateCategorySetSchema, GetCategorySchema, GetCategorySetSchema,
};
use reqwest::Url;
use std::ops::Range;
use uuid::Uuid;

use fake::Fake;

use crate::consumer::error::ConsumerError;
use log::info;

pub struct CategoryEndpoint {
    pub base_url: Url,
}

impl CategoryEndpoint {}

impl Endpoint for CategoryEndpoint {
    type Object = Category;
    type GetSchema = GetCategorySchema;
    type GetSetSchema = GetCategorySetSchema;
    type CreateSchema = CreateCategorySchema;
    type CreateSetSchema = CreateCategorySetSchema;

    fn path(&self) -> &str {
        "categories"
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
            "POSTed set of {} categories successfully verified",
            objs.len()
        );
        info!("response results size: {}", objs.len());
        info!(
            "Created categories IDs: {:?}",
            objs.iter().map(|o| o.id).collect::<Vec<Uuid>>()
        );

        Ok(())
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        CreateCategorySchema {
            name: Self::fake_name(),
            description: Self::fake_description(),
            function: Self::fake_words(1..2),
            function_input: Default::default(),
        }
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = (size).fake::<usize>();

        Self::CreateSetSchema {
            objs: (0..fakes).map(|_| self.fake_create_schema()).collect(),
        }
    }
}
// ...existing code...
