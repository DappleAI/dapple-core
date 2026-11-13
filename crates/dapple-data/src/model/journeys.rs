use crate::error::DataError;
use crate::{InsertColumnArgs, Keyed};
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use std::collections::HashMap;
use time::OffsetDateTime;

// Re-export public DTOs from dapple-types
pub use dapple_types::model::journeys::*;

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertedJourneyModel {
    pub id: Uuid,
    pub playbook_id: Uuid,
    /// Legacy alias during the principals deprecation window: `user_id` here
    /// is the `rev.user_details.object_id` (Uuid) for human principals, NULL
    /// for non-human principals. New code should use `principal_id`. A future
    /// migration drops the `user_id` BIGINT column on `rev.journeys`.
    pub user_id: Option<Uuid>,
    pub principal_id: Uuid,
    pub name: String,
    pub created_at: OffsetDateTime,
    /// Journey-chain primitive — the journey this one continues from.
    /// `None` = chain root. See Migration 44.
    pub predecessor_journey_id: Option<Uuid>,
    /// Journey-chain primitive — the specific `rev.guides.id` (version) this
    /// chain is pinned to. `None` for legacy / ad-hoc journeys created
    /// before Migration 44 or outside any guide context.
    pub guide_id: Option<Uuid>,
    /// Stage-level fork point (Migration 74) — the predecessor's
    /// `rev.playbook_stages.id` this journey forked at. `None` = plain
    /// whole-journey chain edge.
    pub fork_stage_id: Option<Uuid>,
}

