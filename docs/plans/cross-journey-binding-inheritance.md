# Cross-journey binding inheritance (Shape B)

> Backend feature plan. Audience: the dapple-prompt backend team
> (`crates/dapple-prompt-api`, `crates/dapple-data`,
> `crates/dapple-prompt-external-api`). Cross-team prompted by a UX
> issue on dapple-ui-demo.

## Context — why this is needed

A presentation-authoring workflow on dapple-ui-demo is structured as
multiple sequential "chapters", and each chapter is a separate
journey (separate playbook, separate `rev.journeys` row). The
playbooks for chapters 1, 2, 3 share the same brief pack
(`draft-presentation-slides-pack`), which surfaces three input slot
names (presentation context, content sections, delivery concerns).

Today's behaviour: when the user finishes chapter 1 and the system
starts chapter 2, chapter 2's journey is brand-new with zero
bindings, so its rendered brief-form shows all three forms empty
again. The user — who just answered those exact questions in
chapter 1 — sees "the chapter restarted." The visible chapter
heading correctly says "Build Your Slide Structure," but the form
prompts are the same three they already filled in.

There is no dapple-ui code bug. The reply route, dedupe-by-name,
binding fanout, auto-spawn, and `is_journey_fully_bound` are all
working. The frontend has no signal that "the user already answered
these questions in a prior journey," because nothing on the wire
today says it.

### Why Shape B (not Shape A or C)

Three shapes were considered (full discussion lives in the prior
plan file). Shape B was picked because:

- **Shape A (chapter-specific brief packs)** — content rewrite only,
  but loses the architectural property that each chapter's task can
  read every prior context input it needs.
- **Shape C (collapse into one journey)** — biggest content
  restructure, mixes domain modelling with cross-step state in a way
  the journey model doesn't naturally express today.
- **Shape B (cross-journey binding inheritance)** — keeps the
  "chapter = journey" model, keeps brief packs shareable across
  playbooks, and lets the system carry forward what the user has
  already told us. It's a backend behaviour change scoped to journey
  / dispatch creation. The UI need not change.

## The feature

When a journey is created (or its first dispatch is created), the
backend looks up the most recent binding from the same user for each
of this journey's required slot names. If a match exists, it seeds
new `dispatch_intent_bindings` rows on the new journey, carrying the
inherited value forward. The user sees the form pre-filled and, if
all required names are inherited, the dispatch auto-spawns
immediately (the existing `is_journey_fully_bound` path fires).

Concrete user-visible effect on dapple-ui-demo:

- Chapter 1 → user fills 3 forms → bindings recorded against
  chapter 1's journey.
- Chapter 2 → journey created → server seeds 3 inherited bindings
  from chapter 1's journey → brief-form returns with all 3 forms
  pre-filled (defaults read from bindings via the existing
  `extractDefaultValue` path) → `is_journey_fully_bound = true`
  → auto-spawn fires → chapter 2 advances without re-prompting.
- Chapter 3 → same.

If the user wants to change an answer for a particular chapter, they
edit the pre-filled form and submit; the existing brief-binding
flow overwrites the inherited binding with the new value.

## Scoping rules (decide and lock these)

The "find a matching prior binding" lookup needs explicit rules.
Recommended defaults; please ratify or amend:

1. **Same user.** Match on `rev.journeys.user_id` (preferred for
   internal surface) and / or `principal_id` /
   `owner_company_id` (external surface). NEVER inherit across users
   or companies — that's a privacy / namespace leak.
2. **Same value_kind.** Don't copy a `text` binding into an `int`
   slot. Slot names *should* always map to a single value_kind by
   convention, but enforce it anyway.
3. **Most recent wins.** Order by `dib.bound_at DESC` (or
   `dib.created_at DESC`), take the first row per name.
4. **No time bound for v1.** Treat the user's most recent answer to
   "what's your presentation context?" as canonical regardless of
   how long ago it was. If this turns out to be wrong, add a
   `WHERE bound_at > now() - interval '30 days'` knob later.
