# Plan E1 — `edgar advised`: the manager → advised-funds roster

**Status: BUILT AND REAL-ARCHIVE-VERIFIED 2026-08-03.** Engine prerequisite for the
mind-the-13f rollup (`guide-mind-the-13f-advised-rollup.md`). Part of the
promise-leveling umbrella.

> Execution notes (what the verification found, and two design changes it forced):
>
> - **The linchpin held.** The DERA N-CEN set's `ADVISER.tsv` carries a zero-padded
>   `CRD_NUM` per fund (verified live, 2026q2: 4,028 rows, 0 unjoined). `FUND_ID` is a
>   `{accession}_{cik}_{series}` composite; joins to `FUND_REPORTED_INFO` (fund name,
>   series, `IS_MONEY_MARKET`, net assets) and `SUBMISSION` (registrant CIK, dates).
>   `ADVISER_TYPE` distinguishes Advisor / Subadvisor / Terminated variants. 14 rows
>   carry the literal `N/A` CRD — counted in `rejected_crd_rows`, never dropped silently.
> - **DESIGN CHANGE: one quarter is NOT the roster.** N-CEN is annual; a quarterly DERA
>   set holds only funds whose fiscal year ended shortly before it. The command defaults
>   to a rolling-year UNION of the newest four sets (~35 MB, memoized per process);
>   `--quarter` restricts to one. The latest-filing dedup makes the union correct.
> - **DESIGN CHANGE: no `--persist` / migration in V1.** The 13F rollup calls this once
>   per journey and the in-process memo covers repeated lookups; a DB cache buys nothing
>   yet. Deferred until a caller needs cross-process reuse.
> - URL quirk recorded: recent quarters are `YYYYqN_ncen.zip`, older ones
>   `YYYYqN_ncen_0.zip`; the index regex matches both.
> - Tests: 7 fixture + 4 proptest properties (CRD pad symmetry + idempotence,
>   never-panic on junk, order-insensitivity) + 1 `#[ignore]`d real-archive test
>   (`NCEN_TEST_ZIP=...`), run against the live 2026q2 zip: T. Rowe (CRD 105496,
>   padded and unpadded agree) → 34 advised series with roles split. Clippy clean.
>   proptest added to dev-dependencies (first Plan F engine landing).

## Why this command must exist

The 13F rollup needs "the registered funds this manager advises." Verified
2026-08-03: **`edgar adv` cannot answer this.** `src/edgar_adv.rs` parses the
ADV Part 1A bulk roster (the 448-column CSV → `AdvRow` projection: AUM, fee
bases, discipline items) and the bulk CSV carries no advised-fund roster — no
Item 5.G / Schedule D fund list. The join runs the OTHER direction: **N-CEN
names the adviser per series** (`src/edgar_ncen.rs` — `NcenSeries` carries
`investmentAdviserCrdNo`, `investmentAdviserName`, `adviser_lei`). But `edgar
ncen` is per-registrant; inverting it (adviser → all series naming that CRD)
requires a bulk source.

## Design

New pure module `src/edgar_advised.rs` + command `commands/edgar_advised.rs`,
registered in `commands/mod.rs` alongside the other edgar commands.

**Input:** a CRD number, or an adviser name. Name input resolves via
`normalize_company_name` (the `edgar_identity.rs` normalizer) against the
roster and **fails closed on ambiguity** — two candidate CRDs is an error
naming both, never a guess (same three-outcome discipline as `edgar resolve`).

**Primary source: SEC DERA "Form N-CEN Data Sets"** (structured quarterly
archives) — the exact bulk-ingest pattern `src/edgar_dera.rs` already
implements for the 13F data sets. **FIRST task of the session: verify the DERA
N-CEN set's adviser table carries CRD per series** (high confidence — the
XML field `investmentAdviserCrdNo` is a per-series element and DERA sets
mirror form structure — but it is the linchpin; confirm before writing code).

**Fallback if DERA disappoints:** EFTS full-text search (`edgar search`
machinery) for the adviser name restricted to form N-CEN, then `edgar ncen`
per hit. Slower and rate-limited but shippable; keep the same output shape so
the guide never knows which path ran.

**Output:** one row per advised series:

```
{ series_id, fund_name, registrant_cik, net_assets, adviser_role, filing_date }
```

- `adviser_role`: primary vs sub-adviser, as N-CEN distinguishes them — a
  sub-advised fund is a different fact about a manager and the guide must be
  able to say which it is.
- Sorted by net_assets descending so callers can cap deterministically.

**`--persist`:** cache rows with their own ingest-marker table, the
Migration 94 pattern (`edgar manager`'s separate-marker rule: a new marker
table, never reusing another command's marker).

## Traps (known before writing a line)

| Trap | Handling |
|---|---|
| CRD zero-padding: N-CEN files `000105958`, ADV roster keys `105958` | normalize to the unpadded integer form at EVERY join boundary; property test asserts the join is symmetric under padding (Plan F) |
| A 13F carries no CRD at all | out of scope here — the CALLER (mind-the-13f's `mn_adv` name-join) owns manager identity and fails closed; this command takes CRD-or-name and inherits the fail-closed rule for names |
| DERA set staleness | freshness floor by DATE, not row count — copy `edgar_adv.rs`'s `ADV_ROSTER_MIN_LATEST_FILING` pattern and its "do not walk backwards on 404" rule (the series-registry staleness lesson: row counts cannot detect a stale snapshot) |
| One adviser, many CRDs over time (successor registrations) | out of scope V1; the roster row is keyed by the CRD given; document in command help |

## Tests

- Fixture tests on the pure module: a miniature DERA-shaped archive → roster
  rows; padding variants; ambiguous-name failure; sub-adviser flag carried.
- Property tests (Plan F): CRD normalization idempotent + join symmetric under
  zero-padding; malformed CSV rows error, never panic or silently drop
  (dropped rows are counted and reported, the no-silent-caps rule).
- NO live-EDGAR test in the default suite (rate-limited; whole-file runs fail
  deceptively). One `#[ignore]`d live smoke behind the existing live-test
  pattern in `tests/edgar_live_test.rs`.

## Session scope

1. Verify the DERA N-CEN adviser table (download one quarter, inspect columns).
2. Pure module + fixtures.
3. Command layer + `--persist` + migration for the marker table.
4. Property/fuzz arms per Plan F.
5. Update `edgar` parent help stub + `commands/mod.rs` registration.

Rust only, `crates/dapple-prompt-flow`. `cargo build -p dapple-prompt-flow`
compiles against the live DB via the overlay; migration applied with
`dapple sqlx migrate` (never psql).
