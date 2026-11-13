# What dapple-ui needs from a stage, part two: an icon set

**Asking repo:** dapple-ui (BFF + demo UX)
**To:** dapple-prompt
**Date:** 2026-08-03
**Status:** requirements only — the design and the naming are yours
**Supersedes:** the "Why we are *not* asking for an icon name on the wire" section of
[`dapple-ui-stage-kind-and-title.md`](./dapple-ui-stage-kind-and-title.md)
**Pattern we are copying:** the escape hatch that doc itself named — *"a separate optional
open-vocabulary string (an `icon` a client MAY map and MUST tolerate not recognizing)"*

---

## TL;DR

One more optional attribute on `:::stage`, called `icon`, holding **up to five icon names**. Store
them verbatim, serve them per `/steps` row like `stage_kind`, and validate nothing. We draw one of
them; a name we do not recognize costs nothing, because the `kind` glyph underneath it is already
what we would have drawn.

`kind` landed and works. This is the gap it deliberately does not fill.

---

## Why we changed our minds

**What we said.** In the `kind` doc we talked ourselves out of asking for an icon:

> `api.dapple.x` serves dapple-suite, the external SDK, and CLI consumers — none of which draw
> lucide glyphs, and some of which draw nothing at all. An icon name on the wire would encode one
> client's component library into a contract shared by all of them, and would go stale the first
> time any of us restyled.

That objection is correct, and it is an objection to a **required, validated, closed** field. Which
is what we were picturing. It is not what we are asking for, and the same doc drew that exact
distinction one paragraph later — we should have followed our own footnote instead of writing the
ask without it.

**What changed is what shipping `kind` taught us.** `kind` answers *what is this stage*. It cannot
answer *which stage am I on*, and it should not try: `mark-the-bdc` has fifteen stages and nine of
them are honestly `analyze`. The rail now draws nine identical discs in a row. That is a different
failure from the blank-disc bug `kind` fixed, and the same loss of purpose — an operator cannot
locate themselves in it. Distinguishability *within* a kind is authored knowledge with no
client-side substitute, which is the same argument that carried `title`.

**Why staleness no longer bites.** Because the fallback chain absorbs it. We draw:

```
authored icon  →  the kind glyph  →  the neutral `stage` disc
```

A name we do not recognize renders **exactly what we ship today**. So a restyle on our side is a
registry edit, not a contract migration; a consumer that draws nothing ignores a string it was
never obliged to read; and a guide can adopt a name before we curate it, or keep one after we drop
it, with no coordinated release in either direction. That property is worth more to us than
validation, and it is the reason every ask below points away from a second closed enum.

---

## Ask — `icon`

An optional, author-set attribute on `:::stage`, holding up to five icon names.

```
:::stage { name: "Pull the Record", kind: "intake", icon: ["radar", "scan-search", "id-card"] }
```

Five rather than one so a stage can vary without an author committing to a single forever-glyph.
What we do with the set is our business and is written down here only so the shape makes sense: we
pick **one**, deterministically, by hashing the stage id — so the disc is decided on first paint
and never moves while an operator sits on the stage. The variety lands across stages and journeys,
never within one.

Four properties we depend on:

1. **Open vocabulary.** The deliberate inverse of `kind`'s property 1, and the difference is the
   whole ask: **no lookup table, no enum, no ingest warning, no downgrade-to-absent.** Store the
   strings as authored. We are the only party that knows what we can draw, and we degrade by
   design. (`kind` earned its closed set because an unknown kind means a *pane* we cannot
   characterize. An unknown icon means a picture we substitute. Not the same risk.)
2. **A set, up to five, in authored order.** Order is load-bearing because we read the first five
   and ignore the rest — a sorted or set-normalized value would silently change *which* five.
   Duplicates are ours to drop.
3. **kebab-case, matched exactly.** Lucide's own normal form, and the same "we match exactly and do
   not normalize" stance `kind` takes. We fold `FileText` / `file_text` defensively, but the
   documented spelling should be `file-text`.
4. **Stage granularity.** One pane, one disc. Same argument as `kind`, unchanged.

---

## Where it needs to arrive

Identical to `kind`, because it is the same join you already made:

- **`GET /api/journeys/{id}/steps`** — `stage_icon` on every row, repeated identically across a
  stage's rows. First-seen-wins folding is already how we read `stage_name` / `stage_kind`.
- **`GET /api/journeys/{id}/stages`** — as `icon`, for parity with the consumers who do call it.

A JSON array is what we expect. We also accept a bare string for a one-icon stage, so you do not
have to normalize that case on the way out if it is cheaper not to.

---

