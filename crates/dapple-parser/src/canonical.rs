//! Parser for canonical.toml release manifest files.
//!
//! A canonical manifest pins specific versions of versioned objects
//! (guides, inference profiles, tasks, brief packs) per environment,
//! and defines variant pools for A/B testing via Flagsmith.
//!
//! Each scope (common, internal) has its own manifest file located at
//! `canonical/{scope}.toml` in the dapple-guides repository.

use crate::error::ParseError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Valid environments (in promotion order)
// ============================================================================

/// The fixed set of deployment environments, in promotion order.
pub const VALID_ENVIRONMENTS: &[&str] = &["sandbox", "dev", "stage", "performance", "production"];

/// The valid scopes for canonical manifests.
pub const VALID_SCOPES: &[&str] = &["common", "internal"];

/// The versioned object types that can be pinned in a canonical manifest.
pub const VALID_OBJECT_TYPES: &[&str] = &["guides", "inference_profiles", "tasks", "brief_packs"];

// ============================================================================
// Top-level manifest
// ============================================================================

/// The complete parsed representation of a canonical.toml manifest.
///
/// Pins specific versions of versioned objects per environment and
/// defines variant buckets for A/B testing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanonicalManifest {
    /// Manifest metadata (scope, description).
    pub metadata: ManifestMetadata,
    /// Canonical version pins per environment.
    /// Keys are environment names (e.g., "sandbox", "production").
    #[serde(default)]
    pub environments: HashMap<String, EnvironmentPins>,
    /// Variant pools per environment for A/B testing.
    /// Keys are environment names.
    #[serde(default)]
    pub variants: HashMap<String, EnvironmentVariants>,
}

/// Metadata section of the canonical manifest.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestMetadata {
    /// The scope this manifest governs ("common" or "internal").
    pub scope: String,
    /// Human-readable description of the manifest.
    #[serde(default)]
    pub description: Option<String>,
}

// ============================================================================
// Environment pins (the "A" baseline)
// ============================================================================

/// Version pins for all object types in a single environment.
///
/// Each field maps ref_key → version string.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EnvironmentPins {
    /// Guide ref_key → version (e.g., "public/dapple/essay-writing" = "0.1.0").
    #[serde(default)]
    pub guides: HashMap<String, String>,
    /// Inference profile ref_key → version.
    #[serde(default)]
    pub inference_profiles: HashMap<String, String>,
    /// Task ref_key → version.
    #[serde(default)]
    pub tasks: HashMap<String, String>,
    /// Brief pack ref_key → version.
    #[serde(default)]
    pub brief_packs: HashMap<String, String>,
}

// ============================================================================
// Variant pools (for Flagsmith A/B testing)
// ============================================================================

/// Variant configuration for a single environment.
///
/// Contains named buckets, each defining alternative versions for
/// specific objects. Flagsmith controls which users see which bucket.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EnvironmentVariants {
    /// Named variant buckets (e.g., "early-adopters", "cost-optimized").
    #[serde(default)]
    pub buckets: HashMap<String, VariantBucket>,
}

/// A single variant bucket — a named set of alternative version pins.
///
/// Users assigned to this bucket by Flagsmith receive these versions
/// instead of the canonical baseline.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VariantBucket {
    /// Human-readable description of the bucket's purpose.
    #[serde(default)]
    pub description: Option<String>,
    /// Guide ref_key → variant version.
    #[serde(default)]
    pub guides: HashMap<String, String>,
    /// Inference profile ref_key → variant version.
    #[serde(default)]
    pub inference_profiles: HashMap<String, String>,
    /// Task ref_key → variant version.
    #[serde(default)]
    pub tasks: HashMap<String, String>,
    /// Brief pack ref_key → variant version.
    #[serde(default)]
    pub brief_packs: HashMap<String, String>,
}

// ============================================================================
// Parsing and validation
// ============================================================================

