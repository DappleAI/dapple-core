//! Handler for canonical manifest sync and query endpoints.
//!
//! POST /api/canonical/sync  — Parse canonical TOML, resolve version pins, upsert to DB
//! GET  /api/canonical/      — Query persisted canonical pins

use crate::extract_qs::QsQuery as Query;
use crate::handler::index_response_handler;
use crate::schema::canonical::{
    CanonicalSyncErrorResponse, CanonicalSyncQueryParams, CanonicalSyncRequestSchema,
    CanonicalSyncResponse, GetCanonicalPinSetSchema, UnresolvedPinDetail,
};
use crate::{AppState, RegisteredUser};
use aide::axum::IntoApiResponse;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use dapple_data::model::canonical::{
    InsertCanonicalReleasePinArgs, InsertCanonicalVariantPinArgs, ResolveGuidePinArgs,
};
use dapple_data::query::canonical;
use dapple_parser::canonical::{CanonicalManifest, parse_canonical_toml};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

// ============================================================================
// POST /api/canonical/sync
// ============================================================================

pub async fn canonical_sync_handler(
    opts: Query<CanonicalSyncQueryParams>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
    Json(body): Json<CanonicalSyncRequestSchema>,
) -> impl IntoApiResponse {
    let Query(params) = opts;

    // 1. Parse TOML via dapple-parser
    let manifest = match parse_canonical_toml(&body.toml_content) {
        Ok(m) => m,
        Err(e) => {
            return sync_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!("Invalid canonical TOML: {}", e),
            );
        }
    };

    // 2. Validate scope matches request
    if manifest.metadata.scope != body.scope {
        return sync_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            &format!(
                "Scope mismatch: TOML metadata says '{}' but request says '{}'",
                manifest.metadata.scope, body.scope
            ),
        );
    }

    // 3. Check for variant/baseline conflicts
    let mut warnings = manifest.check_variant_baseline_conflicts();

    // 4. Collect all guide pins across environments
    let mut resolve_args = ResolveGuidePinArgs::default();

    for (env_name, pins) in &manifest.environments {
        for (ref_key, version_str) in &pins.guides {
            resolve_args.ref_keys.push(ref_key.clone());
            resolve_args.content_hashes.push(version_str.clone());
            let _ = env_name; // used below for non-guide pins
        }

        // Warn about non-guide pins (V1 only resolves guides)
        for (ref_key, version_str) in &pins.inference_profiles {
            warnings.push(format!(
                "inference_profile pin {ref_key}={version_str} in {env_name}: \
                 resolution not implemented in V1, storing unresolved"
            ));
        }
        for (ref_key, version_str) in &pins.tasks {
            warnings.push(format!(
                "task pin {ref_key}={version_str} in {env_name}: \
                 resolution not implemented in V1, storing unresolved"
            ));
        }
        for (ref_key, version_str) in &pins.brief_packs {
            warnings.push(format!(
                "brief_pack pin {ref_key}={version_str} in {env_name}: \
                 resolution not implemented in V1, storing unresolved"
            ));
        }
    }

    // 5. Batch resolve guide pins
    let resolved_pins = if !resolve_args.ref_keys.is_empty() {
        match canonical::resolve_guide_version_pins(&data.db, &resolve_args).await {
            Ok(rows) => rows,
            Err(e) => {
                return sync_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("Resolution query failed: {}", e),
                );
            }
        }
    } else {
        Vec::new()
    };

    // Build a resolution map: (ref_key, content_hash) -> (versioned_object_id, resolved_full_hash)
    let mut resolution_map: ResolutionMap = HashMap::new();
    for row in &resolved_pins {
        resolution_map.insert(
            (row.ref_key.clone(), row.content_hash.clone()),
            (row.versioned_object_id, row.resolved_content_hash.clone()),
        );
    }

    // 6. Build upsert args for release pins
    let (release_args, unresolved_pins) =
        build_release_pin_args(&manifest, &resolution_map, &mut warnings);

    let total_pins = release_args.ref_keys.len();
    let resolved_count = release_args
        .versioned_object_ids
        .iter()
        .filter(|id| id.is_some())
        .count();
    let unresolved_count = total_pins - resolved_count;

    // 7. Build upsert args for variant pins
    let variant_args = build_variant_pin_args(&manifest, &mut warnings);

    // 8. If dry_run, return summary without persisting
    if params.dry_run {
        let response = CanonicalSyncResponse {
            status: "dry_run".to_string(),
            scope: body.scope,
            total_pins,
            resolved: resolved_count,
            unresolved: unresolved_count,
            warnings,
            unresolved_pins,
        };
        return Ok((StatusCode::OK, Json(json!(response))));
    }

    // 9. Insert release pins (append-only with supersession)
    if !release_args.ref_keys.is_empty()
        && let Err(e) = canonical::insert_canonical_release_pins(&data.db, &release_args).await
    {
        return sync_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Release pin insert failed: {}", e),
        );
    }

    // 10. Insert variant pins (append-only with supersession)
    if !variant_args.ref_keys.is_empty()
        && let Err(e) = canonical::insert_canonical_variant_pins(&data.db, &variant_args).await
    {
        return sync_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Variant pin insert failed: {}", e),
        );
    }

    let response = CanonicalSyncResponse {
        status: "success".to_string(),
        scope: body.scope,
        total_pins,
        resolved: resolved_count,
        unresolved: unresolved_count,
        warnings,
        unresolved_pins,
    };

    Ok((StatusCode::OK, Json(json!(response))))
}

