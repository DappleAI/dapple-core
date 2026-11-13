use fake::Fake;
use std::ops::Range;

use crate::consumer::Endpoint;
use dapple_data::model::fluent_messages::{FluentMessage, FluentMessagePath};
use dapple_prompt_api::schema::fluent_messages::{
    CreateFluentMessageEngineBodySchema, CreateFluentMessageLocalizedBodySchema,
    CreateFluentMessagePathSchema, CreateFluentMessagePathSetSchema, CreateFluentMessageSchema,
    CreateFluentMessageSetSchema, GetFluentMessagePathSetSchema, GetFluentMessageSetSchema,
};
use reqwest::Url;
use uuid::Uuid;

use crate::consumer::error::ConsumerError;

// ============================================================================
// FluentMessagePath Endpoint
// ============================================================================

pub struct FluentMessagePathEndpoint {
    pub base_url: Url,
}

impl Endpoint for FluentMessagePathEndpoint {
    type Object = FluentMessagePath;
    type GetSchema = GetFluentMessagePathSetSchema;
    type GetSetSchema = GetFluentMessagePathSetSchema;
    type CreateSchema = CreateFluentMessagePathSchema;
    type CreateSetSchema = CreateFluentMessagePathSetSchema;

    fn path(&self) -> &str {
        "fluent-message-paths"
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
        _schema: &Self::CreateSetSchema,
        objs: &Vec<Self::Object>,
    ) -> Result<(), ConsumerError> {
        self.test_returned_set(objs).await
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        CreateFluentMessagePathSchema {
            name: Self::fake_name(),
            description: Some(format!("Test description for {}", Self::fake_name())),
        }
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = (size).fake::<usize>();

        CreateFluentMessagePathSetSchema {
            objs: (0..fakes).map(|_| self.fake_create_schema()).collect(),
        }
    }

    fn fake_name() -> String {
        use fake::faker::name::en::Name;
        Name().fake()
    }
}

// ============================================================================
// FluentMessage Endpoint
// ============================================================================

pub struct FluentMessageEndpoint {
    pub base_url: Url,
}

impl FluentMessageEndpoint {
    fn fake_create_schema_with_child_count(&self, count: usize) -> CreateFluentMessageSchema {
        // Available locales to cycle through
        let locales = ["en-US", "es-ES", "fr-FR", "de-DE", "ja-JP"];
        let lineages = [
            ("claude-3-5", "anthropic", "sonnet", Some("20241022")),
            ("claude-4", "anthropic", "opus", Some("20250401")),
            ("gpt-4o", "openai", "standard", Some("20240513")),
        ];

        let localized_bodies: Vec<CreateFluentMessageLocalizedBodySchema> = (0..count)
            .map(|i| CreateFluentMessageLocalizedBodySchema {
                locale: locales[i % locales.len()].to_string(),
                body: format!("{} body: {}", locales[i % locales.len()], Self::fake_name()),
            })
            .collect();

        let engine_bodies: Vec<CreateFluentMessageEngineBodySchema> = (0..count)
            .map(|i| {
                let (lineage, provider, tier, release) = lineages[i % lineages.len()];
                CreateFluentMessageEngineBodySchema {
                    lineage_key: lineage.to_string(),
                    provider_key: provider.to_string(),
                    tier: tier.to_string(),
                    release: release.map(|r| r.to_string()),
                    body: format!("{} optimized: {}", lineage, Self::fake_name()),
                }
            })
            .collect();

        CreateFluentMessageSchema {
            name: Self::fake_name(),
            description: Some(format!("Test description for {}", Self::fake_name())),
            localized_bodies,
            engine_bodies,
        }
    }
}

impl Endpoint for FluentMessageEndpoint {
    type Object = FluentMessage;
    type GetSchema = GetFluentMessageSetSchema;
    type GetSetSchema = GetFluentMessageSetSchema;
    type CreateSchema = CreateFluentMessageSchema;
    type CreateSetSchema = CreateFluentMessageSetSchema;

    fn path(&self) -> &str {
        "fluent-messages"
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
                obj.localized_bodies.len(),
                fetched.localized_bodies.len(),
                "localized_bodies count mismatch"
            );
            assert_eq!(
                obj.engine_bodies.len(),
                fetched.engine_bodies.len(),
                "engine_bodies count mismatch"
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
                    s.localized_bodies.len(),
                    o.localized_bodies.len(),
                    "localized_bodies count mismatch for fluent message {}",
                    o.id
                );
                assert_eq!(
                    s.engine_bodies.len(),
                    o.engine_bodies.len(),
                    "engine_bodies count mismatch for fluent message {}",
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

        CreateFluentMessageSetSchema {
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