/// Parse and validate a canonical.toml manifest from file contents.
///
/// Validates:
/// - Scope is one of the known values ("common", "internal")
/// - All environment names are valid
/// - All version strings are valid content hashes (8-40 lowercase hex chars)
/// - Variant environments reference valid environments
pub fn parse_canonical_toml(content: &str) -> Result<CanonicalManifest, ParseError> {
    let manifest: CanonicalManifest =
        toml::from_str(content).map_err(|e| ParseError::InvalidCanonicalToml {
            detail: e.to_string(),
        })?;

    // Validate scope
    if !VALID_SCOPES.contains(&manifest.metadata.scope.as_str()) {
        return Err(ParseError::InvalidCanonicalToml {
            detail: format!(
                "unknown scope \"{}\"; valid scopes are: {}",
                manifest.metadata.scope,
                VALID_SCOPES.join(", ")
            ),
        });
    }

    // Validate environment names and version strings
    for (env_name, pins) in &manifest.environments {
        if !VALID_ENVIRONMENTS.contains(&env_name.as_str()) {
            return Err(ParseError::InvalidCanonicalToml {
                detail: format!(
                    "unknown environment \"{env_name}\"; valid environments are: {}",
                    VALID_ENVIRONMENTS.join(", ")
                ),
            });
        }
        validate_version_map(&pins.guides, "environments", env_name, "guides")?;
        validate_version_map(
            &pins.inference_profiles,
            "environments",
            env_name,
            "inference_profiles",
        )?;
        validate_version_map(&pins.tasks, "environments", env_name, "tasks")?;
        validate_version_map(&pins.brief_packs, "environments", env_name, "brief_packs")?;
    }

    // Validate variant environment names and version strings
    for (env_name, env_variants) in &manifest.variants {
        if !VALID_ENVIRONMENTS.contains(&env_name.as_str()) {
            return Err(ParseError::InvalidCanonicalToml {
                detail: format!(
                    "unknown variant environment \"{env_name}\"; valid environments are: {}",
                    VALID_ENVIRONMENTS.join(", ")
                ),
            });
        }
        for (bucket_name, bucket) in &env_variants.buckets {
            let section = format!("variants.{env_name}.buckets.{bucket_name}");
            validate_version_map(&bucket.guides, &section, bucket_name, "guides")?;
            validate_version_map(
                &bucket.inference_profiles,
                &section,
                bucket_name,
                "inference_profiles",
            )?;
            validate_version_map(&bucket.tasks, &section, bucket_name, "tasks")?;
            validate_version_map(&bucket.brief_packs, &section, bucket_name, "brief_packs")?;
        }
    }

    Ok(manifest)
}

/// Validate that a version string is a valid content hash (8-40 lowercase hex chars).
fn validate_version_string(version: &str) -> Result<(), String> {
    if version.is_empty() {
        return Err("content hash cannot be empty".into());
    }
    let is_hex = version.chars().all(|c| c.is_ascii_hexdigit());
    if !is_hex {
        return Err("must be a hex content hash (8-40 lowercase hex chars)".into());
    }
    if version.len() < 8 || version.len() > 40 {
        return Err("content hash must be 8-40 hex characters".into());
    }
    let is_lowercase = version.chars().all(|c| !c.is_ascii_uppercase());
    if !is_lowercase {
        return Err("content hash must be lowercase hex".into());
    }
    Ok(())
}

/// Validate that all version strings in a map are valid content hashes.
fn validate_version_map(
    map: &HashMap<String, String>,
    section: &str,
    context: &str,
    object_type: &str,
) -> Result<(), ParseError> {
    for (ref_key, version) in map {
        validate_version_string(version).map_err(|e| ParseError::InvalidCanonicalToml {
            detail: format!(
                "invalid version \"{version}\" for {object_type} \
                 \"{ref_key}\" in [{section}.{context}]: {e}"
            ),
        })?;
    }
    Ok(())
}

// ============================================================================
// Convenience accessors
// ============================================================================

