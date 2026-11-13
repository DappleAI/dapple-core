# Plan — `price-the-mfp`: the money market guide

**Status:** planned, nothing built. First of three follow-on fund guides.

**Slug:** `price-the-mfp` — **provisional**, but the Rust side is already done.

✅ `edgar_fund_registry.rs` `FilerKind::redirect_guide()` returns `Some("price-the-mfp")`
for `MoneyMarketFund`, asserted by `money_market_wins_over_everything`. As of 2026-08-01
**all three** redirect arms are pinned by slug (the 13F and BDC arms were added by
`out_of_scope_kinds_name_their_guide`), so if this name is still being weighed, a later
rename fails a test rather than shipping in silence. Change the arm and its assertion
together.

> **Search by symbol, not by line.** Line numbers in this module move — the tail resolver
> shifted everything once already. Grep `redirect_guide` and
> `money_market_wins_over_everything`.

The name is not a label: `redirect_guide()` is **user-facing text**, printed by
`edgar fund` when an operator types an out-of-scope ticker ("this is a money market
fund, go to X"). A stale slug points them at a guide that does not exist. The directory
basename is also the guide slug (migration 73), so creating the directory fixes the
name — do that only once it is settled.

**Why this name.** The family's verb is what the *reader* does to the document. Here
the promise IS a number, $1.00 per share, and the fund files its own daily measurement
of it, so the reading act is an independent **re-pricing**. It also stops the guide
repeating its sibling's verb, which was flattening the family.

---

## Required reading before you start

This plan assumes the family's doctrine and names it by shorthand throughout. All paths
verified to exist 2026-08-01.

| read | for |
|---|---|
| `dapple-guides/public/dapple/retort-the-n-port/AGENTS.md` (42 kB) | **the direct predecessor and the worked reference.** The evidence-tier contract, the per-claim denominator rule, band-don't-split, computed promotion, and why a probe must be able to fail |
| `dapple-guides/public/dapple/retort-the-n-port/CONDUITS.md` | what the model is told on every prompt, and the banned-output vocabulary |
| `dapple-guides/public/dapple/retort-the-n-port/probes/` | the offline smoke-probe harness this guide copies |
| `dapple-guides/public/dapple/run-the-10k/AGENTS.md` | misfiling vs violation, scene / cast / prominence, the item-header convention |
| `dapple-guides/public/dapple/weigh-the-m-and-a/AGENTS.md` | the invariants. **1 (symmetry) INVERTS here**; 7 (never characterize the distance) and 8 (rank nothing) survive. ⚠️ its three top-level region sections are a known mistake — do not copy them |
| `dapple-prompt/docs/plans/well-run-the-10-k-and-the-compressed-popcorn.md` | the four-round build record of `retort-the-n-port`, including the round where two of three probes silently could not fail |
| `dapple-prompt/CLAUDE.md`, `crates/dapple-prompt-flow/AGENTS.md` | the engine side, the sql-composer rules, and the **all shell commands go through `nu -c`** rule |

Two house rules that are easy to violate on day one:

- **No em-dashes or en-dashes in rendered guide content.** They read as an AI tell.
- **Never `git commit`.** Working-tree edits only; stage only when asked. Guide
  sync and ingest belong to Kenny.

Sibling plans: `guide-mark-the-bdc.md`, `guide-mind-the-13f.md`.

---

## Why this guide exists, and why it is not a branch of `retort-the-n-port`

Money market funds are **exempt from N-PORT** and file **N-MFP3** instead. That is
not a variant of the same form; it is a different schema with a different cadence,
a different portfolio, and a different question. The form-anchored scope rule that
`retort-the-n-port` established already routes them out automatically, and
`classify_filer` puts N-MFP **first in precedence** precisely so an MMF trust that
also files N-CSR can never fall through to "open-end fund".

### The evidentiary position — and it is the strongest in the family

| Guide | The claim | Who authored the evidence |
|---|---|---|
| `run-the-10k` | the subject's narrative | the subject, over time |
| `weigh-the-m-and-a` | neither; two peers | both subjects, symmetric |
| `retort-the-n-port` | the fund's stated strategy | **the holdings**, who did not know it would be read this way |
| **`price-the-mfp`** | **a number: $1.00 per share** | **the fund itself, daily, to four decimals** |

The other three guides test prose against records. Here **the promise is a price**,
and the fund files the market-based measurement of that price every business day.
The claim and its test are the same quantity, and the test is filed by the party
making the claim, at daily resolution, in a machine-readable field.

That collapses the usual tier problem rather than deepening it. A claim tested only
by its claimant is normally the weakest case in the family (`retort-the-n-port`
renders "Tested against the fund's own account only" for exactly that). It is the
**strongest** case here for a specific reason: the fund is not supplying inputs to a
conclusion we then draw, it is supplying **the direct measurement of its own headline
claim**, on a form whose sole purpose is to publish that measurement. Say that
explicitly in `AGENTS.md`, or a future author ports the same-source rule across and
suppresses the guide's best material.

Everything else in the guide is a second character: the **buffer** that has to hold
the promise up, the **shareholder base** that can pull it down, and the **sponsor**
who has stepped in before.

---

## Verified findings (live, 2026-08-01)

All measured through `curl` with `DAPPLE_EDGAR_USER_AGENT`. `WebFetch` remains 403'd
by sec.gov.

### Scale and cadence — the universe is small enough to read whole

From `full-index/2026/QTR2/form.idx`:

| measure | value |
|---|---|
| N-MFP3 filings in one quarter | **960** |
| distinct filer CIKs (trusts) | **176** |
| filings per month | 318 / 320 / 322 |
| trusts filing exactly 3 per quarter (one series) | 125 of 176 |
| largest filer | Federated Hermes Money Market Obligations Trust, **42/quarter = 14 series** |

**One filing is one series for one month.** `seriesLevelInfo` and `seriesId` each
appear exactly once per document. So the N-PORT trap survives intact and gets worse:
a trust emits up to 14 filings *in the same month*, and only `seriesId` + `reportDate`
yields a clean sequence.

**~320 filings per month is the whole industry.** This matters more than it looks:
`retort-the-n-port` had to **defer** its cross-fund late-filing base rate because the
only source was a 418 MB DERA quarterly zip. Here the entire month is ~320 small XML
documents, so a genuine cross-fund base rate ("this fund's weekly liquid assets sat at
32%; 14 of 318 funds filing that month were lower") is computable from the primary
filings. That is a capability the sibling guide does not have, and it is the
difference between an asserted qualifier and a measured one.

### Filing deadline — measured, and the period end is not the month end

355 Federated Hermes N-MFP3 filings, lag = filing date − report date:

| lag (calendar days) | filings |
|---|---|
| 5 | 1 |
| **7** | **260** |
| 8 | 57 |
| 10 | 37 |

Median 7, max 10 — consistent with a five-business-day rule. Report dates observed:
`2026-06-30, 2026-05-29, 2026-04-30, 2026-03-31, 2026-02-27, 2025-12-31, 2025-11-28`.

⚠️ **The period end is the last BUSINESS day of the month, not the calendar month
end.** A guide that computes month ends arithmetically mis-keys the series on
February, May and November. Key on the filed `reportDate`, never on a derived date.

### The schema — 130 distinct elements in a 50 KB document

Full vocabulary captured in this round. The load-bearing groups:

**Identity and category.** `seriesId`, `nameOfSeries`, `totalShareClassesInSeries`,
`moneyMarketFundCategory`, `govMoneyMrktFundFlag`, `fundRetailMoneyMarketFlag`,
`seekStablePricePerShare`, `stablePricePerShare`, `masterFundFlag`, `feederFundFlag`,
`liquidityFeeFundApplyFlag`, `finalFilingFlag`.

**Five daily time series**, each a list of dated points: market-based NAV per share
(`dailyNetAssetValuePerShareSeries` / `...Class`), shareholder flows
(`dailyGrossSubscriptions`, `dailyGrossRedemptions`), seven-day gross yield
(`sevenDayGrossYield`), seven-day net yield (`sevenDayNetYield`), and daily/weekly
liquid assets by value and percentage (`liquidAssetsDetails`).

⚠️ **The daily-flow wrapper element is MISSPELLED IN THE SCHEMA: `dialyShareholderFlowReported`**
(sic — "dialy", not "daily"). Verified live, 16 occurrences in one filing, sitting
directly beside a correctly-spelled `dailyShareHolderFlowDate` — which also has an
inconsistent capital `H`. The typo is on the **wrapper**, so a parser that does not
match it verbatim never descends into the daily subscription and redemption records at
all, and gets an empty flow series with no error. Match the string exactly; do not
"correct" it. There is also a fund-level `monthlyShareholderFlowReported`, spelled
correctly, which is a different element.

**Per-position schedule.** `nameOfIssuer`, `titleOfIssuer`, `CUSIPMember`, `LEIID`,
`ISINId`, `investmentCategory`, `yieldOfTheSecurityAsOfReportingDate`,
`percentageOfMoneyMarketFundNetAssets`, `finalLegalInvestmentMaturityDate`,
`investmentMaturityDateWAM`, `investmentMaturityDateWAL`, plus the flags
`illiquidSecurityFlag`, `dailyLiquidAssetSecurityFlag`, `weeklyLiquidAssetSecurityFlag`,
`securityCategorizedAtLevel3Flag`, `securityEligibilityFlag`, `securityDemandFeatureFlag`,
`securityGuaranteeFlag`, `securityEnhancementsFlag`.

**Repo collateral, itemized inside the same filing.** `repurchaseAgreement` with
`repurchaseAgreementClearedFlag` / `OpenFlag` / `TripartyFlag`, then `collateralIssuers`
carrying `nameOfCollateralIssuer`, `CUSIPMember`, `coupon`, `maturityDate`,
`principalAmountToTheNearestCent`, `valueOfCollateralToTheNearestCent`,
`ctgryInvestmentsRprsntsCollateral`.

**Sponsor support.** `includingValueOfAnySponsorSupport` /
`excludingValueOfAnySponsorSupport` per position; `personPayForFundFlag` and
`nameOfPersonDescExpensePay` at fund level.

**Shareholder base.** `beneficialRecordOwnerCategory` → `beneficialRecordOwnerCategoryType`,
`percentOutstandingSharesRecord`, `percentOutstandingSharesBeneficial`.

**Ratings.** `nameOfNRSRO`, `rating`, `assigningNRSRORating` per security.

**Service providers.** `adviser` + `adviserFileNumber`, `administrator`, `transferAgent`,
`indpPubAccountant`.

### Parser traps — five, all verified on live documents

**1. The daily series elements are SELF-NESTED.** The wrapper and the value carry
the same name:

```xml
<dailyNetAssetValuePerShareSeries>
  <dailyNetAssetValuePerShareSeries>1.0000</dailyNetAssetValuePerShareSeries>
  <dailyNetAssetValuePerShareDateSeries>2026-06-09</dailyNetAssetValuePerShareDateSeries>
</dailyNetAssetValuePerShareSeries>
```

Any "first descendant named X" lookup returns the wrapper. Same class of defect as
N-PORT's `descRefInstrmnt` carrying its own `identifiers`, and it must be depth-tracked
the same way.

**2. Percentages are FRACTIONS here, and N-PORT's are PERCENTAGES.** Verified on live
values: `percentageDailyLiquidAssets = 1.0000` means 100%,
`percentageOfMoneyMarketFundNetAssets = .2478` means 24.78%,
`yieldOfTheSecurityAsOfReportingDate = .0369` means 3.69%,
`percentOutstandingSharesRecord = .9960` means 99.60%.

`retort-the-n-port` documents `pctVal` as a percentage that sums to ~100 and warns
against multiplying by 100. **N-MFP3 is the exact inverse, and both fields are spelled
`percentage*`.** A script ported across without checking reports a fund holding 100%
weekly liquid assets as holding 1%. This is a first-class trap and belongs in
`AGENTS.md` before a line of guide script is written.

**3. The series lengths differ WITHIN one filing, and the difference is informative.**
A Federated fund launched mid-month filed 16 dated points for NAV, flows, liquidity and
gross yield (2026-06-08 → 06-30) but only **11** for seven-day net yield
(2026-06-15 → 06-30) — because a seven-day yield cannot exist until the fund is seven
days old. **Zip the series by DATE, never by index.** Zipping by index misaligns net
yield against NAV by five days and produces a plausible, wrong chart.

**4. `stablePricePerShare` uses 0.0000 as "not applicable", and is not always 1.00.**
Morgan Stanley's Prime Portfolio (floating NAV) files `seekStablePricePerShare = N`
with `stablePricePerShare = 0.0000`. State Street's Prime Money Market ETF prices at
**$100**, with daily NAVs from 100.0006 to 100.2823. A guide that hardcodes $1.00
breaks on both.

**5. Sentinels and misnamed fields.** `LEIID` is the literal string `N/A` on
collateral rows, exactly as in N-PORT. And `nameOfPersonDescExpensePay` holds a
**dollar amount** (`37393.00`), not a name — a guide that renders it as a name prints
a number as the sponsor.

### The category flags contradict the category, routinely

| series | `moneyMarketFundCategory` | `govMoneyMrktFundFlag` |
|---|---|---|
| UBS Select Government Institutional | Government | **N** |
| Goldman Sachs Central Government Fund | Government | **N** |
| Schwab Government Money Fund | Government | **N** |
| State Street Institutional U.S. Government | Government | **N** |
| Stablecoin Reserves Fund | Government | Y |

Fourteen of fifteen sampled funds disagree with themselves on this pair. **Do not
derive either from the other, and do not report the flag as "is this a government
fund".** What the flag actually means was not established in this round; report the
`moneyMarketFundCategory` and leave the flag alone until its item text is read from
the form instructions.

### The asset identity closes TO THE CENT

Morgan Stanley Prime Portfolio, 2026-06-30:

```
totalValuePortfolioSecurities  13,883,496,745.72
cash                              582,341,204.61
totalValueOtherAssets              29,592,910.46
totalValueLiabilities        (     64,022,002.83 )
                             ------------------
                               14,431,408,857.96
netAssetOfSeries               14,431,408,857.96   gap 0.00
```

Exact on every fund tested once `cash` is included; omitting `cash` produced apparent
gaps up to $10.18 bn, which were the cash balances. This is the direct analogue of
`run-the-10k`'s scene check (segment revenue must sum to consolidated) and of
`retort-the-n-port`'s NAV partition — the check that repo memory records as having
caught two defects everything else passed. **It is exact here, so assert equality to
the cent and let a non-zero gap be a finding.**

### A misfiling found in the first five filings looked at

Morgan Stanley Institutional Liquidity Funds, **Prime Portfolio** (`S000004151`), a
$14.4 bn floating-NAV institutional prime fund, report date 2026-06-30, accession
`0001410368-26-069212`:

```
2026-06-11   1.0000
2026-06-12  -0.0144      <-- series-level daily NAV per share
2026-06-15   1.0000
```

It is the only negative number anywhere in the document. The class-level series for
the same month runs 0.9996 to 1.0001 and contains no negative.

This is a **misfiling** in the family's established vocabulary — our detection, by
arithmetic on the filed record — and never a **violation**, which is reserved for
something the SEC itself flagged. The rendered language states the fact and withholds
the motive, per `run-the-10k` doctrine. It is also the guide's own argument for
existing: a quarterly snapshot could not surface it, because the value is only visible
in a **daily** series.

---

## Data model

Reuse the span/cue vocabulary, and note the ratio has inverted from the sibling guide.

| Filing carries | Kind | Resolution |
|---|---|---|
| daily NAV, flows, yields, liquid assets | **span/cue per business day** | ~21 per filing |
| the position schedule | **cue** at `reportDate` | 1 per filing |
| category, flags, service providers | fund-level state | 1 per filing |

`retort-the-n-port` had to warn that performance resolution and drift resolution
differ (monthly returns, quarterly holdings). **Here the gap is wider, not narrower**:
daily measurements against a monthly portfolio snapshot. State the portfolio-cue
interval alongside every drift statement, exactly as stage 15 does there.

Three years of one series = 36 filings at ~50 KB to a few hundred KB. Trivial next to
N-PORT's 3 MB × N.

---

## Engine work (`dapple-prompt-flow`)

### E1. `edgar nmfp <series-id|cik> [--periods N]`

New `src/commands/edgar_nmfp.rs` + `src/edgar_nmfp.rs`. Documents are small, so
`roxmltree` is appropriate — this is the `edgar_ncen.rs` shape, not the streaming
`edgar_nport.rs` shape. Must handle all five traps above, with unit tests on captured
fixtures for each:

- depth-tracked self-nested series elements
- a `FRACTION` newtype or an explicit `as_percent()` at the boundary, so the unit can
  never be lost in a later refactor
- date-keyed series zip, asserted against the 16-vs-11 fixture
- `stablePricePerShare == 0.0` normalized to `None` when `seekStablePricePerShare == N`
- `N/A` normalized to `None`, and `nameOfPersonDescExpensePay` typed as an amount

Return shape: fund-level record + the five dated series + positions + repo collateral
+ shareholder-base categories + the computed asset identity with its gap.

### E2. `edgar nmfp-universe --month YYYY-MM`

The capability the sibling guide could not afford. Walks that month's N-MFP3 filings
(~320) and returns the fund-level fields only — no positions. Feeds every cross-fund
base rate. Cache by month; the set is immutable once filed.

⚠️ **Do not let this become a portfolio pull.** 320 filings × positions is a different
cost class and nothing in the guide needs it.

### E3. Deadline table row

`src/edgar_deadlines.rs` gains an N-MFP3 row: five business days after the period end.
The measured lag distribution above corroborates it. Follow the existing rule — a row
whose regime is unconfirmed is marked unconfirmed and **no misfiling is rendered
against it**, because a wrong deadline manufactures misfilings out of compliant funds.

### E4. Classifier housekeeping

`classify_filer` already handles MMFs correctly and first. Nothing to change. Register
the new commands in `src/commands/mod.rs`, add them to the allowlist in
`src/engine/security.rs`, update the parent help in `src/commands/edgar.rs`, add live
tests to `tests/edgar_live_test.rs`, and **rebuild `dapple-prompt-api` as well as
`dapple-prompt-flow`** — guide script parse happens API-side.

---

## Guide structure (`dapple-guides/public/dapple/price-the-mfp/`)

Same layout and the same generate → finalize → refine triad as the siblings. Copy
`fund-primer.md` and add a money-market primer covering: the stable-value convention,
what a seven-day yield is, daily and weekly liquid assets, why a tax-exempt fund's
buffer profile differs, what a sponsor support event is, and what a liquidity fee is.
`CONDUITS.md` is required at root.

| # | Stage | Notes |
|---|---|---|
| 1 | **Pull the Fund Record** | auto; classify, then N-MFP3 × N months + 485BPOS + N-CSR + N-CEN + 497s |
| 2 | **Pull the Month's Universe** | auto; E2, fund-level only. Every later base rate reads this |
| 3 | Review the Promise | the prospectus objective, the stated price, the category and what it obliges |
| 4 | **The Price It Filed About Itself** | the daily NAV series against the promised price. The centrepiece |
| 5 | **The Buffer** | DLA/WLA daily series, distance to the applicable threshold, composition |
| 6 | **What They Hold** | positions, maturity ladder (WAM/WAL), issuer concentration, ratings, Level 3, illiquid |
| 7 | **What Sits Behind the Repo** | collateral look-through, in-filing. Category, issuer, haircut |
| 8 | **Who Owns the Fund** | shareholder-base concentration; the run risk, filed |
| 9 | **Who Has Stepped In** | sponsor support per position, expense payments, waivers |
| 10 | The Fund's Own Fine Print | N-CEN booleans, `finalFilingFlag`, filing lag as misfiling with a **measured** base rate |
| 11 | **Weigh the Promise Against the Record** | the payoff; verdicts per claim |
| 12 | Review the Cut They Take | fee table, waiver cliff, gross-vs-net seven-day yield spread |
| 13 | **Pull the Current Month** ⚑ | fork point |
| 14 | Layer the Changes | position diffs, flow episodes, buffer movements, category or fee-flag changes |
| 15 | Build the Story | connect (Opus) → synthesize (Sonnet) → HTML + PDF |

### Doctrine carried forward, unchanged

- **Scene / cast.** The fund is the **scene**; positions are its contents. A position
  becomes a character only by **computed promotion** with `reason` always carried. A
  government fund holding six Treasury repos has almost no cast, **and that is true of
  it** — say so rather than letting a thin cast read as a shortfall.
- **Band, do not split.** Region is an embedded four-backtick `admonish orange|teal`
  band inside a claim track. Never a top-level section. `retort-the-n-port` is the
  worked reference; `weigh-the-m-and-a`'s three region sections are the mistake.
- **Every aggregate states its denominator**, and the denominator is never the whole
  portfolio.
- **Misfiling vs violation** stays a hard two-flag distinction.
- **Empty-band doctrine**: a section with nothing in it is omitted with its heading,
  not rendered hollow. A plain government fund should emit no sponsor section at all.
- **No silent caps.** Anything bounded says what was dropped.

### One new rule this guide needs

**A threshold is a category-keyed, versioned table, and an unconfirmed row renders
nothing.** Schwab Municipal Money Fund files DLA at 0.1839 — far below the level that
applies to a prime fund — because a tax-exempt fund's obligations differ. Applying one
fund's threshold to another **manufactures a violation**, which is the worst failure
this family can commit. Same shape as the deadline table, same discipline: ship rows
marked unconfirmed and render no flag against them.

---

## Landmines

1. **The units.** Fractions here, percentages in N-PORT, both spelled `percentage*`.
   Pin it with a test that fails if a value is multiplied by 100.
2. **The thresholds.** Category-keyed, reform-versioned, and unverified in this round.
   Do not flag proximity to a limit until the table is confirmed against the form
   instructions. Report the level and let the reader hold it against the primer.
3. **The self-nesting.** Verified; a naive extractor produced a nonsense number from
   it during this very round of research, which is the best evidence it bites.
4. **Series lengths differ within a filing.** Zip by date.
5. **`stablePricePerShare = 0.0000`** is "not applicable", not a price.
6. **176 trusts is the whole universe** — attractive, and therefore a temptation to
   build a market-wide product. Scope stays one fund; E2 exists to give one fund's
   numbers a denominator, not to become a screener.

---

## Verification

1. `cargo test -p dapple-prompt-flow` with a fixture per trap; `cargo clippy -- -D warnings`.
2. **Unit assertion**: a filed `.2478` renders `24.78%`, never `0.2478%` or `24.78`
   with the wrong sign of scaling. Revert-to-fail it.
3. **Identity assertion**: securities + cash + other − liabilities equals
   `netAssetOfSeries` to the cent on the captured fixture, and a seeded corruption
   fails it.
4. **Series alignment**: the 16-vs-11 fixture must align net yield to the correct
   dates; an index-zip must fail the test.
5. **The misfiling probe**: the Prime Portfolio fixture must surface the −0.0144 point,
   and the rendered language must contain no motive word.
6. Every probe must be **able to fail** — counters aggregated, `exit 1` present,
   revert-to-fail demonstrated on at least one new assertion per section. Two of three
   probes in the sibling guide silently could not fail; do not repeat it.
7. `dapple guide validate` at **0 warnings** before any ingest (embedded nu 0.109 is
   stricter than system nu); `nu-check` every script; the `do {}`-wrapped smoke pattern
   that shadows backend commands by name so guards **run** rather than merely parse.
8. End-to-end on two funds of opposite shape: a government fund with six repo positions
   (thin cast, thick collateral) and a prime institutional fund with a floating NAV.
9. Ingest via `PATCH /api/guides/{path_id}/ingest` with PAT-Bearer, **not** POST.

---

## Deliberately not built

- **No prediction of a break.** The guide reports the filed price, the buffer and the
  ownership concentration. It never forecasts.
- **No market-wide screener.** E2 is a denominator source, not a product.
- **No threshold flag against an unconfirmed rule.** See landmine 2.
- **No interpretation of `govMoneyMrktFundFlag`** until its item text is read.
- **No repo counterparty credit view.** The collateral is itemized and that is what
  gets reported; judging a counterparty is not a filed fact.