## Compatibility rules we are relying on

- **Additive, `#[serde(default, skip_serializing_if)]`.** Absent and `[]` mean the same thing to
  us: no icon, draw the kind glyph — which is exactly today's rendering, so the no-op case is
  genuinely a no-op.
- **An unknown value is a client-side degrade, never an error.** Stronger here than for `kind`,
  because for `icon` an unknown value is not even skew — it is the normal, expected state of a
  guide authored against a vocabulary snapshot different from ours.
- **Backfill: none expected**, same as `kind`. We assume the same two consequences and have planned
  around both: it folds into the stage content hash, and journeys are version-pinned, so icons
  appear on journeys created *after* a guide's re-ingest.

---

## Three questions we would like answered explicitly

1. **Do hyphens survive the YAML-like attribute parser?** `directive-blocks.md` says unquoted array
   values are auto-quoted (`[agent, product]` → `["agent", "product"]`). Every name in our
   vocabulary contains hyphens, and `icon: [file-text, chart-line]` is what an author will type
   without thinking. If that does not round-trip, tell us and the docs will say "quote them".
2. **Do you want to enforce the five-cap server-side?** Our position is **no, or a warning at
   most** — we cap client-side regardless, and an author writing eight has made a harmless mistake,
   not a broken guide. But please document the cap in the `:::stage` attribute table, so an author
   who writes eight knows why three of them never show.
3. **Which deploy carries it?** Standing item from the `kind` ask: we regenerate types from the
   live `/api.json`, so "landed in `main`" is not "usable by us".

---

## The vocabulary

150 names, curated by us from lucide, grouped by the `kind` they read as. **The grouping is
advisory** — nothing stops an author putting `radar` on a `deliver` stage, and nothing should. It
exists because the question an author actually holds is *"I just wrote `kind: analyze`, which of
these fit?"*, and because a group's kind glyph is literally what its members fall back to.

The kind lines below are quoted verbatim from the `kind` doc's own table, so the two documents
cannot drift apart.

#### `intake` — The operator supplies the values the work starts from

```
target, crosshair, search, scan-search, keyboard, form-input, text-cursor-input, id-card, fingerprint, hash, at-sign, key-round, door-open, ticket, clipboard-pen
```

#### `fetch` — Retrieves source data; runs by itself

```
download, arrow-down-to-line, cloud-download, database, server, hard-drive-download, satellite-dish, antenna, rss, globe, plug, inbox, waypoints, folder-down, radio-tower
```

#### `analyze` — Derives a reading from data already held; runs by itself

```
chart-line, chart-column, chart-pie, chart-scatter, chart-spline, activity, trending-up, trending-down, gauge, microscope, radar, sigma, calculator, percent, brain
```

#### `decide` — The operator chooses among generated candidates

```
git-branch, split, shuffle, list-checks, check-check, circle-check-big, hand, pointer, mouse-pointer-click, vote, gavel, dices, signpost, route, toggle-right
```

#### `refine` — The operator sharpens something already chosen

*Ours, not yours: you dropped this member and we agree it names a step beat. It survives here only
because our temporary derivation still emits it, and it retires with that derivation.*

```
pencil, pen-tool, sliders-horizontal, settings-2, wand-sparkles, brush, eraser, scissors, filter, crop, focus, aperture, drafting-compass, spline, ruler
```

#### `review` — The operator reads and confirms; produces no new data

```
eye, glasses, book-open, book-open-check, file-search, clipboard-check, badge-check, shield-check, stamp, list-todo, notebook-pen, scroll-text, receipt, newspaper, binoculars
```

#### `compare` — Weighs two already-produced things against each other

```
scale, arrow-left-right, git-compare, git-merge, columns-2, diff, equal, equal-not, ratio, blend, layers, contrast, align-horizontal-distribute-center, chart-no-axes-column, chart-candlestick
```

#### `assemble` — Builds the deliverable from everything upstream

```
wrench, hammer, blocks, boxes, puzzle, construction, package, combine, group, workflow, factory, cog, drill, layout-grid, package-open
```

#### `deliver` — The finished artifact; the terminal pane

```
party-popper, sparkles, trophy, flag, gift, send, share-2, file-check, file-text, book-check, medal, award, rocket, printer, mail-check
```

#### `stage` — Our neutral default

*Also ours: the kind we draw when none was authored, or when the authored one is a word we do not
know. A stage with no `kind` can still carry an `icon`, and these are the names that read as
"a stage" without claiming what kind of one.*

```
circle-dot, circle, dot, square, hexagon, map-pin, flag-triangle-right, bookmark, asterisk, milestone, folder-check, archive, telescope, compass, layers-2
```

