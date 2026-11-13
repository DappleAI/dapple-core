# Conduit Routing

How conduit routes prompt requests to LLM backends based on routing strategy, model access, and availability.

---

## Overview

<!-- ANCHOR: ops-conduit-routing -->
Conduit is the prompt execution service that:
1. Receives prompt requests from dispatches
2. Checks access via Lago (entitlements, quota)
3. Gets routing strategy from Flagsmith
4. Scores available models based on strategy
5. Executes the prompt with selected model
6. Creates resource claims for billing

**Decision Sources:**

| Source | Provides | Examples |
|--------|----------|----------|
| **Lago** | Model access (entitlements), quota | "Plan includes sonnet", "80% of quota used" |
| **Flagsmith** | Routing strategy, overrides | "cost_optimized", "beta tester gets opus" |
| **InferenceProfile** | Available models for Journey | Priority-ordered model list |
| **Task Analysis** | Complexity score | Prompt length, expected output, task type |
<!-- /ANCHOR -->

---

## Routing Strategies (Flagsmith)

<!-- ANCHOR: ops-routing-strategies -->
Routing strategy determines how conduit scores and selects models. Strategy is set via Flagsmith feature flags, typically based on plan but overridable per segment.

### Available Strategies

| Strategy | Behavior | Scoring Bias |
|----------|----------|--------------|
| `cost_optimized` | Cheapest model that can handle task | Cost weight high, quality weight low |
| `quality_first` | Best model available | Quality weight high, cost weight low |
| `quality_experiment` | A/B test quality vs cost | Varies by variant assignment |

### Strategy Assignment

```
Flagsmith Feature: routing_strategy
Type: Multivariate (string)

Default: "cost_optimized"

Segment overrides:
  - Pro Plan Users     → "quality_first"
  - Enterprise Users   → "quality_first"
  - Quality Experiment → "quality_experiment"
```

### Model Access Overrides

Lago entitlements define what models a plan includes. Flagsmith can override for specific segments:

```
Flagsmith Feature: opus_access_override
Type: Boolean

Default: false (use Lago entitlement)

Segment overrides:
  - Beta Testers → true
  - VIP Customers → true
```

### How Strategy Affects Scoring

Each model gets a score based on multiple factors. Strategy adjusts the weights:

```
score = (quality_score × quality_weight)
      + (cost_score × cost_weight)
      + (availability_score × availability_weight)
      + (task_fit_score × task_fit_weight)

cost_optimized:     quality_weight=0.2, cost_weight=0.5
quality_first:      quality_weight=0.5, cost_weight=0.2
quality_experiment: depends on variant
```

Models are sorted by score descending, first available model is selected.
<!-- /ANCHOR -->

---

## Model Selection

<!-- ANCHOR: ops-tier-model-mapping -->
### InferenceProfiles (Existing System)

Model selection is handled by **InferenceProfiles**, which are attached to Journeys at creation time:

```
Journey = Playbook + BriefPacks + InferenceProfiles
```

Each InferenceProfile defines:
- Which prompts it covers (by `prompt_path_object_id`)
- Which models can be used (by priority)
- Model tier within lineage (e.g., haiku, sonnet, opus)

### Model Tier (Existing Concept)

`model_release.tier` defines capability levels within a model lineage:
- `haiku` — Fast, cost-effective
- `sonnet` — Balanced quality/cost
- `opus` — Highest quality

This is NOT subscription tier — it's model capability tier.

### Runtime Selection

At runtime, Conduit picks from InferenceProfile options based on:
1. Provider availability (is Anthropic API up?)
2. Health status (recent errors?)
3. **Credit/quota** (does user have balance?) ← Lago API check
4. Provider-specific constraints

### Audit Concerns
- Track actual model used per request (`llm_usages.model_release_id`)
- Compare with InferenceProfile options (was selection valid?)
- Alert on model access outside InferenceProfile (security issue)
<!-- /ANCHOR -->

---

## Routing Decision Flow

