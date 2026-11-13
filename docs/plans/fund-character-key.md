# Plan 0 — The fund-character key (family-shared closed vocabulary)

**Status: EXECUTED 2026-08-03 (guide-side session). Offline-verified, not ingested.**
Part of the promise-leveling umbrella (`promise-leveling-umbrella.md`).

> Execution notes: the block landed in all four CONDUITS.md files; the key-line pin landed
> in three static probes (retort §16, 13f §18, mfp §17) — **run-the-10k has no probes/
> directory**, so its copy is unpinned until that guide grows a probe. The reference
> assignment pair (`retort-the-n-port-character-generate` prompt + script) is built and
> lesion-tested (fuzz arms A–H in `probe_nport_fuzz.nu`; lesions L1–L4 each demonstrated
> to redden exactly their arm).

## What it is

A single closed vocabulary that classifies a fund by the character of its stated
promise, assigned by an Opus pass reading the fund's own prospectus material
(`edgar fund-sections` objective/strategy text + `--tagged` risks/benchmarks).
It is the shared substrate for two opposite-direction reads:

- DOWN: the character frames how a fund's holdings chapter is narrated
  (Plan 1's role casting) and labels each advised fund in the 13F rollup (Plan 3).
- UP: it is the cohort key for the insider-conduct story (Plan 4) — "insiders at
  companies held by `explorative-growth` funds, as a group".

## The vocabulary (6 keys + drop rule)

| Key | The promise it names |
|---|---|
| `capital-preservation` | stability of principal; money-market language |
| `broad-index` | tracks a broad benchmark; the promise IS the index |
| `income` | yield / distribution first |
| `balanced-allocation` | multi-asset, target-date, 60/40 |
| `sector-thematic` | a named sector or theme (AI, clean energy, biotech) |
| `explorative-growth` | aggressive growth, small-cap, emerging, "innovative companies" |

Rules, retort-assignment style:
- One key per fund (per series). The pass may also return `none`.
- Unknown keys are DROPPED at parse, counted, never guessed or coerced.
- The key describes the PROMISE, not the portfolio. A "safe" fund holding ZIP is
  still `capital-preservation` — the mismatch is the finding, and it belongs to
  the weigh/role layer, not to this classification.
- The assignment is narrative casting, not a quality signal. No key is better
  than another; no chapter may rank keys.

## Where it lives (duplication by design)

Guides share no code. The key block is an **identically-worded section** —
heading `## Fund character key (family-shared)` — duplicated into the
CONDUITS.md of every guide that uses it:

- `retort-the-n-port/CONDUITS.md` (Plan 1)
- `mind-the-13f/CONDUITS.md` (Plan 3)
- `price-the-mfp/CONDUITS.md` (Plan 6)
- the new cohort guide's CONDUITS.md (Plan 4a)
- `run-the-10k/CONDUITS.md` (Plan 4b's inverse-lens paragraph)

**Consistency mechanism: each guide's `probe_*_static.nu` pins the exact
key-list line** with an identical assertion string, so drift in any one copy
reddens that guide's probe immediately. Write the five probe arms with the SAME
expected string (copy-paste the assertion, not the idea).

## Assignment machinery (guide-side, per-journey)

NOT engine-side. The engine is deterministic by doctrine and has no model
access; a persisted model opinion would put an Opus judgment where only filed
facts live. Each using guide gets its own pattern-copied pass:

- `prompts/<guide>-character-generate/en.md` — receives the fund's objective,
  principal strategies, tagged risk categories, and benchmark (the same material
  retort's promise pass already pulls); returns exactly one line
  `<key>|<one-sentence basis quoting the prospectus language that decided it>`.
- `scripts/<guide>-character-generate.nu` — parses against the closed list;
  unknown key → dropped + counted; carries the basis sentence forward so the
  report can quote WHY a fund was cast (`the prospectus says "maximum capital
  appreciation via early-stage companies"`), never just the label.

Exception (Plan 6): price-the-mfp folds the field into its existing
promise-generate prompt rather than adding a pass — money funds nearly always
assign `capital-preservation`, and saying so explicitly is the point.

Compact variant (Plan 3): mind-the-13f's per-advised-fund call assigns the key
inside the same single Opus call that adjudicates the fund's claims — same
vocabulary, same drop rule, one call per fund.

## Session scope (Session 0)

1. Write the canonical key block once (this doc is the source of truth for the
   wording; the block text lives in each CONDUITS.md).
2. Land the block + probe pin in the guides that exist today
   (retort-the-n-port, mind-the-13f, price-the-mfp, run-the-10k). The cohort
   guide adds its copy when it is created (Plan 4a).
3. Write the reference `character-generate` prompt + script pair ONCE in
   retort-the-n-port (the guide whose promise material is richest); later plans
   pattern-copy it.
4. Fuzz arm (Plan F discipline): the parse script's probe must show an unknown
   key dropping and being counted, an empty model reply yielding `none` without
   error, and a well-formed reply parsing — three lesions, each shown to fail.

## Landmines

- Do NOT let the key leak into deterministic finders as if it were filed fact.
  It is a model reading of filed prose; anything that renders it must be able to
  quote its basis sentence.
- Do NOT extend the vocabulary casually. Six keys is the cast-size discipline —
  a seventh key needs this doc amended and all five probe pins updated in one
  change, which is deliberate friction.
- The key is per SERIES (a trust's funds differ); everything downstream keys by
  series id, never by trust CIK.
