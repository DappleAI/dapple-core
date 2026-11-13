//! Bibliography / Resources section rendering.
//!
//! Collects citations from filtered blocks and renders a numbered bibliography
//! at the end of the PDF. Citation numbers are assigned in order of first
//! appearance across all chapters and blocks.

use crate::Resource;
use crate::filter::FilteredChapter;
use crate::slug::label_for_id;
use crate::theme::FONT_SANS;
use crate::typeset::escape::escape_string;
use std::collections::HashMap;

/// A resolved citation entry with its assigned number.
#[derive(Clone, Debug)]
pub struct CitationEntry {
    /// Sequential number (1-based) assigned by order of first appearance.
    pub number: usize,
    /// The resource key (matches `cite` attribute on blocks).
    pub key: String,
    /// Human-readable title from the resource, if available.
    pub title: Option<String>,
    /// Source URL from the resource, if available.
    pub url: Option<String>,
}

/// An ordered index of citations found in the rendered content.
///
/// Maps cite keys to their sequential numbers and metadata. The order
/// is determined by first appearance in the filtered chapter/block sequence.
#[derive(Clone, Debug, Default)]
pub struct CitationIndex {
    /// Entries in citation-number order (index 0 = citation [1]).
    entries: Vec<CitationEntry>,
    /// Lookup from cite key to index into `entries`.
    key_to_index: HashMap<String, usize>,
}

impl CitationIndex {
    /// Returns true if no citations were found.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get the citation number for a cite key, if it exists.
    ///
    /// Returns a 1-based number suitable for display as `[N]`.
    pub fn number_for_key(&self, key: &str) -> Option<usize> {
        self.key_to_index
            .get(key)
            .map(|&idx| self.entries[idx].number)
    }

    /// Get all citation entries in order.
    pub fn entries(&self) -> &[CitationEntry] {
        &self.entries
    }
}

/// Walk filtered chapters and build a citation index.
///
/// Scans each block's `attributes` JSON for a `cite` key. When found, the
/// cite value is looked up in the provided resources list. Citations are
/// numbered in order of first appearance. A cite key that does not match
/// any provided resource is silently skipped (the resource data was not
/// provided to the renderer).
pub fn collect_citations(chapters: &[FilteredChapter], resources: &[Resource]) -> CitationIndex {
    if resources.is_empty() {
        return CitationIndex::default();
    }

    // Build a quick lookup from resource key to resource data.
    let resource_map: HashMap<&str, &Resource> =
        resources.iter().map(|r| (r.key.as_str(), r)).collect();

    let mut index = CitationIndex::default();

    for chapter in chapters {
        for block in &chapter.blocks {
            if let Some(cite_key) = block.attributes.get("cite").and_then(|v| v.as_str()) {
                // Only include if we have a matching resource.
                if let Some(resource) = resource_map.get(cite_key)
                    && !index.key_to_index.contains_key(cite_key)
                {
                    let number = index.entries.len() + 1;
                    index
                        .key_to_index
                        .insert(cite_key.to_string(), index.entries.len());
                    index.entries.push(CitationEntry {
                        number,
                        key: cite_key.to_string(),
                        title: resource.title.clone(),
                        url: resource.url.clone(),
                    });
                }
            }
        }
    }

    index
}