<!-- ANCHOR: ops-routing-flow -->
```
Prompt Request
      ↓
┌─────────────────────────────────────┐
│ 1. Lookup billable entity           │
│    (from journey → user → billable) │
└─────────────────────────────────────┘
      ↓
┌─────────────────────────────────────┐
│ 2. Call Lago API (pre-flight check) │
│    - GET /current_usage             │
│    - GET /entitlements              │
│    - GET /wallets (if prepaid)      │
└─────────────────────────────────────┘
      ↓
      ├─ Lago unavailable → Reject (503)
      ├─ No entitlement → Reject (403)
      ├─ Quota exceeded & !allow_overage → Reject (429)
      ↓
┌─────────────────────────────────────┐
│ 3. Call Flagsmith (routing config)  │
│    identity: billable_entity_id     │
│    traits: { plan_tier: "pro" }     │
│                                     │
│    Returns:                         │
│    - routing_strategy               │
│    - model access overrides         │
│    - allow_overage                  │
│    - experiment variants            │
└─────────────────────────────────────┘
      ↓
┌─────────────────────────────────────┐
│ 4. Build model pool                 │
│    - Start with InferenceProfile    │
│    - Filter by Lago entitlements    │
│    - Apply Flagsmith overrides      │
└─────────────────────────────────────┘
      ↓
┌─────────────────────────────────────┐
│ 5. Analyze task complexity          │
│    - Prompt length                  │
│    - Expected output size           │
│    - Task type (coding, creative)   │
│    → complexity_score               │
└─────────────────────────────────────┘
      ↓
┌─────────────────────────────────────┐
│ 6. Score models by strategy         │
│    For each model in pool:          │
│    score = weighted(quality, cost,  │
│             availability, task_fit) │
│    Sort by score DESC               │
└─────────────────────────────────────┘
      ↓
┌─────────────────────────────────────┐
│ 7. Select and execute               │
│    - Pick highest-scoring available │
│    - Create resource claim          │
│    - Execute prompt                 │
│    - On failure: try next in list   │
└─────────────────────────────────────┘
      ↓
Response (or error after fallbacks exhausted)
```

### Service Unavailability

**Lago unavailable:**
- **Deny by default** (safe — prevents serving suspended users)
- Log failure for SRE alerting
- Return 503 Service Unavailable

**Flagsmith unavailable:**
- Use cached flags if available (local evaluation mode)
- Fall back to default strategy (`cost_optimized`)
- Log but don't block — routing strategy is optimization, not access control

### Optimization

