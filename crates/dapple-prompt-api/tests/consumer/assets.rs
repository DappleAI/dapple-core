use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use fake::Fake;
use std::ops::Range;

use crate::consumer::Endpoint;
use dapple_data::model::assets::Asset;
use dapple_prompt_api::schema::assets::{
    CreateAssetSchema, CreateAssetSetSchema, GetAssetSetSchema,
};
use reqwest::Url;
use uuid::Uuid;

use crate::consumer::error::ConsumerError;

// ============================================================================
// Asset Endpoint (creates path atomically)
// ============================================================================

pub struct AssetEndpoint {
    pub base_url: Url,
}

impl Endpoint for AssetEndpoint {
    type Object = Asset;
    type GetSchema = GetAssetSetSchema;
    type GetSetSchema = GetAssetSetSchema;
    type CreateSchema = CreateAssetSchema;
    type CreateSetSchema = CreateAssetSetSchema;

    fn path(&self) -> &str {
        "assets"
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
            assert_eq!(
                obj.content_hash, fetched.content_hash,
                "content_hash mismatch"
            );
            // Content is now stored in chunks, compare content_length instead
            assert_eq!(
                obj.content_length, fetched.content_length,
                "content_length mismatch"
            );
            assert_eq!(obj.mime_type, fetched.mime_type, "mime_type mismatch");
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
        let asset_kinds = ["image", "document", "audio", "video", "binary"];
        let kind_idx = (0..asset_kinds.len()).fake::<usize>();

        // Generate some fake binary content
        let fake_content: String = Self::fake_description();
        let contents = BASE64.encode(fake_content.as_bytes());

        // Generate a MIME type based on asset kind
        let mime_types = [
            "image/png",
            "application/pdf",
            "audio/wav",
            "video/mp4",
            "application/octet-stream",
        ];

        CreateAssetSchema {
            asset_kind: asset_kinds[kind_idx].to_string(),
            contents,
            mime_type: Some(mime_types[kind_idx].to_string()),
        }
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = (size).fake::<usize>();

        CreateAssetSetSchema {
            objs: (0..fakes).map(|_| self.fake_create_schema()).collect(),
        }
    }
}