5. **Same namespace** (external surface only). For
   `dapple-prompt-external-api`, also constrain by
   `owner_company_id` so customer A's bindings can never leak into
   customer B's journey. The internal surface (single-tenant) can
   skip this.
6. **Opt-in vs default-on.** Recommended **default ON**, with a
   `inherit_prior_bindings: bool` field on
   `CreateJourneySchema` defaulting to `true`. Allows future callers
   (tests, special flows) to opt out, but the dapple-ui-demo case
   needs zero client-side change to benefit. Optional and discardable
   — feel free to ship without the opt-out if it's clutter.

## Where to hook this

Two reasonable hook points, both viable. The team picks based on
how much wants to live in the journey-creation handler vs the
dispatch-creation handler.

### Option I — at first dispatch creation (recommended)

`crates/dapple-prompt-api/src/handler/dispatches.rs::dispatches_post_handler`
(and the external mirror
`crates/dapple-prompt-external-api/src/handler/dispatches.rs`).

After `insert_dispatches(...)` succeeds and BEFORE the
`is_journey_fully_bound` check (currently around line 160-167 of the
internal file), for each just-created dispatch:

1. Check: is this the journey's first dispatch? (`SELECT COUNT(*)
   FROM rev.dispatches WHERE journey_id = ?` returns 1 — i.e. just
   this one)
2. If yes, run the inheritance lookup (see "Query needed" below).
3. Insert `dispatch_intent_bindings` rows + the matching typed-item
   rows onto this new dispatch for every inheritable
   `(value_slot_id, value)` pair.
4. Let the existing `is_journey_fully_bound` check fire — it will
   now return `true` for chapter 2 because all required names are
   bound.

**Why this hook is preferred:** bindings need a `dispatch_id` to
attach to (`dispatch_intent_bindings.dispatch_id NOT NULL`). The
first dispatch is the right target. The auto-spawn path is already
right after this point.

### Option II — at journey creation

`crates/dapple-prompt-api/src/handler/journeys.rs::journeys_post_handler`,
after Step 6 (inference profile association).

To insert bindings here, you'd need to ALSO create the seed
dispatch in this handler (so `dispatch_intent_bindings` has a
`dispatch_id` to reference). That's a bigger change because today
journey creation and dispatch creation are separate endpoints called
in sequence by the UI. Pulling the "first dispatch" creation up to
journey creation is a meaningful refactor.

Workable, but more surface area. Recommend Option I unless there's
a reason to merge journey + first-dispatch creation generally.

## Query needed (for either hook)

`crates/dapple-data/assets/sqlc/journeys/select_inheritable_bindings_for_user.sqlc`:

```sql
-- Given a journey_id and a user_id, find for every slot.name required
-- by the journey's brief packs the user's most recent binding from
-- ANY of their other journeys, scoped to value_kind and namespace.
-- Returns one row per (slot_name, target_slot_id_in_this_journey),
-- so the caller can fan out one binding insert per matching target.

