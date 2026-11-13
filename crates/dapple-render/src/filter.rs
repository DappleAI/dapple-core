//! Block filtering by tag set.
//!
//! A block is included in the rendered output if:
//! - The block has no tags (universal by convention), OR
//! - Any of the block's tags appear in the profile's `tag_filter` array

use dapple_data::model::guides::{GuideBlock, GuideChapter};

/// A chapter with its blocks filtered by the render profile's tag set.
#[derive(Clone, Debug)]
pub struct FilteredChapter {
    pub title: String,
    pub blocks: Vec<GuideBlock>,
}

/// Filter chapters and their blocks by the profile's tag filter.
///
/// Returns only chapters that have at least one included block after filtering.
/// A block is included if it has no tags (universal) or any of its tags appear
/// in the `tag_filter` set.
pub fn filter_chapters(chapters: &[GuideChapter], tag_filter: &[String]) -> Vec<FilteredChapter> {
    chapters
        .iter()
        .filter_map(|chapter| {
            let filtered_blocks: Vec<GuideBlock> = chapter
                .blocks
                .iter()
                .filter(|block| block_matches_filter(block, tag_filter))
                .cloned()
                .collect();

            if filtered_blocks.is_empty() {
                None
            } else {
                Some(FilteredChapter {
                    title: chapter.title.clone(),
                    blocks: filtered_blocks,
                })
            }
        })
        .collect()
}

/// Check if a block should be included based on the tag filter.
///
/// Rules:
/// - No tags on the block -> always included (universal content)
/// - Block has tags and any tag is in the filter -> included
/// - Block has tags but none are in the filter -> excluded
fn block_matches_filter(block: &GuideBlock, tag_filter: &[String]) -> bool {
    if block.tags.is_empty() {
        return true;
    }

    block.tags.iter().any(|tag| tag_filter.contains(tag))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dapple_data::DateTime;
    use serde_json::json;

    fn test_datetime() -> DateTime {
        DateTime::from(time::OffsetDateTime::UNIX_EPOCH)
    }

    fn make_block(tags: Vec<&str>) -> GuideBlock {
        GuideBlock {
            id: uuid::Uuid::nil(),
            directive_type: "prose".to_string(),
            name: None,
            content: Some("Test content".to_string()),
            attributes: json!({}),
            fluent_message_id: None,
            tags: tags.into_iter().map(String::from).collect(),
            created_at: test_datetime(),
        }
    }

    fn make_chapter(title: &str, blocks: Vec<GuideBlock>) -> GuideChapter {
        GuideChapter {
            id: uuid::Uuid::nil(),
            parent_chapter_id: None,
            title: title.to_string(),
            subtitle: None,
            source_file: format!("{}.md", title),
            blocks,
            created_at: test_datetime(),
        }
    }

    #[test]
    fn test_universal_block_always_included() {
        let block = make_block(vec![]);
        assert!(block_matches_filter(&block, &[]));
        assert!(block_matches_filter(
            &block,
            &["agent".to_string(), "product".to_string()]
        ));
    }

    #[test]
    fn test_tagged_block_included_when_matching() {
        let block = make_block(vec!["agent", "universal"]);
        let filter = vec!["agent".to_string(), "product".to_string()];
        assert!(block_matches_filter(&block, &filter));
    }

    #[test]
    fn test_tagged_block_excluded_when_not_matching() {
        let block = make_block(vec!["coder", "auditor"]);
        let filter = vec!["agent".to_string(), "product".to_string()];
        assert!(!block_matches_filter(&block, &filter));
    }

    #[test]
    fn test_filter_chapters_removes_empty_chapters() {
        let chapters = vec![
            make_chapter(
                "Chapter 1",
                vec![make_block(vec![]), make_block(vec!["agent"])],
            ),
            make_chapter("Chapter 2", vec![make_block(vec!["coder"])]),
        ];

        let filter = vec!["agent".to_string(), "product".to_string()];
        let filtered = filter_chapters(&chapters, &filter);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Chapter 1");
        assert_eq!(filtered[0].blocks.len(), 2);
    }

    #[test]
    fn test_filter_chapters_empty_filter_keeps_universal_only() {
        let chapters = vec![make_chapter(
            "Chapter 1",
            vec![make_block(vec![]), make_block(vec!["agent"])],
        )];

        let filtered = filter_chapters(&chapters, &[]);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].blocks.len(), 1); // Only the universal block
    }

    #[test]
    fn test_internal_profile_includes_all_tagged_blocks() {
        let chapters = vec![make_chapter(
            "Chapter 1",
            vec![
                make_block(vec![]),
                make_block(vec!["agent"]),
                make_block(vec!["product"]),
                make_block(vec!["coder"]),
                make_block(vec!["auditor"]),
                make_block(vec!["planner"]),
            ],
        )];

        let filter = vec![
            "universal".to_string(),
            "agent".to_string(),
            "product".to_string(),
            "coder".to_string(),
            "auditor".to_string(),
            "planner".to_string(),
        ];
        let filtered = filter_chapters(&chapters, &filter);

        assert_eq!(filtered[0].blocks.len(), 6);
    }
}