impl From<InsertedJourneyModel> for JourneyModel {
    fn from(model: InsertedJourneyModel) -> Self {
        Self {
            id: model.id,
            user_id: model.user_id,
            principal_id: model.principal_id,
            playbook_id: model.playbook_id,
            name: model.name,
            created_at: model.created_at,
            // Newly-created journeys never have prefs yet, so the resolved
            // hidden_at is always None on insert.
            hidden_at: None,
            brief_packs: Vec::new(),        // No brief packs from INSERT
            inference_profiles: Vec::new(), // No inference profiles from INSERT
            predecessor_journey_id: model.predecessor_journey_id,
            guide_id: model.guide_id,
            fork_stage_id: model.fork_stage_id,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatJourneyModel {
    pub id: Uuid,
    pub playbook_id: Uuid,
    /// Legacy alias — see [`InsertedJourneyModel::user_id`].
    pub user_id: Option<Uuid>,
    pub principal_id: Uuid,
    /// Effective name — `COALESCE(prefs.name, rev.journeys.name)` resolved at
    /// the SQL layer through the viewer's `rev.principal_journey_preferences`
    /// row (if any). Always non-NULL because the journey row's `name` is.
    pub name: String,
    pub created_at: OffsetDateTime,
    /// Effective hidden_at — the viewer's latest preference row's `hidden_at`,
    /// or NULL if they've never hidden this journey.
    pub hidden_at: Option<OffsetDateTime>,
    // Brief pack association fields (nullable from LEFT JOIN)
    pub journey_brief_pack_id: Option<Uuid>,
    pub brief_pack_id: Option<Uuid>,
    // Inference profile association fields (nullable from LEFT JOIN)
    pub journey_inference_profile_id: Option<Uuid>,
    pub inference_profile_id: Option<Uuid>,
    // Journey-chain / guide-pinning columns (Migration 44), carried through the
    // read path so a created journey round-trips its pin. Both are columns on
    // rev.journeys; nullable for chain roots / ad-hoc journeys.
    pub guide_id: Option<Uuid>,
    pub predecessor_journey_id: Option<Uuid>,
    /// Stage-level fork point (Migration 74); nullable like the other chain
    /// columns.
    pub fork_stage_id: Option<Uuid>,
}

impl FlatJourneyModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<JourneyModel>, DataError> {
        // Preconditions: models ordered by (journey_id, brief_pack_id, inference_profile_id)

        let mut journeys: HashMap<Uuid, JourneyModel> = HashMap::new();
        let mut order: Vec<Uuid> = Vec::new();

        for m in models.iter() {
            let journey = journeys.entry(m.id).or_insert_with(|| {
                order.push(m.id);
                JourneyModel {
                    id: m.id,
                    user_id: m.user_id,
                    principal_id: m.principal_id,
                    playbook_id: m.playbook_id,
                    name: m.name.clone(),
                    created_at: m.created_at,
                    hidden_at: m.hidden_at,
                    brief_packs: Vec::new(),
                    inference_profiles: Vec::new(),
                    predecessor_journey_id: m.predecessor_journey_id,
                    guide_id: m.guide_id,
                    fork_stage_id: m.fork_stage_id,
                }
            });

            // Add brief pack reference if present (avoid duplicates)
            if let Some(bp_id) = m.brief_pack_id
                && !journey.brief_packs.iter().any(|bp| bp.id == bp_id)
            {
                journey.brief_packs.push(BriefPackRef { id: bp_id });
            }

            // Add inference profile reference if present (avoid duplicates)
            if let Some(ip_id) = m.inference_profile_id
                && !journey.inference_profiles.iter().any(|ip| ip.id == ip_id)
            {
                journey
                    .inference_profiles
                    .push(InferenceProfileRef { id: ip_id });
            }
        }

        // Return in original order
        Ok(order
            .into_iter()
            .filter_map(|id| journeys.remove(&id))
            .collect())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JourneyModel {
    id: Uuid,
    user_id: Option<Uuid>,
    principal_id: Uuid,
    playbook_id: Uuid,
    /// Effective name (preference override resolved at the SQL layer, or the
    /// journey row's own `name` if the viewer hasn't set one).
    name: String,
    created_at: OffsetDateTime,
    /// Effective hidden_at — viewer's latest preference row's `hidden_at`, or
    /// NULL when the viewer hasn't hidden this journey.
    hidden_at: Option<OffsetDateTime>,
    brief_packs: Vec<BriefPackRef>,
    inference_profiles: Vec<InferenceProfileRef>,
    predecessor_journey_id: Option<Uuid>,
    guide_id: Option<Uuid>,
    fork_stage_id: Option<Uuid>,
}

impl Keyed for JourneyModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl From<JourneyModel> for Journey {
    fn from(model: JourneyModel) -> Self {
        Self {
            id: model.id,
            name: model.name,
            principal_id: model.principal_id,
            user_id: model.user_id,
            playbook_id: model.playbook_id,
            brief_packs: model.brief_packs,
            inference_profiles: model.inference_profiles,
            created_at: model.created_at.into(),
            hidden_at: model.hidden_at.map(Into::into),
            predecessor_journey_id: model.predecessor_journey_id,
            guide_id: model.guide_id,
            fork_stage_id: model.fork_stage_id,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertJourneyArgs {
    pub playbook_ids: Vec<Uuid>,
    /// Actor for the journey — `rev.principals.id`. The handler resolves
    /// `auth_user_id` (internal) or `external_service_token` (external) to
    /// the corresponding principal before calling.
    pub principal_ids: Vec<Uuid>,
    /// Optional subject — "on whose behalf". None for journeys the principal
    /// acts on its own (Shapes A and C); Some for delegated calls
    /// (Shapes B and D).
    pub subject_principal_ids: Vec<Option<Uuid>>,
    /// Resolved billing target. None is acceptable in V1 (many users have no
    /// billable_entity yet). Resolved caller-side via
    /// `resolve_billable_entity` and locked into the journey row.
    pub billable_entity_ids: Vec<Option<Uuid>>,
    pub names: Vec<String>,
    pub owner_company_ids: Vec<Uuid>,
    /// Journey-chain primitive (Migration 44). `None` for chain roots and
    /// for ad-hoc journeys outside any guide context.
    pub predecessor_journey_ids: Vec<Option<Uuid>>,
    /// Journey-chain primitive (Migration 44). `None` for journeys outside
    /// any guide context. When `predecessor_journey_id` is `Some`, the DB
    /// trigger rejects a mismatched or NULL `guide_id`.
    pub guide_ids: Vec<Option<Uuid>>,
    /// Stage-level fork point (Migration 74). `None` = whole-journey chain
    /// edge. When `Some`, the DB trigger rejects a missing predecessor or a
    /// stage outside the predecessor's playbook.
    pub fork_stage_ids: Vec<Option<Uuid>>,
}

impl InsertColumnArgs for InsertJourneyArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("playbook_ids", self.playbook_ids.len()),
            ("principal_ids", self.principal_ids.len()),
            ("subject_principal_ids", self.subject_principal_ids.len()),
            ("billable_entity_ids", self.billable_entity_ids.len()),
            ("names", self.names.len()),
            ("owner_company_ids", self.owner_company_ids.len()),
            (
                "predecessor_journey_ids",
                self.predecessor_journey_ids.len(),
            ),
            ("guide_ids", self.guide_ids.len()),
            ("fork_stage_ids", self.fork_stage_ids.len()),
        ])
    }
}

impl crate::traits::InsertedModel for InsertedJourneyModel {}
impl crate::Keyed for InsertedJourneyModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
impl crate::traits::RelationalObject for InsertedJourneyModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::RelationalObject for JourneyModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}
impl crate::traits::PrincipalObject for JourneyModel {
    fn display_name(&self) -> Option<&str> {
        Some(&self.name)
    }
}

impl crate::traits::FlatRelationalObject for FlatJourneyModel {
    type ParentInserted = InsertedJourneyModel;
    fn to_parent_inserted(&self) -> Self::ParentInserted {
        InsertedJourneyModel {
            id: self.id,
            playbook_id: self.playbook_id,
            user_id: self.user_id,
            principal_id: self.principal_id,
            name: self.name.clone(),
            created_at: self.created_at,
            predecessor_journey_id: self.predecessor_journey_id,
            guide_id: self.guide_id,
            fork_stage_id: self.fork_stage_id,
        }
    }
}

// ============================================================================
// InsertJourneyBriefPacksArgs (for inserting associations)
// ============================================================================

/// Ownership + guide columns for a single journey, used by handler-level
/// chain-validation before inserting a chained child journey.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JourneyChainAncestor {
    pub id: Uuid,
    pub owner_company_id: Uuid,
    pub principal_id: Uuid,
    pub guide_id: Option<Uuid>,
    /// The ancestor's playbook — used to validate a stage-level fork point
    /// (`fork_stage_id` must be a stage of the predecessor's playbook).
    pub playbook_id: Uuid,
}

/// Flat-row form of `select_chain_summaries_by_principal_and_guide.sql`
/// — one row per leaf, with repeated root metadata. Fold into
/// `Vec<ChainSummary>` via `ChainSummaryRow::fold`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChainSummaryRow {
    pub root_id: Uuid,
    pub root_name: String,
    pub root_created_at: OffsetDateTime,
    pub leaf_id: Uuid,
    pub depth: i32,
    pub leaf_playbook_id: Uuid,
    pub leaf_name: String,
    pub leaf_created_at: OffsetDateTime,
}