### Machine-readable

Flat, in group order, for the journey builder's picker:

```json
["target", "crosshair", "search", "scan-search", "keyboard", "form-input", "text-cursor-input", "id-card", "fingerprint", "hash", "at-sign", "key-round", "door-open", "ticket", "clipboard-pen", "download", "arrow-down-to-line", "cloud-download", "database", "server", "hard-drive-download", "satellite-dish", "antenna", "rss", "globe", "plug", "inbox", "waypoints", "folder-down", "radio-tower", "chart-line", "chart-column", "chart-pie", "chart-scatter", "chart-spline", "activity", "trending-up", "trending-down", "gauge", "microscope", "radar", "sigma", "calculator", "percent", "brain", "git-branch", "split", "shuffle", "list-checks", "check-check", "circle-check-big", "hand", "pointer", "mouse-pointer-click", "vote", "gavel", "dices", "signpost", "route", "toggle-right", "pencil", "pen-tool", "sliders-horizontal", "settings-2", "wand-sparkles", "brush", "eraser", "scissors", "filter", "crop", "focus", "aperture", "drafting-compass", "spline", "ruler", "eye", "glasses", "book-open", "book-open-check", "file-search", "clipboard-check", "badge-check", "shield-check", "stamp", "list-todo", "notebook-pen", "scroll-text", "receipt", "newspaper", "binoculars", "scale", "arrow-left-right", "git-compare", "git-merge", "columns-2", "diff", "equal", "equal-not", "ratio", "blend", "layers", "contrast", "align-horizontal-distribute-center", "chart-no-axes-column", "chart-candlestick", "wrench", "hammer", "blocks", "boxes", "puzzle", "construction", "package", "combine", "group", "workflow", "factory", "cog", "drill", "layout-grid", "package-open", "party-popper", "sparkles", "trophy", "flag", "gift", "send", "share-2", "file-check", "file-text", "book-check", "medal", "award", "rocket", "printer", "mail-check", "circle-dot", "circle", "dot", "square", "hexagon", "map-pin", "flag-triangle-right", "bookmark", "asterisk", "milestone", "folder-check", "archive", "telescope", "compass", "layers-2"]
```

### This list is a snapshot, and that is fine

Source of truth is `dapple-ui/packages/ui/lib/stageIcons.ts` (`STAGE_ICON_GROUPS` /
`STAGE_ICON_NAMES`). The copy above was generated from it on the date in the header.

**Because the field is open, this list going stale is not a bug and needs no doc revision.** A name
we add later draws immediately for guides that already author it. A name we never add draws the
kind glyph. Neither case needs a coordinated release — which is the entire reason we are asking for
an open vocabulary instead of a second closed one.

If a live-consumable list is ever wanted we can serve one from a BFF route
(`GET /api/stage-icons`). Explicitly out of scope; noted only so nobody builds a wire-side
vocabulary table to solve a problem a static list already solves.

---

## Documentation surfaces

The same three the `kind` ask used, so consumers other than us find it:

- the `:::stage` attribute table, `docs/books/journey-planner-guide/directive-blocks.md` — the
  `icon` row sits directly under the `kind` row that is already there;
- the `/steps` row description, `docs/books/api-consumer-guide/endpoint-reference.md`;
- a `CHANGES.md` entry in the additive style.

---

## What we do when this lands

Nothing, which is the point of saying so. **The consumption path is already built, shipped and
tested against the absent case** — `JourneyStageRef.icons`, `resolveStageIcon`, the 150-glyph
registry and the rail's `data-stage-icon` hook are all in `main` behind a fallback that is
byte-identical to today's rendering. The first guide to author `icon:` is the first time any of it
draws anything.

One thing this does *not* retire: `deriveStageKind` still goes when guides carry `kind`,
independently of this.

## What we are not asking for

- **A closed icon enum**, a lookup table, or server-side validation against our 150. Every one of
  those would trade the property this ask exists for.
- **A per-stage colour.** Still ours; the palette cycles.
- **Anything costing a round trip per stage.**
- **A change to `kind`.** It is doing its job. This is the field beside it, not a correction to it.

---

## Reply from dapple-prompt (2026-08-03)

Implemented as asked, open vocabulary and all. Everything below is what you need to consume it.

### The field

Authored as `:::stage { name: "Pull the Record", kind: "intake", icon: ["radar", "scan-search"] }`,
stored on the stage version itself (`rev.stages.icons`, migration 98 — intrinsic, not per-playbook),
and folded into the stage content hash. All four properties you depend on hold:

1. **Open vocabulary — no lookup table, no enum, no ingest warning, no downgrade.** This is the one
   place we deliberately did *less* than we did for `kind`, and the code says so at every site: the
   INSERT has no `LEFT JOIN` to validate against (with a comment explaining why it must not grow
   one), and `collect_stage_icon_cap_warnings` documents that an unrecognized name is never warned
   about. Your framing — an unknown kind means a *pane* we cannot characterize, an unknown icon
   means a *picture* you substitute — is the reasoning we wrote down, because it is the thing a
   future contributor will otherwise "fix".
2. **A set, in authored order, no normalization.** No sort, no dedup, no case folding. Order is
   load-bearing in the hash too: reordering the same names re-versions the stage
   (`test_stage_icon_edit_reversions_stage_iconless_is_stable` asserts it), so a reorder actually
   reaches you instead of silently not.
3. **kebab-case, matched exactly.** We store the bytes you were given. No folding of `FileText` /
   `file_text` on our side — keep yours defensive.
4. **Stage granularity.** Unchanged from `kind`.

Empty strings are the one thing we drop (`["", "globe"]` → `["globe"]`) — they name nothing and
would otherwise fold into the content hash as a phantom entry.

### Where it arrives

- **`GET /api/journeys/{id}/steps`** — `stage_icon` on every row, repeated identically across a
  stage's rows, exactly like `stage_name` / `stage_kind`.
- **`GET /api/journeys/{id}/stages`** — as `icon`.

Always a **JSON array of strings** (we normalize the bare-string authoring shape on the way in, so
you never see a scalar on the wire). `#[serde(default, skip_serializing_if = "Vec::is_empty")]` —
absent and `[]` are the same fact and the key is **omitted entirely** when there are no icons, so
the no-op case really is a no-op. The external surface gets both fields automatically; same DTOs.

### Your three questions

1. **Yes, hyphens survive — no quoting needed.** `icon: [file-text, chart-line]` normalizes to
   `["file-text", "chart-line"]`. The attribute tokenizer breaks bare words only on
   `{ } [ ] : , "` and whitespace, and quotes anything that is not `true`/`false`/`null`/a number,
   so a hyphen is an ordinary word character. Pinned at both levels rather than left to inference:
   `stage_icon_hyphenated_names_survive_unquoted_attributes` (parser) and
   `test_stage_icon_persists_and_reaches_journey_status` (end-to-end, which authors the attribute
   unquoted precisely so a future tokenizer change fails a test instead of a guide). **Docs say
   don't bother quoting.**
2. **No server-side cap — warning only, exactly your position.** Authoring more than five stores
   *all* of them verbatim and emits an ingest warning naming the stage and the names that will
   never be drawn (`test_stage_icon_over_cap_warns_but_stores_all`). Truncating would discard
   authored content to enforce a *render window* that belongs to you, and it would also mean the
   stored value no longer matched what was hashed. The cap is documented in the `:::stage`
   attribute table.
3. **Deploy: not yet.** Kenny drives it and will tell you which dapple-prompt-api deploy carries
   the field — regenerate from the live `/api.json` after that, not after merge. Same standing item
   as `kind`; if the two ship together you will get both fields in one regeneration.
   <!-- DEPLOY PLACEHOLDER: fill in the deploy identifier when it ships -->

### Compatibility rules — confirmed as you assumed

- **Additive**, and the absent case is byte-identical to today's payloads.
- **An unknown value never errors here**, because we never look at the values. There is no code path
  on our side that can reject one.
- **Backfill: none**, and your two consequences are exactly right — it folds into the stage content
  hash, and journeys are version-pinned, so icons appear on journeys created *after* a guide's
  re-ingest. An icon-less guide re-ingests with **no version churn at all** (the empty-omit fold,
  same trick `kind` uses; `hash_stage_icons_empty_is_noop` guards it).

### Two notes you did not ask for

- **The vocabulary list is not stored anywhere on our side, on purpose.** We did not seed your 150
  into a table or copy them into a Rust constant — a copy is a thing that goes stale and then starts
  lying. `docs/books/journey-planner-guide/directive-blocks.md` points authors at your registry as
  the source of truth and says in as many words that an unknown name is normal. If you ever do serve
  `GET /api/stage-icons` from the BFF, nothing here needs to change.
- **`refine`.** Your group survives in your doc as advisory grouping, which is fine — grouping is
  yours. It is still not a `kind` member, so those fifteen names fall back to the neutral disc
  rather than a `refine` glyph, and they will keep doing so after your temporary derivation retires.
  Worth a look when you next touch the registry, since that group is the one whose fallback is
  weakest.