impl CanonicalManifest {
    /// Get the canonical version pin for a specific object in an environment.
    ///
    /// Returns `None` if the environment or ref_key is not pinned.
    pub fn get_pin(&self, environment: &str, object_type: &str, ref_key: &str) -> Option<&str> {
        let pins = self.environments.get(environment)?;
        let map = match object_type {
            "guides" => &pins.guides,
            "inference_profiles" => &pins.inference_profiles,
            "tasks" => &pins.tasks,
            "brief_packs" => &pins.brief_packs,
            _ => return None,
        };
        map.get(ref_key).map(|s| s.as_str())
    }

    /// Get the variant version for a specific bucket, object type, and ref_key.
    ///
    /// Returns `None` if no variant is configured.
    pub fn get_variant(
        &self,
        environment: &str,
        bucket: &str,
        object_type: &str,
        ref_key: &str,
    ) -> Option<&str> {
        let env_variants = self.variants.get(environment)?;
        let variant_bucket = env_variants.buckets.get(bucket)?;
        let map = match object_type {
            "guides" => &variant_bucket.guides,
            "inference_profiles" => &variant_bucket.inference_profiles,
            "tasks" => &variant_bucket.tasks,
            "brief_packs" => &variant_bucket.brief_packs,
            _ => return None,
        };
        map.get(ref_key).map(|s| s.as_str())
    }

    /// List all ref_keys pinned across all environments for a given object type.
    pub fn all_ref_keys(&self, object_type: &str) -> Vec<&str> {
        let mut keys = std::collections::BTreeSet::new();
        for pins in self.environments.values() {
            let map = match object_type {
                "guides" => &pins.guides,
                "inference_profiles" => &pins.inference_profiles,
                "tasks" => &pins.tasks,
                "brief_packs" => &pins.brief_packs,
                _ => continue,
            };
            keys.extend(map.keys().map(|k| k.as_str()));
        }
        keys.into_iter().collect()
    }

    /// Validate that variant versions are distinct from their canonical baseline.
    ///
    /// Returns warnings for any variant that pins the same version as canonical.
    pub fn check_variant_baseline_conflicts(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for (env_name, env_variants) in &self.variants {
            let Some(pins) = self.environments.get(env_name) else {
                continue;
            };
            for (bucket_name, bucket) in &env_variants.buckets {
                check_map_conflicts(
                    &pins.guides,
                    &bucket.guides,
                    env_name,
                    bucket_name,
                    "guides",
                    &mut warnings,
                );
                check_map_conflicts(
                    &pins.inference_profiles,
                    &bucket.inference_profiles,
                    env_name,
                    bucket_name,
                    "inference_profiles",
                    &mut warnings,
                );
                check_map_conflicts(
                    &pins.tasks,
                    &bucket.tasks,
                    env_name,
                    bucket_name,
                    "tasks",
                    &mut warnings,
                );
                check_map_conflicts(
                    &pins.brief_packs,
                    &bucket.brief_packs,
                    env_name,
                    bucket_name,
                    "brief_packs",
                    &mut warnings,
                );
            }
        }
        warnings
    }
}

