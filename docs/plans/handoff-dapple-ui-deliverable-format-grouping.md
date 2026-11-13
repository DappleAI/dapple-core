# Handoff — dapple-ui: deliverable format grouping is now authored, not inferred

**To:** dapple-ui
**From:** dapple-prompt (backend)
**Status:** backend landed, unreleased — see [Rollout](#rollout-order-read-this-first)
**Backend change:** migration 95 + `ExposedSlotDisplay` gains `group` / `format`

---

## TL;DR

Two things changed on `GET /api/journeys/{id}/steps` (both surfaces — internal and
`external-api` serve the same shape):

1. **`display.group` and `display.format` are now authored fields on the wire.**
   You no longer have to infer sibling formats from slot names.
2. **`exposed_slots` now arrive in the order the guide author wrote them.** They
   used to be sorted alphabetically by value-slot name.

Together these let you delete `packages/ui/lib/deliverableFormats.ts`'s inference
entirely — the format-token allowlist, the name splitter, *and* the inline
preference ordering. All three were guesses; all three now come from the author.

There is deliberately **no `primary` field**. Which format leads is the first
non-hidden member in authored order, which change (2) makes meaningful.

---

## Rollout order (read this first)

Neither change is retroactive, and they land independently. Please don't ship a
UI that *requires* either one:

| Change | When it appears | Until then |
|---|---|---|
| `display.group` / `display.format` | Once a **guide authors them** (dapple-guides work, not shipped yet) | Both fields absent. Ungrouped, exactly as an ungrouped slot renders today. |
| Authored `exposed_slots` order | Once a guide is **re-ingested** against migration 95 | Pre-migration briefs carry `ord = 0`, tie, and fall through to the old name ordering — byte-identical to today. |

A journey pinned to a pre-migration brief version keeps the old ordering **for
its whole life** (pinned reads never resolve to a newer version). So expect a
long tail of journeys where `exposed_slots` is still alphabetical. Your grouping
must degrade to something sane there, not break.

**Suggested sequence:** UI change first (it's a no-op while the fields are
absent) → guide authors `group`/`format` and re-ingests → both effects appear
together on new journeys.

---

## 1. Wire contract

`ExposedSlot.display` — present only on `producer: "computed"` slots; operator
slots still carry `input` and `display: null`. Unchanged apart from two additions:

```ts
export interface ExposedSlotDisplay {
	kind: BriefOutputDisplayKind;
	title?: string | null;
	reveal?: boolean;

	/**
	 * Sibling-format grouping. Slots sharing a `group` are the SAME deliverable
	 * in different formats. OPAQUE — any stable string the author picks; never
	 * parse it. Absent ⇒ never grouped.
	 */
	group?: string | null;

	/**
	 * Short label naming this slot's format within its `group` ("html", "pdf", …).
	 * A display label, NOT a mime type, and NOT a closed vocabulary — an author
	 * may coin "epub". Render whatever arrives; never match against a known set.
	 */
	format?: string | null;
}
```

Both are omitted from the JSON entirely when unset (`skip_serializing_if`), so
`display.group === undefined` is the ungrouped case.

### Example payload

The `build-the-story` step, once the guide authors it — note the array order:

```json
{
  "exposed_slots": [
    { "name": "tk_report",      "producer": "computed", "value_kind": "asset",
      "display": { "kind": "hidden",   "reveal": false, "group": "tk_report" } },

    { "name": "tk_report_html", "producer": "computed", "value_kind": "asset",
      "display": { "kind": "document", "reveal": false, "title": "Read the report",
                   "group": "tk_report", "format": "html" } },

    { "name": "tk_report_pdf",  "producer": "computed", "value_kind": "asset",
      "display": { "kind": "file",     "reveal": false, "title": "Download the report",
                   "group": "tk_report", "format": "pdf" } }
  ]
}
```

The resolved **value** is unchanged: still on the bindings read, joined by
`value_slot_id`. `display` remains static authored metadata only.

---

## 2. Ordering change

`exposed_slots` within a step are now ordered by the **authored position of the
brief unit that pins each slot** — inputs and outputs interleaved exactly as
written in the guide.

Previously the backend sorted by value-slot *name*. That `tk_report_html` came
before `tk_report_pdf` was the alphabet agreeing with the author by luck; a guide
naming its formats `report_epub` / `report_html` would have led with EPUB.

**This is now a dependency of your grouping code.** `members[0]` being the right
lead relies on the API's array order surviving to `groupDeliverableSlots`. It
currently does — nothing on the `stepsFetcher → dispatchEnvelopeGenerator` path
re-sorts. If you ever add a sort for display, take a copy; don't sort in place
upstream of the grouper.

Step order itself (`stage_ord`, `step_ord`) is unchanged.

---

## 3. What to delete in `packages/ui/lib/deliverableFormats.ts`

| Symbol | Action | Why |
|---|---|---|
| `FORMAT_TOKENS` | **Delete** | `display.format` is authoritative. The allowlist's real failure mode: an author coining `report_web` / `report_epub` fell out of the set and silently rendered as two unrelated blocks, with no error anywhere. |
| `parseSlotFormat()` | **Delete** (with its tests) | `display.group` is the key; `display.format` is the token. |
| `INLINE_PREFERENCE` + `sortGroupMembers()` | **Delete** | This was your policy (document-before-file, HTML-before-PDF), not the author's. It is right for every guide shipping today, but it is a guess. Authored order replaces it. |
| `groupable()` | **Simplify** | Keep the `kind !== "hidden"` guard (see below). **Drop the `value_kind === "asset"` requirement** — with an explicit `group` the author has declared the relationship, and the asset guard would silently drop a grouped non-asset output. |
| `formatLabel()` | **Keep** | Still the right button label. Feed it `display.format` instead of a parsed token. `toUpperCase()` already handles unknown tokens fine. |

Two comments in that file describe problems that **no longer exist**, since
grouping is now opt-in rather than inferred:

- `vq_video` / `vq_timeline` over-merging onto `vq` — can't happen; neither
  carries a `group`.
- `cover_png` grouping with itself — can't happen; a lone `group` member renders
  as a singleton.

`packages/ui/types/dialogue.ts:202` also has a comment describing the
client-side inference; worth updating.

---

## 4. Replacement algorithm

```
1. Filter out display.kind === "hidden" FIRST, before grouping.
2. Partition the remaining computed slots by display.group.
   - group == null  → its own singleton, always.
   - Slots keep their incoming (authored) order.
3. A group of ONE renders exactly like an ungrouped slot — no switcher chrome.
   (Never a regression for a guide that groups a lone output.)
4. A group of 2+ renders as ONE block:
     - lead        = members[0]   (first non-hidden member, authored order)
     - block kind  = lead.display.kind
     - heading     = lead.display.title
     - reveal      = lead.display.reveal
     - switcher    = one control per member, labelled formatLabel(display.format)
     - each member's value still resolves via its own value_slot_id
5. Group position in the pane = position of its first member.
```

Two edge cases worth handling explicitly:

- **A grouped member with no `format`.** Legal (the author may only label some).
  Fall back to `display.title`, then to `slot.name`. Don't drop the member.
- **Hidden must be filtered before step 2, not after.** The hidden slot is the
  data-flow source the visible formats are rendered from and legitimately carries
  the same `group`. Group first and you get a switcher with a phantom third entry.

### Scoping

`group` is **pane-scoped by convention**, not globally unique. Group within the
slot list you are rendering (per-dispatch today, which is already what
`groupDeliverableSlots` does) and two stages that both expose a `report` group
can't bleed together. There is no DB constraint enforcing uniqueness — if two
briefs on one pane reuse a token, they will merge, and that is the author's call.

---

## 5. What did NOT change

- `display.kind` vocabulary — still the closed enum (`image`, `audio`, `video`,
  `text`, `markdown`, `list`, `checklist`, `table`, `fields`, `file`, `document`,
  `hidden`). **`format` is not a `kind`** and does not switch renderer logic.
- `display.title` / `display.reveal` semantics.
- Operator slots: still `input`, still `display: null`.
- Value resolution: still the bindings read joined by `value_slot_id`.
- `producer`, `bound`, `blocked`, `cardinality`, `min_selections`,
  `max_selections` — all untouched.
- Step ordering and the `done` rollups.
- No new endpoint; no request-shape change.

---

## 6. Verifying against a real payload

Backend tests that pin this contract (in `crates/dapple-prompt-api/tests/guide_ingest_test.rs`):

| Test | Pins |
|---|---|
| `test_steps_orders_exposed_slots_by_authored_position` | `/steps` array order is authored order |
| `test_steps_reports_output_group_and_format` | `group`/`format` reach `/steps`; hidden sibling is grouped but formatless; operator slots carry no `display` |
| `test_brief_authored_ord_spans_units_and_outputs` | storage-level ordinal |
| `test_output_group_edit_reversions_brief` | a `group` edit re-versions the brief |

Their fixture (`create_grouped_outputs_tarball`) is a good shape to mirror in a
UI test double — it deliberately names slots so authored order (`zz`, `mm`, `bb`,
`aa`) disagrees with alphabetical, and puts an operator input *between* two
outputs.

**Regenerate your OpenAPI types** (`packages/ui/types/internal-api/types.gen.ts`)
against the updated API — `ExposedSlotDisplay` picks up both fields from the
schemars derive.

---

## 7. Guide-side prerequisite

Nothing renders differently until a guide authors the attributes. Authoring
reference: `docs/books/journey-planner-guide/directive-blocks.md` → brief-output
→ "One deliverable, several formats".

For `run-the-10k` / build-the-story specifically, the three outputs need:

```
:::brief-output { name: "build-the-story-output", slot: "tk_report", source: "computed",
                  source_path: "/report", display: "hidden", group: "tk_report" }
:::
:::brief-output { name: "build-the-story-html-output", slot: "tk_report_html", source: "computed",
                  source_path: "/report_html", display: "document", title: "Read the report",
                  group: "tk_report", format: "html" }
:::
:::brief-output { name: "build-the-story-pdf-output", slot: "tk_report_pdf", source: "computed",
                  source_path: "/report_pdf", display: "file", title: "Download the report",
                  group: "tk_report", format: "pdf" }
:::
```

Authored order there also fixes the lead: HTML first because it is written first,
not because `h` < `p`.

---

## Backend reference

- Migration: `crates/dapple-data/migrations/00000000000095_brief_authored_ord_and_output_grouping.sql`
- Wire types: `crates/dapple-types/src/model/journeys.rs` (`ExposedSlotDisplay`),
  `crates/dapple-types/src/model/briefs.rs` (`BriefOutput`)
- Query: `crates/dapple-data/assets/sqlc/journeys/select_step_binding_status_for_journey.sqlc`
- Ordinal rationale: `crates/dapple-data/AGENTS.md` → "Stored `ordinal` columns are rarely needed"
