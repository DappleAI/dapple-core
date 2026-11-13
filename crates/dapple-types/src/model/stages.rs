use crate::{ContentHash, DataTransferObject, DateTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Closed, authored vocabulary naming what kind of pane a stage is — the
/// stage-level analog of [`super::briefs::BriefOutputDisplay`]. Semantic, not
/// pictorial: it names the stage's role in the journey, and each client maps it
/// to its own presentation. Authored as `:::stage { kind: "..." }`.
///
/// Unlike `BriefOutputDisplay` there is no neutral member and no `Default`:
/// absence IS the default. The wire field is `Option<StageKind>`; a client
/// applies its own documented fallback when the field is absent.
#[derive(Clone, Copy, Debug, Eq, JsonSchema, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    /// The operator supplies the values the work starts from.
    Intake,
    /// Retrieves source data; runs by itself.
    Fetch,
    /// Derives a reading from data already held; runs by itself.
    Analyze,
    /// The operator chooses among generated candidates.
    Decide,
    /// The operator reads and confirms; produces no new data.
    Review,
    /// Weighs two already-produced things against each other.
    Compare,
    /// Builds the deliverable from everything upstream.
    Assemble,
    /// The finished artifact; the terminal pane.
    Deliver,
}

impl StageKind {
    /// Every valid lookup key, in lockstep with the `rev.stage_kinds` seed.
    /// The single source of truth for materialize-time validation.
    pub const ALL_KEYS: [&'static str; 8] = [
        "intake", "fetch", "analyze", "decide", "review", "compare", "assemble", "deliver",
    ];

    /// Whether `key` is a recognized stage kind.
    pub fn is_valid_key(key: &str) -> bool {
        Self::ALL_KEYS.contains(&key)
    }

    /// Parse a lookup-table key into the enum. Unknown keys return `None` —
    /// there is no neutral member to coerce to; an absent kind is the default.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "intake" => Some(Self::Intake),
            "fetch" => Some(Self::Fetch),
            "analyze" => Some(Self::Analyze),
            "decide" => Some(Self::Decide),
            "review" => Some(Self::Review),
            "compare" => Some(Self::Compare),
            "assemble" => Some(Self::Assemble),
            "deliver" => Some(Self::Deliver),
            _ => None,
        }
    }
}

/// Public DTO for stages
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct Stage {
    pub id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub slug: String,
    pub content_hash: ContentHash,
    pub deprecation_level: Option<String>,
    pub description: String,
    /// Authored stage kind (`:::stage { kind: "..." }`). Absent in older
    /// payloads and for stages with no authored kind ⇒ the client applies its
    /// own documented default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<StageKind>,
    /// Authored icon names (`:::stage { icon: [...] }`), in authored order.
    ///
    /// Deliberately the INVERSE of [`Self::kind`]: an **open** vocabulary,
    /// unvalidated and stored verbatim. The names belong to the drawing client
    /// (dapple-ui curates a lucide set); a consumer that draws nothing ignores
    /// the field, and a consumer that draws MUST tolerate a name it does not
    /// recognize by falling back to its `kind` glyph. Absent and `[]` are the
    /// same fact — empty is omitted from the JSON entirely.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub icon: Vec<String>,
    pub created_at: DateTime,
}

impl DataTransferObject for Stage {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_all_key_parses_to_a_variant() {
        for key in StageKind::ALL_KEYS {
            assert!(StageKind::is_valid_key(key));
            let kind = StageKind::from_key(key)
                .unwrap_or_else(|| panic!("ALL_KEYS member {key:?} must parse"));
            // serde round-trip agrees with the lookup key (snake_case wire form)
            let wire = serde_json::to_string(&kind).unwrap();
            assert_eq!(wire, format!("\"{key}\""));
            let back: StageKind = serde_json::from_str(&wire).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn unknown_key_is_none_not_an_error() {
        assert_eq!(StageKind::from_key("refine"), None);
        assert_eq!(StageKind::from_key(""), None);
        assert!(!StageKind::is_valid_key("shotlist"));
    }

    #[test]
    fn wire_form_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&StageKind::Fetch).unwrap(),
            "\"fetch\""
        );
        assert_eq!(
            serde_json::from_str::<StageKind>("\"deliver\"").unwrap(),
            StageKind::Deliver
        );
    }
}
