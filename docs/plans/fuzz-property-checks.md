# Plan F — Fuzz/property-style checks everywhere we can

**Status: LANDING INCREMENTALLY (as designed).** Part of the promise-leveling
umbrella, but cross-cutting: every umbrella session lands the fuzz arms for the
surfaces it touches, plus ONE dedicated backfill session for the highest-value
existing parsers. Kenny: *"things like fuzzing style checks everywhere we can as
well, I didn't see much of that."*

> Landed so far (2026-08-03):
> - Guide-side: `probe_nport_fuzz.nu` (arms A–L), `probe_bdc_fuzz.nu` (A/B),
>   `probe_mfp_fuzz.nu` (A–E) — seeded deterministic payloads through the REAL
>   scripts, 10 lesions demonstrated able to fail.
> - Engine-side: `proptest` added to dapple-prompt-flow dev-deps. Properties on
>   `edgar_advised` (CRD pad symmetry/idempotence, junk never panics,
>   order-insensitivity) and `edgar_form345` cohort math (permutation invariance,
>   compensation-never-moves-direction, month-partition exactness). First real
>   catch on first run: f64 summation-order sensitivity in the permutation
>   property; shrunk case committed at `proptest-regressions/edgar_form345.txt`.
> - Session 3/4a (2026-08-03, later): `probe_13f_fuzz.nu` (arms A–L: rollup
>   parse drop rules, roster caps, insider-cohort classified-only direction) and
>   `probe_mf4_fuzz.nu` (arms A–F on the new cohort guide; arm E caught a live
>   looseness in the event-date regex on its first run). 12 more lesions
>   demonstrated across the two.
> - Backfill session (2026-08-03, same day): `edgar_sections::partition_props`
>   (every generated body lands in exactly ONE home, document order, four
>   sectioners never panic on junk), `edgar_form345::classify_props` (total on
>   junk, case/trim-insensitive, Discretionary is EXACTLY P and S), and
>   `edgar_book::align_props` (TTM total-preserving, alignment and dedup
>   order-insensitive with the E2 f64 tolerance discipline, aligners never panic
>   on junk dates) — 708 lib tests green. Guide-side:
>   `run-the-10k/probes/probe_10k_connect_fuzz.nu` (verbatim-carry of adversarial
>   kept rows incl. pipes/unicode/undated/note-only, arrival-order insensitivity,
>   empty-in-empty-out; lesion: dropping the fundchar registry row reddens arm A)
>   and `probe_10k_fundchar.nu` (the lens probe, which is also run-the-10k's
>   FIRST probe). The nport streamer cargo-fuzz target remains DEFERRED —
>   cargo-fuzz needs nightly + a fuzz/ crate and was always excluded from the
>   default suite; land it when a fuzz CI lane exists.

## Why the family needs this specifically

The line-pinned probe system is excellent at catching DRIFT (a dropped
passthrough reddens the pinned line) but only for the exact fixture pinned. The
family's own bug history is dominated by SHAPE bugs that a pinned fixture never
exercises:

| Known bug class | What it looked like |
|---|---|
| the dropped-field seam | a record drops a field → a downstream consumer invents a default |
| split sequence loses order | one authored list → two tables → arbitrary fallback sort |
| position truncation is a reading | `str substring 0..N` bets on the genre |
| nushell empty aggregation | `math sum` on empty after `each` returns a RECORD |
| describe table gate | record-list describes as `table<>`; gating on "list" inverts |
| multi-line boolean chain | leading `or` parses clean both ways; only a RENDER probe caught it |
| Form 4 naive counting | disposals overstated ~12x without classification |

Property/fuzz checks generalize the probes: instead of one pinned payload,
generate adversarial payloads and assert the INVARIANTS.

## Engine side (`crates/dapple-prompt-flow`, Rust)

**Tooling: `proptest`** for property tests in the normal test suite (runs in
`cargo test`, no infra), plus **`cargo-fuzz` targets** for the byte-level
parsers (run manually/CI-scheduled, not in the default suite).

Property tests on the pure `edgar_*` modules (they are fixture-testable by
design — that was the point of the command/pure split):