Claim-in-chunks pattern reduces Lago API calls by pre-authorizing usage in batches. See [Lago Integration](../integrations/recurring-billing.md#claim-in-chunks-pattern-pre-authorization).

Flagsmith local evaluation mode caches flags, reducing API calls to periodic refreshes.
<!-- /ANCHOR -->

---

## Implementation Notes

### No New Tables Required

Model routing configuration is handled by existing infrastructure + external services:
- **InferenceProfiles** — Define which models are available per Journey
- **model_releases** — Define model tiers (haiku, sonnet, opus)
- **inference_profile_configs** — Map prompts to models with priority
- **Lago** (external) — Entitlements, quota, usage
- **Flagsmith** (external) — Routing strategy, feature flags, overrides

### Lago + Flagsmith Integration (Phase 5)

```rust
// Pseudocode for routing decision in conduit

pub async fn route_request(
    lago: &LagoClient,
    flagsmith: &Flagsmith,
    billable_entity: &BillableEntity,
    inference_profile: &InferenceProfile,
    task: &Task,
) -> Result<ModelSelection, RoutingError> {
    // 1. Lago pre-flight check
    let usage = lago.get_current_usage(&billable_entity.lago_customer_id).await?;
    let entitlements = lago.get_entitlements(&billable_entity.lago_subscription_id).await?;

    // 2. Get Flagsmith flags with plan trait
    let flags = flagsmith.get_identity_flags_with_traits(
        &billable_entity.id.to_string(),
        vec![("plan_tier", &entitlements.plan_code)],
    ).await.unwrap_or_default();  // Fall back to defaults if Flagsmith down

    // 3. Check quota + overage permission
    let allow_overage = flags.is_feature_enabled("allow_overage");
    if usage.is_over_quota() && !allow_overage {
        return Err(RoutingError::QuotaExceeded);
    }

    // 4. Build model pool
    let mut models: Vec<ScoredModel> = inference_profile.models.iter()
        .filter(|m| {
            // Check Lago entitlement
            let entitled = entitlements.has_model_access(&m.tier);
            // Check Flagsmith override
            let override_key = format!("{}_access_override", m.tier);
            let overridden = flags.is_feature_enabled(&override_key);
            entitled || overridden
        })
        .map(|m| ScoredModel::new(m))
        .collect();

    // 5. Get routing strategy
    let strategy = flags.get_feature_value("routing_strategy")
        .unwrap_or("cost_optimized".into());

    // 6. Score models
    let weights = match strategy.as_str() {
        "quality_first" => Weights { quality: 0.5, cost: 0.2, availability: 0.2, task_fit: 0.1 },
        "quality_experiment" => {
            let variant = flags.get_feature_value("quality_experiment_variant")?;
            match variant.as_str() {
                "treatment" => Weights { quality: 0.5, cost: 0.2, availability: 0.2, task_fit: 0.1 },
                _ => Weights { quality: 0.2, cost: 0.5, availability: 0.2, task_fit: 0.1 },
            }
        },
        _ => Weights { quality: 0.2, cost: 0.5, availability: 0.2, task_fit: 0.1 },
    };

    let task_complexity = analyze_task_complexity(task);

    for model in &mut models {
        model.score = weights.quality * model.quality_score
                    + weights.cost * model.cost_score
                    + weights.availability * check_availability(&model).await
                    + weights.task_fit * task_fit_score(&model, task_complexity);
    }

    // 7. Sort and select
    models.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

    for model in models {
        if model.is_available().await {
            return Ok(ModelSelection {
                model: model.model.clone(),
                strategy,
                score: model.score,
            });
        }
    }

    Err(RoutingError::NoModelsAvailable)
}
```

### Task Complexity Analysis

Complexity score informs task_fit scoring:

```rust
fn analyze_task_complexity(task: &Task) -> ComplexityScore {
    ComplexityScore {
        prompt_tokens: estimate_tokens(&task.prompt),
        expected_output: task.max_output_tokens,
        task_type: classify_task(&task.prompt),  // coding, creative, analysis, chat
        context_depth: count_context_items(&task.context),
    }
}

fn task_fit_score(model: &Model, complexity: &ComplexityScore) -> f32 {
    // Higher-tier models score better on complex tasks
    // Lower-tier models score better on simple tasks
    match (model.tier, complexity.is_complex()) {
        (Tier::Opus, true) => 1.0,
        (Tier::Opus, false) => 0.6,  // Overkill for simple tasks
        (Tier::Sonnet, true) => 0.8,
        (Tier::Sonnet, false) => 0.9,
        (Tier::Haiku, true) => 0.4,  // May struggle with complex tasks
        (Tier::Haiku, false) => 1.0,
    }
}
```

---

## Fallback Behavior

<!-- ANCHOR: ops-fallback-behavior -->
### Model Fallback

When primary model fails (timeout, rate limit, error):
1. Log failure with details
2. If fallback model configured: retry with fallback
3. If fallback also fails: return error to dispatch
4. Create resource claim for failed attempts (we may have consumed tokens)

### Backend Fallback

If Anthropic API unavailable:
1. Check if local Mistral enabled
2. If tier allows local models: route to Mistral
3. Log backend switch for analytics
4. Note: local models may have different capabilities

### Graceful Degradation

During outages, consider:
- Queue requests for later processing
- Return cached responses if applicable
- Inform user of delayed processing
- Don't charge for failed requests (void claims)
<!-- /ANCHOR -->

---

## Audit Queries

### Model Usage by Provider/Tier

```sql
-- Usage breakdown by model tier
SELECT
    p.name as provider,
    mr.tier as model_tier,
    mr.name as model_name,
    COUNT(*) as request_count,
    SUM(lu.our_token_count) as total_tokens
FROM rev.dispatch_task_attempt_event_llm_usages lu
JOIN rev.model_releases mr ON mr.id = lu.model_release_id
JOIN rev.providers p ON p.id = lu.provider_id
WHERE lu.created_at > now() - interval '30 days'
GROUP BY p.name, mr.tier, mr.name
ORDER BY total_tokens DESC;
```

### Model Selection vs InferenceProfile

```sql
-- Verify models used were in the Journey's InferenceProfile
-- (detect if routing selected a model outside the allowed set)
SELECT
    lu.object_id as usage_id,
    j.object_id as journey_id,
    mr.name as model_used,
    lu.created_at
FROM rev.dispatch_task_attempt_event_llm_usages lu
JOIN rev.dispatch_task_attempt_events e ON e.id = lu.dispatch_task_attempt_event_id
JOIN web.journeys j ON j.id = e.journey_id
JOIN rev.model_releases mr ON mr.id = lu.model_release_id
WHERE NOT EXISTS (
    SELECT 1 FROM web.journey_inference_profiles jip
    JOIN web.inference_profile_configs ipc ON ipc.inference_profile_id = jip.inference_profile_id
    WHERE jip.journey_id = j.id
      AND ipc.model_release_id = lu.model_release_id
)
AND lu.created_at > now() - interval '7 days';
```

### Lago API Failures

```sql
-- Track pre-flight check failures (if we log them)
-- This would require a log table for Lago API calls
-- For now, monitor via application logs/metrics
```

---

---

## Flagsmith Feature Flag Reference

<!-- ANCHOR: conduit-flagsmith-features -->
Feature flags that conduit reads from Flagsmith. These must be configured in Flagsmith for routing to work correctly.

### Routing Strategy

| Feature | Type | Values | Default | Description |
|---------|------|--------|---------|-------------|
| `routing_strategy` | Multivariate | `cost_optimized`, `quality_first`, `quality_experiment` | `cost_optimized` | How to score and select models |

**Segment overrides:**
- Pro Plan (`plan_tier = "pro"`) → `quality_first`
- Enterprise (`plan_tier = "enterprise"`) → `quality_first`

### Model Access Overrides

| Feature | Type | Default | Description |
|---------|------|---------|-------------|
| `opus_access_override` | Boolean | `false` | Grant opus access beyond Lago entitlement |
| `sonnet_access_override` | Boolean | `false` | Grant sonnet access beyond Lago entitlement |

**Segment overrides:**
- Beta Testers (`beta_tester = true`) → `true`

### Quota & Limits

| Feature | Type | Default | Description |
|---------|------|---------|-------------|
| `allow_overage` | Boolean | `false` | Allow usage beyond quota (bill overage) |
| `max_context_tokens` | Number | `8192` | Maximum context window size |
| `max_output_tokens` | Number | `4096` | Maximum output tokens per request |

**Segment overrides:**
- Enterprise → `allow_overage = true`
- Pro Plan → `max_context_tokens = 32000`
- Enterprise → `max_context_tokens = 200000`

### Experiments

| Feature | Type | Values | Description |
|---------|------|--------|-------------|
| `quality_experiment_variant` | Multivariate | `control`, `treatment` | A/B test bucket for quality experiment |

**Applies to:** Users in quality experiment segment (50/50 split)

### Required Traits

Conduit passes these traits when calling Flagsmith:

| Trait | Source | Example |
|-------|--------|---------|
| `plan_tier` | Lago entitlements | `"free"`, `"pro"`, `"enterprise"` |
| `beta_tester` | User metadata | `true`, `false` |

### Conduit Flag Resolution

```rust
// Flags conduit expects from Flagsmith
pub struct ConduitFlags {
    // Routing
    pub routing_strategy: RoutingStrategy,  // cost_optimized | quality_first | quality_experiment

    // Model access overrides (beyond Lago entitlements)
    pub opus_access_override: bool,
    pub sonnet_access_override: bool,

    // Quota behavior
    pub allow_overage: bool,
    pub max_context_tokens: u32,
    pub max_output_tokens: u32,

    // Experiment assignment (if in experiment)
    pub quality_experiment_variant: Option<String>,
}

impl ConduitFlags {
    pub fn from_flagsmith(flags: &Flags) -> Self {
        Self {
            routing_strategy: flags
                .get_feature_value("routing_strategy")
                .unwrap_or("cost_optimized".into())
                .parse()
                .unwrap_or(RoutingStrategy::CostOptimized),

            opus_access_override: flags.is_feature_enabled("opus_access_override"),
            sonnet_access_override: flags.is_feature_enabled("sonnet_access_override"),

            allow_overage: flags.is_feature_enabled("allow_overage"),
            max_context_tokens: flags
                .get_feature_value_as::<u32>("max_context_tokens")
                .unwrap_or(8192),
            max_output_tokens: flags
                .get_feature_value_as::<u32>("max_output_tokens")
                .unwrap_or(4096),

            quality_experiment_variant: flags
                .get_feature_value("quality_experiment_variant")
                .ok(),
        }
    }
}
```
<!-- /ANCHOR -->

---

## Related Sections

- [Usage Tracking](../domain/usage-tracking.md) — Resource consumption and claims
- [Lago Integration](../integrations/recurring-billing.md) — Access control via API
- [A/B Testing](ab-testing.md) — Experiment bucketing and outcomes
- [Flagsmith Docs](https://docs.flagsmith.com) — Feature flags and remote config
