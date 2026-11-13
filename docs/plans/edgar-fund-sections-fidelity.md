# Handoff — Fidelity's prospectus has no sections (`edgar fund-sections`)

**Written 2026-08-01**, at the end of the session that closed
[`edgar-fund-sections-instance-text.md`](./edgar-fund-sections-instance-text.md). Read that
first — it establishes the linkbase↔instance join this work extends, and the discipline that
the filing states its own structure rather than a vocabulary here doing it.

This is the last known filer family that `edgar fund-sections` serves badly. Everything below
was measured against **CIK 225322, accession `0000225322-26-000191`** (Fidelity Summer Street
Trust, 485BPOS filed 2026-07-27). Re-derive before trusting it — Fidelity re-files often.

---

## The finding, and it is not what the shape suggests

The first read of this filing is "the standard section keys are on trust-level roles and the
funds are on per-fund roles, so join them." **That does not work, because the trust-level roles
are stubs.** Concept counts per role, from its presentation linkbase
(`fmr-20260730_pre.xml`, 795 KB, 28 `presentationLink` blocks):

| role | real concepts |
|---|---|
| `xbrl.sec.gov/rr/role/InvestmentStrategy` | **`DocumentType`. That is all.** |
| `xbrl.sec.gov/rr/role/RiskReturn` | `DocumentType` |
| `xbrl.sec.gov/rr/role/FeesAndExpenses` | `DocumentType` |
| `xbrl.sec.gov/rr/role/PerformanceManagement` | `DocumentType` |
| `xbrl.sec.gov/rr/role/ShareholderFeesData` | `DocumentType` |
| `xbrl.sec.gov/rr/role/OperatingExpensesData` | `DocumentType` |
| `xbrl.sec.gov/rr/role/BarChartData` | `DocumentType` |
| `xbrl.sec.gov/rr/role/PerformanceTableData` | `DocumentType` |
| `xbrl.sec.gov/rr/role/InvestmentRisks` | `DocumentType`, **`RiskTextBlock`** |
| `xbrl.sec.gov/rr/role/ExpenseExample` | `ExpenseExample*Year01/03/05/10`, `*ColumnName` |
| `www.fidelity.com/role/DisclosureDetailS000067930_…-ComboPRO` | **131 concepts** |
| `www.fidelity.com/role/RiskSummaryS000067930_…-ComboPRO` | none |

The `DisclosureDetail…` role's 131 concepts are the whole prospectus for that fund, in order:

```
RiskReturnHeading  ObjectiveHeading  ObjectivePrimaryTextBlock  ObjectiveSecondaryTextBlock
ExpenseHeading  ExpenseNarrativeTextBlock  ShareholderFeesCaption  ShareholderFeesColumnName  …
```

**So the prose is all there, and all attributed to a fund. It is simply never divided into
sections.** The trust-level roles exist to satisfy the standard role vocabulary; they hold no
disclosure. There is nothing to join them to.

Consequence today: `--fund X --fetch [InvestmentStrategy]` matches only a stub trust-level
section (which `--fund` then excludes anyway), so a Fidelity fund returns nothing. The command
now warns rather than returning silent nulls, but that is a diagnosis, not a fix.

---

## What a fix has to do

Divide a fund's concept blob into standard sections. Only one thing can do that: knowing which
section each CONCEPT belongs to — `StrategyNarrativeTextBlock -> InvestmentStrategy`.

That is the `section_key -> elements` vocabulary the instance-text pass deliberately rejected,
**inverted**. The objection then was that such a table is filer-observed, needs re-verifying per
filer, and silently drops what it has not seen. Two things answer that objection here, and both
should be built:

**1. Derive the map, do not invent it.** A filer that DOES partition states the mapping in its
own linkbase. 1290's `/1/role/InvestmentStrategy` yields exactly
`StrategyHeading, StrategyNarrativeTextBlock, StrategyPortfolioConcentration`. The map is a
standard-taxonomy fact (`oef:`/`rr:` element names, not filer extensions), so it is the same
for every filer.

**2. Verify it in CI against a partitioning filer.** A live test that, for 1290 (and Vanguard),
walks every role and asserts the hardcoded map assigns each of that role's concepts to that
role's own section key. That converts "filer-observed guesswork" into a claim the filings
themselves check every run — and it fails loudly the year the taxonomy adds an element.

