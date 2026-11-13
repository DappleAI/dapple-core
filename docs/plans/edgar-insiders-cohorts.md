# Plan E2 — Batch insiders + deterministic cohort aggregates

**Status: BUILT 2026-08-03 (fixture + property verified; no live batch run).** Engine
prerequisite for the insider-cohort work (`guide-insider-cohorts.md`). Part of the
promise-leveling umbrella.

> Execution notes:
> - Batch mode rides the EXISTING `edgar insiders` (positional now optional; a piped
>   `list<string>` returns one record: `window` / `issuers` / `unread` / `cohort`).
>   Single-company calls are byte-compatible with before. Sequential fetches; the
>   per-filing cache absorbs re-runs.
> - The pure cohort layer lives in `edgar_form345.rs` (`CohortTxn`, `cohort_aggregate`):
>   direction from classified `P`/`S` rows ONLY, compensation counted and never netted
>   (the 12x trap solved once), breadth issuer-granular, monthly buckets + undated
>   partition exactly, `shares_unstated` counted. The cohort is computed over
>   UNFILTERED rows so `--code` can never skew it.
> - Property tests (Plan F): permutation invariance (which CAUGHT a real f64
>   summation-order sensitivity on first run — shrunk case committed to
>   `proptest-regressions/edgar_form345.txt`; counts compare exactly, share sums
>   within relative epsilon), compensation-never-moves-direction, month-partition
>   exactness. Fixture test exercises every bucket. Clippy clean; 698 lib tests green.
> - The `(Type::Any, Type::Any)` input arm was added (the `edgar adv` task-record
>   parse-time lesson).

## Why

The cohort story reads "how insiders across a SET of companies acted as a
group." Today `edgar insiders` (`commands/edgar_insiders.rs` +
`src/edgar_form345.rs`) is per-issuer: one CIK/ticker per call, classified
trading-vs-compensation, cached per issuer (Migration 89). A guide looping 12
issuers would re-implement throttling and — worse — re-implement aggregation,
which is exactly where the known ~12x trap lives: **naive disposal counting
overstates insider selling ~12x** because compensation mechanics (grants,
tax-withholding dispositions, exercises) dwarf open-market trading. That trap
must be solved ONCE, engine-side, in the classified aggregation — never
per-guide.

## Design

Two additions, both in `crates/dapple-prompt-flow`:

### 1. Batch input on `edgar insiders`

Accept a list of CIK/tickers via pipeline input (the `edgar security` list
pattern), with engine-side throttling between issuer fetches. Per-issuer cache
(Migration 89 marker) is reused unchanged — a batch call over cached issuers
costs zero fetches. Output keeps the per-issuer shape, one block per issuer,
plus fetch-failure entries as a coverage bucket (`unread`, with reason) —
fetch failure is never a command failure (the `edgar book` discipline).

### 2. Deterministic cohort aggregate block

A pure aggregation layer in `src/edgar_form345.rs` (fixture-testable, no I/O)
that the batch command runs over its per-issuer results and returns alongside
them:

```
cohort: {
  window: { from, to },                 # caller-supplied date window
  issuers: N, issuers_with_activity: M, # participation breadth inputs
  by_classification: {
    trading:      { buys, sells, net_shares_direction, issuers_buying, issuers_selling },
    compensation: { grants, exercises, withholdings }   # counted, NEVER netted into direction
  },
  by_month: [ { month, trading_buys, trading_sells } ], # timing series
  unread: [ { cik, reason } ]
}
```

Hard rules encoded in the aggregation (not left to callers):

- **Direction comes from CLASSIFIED trading rows only.** Compensation rows are
  counted in their own bucket and never contribute to `net_shares_direction`.
  This is the 12x trap solved once.
- Participation breadth is issuer-granular (how many issuers had any classified
  trading in the window), not transaction-granular — one prolific filer must
  not read as a group.
- Everything is counts, shares, and dates — no ratios that imply a judgment, no
  labels like "heavy" or "unusual". The numbers never editorialize; the story
  voice (guide-side, with its own no-motive-verbs rule) does the reading.
- `unread` is always present; a cohort computed over 9 of 12 issuers says so in
  the block itself (no silent caps).

## Tests

- Fixture tests: a hand-built set of Form 4 rows across 3 issuers exercising
  every classification bucket; asserts compensation never moves direction;
  asserts breadth counts issuers not transactions.
- Property tests (Plan F): aggregation invariant under row permutation and
  under issuer-list order; monthly buckets partition the window with no row
  lost or double-counted; adversarial rows (zero shares, missing dates,
  duplicate accession) never panic — they land in a counted reject bucket.
- One `#[ignore]`d live smoke; default suite is fixture-only (rate limits).

## Session scope

1. Pure aggregation layer + fixtures in `src/edgar_form345.rs`.
2. Batch/pipeline input + throttling in `commands/edgar_insiders.rs`.
3. Property/fuzz arms per Plan F.
4. Command help updated (`edgar` parent stub) — document the window flag and
   the cohort block shape; guides read this shape blind, so it is the contract.

No new migration expected (per-issuer cache unchanged). If a batch-marker
proves necessary it follows the Migration 94 separate-marker rule.