impl ChainSummaryRow {
    /// Fold flat rows (one per leaf) into nested `ChainSummary` DTOs (one
    /// per chain root). Preserves the SQL ORDER BY: outer order is
    /// `root_created_at DESC`; inner branches keep input order
    /// (`leaf_created_at DESC`).
    pub fn fold(rows: Vec<Self>) -> Vec<dapple_types::model::journeys::ChainSummary> {
        use dapple_types::model::journeys::{ChainBranch, ChainSummary};
        use std::collections::HashMap;

        let mut by_root: HashMap<Uuid, ChainSummary> = HashMap::new();
        let mut order: Vec<Uuid> = Vec::new();

        for row in rows {
            let summary = by_root.entry(row.root_id).or_insert_with(|| {
                order.push(row.root_id);
                ChainSummary {
                    root_id: row.root_id,
                    root_name: row.root_name.clone(),
                    root_created_at: row.root_created_at.into(),
                    branches: Vec::new(),
                }
            });
            summary.branches.push(ChainBranch {
                leaf_id: row.leaf_id,
                depth: row.depth,
                leaf_playbook_id: row.leaf_playbook_id,
                leaf_name: row.leaf_name,
                leaf_created_at: row.leaf_created_at.into(),
            });
        }

        order
            .into_iter()
            .filter_map(|id| by_root.remove(&id))
            .collect()
    }
}