WITH required_slots AS (
    -- Every (slot_id, slot_name, value_kind_key) the new journey needs.
    SELECT
        vs.id AS target_slot_id,
        vspd.name AS slot_name,
        vk.key AS value_kind_key
    FROM rev.journey_brief_packs jbp
    JOIN rev.brief_packs bp           ON bp.id = jbp.brief_pack_id
    JOIN rev.brief_pack_groups bpg    ON bpg.brief_pack_id = bp.id
    JOIN rev.brief_groups bg          ON bg.id = bpg.brief_group_id
    JOIN rev.brief_group_briefs bgb   ON bgb.brief_group_id = bg.id
    JOIN rev.briefs b                 ON b.id = bgb.brief_id
    JOIN rev.brief_units bu           ON bu.brief_id = b.id
    JOIN rev.brief_unit_input bui     ON bui.brief_unit_id = bu.id
    JOIN rev.value_slots vs           ON bui.control_spec ? 'value_slot_id'
                                    AND vs.id = (bui.control_spec ->> 'value_slot_id')::UUID
    JOIN rev.versioned_objects vo     ON vo.id = vs.versioned_object_id
    JOIN rev.value_slot_path_details vspd ON vspd.path_id = vo.path_id
    JOIN rev.value_kinds vk           ON vk.id = vspd.value_kind_id
    WHERE jbp.journey_id = :bind(target_journey_id)::UUID
),
candidate_bindings AS (
    -- Bindings on OTHER journeys belonging to this user, joined to the
    -- prior slot's name + value_kind so we can match by name to required.
    SELECT
        vspd.name AS slot_name,
        vk.key AS value_kind_key,
        dib.id AS source_binding_id,
        dib.bound_at,
        ROW_NUMBER() OVER (
            PARTITION BY vspd.name
            ORDER BY dib.bound_at DESC
        ) AS recency_rank
    FROM rev.dispatch_intent_bindings dib
    JOIN rev.dispatches d ON d.id = dib.dispatch_id
    JOIN rev.journeys j ON j.id = d.journey_id
    JOIN rev.value_slots vs ON vs.id = dib.value_slot_id
    JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
    JOIN rev.value_slot_path_details vspd ON vspd.path_id = vo.path_id
    JOIN rev.value_kinds vk ON vk.id = vspd.value_kind_id
    WHERE j.user_id = :bind(user_id)::BIGINT
      AND j.owner_company_id = :bind(owner_company_id)::UUID
      AND j.id <> :bind(target_journey_id)::UUID
      AND vspd.name IN (SELECT slot_name FROM required_slots)
),
chosen AS (
    SELECT slot_name, source_binding_id
    FROM candidate_bindings
    WHERE recency_rank = 1
)
SELECT
    rs.target_slot_id,
    rs.slot_name,
    rs.value_kind_key,
    c.source_binding_id