| Module | Invariants |
|---|---|
| `edgar_sections.rs` (all three sectioners) | output partitions input — every character lands in exactly one section or `unsectioned`; zero text loss; section order follows document order |
| `edgar_form345.rs` (incl. E2 aggregates) | aggregation invariant under row permutation; compensation NEVER contributes to trading direction; monthly buckets partition the window; adversarial rows → counted reject bucket, never panic |
| `edgar_identity.rs` | `normalize_company_name` idempotent (normalize(normalize(x)) == normalize(x)); resolution fails closed on constructed ambiguity; unicode/empty/whitespace names never panic |
| `edgar_advised.rs` (E1) | CRD zero-pad join symmetric; malformed CSV rows counted, never silently dropped |
| `edgar_book.rs` | alignment total-preserving (composite cell = weighted sum of contributing cells, per field per period); latest-filed dedup deterministic under input order; effects decomposition sums to the total move over the intersection |
| `edgar_nport.rs` / `edgar_dera.rs` ingest | cargo-fuzz: malformed XML/CSV/zip must ERROR, never panic and never silently truncate (truncation-is-a-reading doctrine) |
| `edgar_comp.rs` | fail-closed on structurally adversarial `<tr>` soup — wrong answer is worse than no answer |

Placement: property tests live beside each module's existing fixture tests.
Fuzz targets in `crates/dapple-prompt-flow/fuzz/` with a small seed corpus from
the existing fixtures. Live-EDGAR remains excluded (rate-limited).

## Guide side (nushell, per guide)

New `probes/probe_<guide>_fuzz.nu` per guide — pattern duplicated like
everything else in the family. Nushell has no proptest; the probe GENERATES
adversarial payloads with a seeded deterministic generator (a fixed seed list,
so failures reproduce — no `random` in probes) and runs the guide's own
connect/synthesize functions on them.

Generated shapes, per guide's point-record contract:
- shuffled record order (order-preservation invariant: output order derives
  from the authored/id order, never from input arrival);
- missing optional fields on every record (no dropped-field seam: a consumer
  must render absence, never invent a default);
- empty lists at every aggregation site (the `math sum`-after-`each` record
  trap; the `describe`/table-gate inversion);
- unicode names, pipe characters inside text fields (assignment-parse safety:
  `id | key` parsing must not split on content pipes);
- extreme weights / zero weights / duplicate ids (caps carry skipped counts;
  first-wins rules deterministic);
- oversize text at every truncation site (truncation explicit and labeled,
  never a silent `str substring`).

Assertions are the invariants, not golden output: no field lost between
connect input and synthesize render; totals preserved; every cap's skipped
count present; parse rejects counted.

**Every fuzz probe must be demonstrated able to FAIL** (the smoke-coverage
doctrine: a guard that cannot fail guards nothing). The mark-the-bdc precedent
— "nine lesions were applied and each produced its intended failure" — is the
bar: each fuzz probe lands with its lesion list in the session notes.

## Sequencing

- **Woven**: Sessions 0, E1, E2, 1, 3, 4a, 4b, 5, 6 each land the arms named in
  their own plan docs (each doc has a "Fuzz arms" section).
- **Backfill session** (once, after a few umbrella sessions have set the
  pattern): the four highest-value existing surfaces —
  1. `edgar_sections.rs` sectioners (partition invariant),
  2. `edgar_form345.rs` classification (pre-E2 baseline),
  3. `edgar_nport.rs` streamer (cargo-fuzz),
  4. `edgar_book.rs` alignment (total-preserving) —
  plus one guide-side fuzz probe retro-fitted to run-the-10k's connect (the
  family's largest and the one whose bug classes seeded the doctrine list).

## Landmines

- Deterministic seeds only — a flaky fuzz probe will be deleted, and deserves
  to be. Reproducibility beats coverage.
- proptest shrinking output must be committed as regression fixtures when a
  failure is found (`proptest-regressions/` checked in).
- Guide probes must not call the network or the model — fuzz probes exercise
  connect/synthesize logic on generated payloads, exactly like the existing
  render probes stub `edgar` pulls.
- Do not gate CI on cargo-fuzz (open-ended); DO gate on proptest (bounded
  cases, part of `cargo test -p dapple-prompt-flow`).
