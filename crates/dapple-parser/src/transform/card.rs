//! Card metadata extraction from guide.toml [card] section (Fluent-native).
//!
//! The card definition is extracted directly from the parsed metadata.
//! Under the Fluent-native model, subtitle and category have moved to
//! Fluent attributes on the brief-pack message. Only locale-independent
//! visual properties (image, color, accent) remain in the card definition.

use super::CardDef;
use crate::types::GuideMetadata;

/// Extract a card definition from guide metadata.
///
/// Returns `None` if the metadata has no card section.
pub fn extract_card(metadata: &GuideMetadata) -> Option<CardDef> {
    let card = metadata.card.as_ref()?;

    Some(CardDef {
        image: card.image.clone(),
        color: card.color.clone(),
        accent: card.accent.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ParsedCard;

    #[test]
    fn test_extract_card_present() {
        let metadata = GuideMetadata {
            title: "Test".to_string(),
            description: None,
            author: None,
            classification: None,
            source_path: None,
            card: Some(ParsedCard {
                image: Some("assets/images/card.jpg".to_string()),
                color: Some("#2D6A4F".to_string()),
                accent: Some("#95D5B2".to_string()),
            }),
            deprecation_level: None,
            deprecation_reason: None,
            chapters: std::collections::HashMap::new(),
        };

        let card = extract_card(&metadata).unwrap();
        assert_eq!(card.image, Some("assets/images/card.jpg".to_string()));
        assert_eq!(card.color, Some("#2D6A4F".to_string()));
        assert_eq!(card.accent, Some("#95D5B2".to_string()));
    }

    #[test]
    fn test_extract_card_absent() {
        let metadata = GuideMetadata {
            title: "Test".to_string(),
            description: None,
            author: None,
            classification: None,
            source_path: None,
            card: None,
            deprecation_level: None,
            deprecation_reason: None,
            chapters: std::collections::HashMap::new(),
        };

        assert!(extract_card(&metadata).is_none());
    }
}