> Checked and rejected: deriving the map at runtime from the SEC's published taxonomy.
> `https://xbrl.sec.gov/oef/2024q3/oef-2024q3.xsd` fetches (286 KB) but is **element
> declarations only** — no `presentationLink`, no `linkbaseRef`, and
> `oef-2024q3-presentation.xml` 404s. There is no authoritative concept→section file to fetch.

**3. Decide what an unmapped concept does.** It must not vanish. Either emit it under a
catch-all key or name it in a warning — a dropped concept is the exact inverted answer this
family keeps producing.

Scope note: the emitted shape changes. A Fidelity fund gains standard-keyed sections it did not
have. Decide whether the opaque `DisclosureDetail…` section stays alongside them (recommended —
it is the filing's own structure, and dropping it would lose anything the map misses).

---

## Two landmines found while probing — fix these regardless

**A. `RoleContent::series_id` assumes a role reaches at most one series. Fidelity's reach six.**
`absorb_presentation_link` does `entry.series_id.get_or_insert(series)` with the comment "a
per-fund role reaches exactly one series … the first wins". On this filing **every** role —
trust-level AND per-fund — lists all six series members
(`fmr_S000067930Member` … `fmr_S000075899Member`). So `series_id` is whichever appears first in
document order, which for `DisclosureDetailS000067930_…` need not be `S000067930`.

`parse_prospectus_text` prefers `content.series_id` over the role's group, so a fund's prose can
be keyed under **another fund's series**. Today that surfaces as a missed lookup (the section
carries the series from its role URI, which is right, so the two disagree and no text is found)
rather than as visible mis-attribution — which is worse, because it is silent and one change
away from becoming a wrong answer.

**Fix:** where the role URI names a series, that wins over any member reached from the role.
`series_in_role`/`fund_group_of` already extract it. Add a test with a role whose URI says one
series and whose locators list three.

**B. `is_line_item` lets typed domain members through.** It excludes names ending `Member`, but
Fidelity's fund domain members are `fmr_FidelityClimateActionFund`,
`fmr_FidelitySustainableUSEquityFund` — no suffix. They land in `line_items` on every role
(4 of the 5 "concepts" on each stub role above are these). Harmless today because no fact uses
those element names, so the instance lookup simply misses — but they pollute `line_items` and
would confuse any concept→section mapping built on it.

**Fix:** a locator is a member if the role's own `explicitMember`/domain arcs say so, or —
cheaper — if the element is a `*Domain`'s child in the arcs. Alternatively exclude locators
that appear as dimension members anywhere in the filing. Pick one and pin it with a test using
Fidelity's real names.

---

## Reproducing the probes

```bash
# The filing
UA="dapple you@example.com"
BASE=https://www.sec.gov/Archives/edgar/data/225322/000022532226000191
curl -sA "$UA" $BASE/index.json            # fmr-20260730_pre.xml (795 KB), filing12386_htm.xml
curl -sA "$UA" $BASE/fmr-20260730_pre.xml -o fid_pre.xml
```

Concepts per role — split on `<link:presentationLink`, take each block's locator fragments,
drop names ending `Member`/`Abstract`/`Table`/`Axis`/`Domain`/`LineItems`. The scratch script
used was `fid_items.nu`; it is a dozen lines and faster to rewrite than to find.

**Run live EDGAR tests a few at a time** — the whole `edgar_live_test.rs` file trips SEC's
~10 req/s limit and the failures then read as ordinary data bugs. See
`project-edgar-live-tests-are-rate-limited`.

## Current state (what you are starting from)

```
1290 (control)   funds=9  series=8   fragments=89  text=16
Vanguard         funds=12 series=12  fragments=0   text=95
Fidelity         funds=7  series=6   fragments=20  text=2    ← the 2 are trust-level fragments
```

Guarded by `fund_sections_works_across_three_fund_families`, whose Fidelity row is flagged
`per_fund_standard_keys: false` and asserts the starvation warning fires. **When this work
lands, flip that flag to `true`** — the chrome assertion should then hold for Fidelity too.