// ============================================================================
// GET /api/canonical/
// ============================================================================

pub async fn canonical_index_handler(
    opts: Query<GetCanonicalPinSetSchema>,
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    let Query(query) = opts;

    let objs = match query {
        GetCanonicalPinSetSchema::All { options } => {
            canonical::select_canonical_release_pins(&data.db, options.unwrap_or_default()).await
        }
        GetCanonicalPinSetSchema::ByEnvironment {
            scope,
            environment,
            options,
        } => {
            canonical::select_canonical_release_pins_by_environment(
                &data.db,
                &scope,
                &environment,
                options.unwrap_or_default(),
            )
            .await
        }
        GetCanonicalPinSetSchema::ByObjectIds { ids, .. } => {
            canonical::select_canonical_release_pins_by_object_ids(&data.db, &ids).await
        }
    };

    index_response_handler("canonical_release_pins", objs)
}

// ============================================================================
// POST /api/canonical/resolve
// ============================================================================

pub async fn canonical_resolve_handler(
    State(data): State<Arc<AppState>>,
    _user: RegisteredUser,
) -> impl IntoApiResponse {
    match canonical::resolve_unresolved_guide_pins(&data.db).await {
        Ok(resolved) => {
            let count = resolved.len();
            Ok((
                StatusCode::OK,
                Json(json!({
                    "status": "success",
                    "resolved": count,
                })),
            ))
        }
        Err(e) => sync_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Resolution failed: {}", e),
        ),
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Maps a `(ref_key, version_str)` request to its resolved
/// `(version_id, full_content_hash)` pair.
type ResolutionMap = HashMap<(String, String), (Option<Uuid>, Option<String>)>;

/// Build InsertCanonicalReleasePinArgs from the manifest and resolution results.
fn build_release_pin_args(
    manifest: &CanonicalManifest,
    resolution_map: &ResolutionMap,
    _warnings: &mut Vec<String>,
) -> (InsertCanonicalReleasePinArgs, Vec<UnresolvedPinDetail>) {
    let mut args = InsertCanonicalReleasePinArgs::default();
    let mut unresolved_pins = Vec::new();
    let now = OffsetDateTime::now_utc();

    for (env_name, pins) in &manifest.environments {
        // Process guide pins (resolved)
        for (ref_key, version_str) in &pins.guides {
            let key = (ref_key.clone(), version_str.clone());
            let (resolved_id, resolved_full_hash) =
                resolution_map.get(&key).cloned().unwrap_or((None, None));

            // Use the resolved full hash (from DB) when available,
            // falling back to the input hash for unresolved pins.
            let content_hash = resolved_full_hash.unwrap_or_else(|| version_str.clone());

            args.scopes.push(manifest.metadata.scope.clone());
            args.environments.push(env_name.clone());
            args.object_types.push("guides".to_string());
            args.ref_keys.push(ref_key.clone());
            args.content_hashes.push(content_hash);
            args.versioned_object_ids.push(resolved_id);
            args.resolved_ats.push(if resolved_id.is_some() {
                Some(now)
            } else {
                None
            });

            if resolved_id.is_none() {
                unresolved_pins.push(UnresolvedPinDetail {
                    environment: env_name.clone(),
                    object_type: "guides".to_string(),
                    ref_key: ref_key.clone(),
                    version: version_str.clone(),
                    reason: "Guide version not found in database".to_string(),
                });
            }
        }

        // Store non-guide pins as unresolved (V1 limitation)
        add_unresolved_pins(
            &mut args,
            &mut unresolved_pins,
            &manifest.metadata.scope,
            env_name,
            "inference_profiles",
            &pins.inference_profiles,
        );
        add_unresolved_pins(
            &mut args,
            &mut unresolved_pins,
            &manifest.metadata.scope,
            env_name,
            "tasks",
            &pins.tasks,
        );
        add_unresolved_pins(
            &mut args,
            &mut unresolved_pins,
            &manifest.metadata.scope,
            env_name,
            "brief_packs",
            &pins.brief_packs,
        );
    }

    (args, unresolved_pins)
}

/// Add unresolved pins for non-guide object types.
fn add_unresolved_pins(
    args: &mut InsertCanonicalReleasePinArgs,
    unresolved_pins: &mut Vec<UnresolvedPinDetail>,
    scope: &str,
    env_name: &str,
    object_type: &str,
    pins: &std::collections::HashMap<String, String>,
) {
    for (ref_key, version_str) in pins {
        args.scopes.push(scope.to_string());
        args.environments.push(env_name.to_string());
        args.object_types.push(object_type.to_string());
        args.ref_keys.push(ref_key.clone());
        args.content_hashes.push(version_str.clone());
        args.versioned_object_ids.push(None);
        args.resolved_ats.push(None);

        unresolved_pins.push(UnresolvedPinDetail {
            environment: env_name.to_string(),
            object_type: object_type.to_string(),
            ref_key: ref_key.clone(),
            version: version_str.clone(),
            reason: format!("{} resolution not implemented in V1", object_type),
        });
    }
}

/// Build InsertCanonicalVariantPinArgs from the manifest.
/// V1: all variant pins are stored unresolved.
fn build_variant_pin_args(
    manifest: &CanonicalManifest,
    _warnings: &mut Vec<String>,
) -> InsertCanonicalVariantPinArgs {
    let mut args = InsertCanonicalVariantPinArgs::default();

    for (env_name, env_variants) in &manifest.variants {
        for (bucket_name, bucket) in &env_variants.buckets {
            let add_variant =
                |args: &mut InsertCanonicalVariantPinArgs,
                 object_type: &str,
                 pins: &std::collections::HashMap<String, String>| {
                    for (ref_key, version_str) in pins {
                        args.scopes.push(manifest.metadata.scope.clone());
                        args.environments.push(env_name.clone());
                        args.buckets.push(bucket_name.clone());
                        args.object_types.push(object_type.to_string());
                        args.ref_keys.push(ref_key.clone());
                        args.content_hashes.push(version_str.clone());
                        args.versioned_object_ids.push(None);
                        args.resolved_ats.push(None);
                    }
                };

            add_variant(&mut args, "guides", &bucket.guides);
            add_variant(&mut args, "inference_profiles", &bucket.inference_profiles);
            add_variant(&mut args, "tasks", &bucket.tasks);
            add_variant(&mut args, "brief_packs", &bucket.brief_packs);
        }
    }

    args
}

fn sync_error(
    status: StatusCode,
    message: &str,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    Err((
        status,
        Json(json!(CanonicalSyncErrorResponse {
            error: super::status_to_error_code(status).to_string(),
            message: message.to_string(),
        })),
    ))
}