FROM required_slots rs
JOIN chosen c ON c.slot_name = rs.slot_name;
```

The caller then reads the typed-item rows from the source binding
and inserts new typed-item rows for each `target_slot_id`. (A
cleaner one-shot would be `INSERT … SELECT` that creates the new
binding rows + typed-item rows in a single statement; the
single-query form is preferred because it keeps the inheritance
atomic.)

For implementation, look at how
`crates/dapple-data/src/query/dispatch_intent_bindings.rs::insert_dispatch_intent_bindings`
handles the typed-item fan-out today — the inheritance insert is a
similar shape, just sourcing values from a prior binding instead of
from `CreateDispatchIntentBindingSchema`.

## Schema considerations

No DDL change needed for v1. The existing
`rev.dispatch_intent_bindings` + typed-item tables already represent
everything we need. An inherited binding is just another row with the
same `value_slot_id` (or a different `value_slot_id` carrying the
same `slot_name`) and the inherited value.

Future improvement (optional, not required for v1): add a
`source_binding_id UUID NULL REFERENCES rev.dispatch_intent_bindings(id)`
column to track the lineage of inherited bindings. Useful for
audit, debugging "where did this value come from", and for an
eventual "this is inherited, edit to override" UI affordance. Skip
for v1 unless it's trivial.

## Tests

In `crates/dapple-prompt-api/tests/user_lifecycle.rs` (or a new
focused test file):

1. **Happy path: bindings carry forward.** Create journey A on
   playbook 1 with brief pack X (three slot names). User submits
   all three brief forms → 3 bindings (each fans out to ~3 slot ids
   via the existing name-fanout). Create journey B on playbook 2
   with the same brief pack X. Assert via DB query: journey B's
   first dispatch has bindings for all required slot ids, with the
   same values as journey A's bindings. Assert the journey B
   dispatch's task auto-spawned (attempt count ≥ 1).
2. **Cross-user isolation.** User U1 has bindings on journey A. User
   U2 creates journey B with the same brief pack. Assert journey B
   has zero inherited bindings.
3. **Cross-namespace isolation (external).** Customer C1 has
   bindings on a journey. Customer C2 creates a journey with the
   same brief pack content. Assert no inheritance (via
   `owner_company_id` filter).
4. **Value-kind mismatch.** User has a `text` binding for slot.name
   "foo". A new journey requires a slot named "foo" with value_kind
   `int` (unlikely in practice, but defensive). Assert no
   inheritance — value_kind mismatch.
5. **Most recent wins.** User has three bindings for the same
   slot.name across three different prior journeys. New journey
   requires that name. Assert the most recent (highest `bound_at`)
   value is the one inherited.
6. **No prior binding.** User has never bound this name. New journey
   has the name as required. Assert no inheritance; the form
   renders empty as today.

In `crates/dapple-prompt-external-api/tests/external_lifecycle.rs`:
mirror tests 1 and 3 against the external surface.

## Verification end-to-end

For the dapple-ui-demo motivating case:

1. Use a fresh dapple-ui-demo user.
2. Walk chapter 1 ("Define Your Core Message"): fill all 3 forms,
   click Continue, confirm chapter advances to a non-brief_form
   envelope.
3. Walk chapter 2 ("Build Your Slide Structure"): expect chapter 2's
   brief-form to arrive with all 3 forms pre-filled with the user's
   chapter 1 answers. Expect the dispatch to auto-spawn within seconds
   (no need for the user to click Continue at all, ideally — or the
   UI surfaces a "your prior answers carried over, click Continue to
   proceed" affordance, but that's a UX call separate from this
   backend change).
4. Walk chapter 3 ("Write Presenter Notes"): same.

## Files the backend team will most likely touch

| File | Change |
|---|---|
| `crates/dapple-data/assets/sqlc/journeys/select_inheritable_bindings_for_user.sqlc` | New — the inheritance lookup query |
| `crates/dapple-data/assets/sqlc/dispatch_intent_bindings/insert_inherited_bindings.sqlc` | New — the bulk INSERT of inherited bindings + typed-item rows |
| `crates/dapple-data/src/query/journeys.rs` or `dispatch_intent_bindings.rs` | New Rust query function wrapping the SQL |
| `crates/dapple-data/src/model/journeys.rs` (or related) | New `InheritedBindingRow` model if helpful |
| `crates/dapple-prompt-api/src/handler/dispatches.rs:151-160` | Wire the inheritance call between `insert_dispatches` and `is_journey_fully_bound` |
| `crates/dapple-prompt-external-api/src/handler/dispatches.rs` | Mirror the same hook, scoped by `owner_company_id` |
| `crates/dapple-prompt-api/src/schema/journeys.rs` | (optional) `inherit_prior_bindings: bool` on `CreateJourneySchema` |

## Open questions for the backend team

1. **Hook point** — Option I (first dispatch) or Option II (journey
   creation)? Recommend Option I; flag if you'd prefer the journey-creation
   hook for some architectural reason I'm not seeing.
2. **Lineage tracking** — bother with the `source_binding_id`
   column now (extra Migration), or defer until something needs it?
3. **External surface scope** — recommend constraining inheritance
   by `owner_company_id` only. Is there a customer scenario where
   inheritance should ALSO cross between two of their own users
   (shared workspaces)?
4. **Audit log** — do you want to emit an info-level log when
   inheritance fires (e.g. "Seeded N inherited bindings on
   dispatch D from prior journeys [A, B, C]")? Useful for
   debugging but easy to deluge with.
5. **Behaviour when user edits an inherited value** — current
   `briefBindingBuilder.ts` writes a new binding on every submit
   whose value differs from the existing one. Inherited bindings
   look exactly like fresh bindings, so this works correctly out
   of the box. Just confirming the expected mental model: the
   inherited binding is "the user's prior answer, carried forward,"
   and overwriting it is fine.

## Out of scope

- No dapple-ui code change required. The brief-form `default` path
  in `briefEnvelopeGenerator.ts::extractDefaultValue` already
  surfaces bindings as form defaults; the only new behaviour the UI
  needs to be aware of is "your forms may arrive pre-filled now"
  (which is a visual outcome, not a code change).
- No `is_journey_fully_bound` change. It already does the right
  thing — the seeded bindings make it return true for chapter 2 the
  moment its dispatch is created.
- No backend playbook content change. The shared
  `draft-presentation-slides-pack` stays as it is.
- No change to the existing per-submit binding fanout in
  `briefBindingBuilder.ts`. That code path remains the source of
  truth for new bindings; inheritance is a parallel seeding path,
  not a replacement.