/// Row form of `select_chain_journeys_from_id.sql` — one row per
/// journey in the chain, ordered root → starting journey. `hidden_at`
/// reflects the viewer principal's latest `principal_journey_preferences`
/// row; `None` means visible to the viewer.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChainJourneyRow {
    pub id: Uuid,
    pub playbook_id: Uuid,
    pub name: String,
    pub created_at: OffsetDateTime,
    pub principal_id: Uuid,
    pub owner_company_id: Uuid,
    pub predecessor_journey_id: Option<Uuid>,
    pub guide_id: Option<Uuid>,
    /// Stage-level fork point of THIS journey's edge to its predecessor
    /// (Migration 74); `None` for plain chain edges and roots.
    pub fork_stage_id: Option<Uuid>,
    pub depth: i32,
    pub hidden_at: Option<OffsetDateTime>,
}

/// Arguments for inserting journey_brief_packs associations
/// Uses ids which are resolved to internal IDs via CTE in SQL
#[derive(Clone, Debug)]
pub struct InsertJourneyBriefPacksArgs {
    pub journey_ids: Vec<Uuid>,
    pub brief_pack_ids: Vec<Uuid>,
}

// ============================================================================
// InsertJourneyInferenceProfilesArgs (for inserting associations)
// ============================================================================

/// Arguments for inserting journey_inference_profiles associations
/// Uses ids which are resolved to internal IDs via CTE in SQL
#[derive(Clone, Debug)]
pub struct InsertJourneyInferenceProfilesArgs {
    pub journey_ids: Vec<Uuid>,
    pub inference_profile_ids: Vec<Uuid>,
}

// ============================================================================
// InsertedJourneyBriefPackModel (result from association INSERT)
// ============================================================================

/// Result from journey_brief_packs INSERT
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedJourneyBriefPackModel {
    pub id: Uuid,
    pub journey_id: Uuid,
    pub brief_pack_id: Uuid,
    pub created_at: OffsetDateTime,
}

// ============================================================================
// InsertedJourneyInferenceProfileModel (result from association INSERT)
// ============================================================================

/// Result from journey_inference_profiles INSERT
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedJourneyInferenceProfileModel {
    pub id: Uuid,
    pub journey_id: Uuid,
    pub inference_profile_id: Uuid,
}

// ============================================================================
// JourneyBrief (flat result for /journeys/{id}/briefs endpoint)
// ============================================================================

/// Flat SQL result for briefs with journey context
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatJourneyBriefModel {
    // Brief fields
    pub id: Uuid,
    pub brief_versioned_object_id: Uuid,
    pub brief_path_id: Uuid,
    pub path_id: Uuid,
    pub name: String,
    pub content_hash: String,
    pub title: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
    // Context: BriefPack
    pub brief_pack_id: Uuid,
    // Context: BriefGroup
    pub brief_group_id: Uuid,
    pub brief_group_name: String,
    pub brief_group_title: String,
}

impl From<FlatJourneyBriefModel> for JourneyBrief {
    fn from(model: FlatJourneyBriefModel) -> Self {
        Self {
            id: model.id,
            path_id: model.path_id,
            name: model.name,
            content_hash: crate::ContentHash::new(model.content_hash),
            title: model.title,
            description: model.description,
            created_at: model.created_at.into(),
            context: JourneyBriefContext {
                brief_pack_id: model.brief_pack_id,
                brief_group_id: model.brief_group_id,
                brief_group_name: model.brief_group_name,
                brief_group_title: model.brief_group_title,
            },
        }
    }
}
