use crate::consumer::Endpoint;
use dapple_data::model::guides::Guide;
use dapple_prompt_api::schema::guides::{
    CreateGuideBlockSchema, CreateGuideChapterSchema, CreateGuideSchema, CreateGuideSetSchema,
    GetGuideSchema, GetGuideSetSchema,
};
use reqwest::Url;
use std::ops::Range;
use uuid::Uuid;

use crate::consumer::error::ConsumerError;
use log::info;

pub struct GuideEndpoint {
    pub base_url: Url,
}

impl GuideEndpoint {
    /// Generate a fake block with a random directive type and optional tags.
    /// Fluent-native: content is optional. For test purposes, prose blocks
    /// still get inline content; other types may get None.
    fn fake_block(directive_type: &str, tags: Vec<&str>) -> CreateGuideBlockSchema {
        CreateGuideBlockSchema {
            directive_type: directive_type.to_string(),
            name: if directive_type == "prose" {
                None
            } else {
                Some(Self::fake_name())
            },
            content: Some(Self::fake_sentences(2..5)),
            attributes: Some(serde_json::json!({})),
            fluent_message_id: None,
            tags: if tags.is_empty() {
                None
            } else {
                Some(tags.into_iter().map(String::from).collect())
            },
        }
    }

    /// Generate a fake chapter with a mix of block types.
    fn fake_chapter(
        title: &str,
        source_file: &str,
        parent_index: Option<usize>,
    ) -> CreateGuideChapterSchema {
        // Create 2-4 blocks per chapter with varied directive types
        let blocks = vec![
            Self::fake_block("prose", vec!["universal"]),
            Self::fake_block("prose", vec!["agent", "product"]),
        ];

        CreateGuideChapterSchema {
            title: title.to_string(),
            subtitle: None,
            source_file: source_file.to_string(),
            parent_index,
            blocks,
        }
    }
}

impl Endpoint for GuideEndpoint {
    type Object = Guide;
    type GetSchema = GetGuideSchema;
    type GetSetSchema = GetGuideSetSchema;
    type CreateSchema = CreateGuideSchema;
    type CreateSetSchema = CreateGuideSetSchema;

    fn path(&self) -> &str {
        "guides"
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
            assert_eq!(obj.slug, fetched.slug, "slug mismatch");
            assert_eq!(
                obj.content_hash, fetched.content_hash,
                "content_hash mismatch"
            );

            // Verify chapter count matches
            assert_eq!(
                obj.chapters.len(),
                fetched.chapters.len(),
                "chapter count mismatch for guide '{}'",
                obj.title,
            );

            // Verify blocks within each chapter
            for (ch_created, ch_fetched) in obj.chapters.iter().zip(fetched.chapters.iter()) {
                assert_eq!(ch_created.title, ch_fetched.title, "chapter title mismatch");
                assert_eq!(
                    ch_created.source_file, ch_fetched.source_file,
                    "chapter source_file mismatch"
                );
                assert_eq!(
                    ch_created.blocks.len(),
                    ch_fetched.blocks.len(),
                    "block count mismatch for chapter '{}'",
                    ch_created.title,
                );

                for (blk_created, blk_fetched) in
                    ch_created.blocks.iter().zip(ch_fetched.blocks.iter())
                {
                    assert_eq!(
                        blk_created.directive_type, blk_fetched.directive_type,
                        "block directive_type mismatch"
                    );
                    assert_eq!(
                        blk_created.content, blk_fetched.content,
                        "block content mismatch"
                    );
                }
            }
        }
        Ok(())
    }

    async fn test_returned_post_set(
        &self,
        schema: &Self::CreateSetSchema,
        objs: &Vec<Self::Object>,
    ) -> Result<(), ConsumerError> {
        assert_eq!(
            schema.objs.len(),
            objs.len(),
            "POST response count mismatch"
        );

        for (s, o) in schema.objs.iter().zip(objs.iter()) {
            assert_eq!(s.title, o.title, "title mismatch");
            assert_eq!(s.description, o.description, "description mismatch");
            assert_eq!(
                s.chapters.len(),
                o.chapters.len(),
                "chapter count mismatch for guide '{}'",
                s.title,
            );

            // Verify content_hash is a valid 40-char hex hash
            assert_eq!(
                o.content_hash.to_string().len(),
                40,
                "new guide content_hash should be a 40-char hex SHA-1 hash"
            );

            // Verify a slug was generated (derived from the source_path
            // directory basename, falling back to the title when absent).
            assert!(
                !o.slug.is_empty(),
                "slug should not be empty for guide '{}'",
                s.title,
            );

            // Verify chapters and blocks were created
            for (ch_schema, ch_obj) in s.chapters.iter().zip(o.chapters.iter()) {
                assert_eq!(ch_schema.title, ch_obj.title, "chapter title mismatch");
                assert_eq!(
                    ch_schema.source_file, ch_obj.source_file,
                    "chapter source_file mismatch"
                );
                assert_eq!(
                    ch_schema.blocks.len(),
                    ch_obj.blocks.len(),
                    "block count mismatch for chapter '{}'",
                    ch_schema.title,
                );

                // Verify parent_chapter_id for nested chapters
                if ch_schema.parent_index.is_some() {
                    assert!(
                        ch_obj.parent_chapter_id.is_some(),
                        "nested chapter '{}' should have parent_chapter_id",
                        ch_obj.title,
                    );
                }

                // Verify block details
                for (blk_schema, blk_obj) in ch_schema.blocks.iter().zip(ch_obj.blocks.iter()) {
                    assert_eq!(
                        blk_schema.directive_type, blk_obj.directive_type,
                        "block directive_type mismatch"
                    );
                    assert_eq!(blk_schema.name, blk_obj.name, "block name mismatch");
                    assert_eq!(
                        blk_schema.content, blk_obj.content,
                        "block content mismatch"
                    );

                    // Verify tags were created
                    let expected_tags = blk_schema.tags.clone().unwrap_or_default();
                    for expected_tag in &expected_tags {
                        assert!(
                            blk_obj.tags.contains(expected_tag),
                            "block should contain tag '{}', found {:?}",
                            expected_tag,
                            blk_obj.tags,
                        );
                    }
                }
            }
        }

        info!("POSTed set of {} guides successfully verified", objs.len());
        Ok(())
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        // Generate a unique guide title to avoid unique constraint conflicts
        let guide_name = Self::fake_name();
        let guide_title = format!("{} Guide", guide_name);

        // Create a guide with 2-3 chapters including one nested chapter
        let chapters = vec![
            Self::fake_chapter(
                "Introduction",
                &format!("{}/intro.md", guide_name.to_lowercase().replace(' ', "-")),
                None, // top-level
            ),
            Self::fake_chapter(
                "Getting Started",
                &format!(
                    "{}/getting-started.md",
                    guide_name.to_lowercase().replace(' ', "-")
                ),
                None, // top-level
            ),
            Self::fake_chapter(
                "Setup Details",
                &format!(
                    "{}/getting-started/setup.md",
                    guide_name.to_lowercase().replace(' ', "-")
                ),
                Some(1), // child of "Getting Started" (index 1)
            ),
        ];

        CreateGuideSchema {
            title: guide_title,
            description: Some(Self::fake_description()),
            author: Some("Test Author".to_string()),
            classification: Some("workflow".to_string()),
            source_path: Some(format!(
                "docs/books/{}",
                guide_name.to_lowercase().replace(' ', "-")
            )),
            chapters,
        }
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = size.start;

        CreateGuideSetSchema {
            objs: (0..fakes).map(|_| self.fake_create_schema()).collect(),
        }
    }
}
