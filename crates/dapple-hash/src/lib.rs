//! Content-addressable hashing for dapple versioned objects.
//!
//! Provides deterministic SHA-1 hashing for content deduplication.
//! Hash inputs are normalized (line endings, trimmed whitespace, sorted JSON keys)
//! to ensure identical logical content always produces the same hash.

use sha1::{Digest, Sha1};
use std::collections::BTreeMap;
use std::fmt::Display;

use serde::{Deserialize, Serialize};

// ============================================================================
// ContentHash type
// ============================================================================

/// A 40-character hex SHA-1 content hash for content-addressable versioning.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContentHash(String);

impl ContentHash {
    pub fn new(hash: String) -> Self {
        Self(hash)
    }

    /// 8-char short display form (like git)
    pub fn short(&self) -> &str {
        &self.0[..8.min(self.0.len())]
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ContentHash {
    fn default() -> Self {
        Self("0000000000000000000000000000000000000000".to_string())
    }
}

impl Display for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================================
// Utility functions
// ============================================================================

/// Normalize text for deterministic hashing.
///
/// - Replaces `\r\n` and `\r` with `\n`
/// - Trims leading/trailing whitespace
pub fn normalize_text(s: &str) -> String {
    s.replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

/// Produce canonical JSON from a serde_json::Value by sorting object keys.
///
/// Uses BTreeMap for deterministic key ordering. Arrays preserve order.
pub fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<&String, &serde_json::Value> = map.iter().collect();
            let mut out = String::from("{");
            for (i, (k, v)) in sorted.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('"');
                out.push_str(k);
                out.push_str("\":");
                out.push_str(&canonical_json(v));
            }
            out.push('}');
            out
        }
        serde_json::Value::Array(arr) => {
            let mut out = String::from("[");
            for (i, v) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&canonical_json(v));
            }
            out.push(']');
            out
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Compute SHA-1 hex digest from bytes.
pub fn sha1_hex(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// NUL byte separator for multi-field hashing.
const NUL: &[u8] = b"\0";

// ============================================================================
// Entity hash functions
// ============================================================================

/// Hash a task's versioned content.
///
/// Input: `description \0 function \0 function_input_json`
pub fn hash_task(
    description: &str,
    function: &str,
    function_input: &serde_json::Value,
) -> ContentHash {
    let mut hasher = Sha1::new();
    hasher.update(normalize_text(description).as_bytes());
    hasher.update(NUL);
    hasher.update(normalize_text(function).as_bytes());
    hasher.update(NUL);
    hasher.update(canonical_json(function_input).as_bytes());
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Deterministic fingerprint of a node's value-slot overlays (expose/mask).
///
/// Each list is sorted and de-duplicated so authoring order is irrelevant.
/// Returns `""` when both lists are empty — callers MUST treat the empty
/// fingerprint as "no overlay contribution" so overlay-free stages/steps keep
/// their pre-overlay content hash (no spurious re-versioning on re-ingest).
///
/// Folding this into [`hash_step`] / [`hash_stage`] is what makes overlays
/// participate in content-addressable versioning: an expose/mask edit forks a
/// new step→stage→playbook version instead of mutating the existing one. This
/// mirrors the Merkle treatment already used for prompts, inference profiles,
/// and brief groups.
pub fn overlay_fingerprint(expose: &[String], mask: &[String]) -> String {
    let mut e: Vec<&str> = expose.iter().map(|s| s.as_str()).collect();
    e.sort_unstable();
    e.dedup();
    let mut m: Vec<&str> = mask.iter().map(|s| s.as_str()).collect();
    m.sort_unstable();
    m.dedup();
    if e.is_empty() && m.is_empty() {
        return String::new();
    }
    format!("e:{}|m:{}", e.join(","), m.join(","))
}

/// Hash a step's versioned content (Merkle: ordered child task hashes + overlays).
///
/// A step is an ordered, fail-together set of tasks, so its hash folds the
/// **ordered** task content hashes — reordering or changing any task re-versions
/// the step.
///
/// Input: `description \0 flow_control_json \0 task_hash_1 \0 task_hash_2 \0 ...`
/// (task hashes joined by NUL as a **separator**, not a terminator) followed by
/// `\0 overlay:<fingerprint>` ONLY when `overlay_fingerprint` is non-empty.
///
/// The separator (not terminator) join is deliberate and unlike [`hash_stage`]:
/// it makes the N=1 case **byte-identical** to the pre-multitask function — a
/// single task writes just its hash with no surrounding NUL, so existing
/// single-task steps do NOT re-version on next ingest. An empty slice (a
/// task-less step) writes nothing, matching the prior empty-string framing.
/// Preserving N=1 back-compat takes priority over cross-function symmetry.
pub fn hash_step(
    description: &str,
    flow_control: &Option<serde_json::Value>,
    task_content_hashes: &[&str],
    overlay_fingerprint: &str,
) -> ContentHash {
    let mut hasher = Sha1::new();
    hasher.update(normalize_text(description).as_bytes());
    hasher.update(NUL);
    if let Some(fc) = flow_control {
        hasher.update(canonical_json(fc).as_bytes());
    }
    hasher.update(NUL);
    for (i, task_content_hash) in task_content_hashes.iter().enumerate() {
        if i > 0 {
            hasher.update(NUL);
        }
        hasher.update(task_content_hash.as_bytes());
    }
    if !overlay_fingerprint.is_empty() {
        hasher.update(NUL);
        hasher.update(b"overlay:");
        hasher.update(overlay_fingerprint.as_bytes());
    }
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Hash a stage's versioned content (Merkle: ordered child step hashes + overlays).
///
/// Input: `description \0 flow_control_json \0 step_hash_1 \0 step_hash_2 \0 ...`
/// followed by `overlay:<fingerprint>` ONLY when `overlay_fingerprint` is
/// non-empty, so a stage with no overlays hashes identically to the
/// pre-overlay function; followed by `\0 kind:<key>` ONLY when `kind` is
/// `Some` and non-empty, so a kindless stage hashes identically to the
/// pre-kind function (the do-not-mass-re-version guarantee). Callers pass only
/// VALID vocabulary keys (an unknown authored key hashes as absent, matching
/// the NULL the LEFT JOIN stores).
///
/// `icons` folds last under the same empty-omit rule, so an icon-less stage
/// keeps its pre-icon hash. Unlike `kind` the names are NOT filtered against a
/// vocabulary — the set is open and stored verbatim, so the hash must fold
/// exactly what the row stores. The fold is ORDER-SENSITIVE (authored order is
/// load-bearing: the client reads the first five) and each name is
/// NUL-terminated, so `["ab"]` and `["a", "b"]` cannot collide.
pub fn hash_stage(
    description: &str,
    flow_control: &Option<serde_json::Value>,
    step_content_hashes: &[&str],
    overlay_fingerprint: &str,
    kind: Option<&str>,
    icons: &[String],
) -> ContentHash {
    let mut hasher = Sha1::new();
    hasher.update(normalize_text(description).as_bytes());
    hasher.update(NUL);
    if let Some(fc) = flow_control {
        hasher.update(canonical_json(fc).as_bytes());
    }
    hasher.update(NUL);
    for hash in step_content_hashes {
        hasher.update(hash.as_bytes());
        hasher.update(NUL);
    }
    if !overlay_fingerprint.is_empty() {
        hasher.update(b"overlay:");
        hasher.update(overlay_fingerprint.as_bytes());
    }
    if let Some(k) = kind.filter(|k| !k.is_empty()) {
        hasher.update(NUL);
        hasher.update(b"kind:");
        hasher.update(k.as_bytes());
    }
    if !icons.is_empty() {
        hasher.update(NUL);
        hasher.update(b"icon:");
        for name in icons {
            hasher.update(name.as_bytes());
            hasher.update(NUL);
        }
    }
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Hash a playbook's versioned content (Merkle: includes ordered child stage hashes).
///
/// Input: `description \0 stage_hash_1 \0 stage_hash_2 \0 ...`
pub fn hash_playbook(description: &str, stage_content_hashes: &[&str]) -> ContentHash {
    let mut hasher = Sha1::new();
    hasher.update(normalize_text(description).as_bytes());
    hasher.update(NUL);
    for hash in stage_content_hashes {
        hasher.update(hash.as_bytes());
        hasher.update(NUL);
    }
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Hash a prompt's versioned content.
///
/// Hashes ALL locales together in sorted order, then all engine bodies in
/// sorted order — identical structure to [`hash_fluent_message`]. The
/// prompt entity's content_hash and its linked fluent message's
/// content_hash will coincide, which is correct: a prompt's semantic
/// content IS its fluent message body(ies).
///
/// Note: prior versions of this function hashed only a 200-char
/// truncation of the body (passed in as `description`). That meant edits
/// past character 200 silently reused the old prompt versioned_object
/// during materialization, so prompt body changes never reached runtime.
/// Hashing the full body fixes that class of stale-prompt bugs.
pub fn hash_prompt(
    localized_bodies: &BTreeMap<String, String>,
    engine_bodies: &BTreeMap<String, String>,
) -> ContentHash {
    let mut hasher = Sha1::new();
    for (locale, body) in localized_bodies {
        hasher.update(b"locale:");
        hasher.update(locale.as_bytes());
        hasher.update(NUL);
        hasher.update(normalize_text(body).as_bytes());
        hasher.update(NUL);
    }
    for (engine_key, body) in engine_bodies {
        hasher.update(b"engine:");
        hasher.update(engine_key.as_bytes());
        hasher.update(NUL);
        hasher.update(normalize_text(body).as_bytes());
        hasher.update(NUL);
    }
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Hash a fluent message's versioned content.
///
/// Hashes ALL locales together in sorted order, then all engine bodies in sorted order.
pub fn hash_fluent_message(
    locale_bodies: &BTreeMap<String, String>,
    engine_bodies: &BTreeMap<String, String>,
) -> ContentHash {
    let mut hasher = Sha1::new();
    for (locale, body) in locale_bodies {
        hasher.update(b"locale:");
        hasher.update(locale.as_bytes());
        hasher.update(NUL);
        hasher.update(normalize_text(body).as_bytes());
        hasher.update(NUL);
    }
    for (engine_key, body) in engine_bodies {
        hasher.update(b"engine:");
        hasher.update(engine_key.as_bytes());
        hasher.update(NUL);
        hasher.update(normalize_text(body).as_bytes());
        hasher.update(NUL);
    }
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Hash a category's versioned content.
pub fn hash_category(description: &str) -> ContentHash {
    let mut hasher = Sha1::new();
    hasher.update(normalize_text(description).as_bytes());
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Hash a hat's versioned content.
pub fn hash_hat(description: &str) -> ContentHash {
    let mut hasher = Sha1::new();
    hasher.update(normalize_text(description).as_bytes());
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Hash a brief's versioned content (canonical JSON of content fields).
pub fn hash_brief(content: &serde_json::Value) -> ContentHash {
    let mut hasher = Sha1::new();
    hasher.update(canonical_json(content).as_bytes());
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Hash a brief group's versioned content (Merkle pattern over child briefs).
///
/// Input: `description \0 brief_content_hash_1 \0 brief_content_hash_2 \0 ...`
///
/// Hashing child *content_hashes* rather than child *names* means edits to
/// any brief in the group propagate up: the brief gets a new content_hash,
/// which changes the group's hash, which triggers a new group versioned_object
/// referencing the new brief versions. Hashing names — the prior behavior —
/// silently kept the group pointing at the OLD brief versions because
/// membership names didn't change.
///
/// Same Merkle pattern as [`hash_step`] (which includes its child
/// task_content_hash).
pub fn hash_brief_group(description: &str, brief_content_hashes: &[&str]) -> ContentHash {
    let mut hasher = Sha1::new();
    hasher.update(normalize_text(description).as_bytes());
    hasher.update(NUL);
    for hash in brief_content_hashes {
        hasher.update(hash.as_bytes());
        hasher.update(NUL);
    }
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Hash a brief pack's versioned content (Merkle pattern over child groups).
///
/// Input: `description \0 group_content_hash_1 \0 group_content_hash_2 \0 ...`
///
/// Same Merkle rationale as [`hash_brief_group`]: a brief edit propagates
/// up through the brief's hash → group's hash → pack's hash, so each
/// level gets a new versioned_object. Prior behavior hashed child names
/// only, so pack-level edits to any brief silently reused the old pack.
pub fn hash_brief_pack(description: &str, group_content_hashes: &[&str]) -> ContentHash {
    let mut hasher = Sha1::new();
    hasher.update(normalize_text(description).as_bytes());
    hasher.update(NUL);
    for hash in group_content_hashes {
        hasher.update(hash.as_bytes());
        hasher.update(NUL);
    }
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Hash a value slot's versioned content.
///
/// Input: `title \0 description \0 kind \0 constraints_json \0 cardinality \0 min \0 max`
///
/// `value_kind` is folded in (unconditionally) so a kind change (e.g. text→json)
/// forks a new value-slot version — kind is a versioned property, not path
/// identity (migration 70). This changes every existing value-slot hash, so the
/// first re-ingest after this lands re-versions every slot once (the append-only
/// cost of moving kind onto the version).
///
/// Cardinality and the selection bounds are likewise folded in so a single→multi
/// change (or a min/max edit) forks a new version. The cardinality/min/max suffix
/// is still only appended when it diverges from the implicit `'single'` default.
pub fn hash_value_slot(
    title: &str,
    description: &str,
    value_kind: &str,
    constraints: &Option<serde_json::Value>,
    cardinality: &str,
    min_selections: Option<i32>,
    max_selections: Option<i32>,
) -> ContentHash {
    let mut hasher = Sha1::new();
    hasher.update(normalize_text(title).as_bytes());
    hasher.update(NUL);
    hasher.update(normalize_text(description).as_bytes());
    hasher.update(NUL);
    hasher.update(b"kind:");
    hasher.update(normalize_text(value_kind).as_bytes());
    hasher.update(NUL);
    if let Some(c) = constraints {
        hasher.update(canonical_json(c).as_bytes());
    }
    // Preserve legacy hash bytes for plain single slots: only append the
    // cardinality fingerprint when it diverges from the implicit default.
    if cardinality != "single" || min_selections.is_some() || max_selections.is_some() {
        hasher.update(NUL);
        hasher.update(b"cardinality:");
        hasher.update(normalize_text(cardinality).as_bytes());
        hasher.update(NUL);
        if let Some(min) = min_selections {
            hasher.update(b"min:");
            hasher.update(min.to_string().as_bytes());
        }
        hasher.update(NUL);
        if let Some(max) = max_selections {
            hasher.update(b"max:");
            hasher.update(max.to_string().as_bytes());
        }
    }
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Hash an asset's versioned content (raw binary bytes).
pub fn hash_asset(data: &[u8]) -> ContentHash {
    ContentHash::new(sha1_hex(data))
}

/// Hash a directive's versioned content.
///
/// Input: `directive_type \0 config_json`
pub fn hash_directive(directive_type: &str, config: &Option<serde_json::Value>) -> ContentHash {
    let mut hasher = Sha1::new();
    hasher.update(normalize_text(directive_type).as_bytes());
    hasher.update(NUL);
    if let Some(c) = config {
        hasher.update(canonical_json(c).as_bytes());
    }
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Hash an inference profile's versioned content.
///
/// Hashes the linked fluent message's locale bodies and per-locale
/// attributes (system-prompt, temperature, max-tokens, etc. — the actual
/// runtime configuration). Prior versions hashed only a `description`
/// string — and the guide-ingest materializer called it with `""`, so
/// every inference profile in a guide got the same hash and edits to
/// system prompts / temperatures / model selection were silently reused
/// against the original 2026-04-27-style version.
///
/// Per-locale attributes are sorted by attribute name so insertion order
/// doesn't matter, mirroring the BTreeMap discipline used by
/// [`hash_fluent_message`].
pub fn hash_inference_profile(
    locale_bodies: &BTreeMap<String, String>,
    locale_attributes: &BTreeMap<String, BTreeMap<String, String>>,
) -> ContentHash {
    let mut hasher = Sha1::new();
    for (locale, body) in locale_bodies {
        hasher.update(b"locale:");
        hasher.update(locale.as_bytes());
        hasher.update(NUL);
        hasher.update(normalize_text(body).as_bytes());
        hasher.update(NUL);
    }
    for (locale, attrs) in locale_attributes {
        for (attr_name, attr_value) in attrs {
            hasher.update(b"attr:");
            hasher.update(locale.as_bytes());
            hasher.update(NUL);
            hasher.update(attr_name.as_bytes());
            hasher.update(NUL);
            hasher.update(normalize_text(attr_value).as_bytes());
            hasher.update(NUL);
        }
    }
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Hash a guide's content from sorted file entries.
///
/// Uses the raw directory hash model: hash each file's content sorted by relative path.
pub fn hash_guide_directory(files: &BTreeMap<String, Vec<u8>>) -> ContentHash {
    let mut hasher = Sha1::new();
    for (relative_path, file_bytes) in files {
        hasher.update(b"file:");
        hasher.update(relative_path.as_bytes());
        hasher.update(NUL);
        hasher.update(sha1_hex(file_bytes).as_bytes());
        hasher.update(NUL);
    }
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Fold a guide's derived-object Merkle roots into its directory hash.
///
/// A guide is the parent of the playbooks, brief-packs, and inference profiles
/// it derives. Per the project's content-addressing rule a parent MUST fold its
/// children's `content_hash` so any descendant change re-versions the parent —
/// the same rule [`hash_stage`]/[`hash_brief_group`] already obey for their
/// children. [`hash_guide_directory`] alone hashes only the tarball bytes, so a
/// change to a DERIVED object that does NOT change the source bytes (e.g. a
/// materialization fix that corrects a playbook's stage→step structure) left the
/// guide hash unchanged. The corrected playbook was then grafted onto the SAME
/// guide version as a second `guide_playbooks` row instead of minting a new guide
/// version — producing duplicate exposures (a journey pinning a stale/broken
/// playbook, the chapter view rendering two of a stage) and a backfill that
/// reports "nothing to pin". Folding the derived roots in closes that hole:
/// distinct derived content ⇒ distinct guide version ⇒ exactly one playbook per
/// chapter per guide version.
///
/// Each root slice is sorted + de-duplicated before folding: a guide exposes its
/// playbooks/packs/profiles as an unordered SET (keyed by path), so authoring
/// order is irrelevant and a root that appears under two chapters contributes
/// once. When every derived slice is empty the result is the bare
/// `directory_hash` unchanged, so a guide with no derived objects (pure prose)
/// keeps its content hash and does NOT spuriously re-version.
pub fn hash_guide_with_derived(
    directory_hash: &ContentHash,
    playbook_roots: &[&str],
    brief_pack_roots: &[&str],
    profile_roots: &[&str],
) -> ContentHash {
    // No derived children → identical to the directory hash (no-op), so prose-only
    // guides and any pre-existing content keep their hash where there is nothing
    // to fold.
    if playbook_roots.is_empty() && brief_pack_roots.is_empty() && profile_roots.is_empty() {
        return directory_hash.clone();
    }
    let mut hasher = Sha1::new();
    hasher.update(b"dir:");
    hasher.update(directory_hash.as_str().as_bytes());
    hasher.update(NUL);
    for (tag, roots) in [
        (b"playbook:".as_slice(), playbook_roots),
        (b"brief_pack:".as_slice(), brief_pack_roots),
        (b"profile:".as_slice(), profile_roots),
    ] {
        let mut sorted: Vec<&str> = roots.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        for h in sorted {
            hasher.update(tag);
            hasher.update(h.as_bytes());
            hasher.update(NUL);
        }
    }
    ContentHash::new(hex::encode(hasher.finalize()))
}

/// Returns true if the given path should be excluded from guide directory hashing.
pub fn is_excluded_path(relative_path: &str) -> bool {
    let excluded_prefixes = [".git/", "__MACOSX/"];
    let excluded_names = [".DS_Store", ".gitignore"];
    let excluded_extensions = [".swp", ".swo"];

    for prefix in &excluded_prefixes {
        if relative_path.starts_with(prefix) {
            return true;
        }
    }
    for name in &excluded_names {
        if relative_path == *name || relative_path.ends_with(&format!("/{name}")) {
            return true;
        }
    }
    for ext in &excluded_extensions {
        if relative_path.ends_with(ext) {
            return true;
        }
    }
    false
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_normalize_text() {
        assert_eq!(normalize_text("  hello\r\nworld\r  "), "hello\nworld");
        assert_eq!(normalize_text("no change"), "no change");
        assert_eq!(normalize_text("  trimmed  "), "trimmed");
    }

    #[test]
    fn test_canonical_json_sorts_keys() {
        let value = json!({"z": 1, "a": 2, "m": 3});
        let canonical = canonical_json(&value);
        assert_eq!(canonical, r#"{"a":2,"m":3,"z":1}"#);
    }

    #[test]
    fn test_canonical_json_nested() {
        let value = json!({"b": {"z": 1, "a": 2}, "a": [3, 2, 1]});
        let canonical = canonical_json(&value);
        assert_eq!(canonical, r#"{"a":[3,2,1],"b":{"a":2,"z":1}}"#);
    }

    #[test]
    fn test_sha1_hex() {
        let hash = sha1_hex(b"hello");
        assert_eq!(hash, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
        assert_eq!(hash.len(), 40);
    }

    #[test]
    fn test_hash_task_deterministic() {
        let h1 = hash_task("desc", "func", &json!({"key": "val"}));
        let h2 = hash_task("desc", "func", &json!({"key": "val"}));
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_task_different_content() {
        let h1 = hash_task("desc1", "func", &json!({}));
        let h2 = hash_task("desc2", "func", &json!({}));
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_hash_task_normalizes_text() {
        let h1 = hash_task("  desc  ", "func", &json!({}));
        let h2 = hash_task("desc", "func", &json!({}));
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_task_normalizes_line_endings() {
        let h1 = hash_task("line1\r\nline2", "func", &json!({}));
        let h2 = hash_task("line1\nline2", "func", &json!({}));
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_task_json_key_order_irrelevant() {
        let h1 = hash_task("d", "f", &json!({"a": 1, "b": 2}));
        let h2 = hash_task("d", "f", &json!({"b": 2, "a": 1}));
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_is_40_char_hex() {
        let h = hash_task("desc", "func", &json!({}));
        assert_eq!(h.as_str().len(), 40);
        assert!(h.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_hash_step_includes_task_hash() {
        let task_hash = hash_task("td", "tf", &json!({}));
        let h1 = hash_step("sd", &None, &[task_hash.as_str()], "");
        let different_task = hash_task("td2", "tf", &json!({}));
        let h2 = hash_step("sd", &None, &[different_task.as_str()], "");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_step_n1_is_byte_identical_to_legacy() {
        // CRITICAL back-compat: a single-task step (N=1) MUST hash byte-identically
        // to the pre-multitask framing, or every existing single-task step
        // re-versions on next ingest. Pin it by recomputing the exact legacy byte
        // string here, independent of hash_step's loop:
        //   normalize(description) \0 [fc] \0 task_hash   (no overlay)
        use sha1::{Digest, Sha1};
        let task = "1111111111111111111111111111111111111111";
        let mut h = Sha1::new();
        h.update(normalize_text("step desc").as_bytes());
        h.update(NUL);
        // flow_control None -> contributes nothing between the two NULs
        h.update(NUL);
        h.update(task.as_bytes());
        let expected = hex::encode(h.finalize());
        assert_eq!(
            hash_step("step desc", &None, &[task], "").as_str(),
            expected
        );
    }

    #[test]
    fn hash_step_empty_task_set_is_legacy_no_task() {
        // A task-less step (empty slice) must equal the prior empty-string framing.
        use sha1::{Digest, Sha1};
        let mut h = Sha1::new();
        h.update(normalize_text("d").as_bytes());
        h.update(NUL);
        h.update(NUL);
        let expected = hex::encode(h.finalize());
        assert_eq!(hash_step("d", &None, &[], "").as_str(), expected);
    }

    #[test]
    fn hash_step_task_order_and_membership_matter() {
        let a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        // Reordering tasks re-versions the step (Merkle over the ordered set).
        assert_ne!(
            hash_step("s", &None, &[a, b], ""),
            hash_step("s", &None, &[b, a], "")
        );
        // Dropping/adding a task re-versions it.
        assert_ne!(
            hash_step("s", &None, &[a], ""),
            hash_step("s", &None, &[a, b], "")
        );
    }

    #[test]
    fn test_hash_stage_merkle() {
        let step1 = hash_step(
            "s1",
            &None,
            &["0000000000000000000000000000000000000000"],
            "",
        );
        let step2 = hash_step(
            "s2",
            &None,
            &["0000000000000000000000000000000000000000"],
            "",
        );
        let h1 = hash_stage(
            "stage",
            &None,
            &[step1.as_str(), step2.as_str()],
            "",
            None,
            &[],
        );
        let h2 = hash_stage(
            "stage",
            &None,
            &[step2.as_str(), step1.as_str()],
            "",
            None,
            &[],
        );
        // Different order = different hash (order matters)
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_stage_kind_none_is_noop() {
        // The do-not-mass-re-version guarantee: an absent (or empty) kind must
        // hash identically to the pre-kind function, so every already-ingested
        // kindless stage keeps its hash on the next ingest.
        let base = hash_stage("st", &None, &["abc"], "", None, &[]);
        assert_eq!(base, hash_stage("st", &None, &["abc"], "", Some(""), &[]));
        assert_eq!(base, hash_stage("st", &None, &["abc"], "", None, &[]));
    }

    #[test]
    fn hash_stage_kind_forks() {
        let base = hash_stage("st", &None, &["abc"], "", None, &[]);
        let fetch = hash_stage("st", &None, &["abc"], "", Some("fetch"), &[]);
        let review = hash_stage("st", &None, &["abc"], "", Some("review"), &[]);
        assert_ne!(base, fetch, "authoring a kind must fork the hash");
        assert_ne!(fetch, review, "changing the kind must fork the hash");
        // Kind composes with overlays: each alone and both together all differ.
        let overlay = hash_stage("st", &None, &["abc"], "e:topic|m:", None, &[]);
        let both = hash_stage("st", &None, &["abc"], "e:topic|m:", Some("fetch"), &[]);
        assert_ne!(overlay, both);
        assert_ne!(fetch, both);
    }

    #[test]
    fn hash_stage_icons_empty_is_noop() {
        // Same do-not-mass-re-version guarantee as `kind`: an empty icon set
        // must hash identically to the pre-icon function, so every stage in
        // the nineteen already-ingested guides keeps its hash. This is the
        // test that fails if anyone "simplifies" the empty-omit branch away.
        let base = hash_stage("st", &None, &["abc"], "", None, &[]);
        let with_kind = hash_stage("st", &None, &["abc"], "", Some("fetch"), &[]);
        assert_eq!(base, hash_stage("st", &None, &["abc"], "", None, &[]));
        assert_eq!(
            with_kind,
            hash_stage("st", &None, &["abc"], "", Some("fetch"), &[]),
            "an icon-less kinded stage must keep its migration-97 hash"
        );
    }

    #[test]
    fn hash_stage_icons_fork_and_are_order_sensitive() {
        let base = hash_stage("st", &None, &["abc"], "", None, &[]);
        let radar = hash_stage("st", &None, &["abc"], "", None, &[s("radar")]);
        let pair_ab = hash_stage("st", &None, &["abc"], "", None, &[s("radar"), s("globe")]);
        let pair_ba = hash_stage("st", &None, &["abc"], "", None, &[s("globe"), s("radar")]);

        assert_ne!(base, radar, "authoring an icon must fork the hash");
        assert_ne!(radar, pair_ab, "adding a second icon must fork the hash");
        assert_ne!(
            pair_ab, pair_ba,
            "authored ORDER is load-bearing (the client reads the first five), \
             so reordering must fork the hash"
        );

        // NUL-termination per element: no concatenation collision.
        let split = hash_stage("st", &None, &["abc"], "", None, &[s("ra"), s("dar")]);
        assert_ne!(
            split, radar,
            "[\"ra\",\"dar\"] must not collide with [\"radar\"]"
        );

        // Icons compose with kind: neither field masks the other.
        let kinded = hash_stage("st", &None, &["abc"], "", Some("fetch"), &[s("radar")]);
        assert_ne!(kinded, radar);
        assert_ne!(
            kinded,
            hash_stage("st", &None, &["abc"], "", Some("fetch"), &[])
        );
    }

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn overlay_fingerprint_sorts_dedups_and_empties() {
        assert_eq!(overlay_fingerprint(&[], &[]), "");
        // Order-independent + sorted.
        assert_eq!(
            overlay_fingerprint(&["topic".into(), "audience".into()], &[]),
            "e:audience,topic|m:"
        );
        assert_eq!(
            overlay_fingerprint(&["topic".into(), "topic".into()], &["x".into()]),
            "e:topic|m:x"
        );
    }

    #[test]
    fn hash_step_empty_fingerprint_is_no_op() {
        // The non-regression guarantee: an empty fingerprint must not change
        // the hash, so overlay-free steps don't re-version on next ingest.
        let base = hash_step("d", &None, &["abc"], "");
        let again = hash_step("d", &None, &["abc"], "");
        assert_eq!(base, again);
        let with_overlay = hash_step("d", &None, &["abc"], "e:topic|m:");
        assert_ne!(base, with_overlay, "an overlay edit must fork the hash");
    }

    #[test]
    fn hash_stage_overlay_change_forks() {
        let s = hash_step(
            "s",
            &None,
            &["0000000000000000000000000000000000000000"],
            "",
        );
        let base = hash_stage("st", &None, &[s.as_str()], "", None, &[]);
        let with_overlay = hash_stage("st", &None, &[s.as_str()], "e:topic|m:", None, &[]);
        assert_ne!(
            base, with_overlay,
            "a stage overlay edit must fork the hash"
        );
        // Empty fingerprint is a no-op.
        assert_eq!(base, hash_stage("st", &None, &[s.as_str()], "", None, &[]));
    }

    #[test]
    fn hash_prompt_changes_when_body_changes() {
        // The historic bug: a 200-char description prefix hash made
        // mid-body edits invisible. The new function hashes full bodies.
        let mut bodies_a = BTreeMap::new();
        bodies_a.insert(
            "en".to_string(),
            "You are restructuring multiple steps. <-- short body, alpha tail -->".to_string(),
        );
        let mut bodies_b = BTreeMap::new();
        bodies_b.insert(
            "en".to_string(),
            "You are restructuring multiple steps. <-- short body, BETA tail -->".to_string(),
        );
        let h_a = hash_prompt(&bodies_a, &BTreeMap::new());
        let h_b = hash_prompt(&bodies_b, &BTreeMap::new());
        assert_ne!(h_a, h_b, "different bodies must produce different hashes");
    }

    #[test]
    fn hash_prompt_changes_when_edit_is_past_first_200_chars() {
        // Regression test for the truncation bug. Both bodies share
        // the same first 251 characters (an opening longer than the old
        // 200-char window). Mid-body edits past the window must still
        // change the hash.
        let opening = "You are restructuring multiple steps in a Dapple guide that each ask the LLM for too much information at once. Your job is to propose a single, coherent guide-wide restructure where every monolithic step is broken into smaller focused steps.";
        assert!(opening.chars().count() > 200);
        let mut bodies_a = BTreeMap::new();
        bodies_a.insert(
            "en".to_string(),
            format!("{opening}\n\nVersion A specifics."),
        );
        let mut bodies_b = BTreeMap::new();
        bodies_b.insert(
            "en".to_string(),
            format!("{opening}\n\nVersion B specifics."),
        );
        let h_a = hash_prompt(&bodies_a, &BTreeMap::new());
        let h_b = hash_prompt(&bodies_b, &BTreeMap::new());
        assert_ne!(
            h_a, h_b,
            "edits past char 200 must produce a new hash (regression)"
        );
    }

    #[test]
    fn hash_prompt_stable_for_identical_bodies() {
        let mut a = BTreeMap::new();
        a.insert("en".to_string(), "body".to_string());
        a.insert("pt-BR".to_string(), "corpo".to_string());
        let mut b = BTreeMap::new();
        // Insert in reverse order — BTreeMap sorts so order doesn't matter.
        b.insert("pt-BR".to_string(), "corpo".to_string());
        b.insert("en".to_string(), "body".to_string());
        assert_eq!(
            hash_prompt(&a, &BTreeMap::new()),
            hash_prompt(&b, &BTreeMap::new())
        );
    }

    #[test]
    fn hash_prompt_sensitive_to_locale_addition() {
        let mut a = BTreeMap::new();
        a.insert("en".to_string(), "body".to_string());
        let mut b = a.clone();
        b.insert("pt-BR".to_string(), "corpo".to_string());
        assert_ne!(
            hash_prompt(&a, &BTreeMap::new()),
            hash_prompt(&b, &BTreeMap::new()),
            "adding a locale must change the hash"
        );
    }

    #[test]
    fn hash_prompt_includes_engine_bodies() {
        let mut locales = BTreeMap::new();
        locales.insert("en".to_string(), "body".to_string());
        let mut engines_a = BTreeMap::new();
        engines_a.insert("claude-4-6/sonnet/".to_string(), "engine A".to_string());
        let mut engines_b = BTreeMap::new();
        engines_b.insert("claude-4-6/sonnet/".to_string(), "engine B".to_string());
        assert_ne!(
            hash_prompt(&locales, &engines_a),
            hash_prompt(&locales, &engines_b),
        );
    }

    #[test]
    fn hash_inference_profile_changes_when_body_changes() {
        let mut a = BTreeMap::new();
        a.insert("en".to_string(), "system prompt A".to_string());
        let mut b = BTreeMap::new();
        b.insert("en".to_string(), "system prompt B".to_string());
        assert_ne!(
            hash_inference_profile(&a, &BTreeMap::new()),
            hash_inference_profile(&b, &BTreeMap::new()),
        );
    }

    #[test]
    fn hash_inference_profile_changes_when_attribute_changes() {
        // Regression: edits to temperature / max-tokens / model selection
        // arrive as fluent attributes. The hash must reflect them or
        // sampling-param edits are silently reused.
        let mut bodies = BTreeMap::new();
        bodies.insert("en".to_string(), "body".to_string());
        let mut attrs_a: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut en_attrs_a = BTreeMap::new();
        en_attrs_a.insert("temperature".to_string(), "0.7".to_string());
        attrs_a.insert("en".to_string(), en_attrs_a);
        let mut attrs_b = attrs_a.clone();
        attrs_b
            .get_mut("en")
            .unwrap()
            .insert("temperature".to_string(), "0.9".to_string());
        assert_ne!(
            hash_inference_profile(&bodies, &attrs_a),
            hash_inference_profile(&bodies, &attrs_b),
        );
    }

    #[test]
    fn hash_brief_group_changes_when_child_brief_hash_changes() {
        // Merkle invariant: editing a brief flips its content_hash, and
        // that propagates up through the group's hash. Prior behavior
        // (hashing names alone) silently kept the group frozen.
        let h_before = hash_brief_group("desc", &["brief_v1_hash", "other_brief"]);
        let h_after = hash_brief_group("desc", &["brief_v2_hash", "other_brief"]);
        assert_ne!(h_before, h_after);
    }

    #[test]
    fn hash_brief_group_stable_for_same_children() {
        let h1 = hash_brief_group("d", &["a", "b", "c"]);
        let h2 = hash_brief_group("d", &["a", "b", "c"]);
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_brief_group_sensitive_to_membership_change() {
        let h1 = hash_brief_group("d", &["a", "b"]);
        let h2 = hash_brief_group("d", &["a", "b", "c"]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_brief_pack_changes_when_child_group_hash_changes() {
        let h_before = hash_brief_pack("desc", &["group_v1_hash", "other_group"]);
        let h_after = hash_brief_pack("desc", &["group_v2_hash", "other_group"]);
        assert_ne!(h_before, h_after);
    }

    #[test]
    fn hash_brief_pack_stable_for_same_children() {
        let h1 = hash_brief_pack("d", &["g1", "g2"]);
        let h2 = hash_brief_pack("d", &["g1", "g2"]);
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_inference_profile_no_collision_for_distinct_profiles() {
        // The historic bug: every IP in a guide hashed to the same
        // empty-string SHA1. Two IPs with different content must
        // produce different hashes.
        let mut a = BTreeMap::new();
        a.insert(
            "en".to_string(),
            "You are a careful summarizer.".to_string(),
        );
        let mut b = BTreeMap::new();
        b.insert(
            "en".to_string(),
            "You are a creative storyteller.".to_string(),
        );
        assert_ne!(
            hash_inference_profile(&a, &BTreeMap::new()),
            hash_inference_profile(&b, &BTreeMap::new()),
        );
    }

    #[test]
    fn hash_prompt_matches_fluent_message_for_same_inputs() {
        // Documented invariant from the rewrite: a prompt's content_hash
        // and its linked fluent message's content_hash should coincide
        // for identical (locales, engines) inputs.
        let mut locales = BTreeMap::new();
        locales.insert("en".to_string(), "body".to_string());
        let mut engines = BTreeMap::new();
        engines.insert("claude-4-6/sonnet/".to_string(), "engine".to_string());
        assert_eq!(
            hash_prompt(&locales, &engines),
            hash_fluent_message(&locales, &engines),
        );
    }

    #[test]
    fn test_hash_fluent_message_sorted() {
        let mut locales1 = BTreeMap::new();
        locales1.insert("en".to_string(), "hello".to_string());
        locales1.insert("fr".to_string(), "bonjour".to_string());

        let mut locales2 = BTreeMap::new();
        locales2.insert("fr".to_string(), "bonjour".to_string());
        locales2.insert("en".to_string(), "hello".to_string());

        let h1 = hash_fluent_message(&locales1, &BTreeMap::new());
        let h2 = hash_fluent_message(&locales2, &BTreeMap::new());
        assert_eq!(h1, h2); // BTreeMap ensures sorted order
    }

    #[test]
    fn test_hash_guide_directory() {
        let mut files = BTreeMap::new();
        files.insert(
            "guide.toml".to_string(),
            b"[guide]\ntitle = \"Test\"".to_vec(),
        );
        files.insert("locales/en.ftl".to_string(), b"hello = Hello".to_vec());

        let h1 = hash_guide_directory(&files);
        let h2 = hash_guide_directory(&files);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_is_excluded_path() {
        assert!(is_excluded_path(".git/config"));
        assert!(is_excluded_path(".DS_Store"));
        assert!(is_excluded_path("subdir/.DS_Store"));
        assert!(is_excluded_path("__MACOSX/file"));
        assert!(is_excluded_path(".gitignore"));
        assert!(is_excluded_path("file.swp"));
        assert!(is_excluded_path("file.swo"));
        assert!(!is_excluded_path("guide.toml"));
        assert!(!is_excluded_path("locales/en.ftl"));
    }

    #[test]
    fn test_content_hash_short() {
        let h = hash_task("desc", "func", &json!({}));
        assert_eq!(h.short().len(), 8);
        assert!(h.as_str().starts_with(h.short()));
    }

    /// Golden vector test: ensures the guide directory hash algorithm is stable.
    /// If this test fails, the hash algorithm has changed and all existing
    /// content hashes in the database are invalidated.
    #[test]
    fn test_guide_hash_golden_vector() {
        let mut files = BTreeMap::new();
        files.insert(
            "./guide.toml".to_string(),
            b"title = \"Test Guide\"\n".to_vec(),
        );
        files.insert(
            "./chapter1.md".to_string(),
            b"# Chapter 1\n\nHello world.\n".to_vec(),
        );
        files.insert(
            "./locales/en.ftl".to_string(),
            b"greeting = Hello\n".to_vec(),
        );

        let hash = hash_guide_directory(&files);
        assert_eq!(hash.as_str(), "76acb25502c02f282ebfc160c810284b0ce77e99");
    }

    /// Verify is_excluded_path works correctly with ./ prefix paths from tarballs.
    #[test]
    fn test_exclusion_with_dot_prefix() {
        // Paths in tarballs have ./ prefix; is_excluded_path works on the suffix
        assert!(!is_excluded_path("guide.toml"));
        assert!(is_excluded_path(".git/config"));
        assert!(is_excluded_path(".DS_Store"));
    }

    #[test]
    fn hash_value_slot_single_default_is_stable() {
        // A single slot folds in title, description, and kind; the cardinality
        // suffix stays omitted for the single default.
        let expected = {
            let mut h = Sha1::new();
            h.update(normalize_text("topic").as_bytes());
            h.update(NUL);
            h.update(normalize_text("").as_bytes());
            h.update(NUL);
            h.update(b"kind:");
            h.update(normalize_text("text").as_bytes());
            h.update(NUL);
            ContentHash::new(hex::encode(h.finalize()))
        };
        let now = hash_value_slot("topic", "", "text", &None, "single", None, None);
        assert_eq!(now.as_str(), expected.as_str());
    }

    #[test]
    fn hash_value_slot_kind_forks() {
        // The whole point of migration 70: a kind change re-versions the slot.
        let text = hash_value_slot("s", "", "text", &None, "single", None, None);
        let json = hash_value_slot("s", "", "json", &None, "single", None, None);
        assert_ne!(text.as_str(), json.as_str());
    }

    #[test]
    fn hash_value_slot_multi_forks_from_single() {
        let single = hash_value_slot("kept", "", "json", &None, "single", None, None);
        let multi = hash_value_slot("kept", "", "json", &None, "multi", None, None);
        assert_ne!(single.as_str(), multi.as_str());
    }

    #[test]
    fn hash_value_slot_bounds_change_the_hash() {
        let a = hash_value_slot("kept", "", "json", &None, "multi", Some(1), Some(5));
        let b = hash_value_slot("kept", "", "json", &None, "multi", Some(1), Some(3));
        let c = hash_value_slot("kept", "", "json", &None, "multi", None, None);
        assert_ne!(a.as_str(), b.as_str());
        assert_ne!(a.as_str(), c.as_str());
    }

    #[test]
    fn hash_guide_with_derived_no_children_is_directory_hash() {
        // The non-regression guarantee: a guide with no derived objects keeps
        // its directory hash exactly, so prose-only guides don't re-version.
        let dir = hash_guide_directory(&{
            let mut f = BTreeMap::new();
            f.insert("guide.toml".to_string(), b"title = \"x\"".to_vec());
            f
        });
        assert_eq!(
            hash_guide_with_derived(&dir, &[], &[], &[]).as_str(),
            dir.as_str()
        );
    }

    #[test]
    fn hash_guide_with_derived_folds_playbook_root() {
        // The core fix: a changed playbook root (same tarball) changes the
        // guide hash, so the guide re-versions and exposes the new playbook.
        let dir = ContentHash::new("a".repeat(40));
        let before = hash_guide_with_derived(&dir, &["pb_v1"], &[], &[]);
        let after = hash_guide_with_derived(&dir, &["pb_v2"], &[], &[]);
        assert_ne!(
            before, after,
            "a playbook-root change must fork the guide hash"
        );
        assert_ne!(
            before.as_str(),
            dir.as_str(),
            "folding a playbook must diverge from the bare directory hash"
        );
    }

    #[test]
    fn hash_guide_with_derived_is_set_wise_and_dedups() {
        let dir = ContentHash::new("b".repeat(40));
        // Order-independent across chapters.
        assert_eq!(
            hash_guide_with_derived(&dir, &["p1", "p2"], &[], &[]),
            hash_guide_with_derived(&dir, &["p2", "p1"], &[], &[]),
        );
        // The same root under two chapters contributes once.
        assert_eq!(
            hash_guide_with_derived(&dir, &["p1", "p1"], &[], &[]),
            hash_guide_with_derived(&dir, &["p1"], &[], &[]),
        );
    }

    #[test]
    fn hash_guide_with_derived_distinguishes_root_kinds() {
        // The same hash string folded as a playbook vs a brief-pack vs a profile
        // must NOT collide (tagged sections).
        let dir = ContentHash::new("c".repeat(40));
        let as_pb = hash_guide_with_derived(&dir, &["x"], &[], &[]);
        let as_pack = hash_guide_with_derived(&dir, &[], &["x"], &[]);
        let as_profile = hash_guide_with_derived(&dir, &[], &[], &["x"]);
        assert_ne!(as_pb, as_pack);
        assert_ne!(as_pb, as_profile);
        assert_ne!(as_pack, as_profile);
    }
}
