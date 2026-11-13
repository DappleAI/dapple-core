# Rename `edgar filings`/`edgar exhibits` `fiscal_year` → `report_year`

> **Status:** open, near-term. Queued 2026-07-30 while landing the canonical
> fiscal-year label in `run-the-10k` and `weigh-the-m-and-a`.
> **Owner:** unassigned.

## Why

`edgar filings` and `edgar exhibits` both emit a field called `fiscal_year` that is
**not a fiscal year**. It is the calendar year of `report_date`:

```rust
// crates/dapple-prompt-flow/src/commands/edgar_filings.rs  (~626-631)
// crates/dapple-prompt-flow/src/commands/edgar_exhibits.rs (~232-237)   <- identical
let fiscal_year = filing
    .report_date.as_deref()
    .or(Some(filing.filing_date.as_str()))
    .and_then(|d| d.get(..4))
    .and_then(|y| y.parse::<i64>().ok());
```

For a December filer that happens to equal the filer's own label, so it looks correct
in every test built from Apple/Ford/Costco/Pfizer. For an off-calendar filer it does
not: a retailer whose year ends 1 Feb 2025 and who calls that **fiscal 2024** gets
`fiscal_year = 2025`.

**The name is what caused the bugs, not the value.** Guide authors read `fiscal_year`
as "the filer's fiscal year", and it was consumed that way in two guides across five
duplicated derivations. Two rounds of fixes have now landed:

1. `run-the-10k` invariant 31 — restamped the XBRL **facts** side to the filer's own
   label where unambiguous.
2. 2026-07-30 — the **filings** side never got that rule, so a restamped `fy` was being
   joined against the raw `fiscal_year`. A row rendered "FY2024" while linking to the
   FY2023 10-K, and a whole year was dropped from the window filter. Fixed by deriving
   one canonical label in each guide's `pull.nu` (`fy_label` per filing, `fy_index` on
   `facts`) and reading it everywhere.

A comment saying "this is not what it sounds like" has now been written in both guides
and did not prevent the second occurrence. Rename it.

## What to do

| Site | Holds | Action |
|---|---|---|
| `commands/edgar_filings.rs` (~626) | calendar year of `report_date` | rename → `report_year` |
| `commands/edgar_exhibits.rs` (~232) | identical derivation, same misnomer | rename → `report_year` |
| `edgar_comp.rs` (`CompRow`) | the filer's own label, parsed from the proxy SCT year column | **LEAVE — correctly named** |

Emit `report_year` **and** keep `fiscal_year` as a deprecated alias for one release, so a
journey pinned to an older guide version keeps working, then drop the alias.

## Consumer sweep

Small, and already known:

```
rg -l fiscal_year --glob '*.nu' ~/src/dapple/dapple-guides
```

returns **only** `run-the-10k` and `weigh-the-m-and-a` (27 script files). Nothing else in
the repo reads it. Both guides already route every filings-record read through
`fy_label`, so the sweep is mostly deleting the `| default ($f.fiscal_year? …)` fallback
arms once the alias is gone.

Do **not** sweep these two exempt receivers:

- `$c.fiscal_year` in the governance passes — `edgar comp` rows, already the filer's label.
- `$s1.fiscal_year` in `run-the-10k-origin-generate.nu` — an S-1, not an annual report.
- The pre-facts 20-F dedup in both `pull.nu` files — a dedup key, not a label, and it runs
  before any XBRL is fetched. Both sites carry a comment saying so.

## Verification

`dapple guide validate --no-build <guide-dir>` on both guides (the embedded parser is the
only thing that sees command signatures), plus the canonical-vs-fallback smoke described in
each guide's AGENTS.md — an off-calendar synthetic side must produce identical output on
both paths.
