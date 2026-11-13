# Plan 1 — `retort-the-n-port`: the promise as narrative organizer

**Status: EXECUTED 2026-08-03 (guide-side session). Offline-verified, not ingested.**
Part of the promise-leveling umbrella.

> Execution notes, two deviations from the spec below, both deliberate:
> - **`role_basis` was dropped.** The carrying point IS the basis (headline, so_what,
>   cite are already the evidence); a third field would duplicate it. Businesses emits
>   `role | role_claim` (11-field contract).
> - **The character stage is "Name the Character of the Promise"** (no apostrophe-slug
>   risk), placed after the composite stage; the pack wiring lesson fired live —
>   `character-of-the-promise-group` had to join the `:::brief-pack` groups list in
>   `introduction.md` (validate caught it, exactly as the brief-pack-seam memory says).
>
> All five retort probes green including `probe_nport_tracks.nu` (roles never touched
> track payloads — the explicit acceptance check). Fuzz arms I–L; lesions L5/L6
> demonstrated. `dapple guide validate`: OK, 21 tasks, 0 warnings.

## Why this guide, and why it's the smallest conceptual change

Retort already has the yardstick's SKELETON: track root = a prospectus claim;
tracks carry `region` (both / promise_only / portfolio_only), `evidence_tier`
(A/B/C), `rests_same_tier`, `basis`; the weigh pass adjudicates claims against
positions; the deterministic absence finder fills promise_only ("the prospectus
names a thing; the portfolio holds none of it"). What's missing is Kenny's
narrative half: *"that yardstick... will really shape the narrative of the group
of stories that make up the 10Ks in the n-port."* Today the holdings chapter
narrates the cast in weight order — the promise adjudicates but does not
ORGANIZE. After this plan, the promise casts every promoted holding into a role.

## Design

### 1. The `role` vocabulary (CONDUITS.md, beside the verdict vocabulary)

Three keys + drop rule, closed:

| Role | Meaning |
|---|---|
| `embody` | the holding is what the promise says the fund buys |
| `strain` | defensible under the promise, but only with an argument the prospectus doesn't quite make |
| `contradict` | the holding's own filings are evidence against the promise |

Guards written next to the vocabulary:
- Roles are narrative casting, NOT scores. Never count roles into an opinion of
  the fund; the weigh prompt's "NEVER GRADE THE FUND" rule is restated here.
- A role must name its basis: the claim it plays against (`role_claim`) or the
  fund's character key, plus the holding-filed fact that decided it.

### 2. Placement: roles hang off the CAST, not the claim tracks

The load-bearing decision. Per-holding stacks already hang off the cast
(2026-08-03 parity work); claim tracks carry the region/tier/basis machinery
and `probe_nport_tracks.nu` line-pins their payload. Re-rooting or extending
track payloads fires the full blast radius for zero benefit. Each promoted cast
entry gains `role`, `role_claim`, `role_basis` — connect passes them through,
synthesize renders them, tracks never see them.

### 3. Assignment rides the businesses pass (no new LLM call)

`prompts/retort-the-n-port-businesses-generate/en.md` — the per-holding 10-K
read that already produces each company's story and label-vs-substance findings
— additionally emits `role|role_claim|role_basis` per holding, judged against
the kept claims list + the fund's character key (both passed in as context).
`scripts/retort-the-n-port-businesses-generate.nu` parses against the closed
role list; unknown role → dropped + counted (retort's own assignment-parse
shape, reused).

### 4. The ZIP rule lands in weigh

`prompts/retort-the-n-port-weigh-generate/en.md` already receives findings
grouped by who filed them and ranks holding-filed evidence strongest. Add one
rule: **a holding's own filing character is evidence in both directions** — a
company whose filings show a label without the substance (revenue mix, segment
facts, R&D that don't support the theme it claims) counts AGAINST a thematic
claim that rests on that holding, with the holding's finding cited. Stays
inside the existing four-verdict vocabulary; `contradicted` still requires
named positions and figures. Do NOT add a field to the weigh output (it is a
fixed pipe-field contract) — the ZIP rule changes how existing verdicts are
reasoned, not their shape.

### 5. The holdings chapter reorganizes by role

`prompts/retort-the-n-port-write-portfolio/en.md`: "What is in it" becomes
role-organized — **"What embodies the promise" / "What strains it" / "What
contradicts it"** — weight still visible inside each band, unroled holdings in
a final unlabeled band (absence of a role is a fact, not a defect). This is the
band-don't-split discipline: one chapter, embedded bands, never three chapters.

### 6. The character pass (reference implementation for the family)

New `scripts/retort-the-n-port-character-generate.nu` +
`prompts/retort-the-n-port-character-generate/en.md` per Plan 0, fed from the
same promise material the promise pass already pulls. Output feeds businesses
(role context) and write-portfolio (chapter framing sentence: what kind of
promise this is, quoting the prospectus basis).

## Cascade (the full chain, walked deliberately)

`retort-the-nport.md` (new character stage/step + expose lists; businesses task
context gains claims+character) → `scripts/...-character-generate.nu` (new),
`...-businesses-generate.nu` (parse roles), `...-connect.nu` (cast passthrough
of the three role fields — NOT track payloads), `...-synthesize.nu` (role bands
in the portfolio blob; character line in write contexts) →
`prompts/...-businesses-generate`, `...-weigh-generate` (ZIP rule),
`...-write-portfolio` (bands), `...-character-generate` (new) → `CONDUITS.md`
(two vocabularies: character key block per Plan 0 + role vocabulary) →
`probes/probe_nport_static.nu` (key-line pins for both vocabularies; role-parse
lesions), `probe_nport_render.nu` (role bands render; unroled band renders;
empty-role degradation), `probe_nport_tracks.nu` (UNTOUCHED — and run to prove
it) → `AGENTS.md` (role doctrine + the never-grade restatement) →
`locales/en.ftl` (new slots/stage strings).

## Fuzz arms (Plan F, landed this session)

- Role parse: unknown role, malformed pipe count, empty reply — each dropped +
  counted, shown able to fail.
- Cast passthrough: adversarial cast records (missing optional fields, unicode
  names, shuffled order) through connect/synthesize — no dropped-field seam,
  order preserved, role fields either present or explicitly absent.
- Render: zero roled holdings degrades to the current weight-ordered chapter
  (band headers absent, not empty).

## Landmines

- `probe_nport_tracks.nu` is line-pinned: if it reddens, role data leaked into
  track payloads — that is a defect in the change, not a probe to update.
- The weigh output contract (fixed pipe fields) must not grow.
- Roles must never appear in the claim index or the verdict chapter ("The
  weighing, and the cut") — they are portfolio-chapter narrative only.
- The character key describes the promise, not the portfolio (Plan 0 rule) —
  the businesses prompt must not "correct" the key based on holdings.
