# What dapple-ui needs from a stage: a kind and a title

**Asking repo:** dapple-ui (BFF + demo UX)
**To:** dapple-prompt
**Date:** 2026-08-03
**Status:** requirements only — the design and the naming are yours
**Pattern we are copying:** `BriefOutputDisplay` + migration 55 (lookup table, `ALL_KEYS`, unknown → default)

---

## TL;DR

A stage reaches a client as a UUID and two booleans. Nothing on the wire says what a stage
*is*, so every client that draws a stage has to invent its identity locally. Ours invented it
against five-element arrays, which was right when guides had five stages and is wrong now that
`mark-the-bdc` has fifteen.

Two fields would fix it, both derivable from things guides already author:

1. **`kind`** — a closed, author-set value naming what kind of stage this is.
2. **`title`** — the authored, localized stage name (and its description if that's free).

Everything below is the argument for those two. We are not asking for an implementation, a
schema, or a migration shape — you own where these live and what the members are called. We do
have a stake in four properties, listed under [Ask 1](#ask-1--kind), and in one delivery detail
under [The deploy is part of the ask](#the-deploy-is-part-of-the-ask).

---

## What we have to work with today

The wire types carry no stage identity at all:

- `JourneyStageStatus` (`crates/dapple-types/src/model/journeys.rs:28-38`) — `playbook_stage_id`,
  `done`, `blocked`.
- `JourneyStepStatus` (`crates/dapple-types/src/model/journeys.rs:236-247`) — the same
  `playbook_stage_id` UUID, plus per-step `done` and `exposed_slots`.
- `PlaybookStage` (`crates/dapple-types/src/model/playbooks.rs:33-40`) — `id`, `playbook_id`,
  `steps`, `created_at`. No name, no description, no kind.

`Stage` (`crates/dapple-types/src/model/stages.rs:8-17`) *does* carry `name`/`slug`/`description`,
but that is the object-CRUD DTO for `GET /api/stages/{id}`; a client walking a journey never
holds a stage id to look up, and doing so would be one request per stage.

So on our side, `JourneyStageRef` (`dapple-ui/packages/ui/lib/journeyStages.ts:81-94`) is
`{ ordinal, playbookStageId, done, blocked, dispatchId, steps, activeStepOrdinal }` — position and
progress, no identity. Which produced exactly the two local inventions you would predict:

| What a stage needs | Where we got it | How it fails at 15 stages |
|---|---|---|
| An icon | A 5-element array of inline SVGs indexed by stage number (`StageProgressBar.tsx`) | `STAGE_ICONS[5]` is `undefined`. Stages 6-15 rendered a **blank white disc**. |
| A title | 5 hard-coded i18n strings, `DialogueProgress.stages.0..4` (`packages/ui/messages/en.json:257-295`, same keys in es/fr/pt-br/pt-pt) | Stage 6 onward reads literally **"Stage 6"**, "Stage 7"… |
| An accent colour | A 5-element palette indexed by stage number | Everything past the fifth circle collapsed to one fallback colour. |

The colour is genuinely ours to solve and we have: the palette now cycles, and the UX team owns
its length. The icon we have made *data-driven* pending this ask (see
[What we do when this lands](#what-we-do-when-this-lands)). The title we cannot solve at all —
there is no client-side answer to "what is this stage called", only to "what number is it".

Worth saying plainly, because it is the strongest argument here: **the stage titles already
exist, authored, localized, and hash-pinned.** A guide author writes

```
:::stage { name: "Pull the Record" }
```

`name` is a Fluent message key whose body is the stage description
(`docs/books/journey-planner-guide/directive-blocks.md:151-160`), it is stored on
`rev.stage_path_details.name` and `rev.stages.description_fluent_message_id`, and then it stops.
Five English strings in our repo stand in for it. The guide says "Pull the Record" and our UI says
"Let's Get Specific".

---

## Ask 1 — `kind`

A closed, snake_case, author-set value naming what kind of stage a stage is. One per stage.

Below is a vocabulary drawn from stages that actually exist across `mark-the-bdc` (15 stages),
`mind-the-13f` (14) and `retort-the-n-port` (18). **The member names are yours to change** — we
offer them so the ask is concrete, not because we need these words.

| proposed | means | real authored stages it covers |
|---|---|---|
| `intake` | The operator supplies the values the work starts from | "Pull the Record" (name a BDC by ticker or CIK) |
| `fetch` | Retrieves source data; runs by itself | "Pull the Portfolio", "Resolve the Portfolio", "Pull the Current Period" |
| `analyze` | Derives a reading from data already held; runs by itself | "The Marks", "What the Income Is Made Of", "The Leverage", "Signals in the Holdings" |
| `decide` | The operator chooses among generated candidates | "Review the Promise", "Name the Character of the Promise" |
| `refine` | The operator sharpens something already chosen | the refine beat of a three-step stage |
| `review` | The operator reads and confirms; produces no new data | "Review the Story", "Review What They Hold" |
| `compare` | Weighs two already-produced things against each other | "Weigh the Promise Against the Portfolio", "Layer the Changes", "What Changed" |
| `assemble` | Builds the deliverable from everything upstream | "Build the Story" |
| `deliver` | The finished artifact; the terminal pane | the last stage of every guide |

Four properties we do depend on, whatever the members end up being called:

1. **Closed vocabulary.** Same reasoning as `BriefOutputDisplay`: a closed set means our schema
   codegen emits a union and our icon registry is a total map, so a member you add is a compile
   error on our side until it has a glyph — rather than a blank disc in production, which is
   precisely the bug being fixed.
2. **snake_case on the wire**, matching `display.kind`. We match exactly and do not normalize.
3. **Exactly one per stage.**
4. **Semantic, not pictorial.** See below.

### Stage granularity, not step granularity

This is the one place we would push back if the design drifted. The canonical
generate → pick → refine stage is **three steps but one pane**
(`docs/books/_foundation/concepts/core-execution-model.md:45`), so it needs one kind, describing
the pane. A per-*step* kind would not answer the question we are asking; we already have a
per-step notion and it is not this.

We also have direct experience of why the step-level signal cannot substitute. Because `/steps`
carries no `kind`, we infer one per step from exposed-slot structure (`deriveStepKind`,
`packages/ui/lib/journeyStages.ts:157-163`, documented as advisory). It keys "pick" off the
journey's *current* pick target and "refine" off whether a slot is *currently* bound — so both
values move as the operator works. We tried deriving a stage's identity from them and a single
stage's glyph changed three times while the operator sat on it. We now derive from the authored,
non-moving half of that data instead. **You know what the author meant; we are inferring it from
the shape of their overlays.**

### Why we are *not* asking for an icon name on the wire

We started out wanting one and talked ourselves out of it. `api.dapple.x` serves dapple-suite,
the external SDK, and CLI consumers — none of which draw lucide glyphs, and some of which draw
nothing at all. An icon name on the wire would encode one client's component library into a
contract shared by all of them, and would go stale the first time any of us restyled.

Semantics on the wire; pictures in the client. For reference, this is what our surface currently
does with each kind — **informative only, not part of the ask, and not something to validate
against**:

| kind | our glyph |
|---|---|
| `intake` | the existing hand-drawn target |
| `fetch` | lucide `ArrowDownToLine` |
| `analyze` | lucide `ChartLine` |
| `decide` | the existing hand-drawn finger-press |
| `refine` | the existing hand-drawn pencil |
| `review` | lucide `Eye` |
| `compare` | lucide `Scale` |
| `assemble` | the existing hand-drawn wrench |
| `deliver` | the existing hand-drawn celebrate |
| *(unknown / absent)* | lucide `CircleDot` |

If you *do* want authors to influence presentation directly, the shape we would honor is a
separate optional open-vocabulary string (an `icon` a client MAY map and MUST tolerate not
recognizing) — deliberately not the same field as `kind`, and explicitly **not needed for us to
ship**. Please do not block `kind` on it.

---

## Ask 2 — `title`

The authored, localized stage name. The stage's description too, if it comes for free on the same
read — we would use it in the expanded stage cards, which currently show generated filler.

This is the smaller ask and the one with no client-side alternative. It also has a nice property:
it deletes code rather than adding it. Our `DialogueProgress.stages.0..4` block disappears from
five locale files, and the "Stage 6" fallback disappears with it.

One naming caution: "title" is our word for it. The authored key is `name`, and
`rev.stage_path_details.name` is where it lives, so if `name` is the natural field, use `name`.

---

## Where it needs to arrive

**We read `GET /api/journeys/{id}/steps`. We do not call `/journeys/{id}/stages` at all** — the
step index is a strict superset of it (per-step `done` plus `exposed_slots`), so it superseded the
stage index for us some time ago. Both new fields need to reach us without a second round trip.

Our preference, in order:

1. **On each `/steps` row**, repeated for every step of the stage. Cheapest for us by a wide
   margin: we already fold rows by `playbook_stage_id`
   (`deriveStageCompletionsFromSteps`, `packages/ui/lib/stepsFetcher.ts:363`), so a value repeated
   across a stage's rows is picked up first-seen-wins with no new code. Redundant on the wire, and
   we think that is the right trade.
   The query looks one join away: `select_step_binding_status_for_journey.sql:82-89` already joins
   `rev.playbook_stages`, and `rev.stages` / `rev.stage_path_details` are one more hop.
2. **On `/journeys/{id}/stages`**, if you would rather keep the step index lean. Workable — we
   add the call and join it to the step fold ourselves. It costs us a second request per poll,
   which is why it is second.

Where the `kind` *lives* is entirely yours. The one question worth deciding deliberately:
`rev.playbook_stages` scopes it per playbook (right if two playbooks can reuse one stage with
different intent), `rev.stages` makes it intrinsic to the stage version (right if the kind is a
property of the stage itself, and it is where `description_fluent_message_id` already lives).

---

## Compatibility rules we are relying on

- **Additive, `#[serde(default)]`.** Absent means the client applies its own documented default,
  so already-ingested guides keep rendering. Ours is the neutral kind plus the ordinal label
  ("Stage 6") — i.e. exactly today's behaviour, which is the honest baseline.
- **An unknown value is a client-side degrade, never an error.** We coerce anything we do not
  recognize to the neutral kind and render it; we will not throw, warn at the operator, or block
  a journey over a vocabulary we predate. Same stance `BriefOutputDisplay::from_key` takes with
  unknown → `Text`.
- **Backfill is your call, and we would like to know it.** Nineteen guides are already ingested
  with no kind. If they need a re-ingest to pick one up, we would rather plan around that than
  discover it (precedent: `docs/plans/ingest-backfill-systemic-sweep.md`). Absent → default holds
  fine in the meantime.

## The deploy is part of the ask

dapple-ui regenerates its types from the **live deployment**, not from source:
`packages/ui/scripts/fetch-internal-openapi-spec.ts` fetches
`https://dapple-prompt-api.kube.dappleai.dev/api.json`. There is no local spec-dump step wired up
on our side. So "landed in `main`" is not "usable by us" — **please tell us which deploy carries
the field.**

Two related facts, offered as evidence rather than complaint:

- Our checked-in spec and generated types were last regenerated in `8b93923` (2026-06-28).
  `grep -c blocked packages/ui/types/internal-api/types.gen.ts` returns `0`, so the generated
  `ExposedSlot` is missing fields your Rust type already emits.
- Which is why `JourneyStepStatus` is hand-typed on our side with a standing TODO
  (`packages/ui/lib/stepsFetcher.ts:165-167`: *"the internal-api OpenAPI generator does not yet
  emit a DTO for this index"*). We will hand-type `kind`/`title` there in the interim exactly as
  we did for `display`, and switch to the generated type when the spec catches up.
- One live drift datapoint for the lockstep: our mirrored `BriefOutputDisplayKind` union carries a
  `shotlist` member (`packages/ui/lib/stepsFetcher.ts:56`) that appears nowhere in
  `crates/dapple-types/src/model/briefs.rs`. Whichever side is wrong, a seed/enum drift
  test on the new vocabulary — you already have the pattern at
  `crates/dapple-prompt-api/tests/migration_seed_test.rs:40-56` — would keep this one honest from
  the start.

---

## The precedent we are copying

We are not asking for a new mechanism. `BriefOutputDisplay` already solved this exact problem one
level down, for brief outputs, and we would be happy with the same shape applied to stages:

- The enum with a default member and an unknown-tolerant constructor —
  `crates/dapple-types/src/model/briefs.rs:8-92` (`ALL_KEYS`, `is_valid_key`, `from_key` with
  unknown → `Text`).
- The lookup table and FK — migration 55, `rev.brief_output_display_hints`, whose header states
  the motivation in terms that transfer verbatim: *"until now it carried no machine-readable
  instruction for HOW to render its value, so UI clients guessed per-slot."*
- The extensibility story: new members arrive later as one-line seed migrations (69, 72, 76, 84),
  not schema changes.
- The drift guard — `crates/dapple-prompt-api/tests/migration_seed_test.rs:40-56`.

And the three documentation surfaces where a new stage field would need to land, so consumers
other than us find it:

- the `:::stage` attribute table, `docs/books/journey-planner-guide/directive-blocks.md:151-160`
  (four keys today: `name`, `tags`, `version`, `expose`/`mask`);
- the `/steps` row description, `docs/books/api-consumer-guide/endpoint-reference.md:113`;
- a `CHANGES.md` entry in the additive style of 0.3.0
  (*"Additive — existing clients that ignore the new fields keep working"*).

### One naming collision to avoid

**Do not call this a "display hint".** That phrase is taken: `rev.brief_output_display_hints` is
the closed brief-output render vocabulary, and a reader will assume you mean that enum. `stage
kind` and a stage-level `icon` are both unclaimed — a grep of `docs/` for stage kind / role / type
/ icon returns nothing. Also avoid `role`: it is doubly loaded already, by investor-guide role
casting (`docs/plans/guide-retort-the-n-port-role-casting.md:33`) and by XBRL `xlink:role`.

---

## What we are not asking for

- **Stage ordering.** Authored order already holds through `/steps`, and we derive ordinals from
  it. Nothing needed.
- **A per-stage colour.** The palette is a client concern, it now cycles, and our UX team owns it.
- **The per-step kind we derive locally, promoted to a wire field.** It is advisory, it is ours,
  and per-stage `kind` is what we actually need.
- **Anything costing a round trip per stage.** A per-stage lookup endpoint would not be usable by
  us; see [Where it needs to arrive](#where-it-needs-to-arrive).
- **An icon name**, on the terms described above.

## What we do when this lands

The consumption seam is already built and shipped, so this is a small, contained change on our
side — deliberately, so that the ask is cheap to say yes to:

1. Thread `kind` onto `JourneyStageRef` and swap our temporary `deriveStageKind` for
   `toStageKind(row.kind)`. Everything downstream already reads a `StageKind`, so the vocabulary,
   the coercion and the icon registry (`packages/ui/lib/stageKinds.ts`,
   `components/DialogueRenderer/StageKindIcon.tsx`) stay exactly as they are.
2. Delete `deriveStageKind` and its tests. The derivation exists only to bridge to this field, and
   its module says so.
3. Delete the `DialogueProgress.stages.0..4` block from all five locale files and render the
   authored title in the rail, the title cube, and the fork submenu.
4. Rebuild the expanded stage drawer, which we are currently leaving alone: at 15 stages its cards
   are ~60px wide and there is little point making them readable while the only thing they can say
   is "Stage 11".

---

## Reply from dapple-prompt (2026-08-03)

Both asks are implemented. Everything below is what you need to consume it.

### The vocabulary (8 members — `refine` dropped)

```
intake, fetch, analyze, decide, review, compare, assemble, deliver
```

We took your nine minus `refine`: by your own stage-granularity argument it names a step beat
(the refine half of generate→pick→refine), not a pane — your only example for it was "the refine
beat of a three-step stage". If a genuine refine-pane ever shows up, adding the member back is a
one-line seed migration plus an enum variant; removing one later would not have been. Your four
structural properties all hold: closed set (`rev.stage_kinds` lookup + `StageKind` enum in
`dapple-types`, so the OpenAPI schema emits a closed union), snake_case on the wire, exactly one
per stage, semantic-not-pictorial. The seed↔enum drift test you suggested exists
(`stage_kinds_seed_matches_enum` in `crates/dapple-prompt-api/tests/migration_seed_test.rs`).

### Where it arrives (your preference 1, plus parity)

- **`GET /api/journeys/{id}/steps`** — every row carries `stage_name`, `stage_description`, and
  optional `stage_kind`, repeated identically across a stage's rows (first-seen-wins folding
  works as you planned).
- **`GET /api/journeys/{id}/stages`** — same fields as `name`, `description`, `kind` (parity;
  you don't call it, others do).

Naming: we used `name` as you suspected we would (`stage_name` on step rows only to
disambiguate from step-level semantics). The description came for free on the same read —
`COALESCE(fluent_body, fallback text, name)`, so it is always populated; render it in the
expanded stage cards as planned.

`kind` is authored as `:::stage { name: "...", kind: "fetch" }`, stored on the stage version
itself (`rev.stages.kind_id`, intrinsic — not per-playbook), and folds into the stage content
hash.

### Compatibility rules — confirmed as you assumed

- Additive. `kind`/`stage_kind` is `#[serde(default, skip_serializing_if)]` — **absent from the
  JSON entirely** when unauthored; `name`/`description` are plain strings. Your
  absent-⇒-ordinal-label baseline is exactly right.
- Unknown values: an unrecognized authored kind produces an ingest **warning** and is stored as
  absent — it can never reach you as an unknown string from our own vocabulary, but coerce
  defensively anyway as you described.
- **Backfill: none, by design.** The kind is authored, so the nineteen ingested guides serve it
  absent until each guide adds `kind:` to its `:::stage` blocks and re-ingests (the hash fold
  makes that a normal re-version — kindless guides re-ingest with no version churn). One
  consequence to plan around: journeys are version-pinned, so kinds appear on journeys created
  **after** a guide's re-ingest, not retroactively on running ones.

### The deploy

Not deployed yet. Kenny drives the deploy and will tell you which dapple-prompt-api deploy
carries the fields — regenerate your OpenAPI types from `/api.json` after that, not after merge.
<!-- DEPLOY PLACEHOLDER: fill in the deploy identifier when it ships -->

### One bug on your side

Your mirrored `BriefOutputDisplayKind` union's `shotlist` member
(`packages/ui/lib/stepsFetcher.ts:56`) is not real: it appears in no Rust vocabulary, no seed
migration, and no `ALL_KEYS`. It is the wimmelbook guide's *slot name* (`wimmelbook_shotlist`)
that got absorbed into the hand-mirrored union. Our server would warn at ingest and downgrade
any `display: "shotlist"` to `text`. Drop the member when you touch that file.