fn check_map_conflicts(
    canonical: &HashMap<String, String>,
    variant: &HashMap<String, String>,
    env: &str,
    bucket: &str,
    object_type: &str,
    warnings: &mut Vec<String>,
) {
    for (ref_key, variant_version) in variant {
        if canonical.get(ref_key) == Some(variant_version) {
            warnings.push(format!(
                "variant bucket \"{bucket}\" in {env} pins {object_type} \
                 \"{ref_key}\" to version {variant_version}, which is the \
                 same as the canonical baseline"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_manifest() {
        let content = r#"
[metadata]
scope = "common"
description = "Test manifest"
"#;
        let manifest = parse_canonical_toml(content).unwrap();
        assert_eq!(manifest.metadata.scope, "common");
        assert_eq!(
            manifest.metadata.description,
            Some("Test manifest".to_string())
        );
        assert!(manifest.environments.is_empty());
        assert!(manifest.variants.is_empty());
    }

    #[test]
    fn test_parse_with_environment_pins() {
        let content = r#"
[metadata]
scope = "common"

[environments.sandbox.guides]
"public/dapple/essay-writing" = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0"

[environments.sandbox.inference_profiles]
"common/claude-sonnet-default" = "b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1"

[environments.production.guides]
"public/dapple/essay-writing" = "c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2"
"#;
        let manifest = parse_canonical_toml(content).unwrap();
        assert_eq!(manifest.environments.len(), 2);

        let sandbox = &manifest.environments["sandbox"];
        assert_eq!(
            sandbox.guides.get("public/dapple/essay-writing"),
            Some(&"a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0".to_string())
        );
        assert_eq!(
            sandbox
                .inference_profiles
                .get("common/claude-sonnet-default"),
            Some(&"b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1".to_string())
        );

        let production = &manifest.environments["production"];
        assert_eq!(
            production.guides.get("public/dapple/essay-writing"),
            Some(&"c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2".to_string())
        );
    }

    #[test]
    fn test_parse_with_variants() {
        let content = r#"
[metadata]
scope = "common"

[environments.production.guides]
"public/dapple/essay-writing" = "c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2"

[variants.production.buckets.early-adopters]
description = "Production early adopters"

[variants.production.buckets.early-adopters.guides]
"public/dapple/essay-writing" = "d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3"

[variants.production.buckets.cost-optimized]
description = "Cost-optimized users"

[variants.production.buckets.cost-optimized.inference_profiles]
"common/claude-haiku-fast" = "e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4"
"#;
        let manifest = parse_canonical_toml(content).unwrap();
        assert_eq!(manifest.variants.len(), 1);

        let prod_variants = &manifest.variants["production"];
        assert_eq!(prod_variants.buckets.len(), 2);

        let early = &prod_variants.buckets["early-adopters"];
        assert_eq!(
            early.description,
            Some("Production early adopters".to_string())
        );
        assert_eq!(
            early.guides.get("public/dapple/essay-writing"),
            Some(&"d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3".to_string())
        );

        let cost = &prod_variants.buckets["cost-optimized"];
        assert_eq!(
            cost.inference_profiles.get("common/claude-haiku-fast"),
            Some(&"e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4".to_string())
        );
    }

    #[test]
    fn test_invalid_scope_rejected() {
        let content = r#"
[metadata]
scope = "public"
"#;
        let result = parse_canonical_toml(content);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("unknown scope"), "Error: {}", err);
    }

    #[test]
    fn test_invalid_environment_rejected() {
        let content = r#"
[metadata]
scope = "common"

[environments.testing.guides]
"public/dapple/essay-writing" = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0"
"#;
        let result = parse_canonical_toml(content);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("unknown environment"), "Error: {}", err);
    }

    #[test]
    fn test_invalid_version_rejected() {
        let content = r#"
[metadata]
scope = "common"

[environments.sandbox.guides]
"public/dapple/essay-writing" = "not-a-hash"
"#;
        let result = parse_canonical_toml(content);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("hex"), "Error: {}", err);
    }

    #[test]
    fn test_invalid_variant_environment_rejected() {
        let content = r#"
[metadata]
scope = "common"

[variants.bogus.buckets.test]
description = "Bad env"
"#;
        let result = parse_canonical_toml(content);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("unknown variant environment"),
            "Error: {}",
            err
        );
    }

    #[test]
    fn test_invalid_variant_version_rejected() {
        let content = r#"
[metadata]
scope = "common"

[variants.production.buckets.test.guides]
"public/dapple/essay-writing" = "xyz"
"#;
        let result = parse_canonical_toml(content);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("hex"), "Error: {}", err);
    }

    #[test]
    fn test_get_pin() {
        let content = r#"
[metadata]
scope = "common"

[environments.production.guides]
"public/dapple/essay-writing" = "c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2"

[environments.production.inference_profiles]
"common/claude-sonnet-default" = "f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5"
"#;
        let manifest = parse_canonical_toml(content).unwrap();

        assert_eq!(
            manifest.get_pin("production", "guides", "public/dapple/essay-writing"),
            Some("c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2")
        );
        assert_eq!(
            manifest.get_pin(
                "production",
                "inference_profiles",
                "common/claude-sonnet-default"
            ),
            Some("f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5")
        );
        assert_eq!(
            manifest.get_pin("production", "guides", "nonexistent"),
            None
        );
        assert_eq!(
            manifest.get_pin("sandbox", "guides", "public/dapple/essay-writing"),
            None
        );
    }

    #[test]
    fn test_get_variant() {
        let content = r#"
[metadata]
scope = "common"

[variants.production.buckets.early-adopters.guides]
"public/dapple/essay-writing" = "d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3"
"#;
        let manifest = parse_canonical_toml(content).unwrap();

        assert_eq!(
            manifest.get_variant(
                "production",
                "early-adopters",
                "guides",
                "public/dapple/essay-writing"
            ),
            Some("d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3")
        );
        assert_eq!(
            manifest.get_variant("production", "early-adopters", "guides", "nonexistent"),
            None
        );
        assert_eq!(
            manifest.get_variant(
                "production",
                "nonexistent",
                "guides",
                "public/dapple/essay-writing"
            ),
            None
        );
    }

    #[test]
    fn test_all_ref_keys() {
        let content = r#"
[metadata]
scope = "common"

[environments.sandbox.guides]
"public/dapple/essay-writing" = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0"

[environments.production.guides]
"public/dapple/essay-writing" = "c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2"
"public/dapple/report-helper" = "f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5"
"#;
        let manifest = parse_canonical_toml(content).unwrap();
        let keys = manifest.all_ref_keys("guides");
        assert_eq!(
            keys,
            vec!["public/dapple/essay-writing", "public/dapple/report-helper"]
        );
    }

    #[test]
    fn test_check_variant_baseline_conflicts() {
        let content = r#"
[metadata]
scope = "common"

[environments.production.guides]
"public/dapple/essay-writing" = "c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2"

[variants.production.buckets.early-adopters.guides]
"public/dapple/essay-writing" = "c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2"
"#;
        let manifest = parse_canonical_toml(content).unwrap();
        let warnings = manifest.check_variant_baseline_conflicts();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("same as the canonical baseline"));
    }

    #[test]
    fn test_check_variant_no_conflicts() {
        let content = r#"
[metadata]
scope = "common"

[environments.production.guides]
"public/dapple/essay-writing" = "c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2"

[variants.production.buckets.early-adopters.guides]
"public/dapple/essay-writing" = "d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3"
"#;
        let manifest = parse_canonical_toml(content).unwrap();
        let warnings = manifest.check_variant_baseline_conflicts();
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_parse_full_manifest() {
        let content = r#"
[metadata]
scope = "common"
description = "Canonical versions for objects available to all guide tiers"

[environments.sandbox.guides]
"public/dapple/essay-writing" = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0"

[environments.sandbox.inference_profiles]
"common/claude-sonnet-default" = "b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1"
"common/claude-haiku-fast" = "c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2"

[environments.sandbox.tasks]
"common/validate-output" = "d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3"

[environments.dev.guides]
"public/dapple/essay-writing" = "e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4"

[environments.dev.inference_profiles]
"common/claude-sonnet-default" = "f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5"
"common/claude-haiku-fast" = "1122334455667788990011223344556677889900"

[environments.stage.guides]
"public/dapple/essay-writing" = "aabbccdd11223344556677889900aabbccddeeff"

[environments.stage.inference_profiles]
"common/claude-sonnet-default" = "1234567890abcdef1234567890abcdef12345678"

[environments.performance.guides]
"public/dapple/essay-writing" = "aabbccdd11223344556677889900aabbccddeeff"

[environments.performance.inference_profiles]
"common/claude-sonnet-default" = "1234567890abcdef1234567890abcdef12345678"

[environments.production.guides]
"public/dapple/essay-writing" = "aabbccdd11223344556677889900aabbccddeeff"

[environments.production.inference_profiles]
"common/claude-sonnet-default" = "1234567890abcdef1234567890abcdef12345678"

[variants.stage.buckets.early-adopters]
description = "Power users testing next-gen models"

[variants.stage.buckets.early-adopters.inference_profiles]
"common/claude-sonnet-default" = "f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5"

[variants.stage.buckets.early-adopters.guides]
"public/dapple/essay-writing" = "e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4"

[variants.production.buckets.early-adopters]
description = "Production early adopters"

[variants.production.buckets.early-adopters.inference_profiles]
"common/claude-sonnet-default" = "fedcba9876543210fedcba9876543210fedcba98"

[variants.production.buckets.cost-optimized]
description = "Users testing cheaper model configurations"

[variants.production.buckets.cost-optimized.inference_profiles]
"common/claude-haiku-fast" = "abcdef1234567890abcdef1234567890abcdef12"
"#;
        let manifest = parse_canonical_toml(content).unwrap();
        assert_eq!(manifest.metadata.scope, "common");
        assert_eq!(manifest.environments.len(), 5);
        assert_eq!(manifest.variants.len(), 2);

        // Spot check various pins
        assert_eq!(
            manifest.get_pin("sandbox", "tasks", "common/validate-output"),
            Some("d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3")
        );
        assert_eq!(
            manifest.get_pin("dev", "inference_profiles", "common/claude-haiku-fast"),
            Some("1122334455667788990011223344556677889900")
        );
        assert_eq!(
            manifest.get_variant(
                "production",
                "cost-optimized",
                "inference_profiles",
                "common/claude-haiku-fast"
            ),
            Some("abcdef1234567890abcdef1234567890abcdef12")
        );
    }

    #[test]
    fn test_internal_scope() {
        let content = r#"
[metadata]
scope = "internal"
description = "Internal-only canonical versions"

[environments.sandbox.guides]
"internal/dapple/admin-dashboard" = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0"
"#;
        let manifest = parse_canonical_toml(content).unwrap();
        assert_eq!(manifest.metadata.scope, "internal");
    }

    #[test]
    fn test_short_hash_accepted() {
        let content = r#"
[metadata]
scope = "common"

[environments.sandbox.guides]
"public/dapple/essay-writing" = "a1b2c3d4"
"#;
        let manifest = parse_canonical_toml(content).unwrap();
        assert_eq!(
            manifest.get_pin("sandbox", "guides", "public/dapple/essay-writing"),
            Some("a1b2c3d4")
        );
    }

    #[test]
    fn test_too_short_hash_rejected() {
        let content = r#"
[metadata]
scope = "common"

[environments.sandbox.guides]
"public/dapple/essay-writing" = "a1b2c3"
"#;
        let result = parse_canonical_toml(content);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("8-40"), "Error: {}", err);
    }

    #[test]
    fn test_uppercase_hash_rejected() {
        let content = r#"
[metadata]
scope = "common"

[environments.sandbox.guides]
"public/dapple/essay-writing" = "A1B2C3D4E5F6A7B8"
"#;
        let result = parse_canonical_toml(content);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("lowercase"), "Error: {}", err);
    }

    #[test]
    fn test_empty_environment_pins() {
        let content = r#"
[metadata]
scope = "common"

[environments.sandbox]
"#;
        let manifest = parse_canonical_toml(content).unwrap();
        let sandbox = &manifest.environments["sandbox"];
        assert!(sandbox.guides.is_empty());
        assert!(sandbox.inference_profiles.is_empty());
        assert!(sandbox.tasks.is_empty());
        assert!(sandbox.brief_packs.is_empty());
    }
}