/// Typst markup for the bibliography section at the end of the PDF.
///
/// Empty string when there are no citations. Each entry carries a
/// `<a--bib-N>` label so inline citation markers (and explicit
/// `[text](#bib-N)` links) jump to it. Entries render the title in bold with
/// the URL as a real clickable link beneath.
pub fn bibliography_markup(citation_index: &CitationIndex) -> String {
    if citation_index.is_empty() {
        return String::new();
    }

    let mut out = String::from("#pagebreak(weak: true)\n#heading(level: 1)[Resources]\n\n");
    for entry in citation_index.entries() {
        let title = entry.title.as_deref().unwrap_or(&entry.key);
        out.push_str(&format!(
            "#block(above: 10pt, [#text(font: \"{FONT_SANS}\", weight: \"bold\", \
             size: 0.95em, \"[{}] {}\")",
            entry.number,
            escape_string(title)
        ));
        if let Some(url) = &entry.url {
            let escaped = escape_string(url);
            out.push_str(&format!(
                " #linebreak() #link(\"{escaped}\")[#text(size: 0.9em, raw(\"{escaped}\"))]"
            ));
        }
        out.push_str(&format!(
            "]) <{}>\n\n",
            label_for_id(&format!("bib-{}", entry.number))
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::FilteredChapter;
    use dapple_data::model::guides::GuideBlock;
    use serde_json::json;

    fn test_datetime() -> dapple_data::DateTime {
        dapple_data::DateTime::from(time::OffsetDateTime::UNIX_EPOCH)
    }

    fn make_block_with_cite(cite: Option<&str>, cite_pages: Option<&str>) -> GuideBlock {
        let mut attrs = serde_json::Map::new();
        if let Some(c) = cite {
            attrs.insert("cite".to_string(), json!(c));
        }
        if let Some(p) = cite_pages {
            attrs.insert("cite_pages".to_string(), json!(p));
        }
        GuideBlock {
            id: uuid::Uuid::nil(),
            directive_type: "prose".to_string(),
            name: Some("test-block".to_string()),
            content: Some("Some cited content.".to_string()),
            attributes: serde_json::Value::Object(attrs),
            fluent_message_id: None,
            tags: vec![],
            created_at: test_datetime(),
        }
    }

    fn make_resources() -> Vec<Resource> {
        vec![
            Resource {
                key: "portugal-guide".to_string(),
                title: Some("Starting a Business in Portugal".to_string()),
                url: Some("https://example.com/portugal.pdf".to_string()),
            },
            Resource {
                key: "startup-reg".to_string(),
                title: Some("Startup Registration Guide".to_string()),
                url: None,
            },
        ]
    }

    #[test]
    fn test_collect_citations_empty_resources() {
        let chapters = vec![FilteredChapter {
            title: "Ch1".to_string(),
            blocks: vec![make_block_with_cite(Some("portugal-guide"), None)],
        }];
        let index = collect_citations(&chapters, &[]);
        assert!(index.is_empty());
    }

    #[test]
    fn test_collect_citations_no_cite_attributes() {
        let chapters = vec![FilteredChapter {
            title: "Ch1".to_string(),
            blocks: vec![make_block_with_cite(None, None)],
        }];
        let index = collect_citations(&chapters, &make_resources());
        assert!(index.is_empty());
    }

    #[test]
    fn test_collect_citations_ordered_by_first_appearance() {
        let chapters = vec![
            FilteredChapter {
                title: "Ch1".to_string(),
                blocks: vec![
                    make_block_with_cite(Some("startup-reg"), None),
                    make_block_with_cite(Some("portugal-guide"), Some("12-14")),
                ],
            },
            FilteredChapter {
                title: "Ch2".to_string(),
                blocks: vec![
                    // Duplicate cite -- should not create a second entry.
                    make_block_with_cite(Some("startup-reg"), Some("3")),
                ],
            },
        ];
        let resources = make_resources();
        let index = collect_citations(&chapters, &resources);

        assert_eq!(index.entries().len(), 2);
        assert_eq!(index.entries()[0].key, "startup-reg");
        assert_eq!(index.entries()[0].number, 1);
        assert_eq!(index.entries()[1].key, "portugal-guide");
        assert_eq!(index.entries()[1].number, 2);
    }

    #[test]
    fn test_number_for_key() {
        let chapters = vec![FilteredChapter {
            title: "Ch1".to_string(),
            blocks: vec![
                make_block_with_cite(Some("portugal-guide"), None),
                make_block_with_cite(Some("startup-reg"), None),
            ],
        }];
        let index = collect_citations(&chapters, &make_resources());

        assert_eq!(index.number_for_key("portugal-guide"), Some(1));
        assert_eq!(index.number_for_key("startup-reg"), Some(2));
        assert_eq!(index.number_for_key("nonexistent"), None);
    }

    #[test]
    fn test_unmatched_cite_key_is_skipped() {
        let chapters = vec![FilteredChapter {
            title: "Ch1".to_string(),
            blocks: vec![make_block_with_cite(Some("unknown-resource"), None)],
        }];
        let index = collect_citations(&chapters, &make_resources());
        assert!(index.is_empty());
    }
}
