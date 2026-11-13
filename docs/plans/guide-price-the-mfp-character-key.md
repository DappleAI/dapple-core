# Plan 6 — `price-the-mfp`: character key adoption (minimal touch)

**Status: EXECUTED 2026-08-03 (guide-side session). Offline-verified, not ingested.**
Part of the promise-leveling umbrella.

> Execution notes: the assignment rides the promise pass's reply as a FIRST LINE
> (`character | <key> | <basis>`) — same call, no new pass — and the claim parse skips
> character lines explicitly so a padded line cannot leak the key as a claim. One
> deviation: a `fn_character` slot + second collector on the promise brief WAS needed
> (the "no new slot preferred" hope had no path to synthesize without a binding).
> One trap hit: rewording the prompt's "Return each claim..." sentence broke the static
> probe's pre-existing pipe-contract scanner (it anchors on "Return each"); reworded to
> keep the anchor. All three mfp probes green; fuzz arms A–E with lesions LM1/LM2
> demonstrated. `dapple guide validate`: OK, 0 warnings.

## Why minimal

price-the-mfp is already leveled: its promise IS a number ($1.00), tested
against the fund's own daily four-decimal shadow-price record, and the
same-source rule is already inverted in the fund's favor (*"the strongest
evidence in the family"*). Its four fixed track characters
(price/buffer/ownership/sponsor) carry the daily series and are untouched.

## Changes

1. **Character key block** (Plan 0's identically-worded section) into
   `price-the-mfp/CONDUITS.md`, with the key-list line pinned in
   `probes/probe_mfp_static.nu` — the fifth pin site that makes family drift
   redden everywhere at once.

2. **Assignment folded into the existing promise pass** — one extra
   closed-vocabulary output field on `prompts/price-the-mfp-promise-generate/en.md`
   (+ its script's parse, drop rule, counted unknowns). NOT a new pass: a money
   fund nearly always assigns `capital-preservation`, and *saying so
   explicitly* is the point — the report states the obvious as a fact with its
   prospectus basis quoted, which is what lets 13F rollups and cohort work
   treat money funds uniformly with everything else.

3. **No change needed for 13F rollup participation.** The rollup's compact
   check lives in mind-the-13f and reads N-MFP directly for advised money-market
   series. Document the relationship in both guides' CONDUITS.md (one sentence
   each) so neither future editor invents a coupling.

## Cascade

`CONDUITS.md` (key block) → `prompts/price-the-mfp-promise-generate/en.md` +
`scripts/price-the-mfp-promise-generate.nu` (one field + parse) →
`synthesize.nu` only if the character line is rendered (recommended: one
sentence in "What this is") → `probes/probe_mfp_static.nu` (key-line pin +
parse lesions) → `AGENTS.md` (one paragraph) → `locales/en.ftl` if a slot is
added (prefer riding the existing promise slot's payload — no new slot).

## Fuzz arms (Plan F)

Parse lesions on the extended promise output: unknown key dropped + counted,
missing field yields `none` without error, well-formed parses. Each shown able
to fail.

## Landmines

- Do not re-root or touch the four fixed characters; the key is report-voice
  material, not a track.
- A prime fund that genuinely reads `income` or `balanced-allocation` is
  possible — the pass must not hardcode `capital-preservation`; the near-
  constant outcome is a finding, not an assumption.
