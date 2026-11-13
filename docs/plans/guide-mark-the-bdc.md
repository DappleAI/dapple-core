# Plan — `mark-the-bdc`: the BDC guide

**Status:** **ENGINE BUILT AND LIVE-VERIFIED 2026-08-01. GUIDE BUILT 2026-08-01. STORY MODEL
REBUILT 2026-08-01 (round 2). Offline-verified, not yet ingested.** Second of three follow-on
fund guides.

> ## Round 2, 2026-08-01: the scene and the tracks were inherited, and both were wrong
>
> Kenny: *"we based this guide off of the others and likely don't have the scene (the BDC itself)
> and the tracks quite right on the first try."* Correct on both, and one of the two was a
> defect rather than a design preference.
>
> **There were no tracks.** `connect` asked the model to FIND cross-section connections and
> return them as freeform prose in `skeletons`. Nothing validated it and nothing could drop an
> invented connection. And `synthesize` handed that string to `write-arc` and **to nothing else**,
> so the guide's only cross-section layer reached ONE of four write calls: three quarters of the
> report was written by a model that could not know two of its findings were the same finding.
> Built, wired and starved, which is the same shape `run-the-10k` found in seven of nine generators.
>
> **The scene was a stat block, and a single frame of one** (twenty counts off `$marks | get -i 0`,
> rendered as an eleven-row table). The pull returns up to six period ends and it used one.
>
> | | before | after |
> |---|---|---|
> | scene | 20 counts at one period end | the BOOK: the same measures across every period end, the instrument mix, and the turnover between them (3 tables) |
> | track root | none | one of NINE mechanisms of the corporate form, computed deterministically, kept by the operator |
> | `connect`'s job | invent the through-lines as prose from a 160-line chronology | pick one of nine keys per finding, `id \| key`, unknown id dropped, unknown key dropped |
> | write calls seeing the cross-section layer | 1 of 4 | 4 of 5 (`write-short-story` is exempt: it must stand alone) |
> | stages | 14 | 15 (What Could Go Wrong, stage 12, pre-fork) |
>
> **"What Could Go Wrong" is what the track layer renders as**, not a section bolted beside it,
> and it answers Kenny's *"there are probably some inherent risks in a BDC that warrant a What
> Could Go Wrong section you wouldn't see on a normal company."* It is **not Item 1A read back**:
> every row is two facts side by side, a mechanical property of the form and this filer's own
> filed numbers against it. Every row carries a `trigger` (`form` = true of every BDC by
> construction; `measured` = this filer's figures fired it), which is the misfiling-versus-violation
> rule one level up. The nine: `self` `retain` `base` `earned` `cash` `ratchet` `call`
> `concentration` `lag`. What did NOT fire is reported.
>
> **Conduit load (Kenny's third item) got better, not worse, despite one more call.** The story
> stage went 5 LLM calls to 6, `connect` dropped from 8192 to 4096 max-tokens, and the hard part
> (invent the through-lines) was replaced by a checkable one (choose from nine). The section list
> `connect` receives lost the raw scene JSON blob, the denominators and the operator notes.
>
> Probes: 18 static arms + 11 render arms, all passing, and **nine lesions were applied and each
> produced its intended failure**. New coverage: the book derivation RUNS against a stubbed
> `edgar portfolio` (borrowers, mix, turnover), the mechanism vocabulary is closed at nine, the
> assignment parser refuses an unknown id / unknown key / duplicate filing, and every scene table
> is asserted separately (a scene missing two of three tables passed a check that looked for one).
>
> Four defects the round found that were not on the list: `write-record` and `write-verdict` were
> never receiving the tracks; `class_mix` was read off `fn_marks` rather than off the book, so the
> instrument table silently never rendered; `"Class A units"` fell to `unclassified` because the
> equity spellings were too short (an equity stake in a borrower is usually the residue of a
> workout, so it is the last thing that should vanish into a bucket); and the facts table and the
> book table derived their period counts from different records and could disagree on one page.

> ## Guide pass, 2026-08-01
>
> `dapple-guides/public/dapple/mark-the-bdc/` exists: 44 files, 14 stages, 49 value slots,
> 17 task scripts, 10 prompts, 81 fluent keys, 2 probes. Both probes pass and both were
> demonstrated to FAIL under six separate lesions. Nothing is committed and nothing is ingested.
>
> **Three more measurements changed the build, and all three are in `AGENTS.md` under "traps":**
>
> | measured | consequence |
> |---|---|
> | `InvestmentCompanySeniorSecurityIndebtednessAssetCoverageRatio` appears 5x on Ares and **every occurrence is dimensioned**. The 3 that read as undimensioned are dated 2019-06-20/21, the APPROVAL dates, and carry the statutory thresholds (2 and 1.50) rather than the company's own 1.89 | `edgar bdc` returns null coverage for this filer, and would otherwise surface two phantom "periods". The guide filters a period carrying only a ratio, reports the ratio with NO threshold comparison, and computes liabilities-to-net-assets labelled as its own |
> | non-accrual is tagged **undimensioned and present**, as `arcc:...PercentOfCost` = **0.018** (1.8%) | a tagged percent is a decimal fraction. Conversion lives in one named helper; a probe arm asserts 0.018 renders as 1.8%. Also: the `arcc:` prefix means these are FILER EXTENSIONS, so absence is the common case, not a gap |
> | a multi-line boolean chain fails in nushell **both ways round**, and `nu-check` accepts both | three scripts shipped with a leading `or` and parsed clean. Caught by the RENDER probe, which is the only guard that executes anything. Written as `[..] \| any {..}` now; probe arm 14 checks both directions |
>
> A fourth, smaller one worth keeping: `point(s)` inside a `$"..."` is a subexpression call to a
> command named `s`. It crashed the render probe twice and was invisible until an assertion was
> deliberately lesioned, because the FAILURE PATH had never run. Probe arm 15 checks it.

> ## ⚠️ Read this box before anything below it
>
> The engine pass measured several claims in this plan and **three of them were wrong**. The
> sections still read as originally written, with corrections marked inline. The build record,
> including the gate's full output, is in
> [`well-run-the-10-k-and-the-compressed-popcorn.md`](./well-run-the-10-k-and-the-compressed-popcorn.md)
> → ROUND 8.
>
> | this plan said | measured |
> |---|---|
> | adding `InvestmentIdentifierAxis` to `edgar_xbrl_dims` is "a list entry, not a new subsystem" | it is a **typed** dimension; that module reads only `explicitMember` and sees zero positions. A new module, `src/edgar_portfolio.rs` |
> | report the filer's own `InvestmentCompanyNumberOfBorrowers` | it counts a JOINT VENTURE's borrowers — 39 against Ares' 1,446 positions. **No borrower count is reported from any source** |
> | multi-currency lands on the schedule's values | every fair value is USD; the instrument currencies are on the **principal** |
> | the cross-holder MARK comparison is "the guide's most valuable idea" | half of it is. Borrower overlap is real (85 borrowers, up to 18.65% of NAV); **instrument-level mark comparison is not reachable** — see the gate result below |
>
> **What shipped:** `edgar portfolio` (the schedule reader), `edgar bdc` (fund-level '40 Act
> facts), the classifier split, and the gate as a live test. 528 unit tests, clippy clean,
> `dapple-prompt-api` rebuilt. Nothing committed.

**Slug:** `mark-the-bdc` — settled. A BDC's central question is whether its own marks
are honest, "mark" is the industry's own verb, and it bridges the way the family's best
names do: the BDC *marks* its portfolio, the reader *marks* its work. The directory
basename is also the guide slug (migration 73), so creating the directory fixes it.

**⚠️ There is no BDC slug in code at all, and adding one is not a new line — it is a
split.** `edgar_fund_registry.rs` `FilerKind::redirect_guide()` sends BDCs to
`run-the-10k` today, because `BusinessDevelopmentCompany` **shares an arm** with
`OperatingCompany`. Building this guide means separating them deliberately.

✅ **The split is now GUARDED — that changed on 2026-08-01 and it is in your favour.**
When this plan was written no test named the BDC slug, so the arm could be changed, left
stale or mistyped in silence. `out_of_scope_kinds_name_their_guide` now pins **all three**
`redirect_guide` arms by slug, the BDC one included:

```rust
let bdc = classify_filer(&forms(&["10-K", "N-54A"]));
assert_eq!(bdc.kind, FilerKind::BusinessDevelopmentCompany);
assert_eq!(bdc.redirect_guide(), Some("run-the-10k"));
```

So splitting the arm to `mark-the-bdc` **fails that assertion loudly**, which is exactly
what you want: the test is the checklist item reminding you the redirect is user-facing
text (it is what `edgar fund` prints when an operator types a BDC ticker) and that both
halves of the split need deciding. Change the arm and the assertion together, and leave
the `OperatingCompany` assertion alone — it must keep saying `run-the-10k`.

> **Search by symbol, not by line.** Line numbers in this module move — the tail resolver
> shifted everything once already. Grep `redirect_guide` and
> `out_of_scope_kinds_name_their_guide`.

---

## Required reading before you start

This plan assumes the family's doctrine and names it by shorthand throughout. All paths
verified to exist 2026-08-01.

| read | for |
|---|---|
| `dapple-guides/public/dapple/run-the-10k/AGENTS.md` | **the closest data-layer relative** — a BDC files a 10-K, so most of that guide's engine surface applies. Also misfiling vs violation, scene / cast / prominence, the item-header convention |
| `dapple-guides/public/dapple/retort-the-n-port/AGENTS.md` (42 kB) | **the frame this guide inverts.** The evidence-tier contract, the per-claim denominator rule, band-don't-split, computed promotion, why a probe must be able to fail |
| `dapple-guides/public/dapple/retort-the-n-port/CONDUITS.md` | what the model is told on every prompt, and the banned-output vocabulary |
| `dapple-guides/public/dapple/retort-the-n-port/probes/` | the offline smoke-probe harness this guide copies |
| `dapple-guides/public/dapple/weigh-the-m-and-a/AGENTS.md` | the invariants. **1 (symmetry) INVERTS here**; 7 (never characterize the distance) and 8 (rank nothing) survive, and 8 governs the cross-BDC comparison. ⚠️ its three top-level region sections are a known mistake — do not copy them. Its **currency hazard** is directly relevant, since Blue Owl reports four currencies in one schedule |
| `dapple-prompt/docs/plans/well-run-the-10-k-and-the-compressed-popcorn.md` | the four-round build record of `retort-the-n-port`, including the round where two of three probes silently could not fail |
| `dapple-prompt/CLAUDE.md`, `crates/dapple-prompt-flow/AGENTS.md` | the engine side, the sql-composer rules, and the **all shell commands go through `nu -c`** rule |

Two house rules that are easy to violate on day one:

- **No em-dashes or en-dashes in rendered guide content.** They read as an AI tell.
- **Never `git commit`.** Working-tree edits only; stage only when asked. Guide
  sync and ingest belong to Kenny.

Sibling plans: `guide-price-the-mfp.md`, `guide-mind-the-13f.md`.

---

## Why this guide exists, and why it is not part of `run-the-10k`

A business development company files a **10-K**. EDGAR agrees: `entityType` is
`operating` and `sic` is empty for all three BDCs sampled. `run-the-10k` will happily
read one and produce a report about an operating company.

That report would be wrong in the way that matters. A BDC is not a company that has a
portfolio; it **is** a portfolio, regulated under the Investment Company Act, wearing
a 10-K because it elected to be. Its revenue is interest and fees on loans it holds,
its balance sheet is the positions themselves, and the number that decides whether
anything else is true is a **fair-value mark on a private borrower that nobody else
publishes**.

Repo memory records this as the guide "most at risk of being built as a special case
inside `run-the-10k`". The risk is worse than that: the shipped code already routes
BDCs there.

### The evidentiary position — `retort-the-n-port` inverted

| Guide | The check | Who authored the evidence |
|---|---|---|
| `retort-the-n-port` | the fund's claim against its holdings' own filings | **the holdings** — public companies who did not know it would be read this way |
| **`mark-the-bdc`** | the BDC's marks against **what?** | **almost nobody** — the borrowers are private and file nothing |

`retort-the-n-port`'s whole strength was evidence the claimant did not write. A BDC's
portfolio companies are private middle-market borrowers; that is the entire point of a
BDC. **There is no Tier-A evidence for the portfolio**, and pretending otherwise is
the failure mode to design against.

So the guide's spine has to be different, and there are exactly three honest sources of
a second opinion:

1. **The BDC's own two numbers for the same asset.** Amortized cost and fair value are
   both filed, per position. The spread between them is the mark, stated by
   subtraction, and it needs no outside party.
2. **Another BDC's mark on the same borrower.** Club deals and syndicated unitranches
   mean one borrower can sit in several BDCs' schedules. A different BDC is a
   *different party* marking the same asset, which is as close to Tier A as this
   subject gets. **This is the guide's most valuable idea and it is not yet
   measured — see the gate below.**
3. **The auditor and the SEC.** UPLOAD/CORRESP comment letters, 8-K Item 4.01 and
   4.02, and the critical audit matter in the opinion, which for a BDC is almost
   always the Level 3 valuation itself.

State this inversion in `AGENTS.md` in the same breath as the frame, or an author
ports `retort`'s look-through and finds nothing to look through to.

---

## Verified findings (live, 2026-08-01)

### The portfolio IS machine-readable, through 10-K XBRL

Ares Capital (CIK 1287750), 10-K `0001287750-26-000006` for FY2025:

| measure | value |
|---|---|
| `FilingSummary.xml` reports | 96 |
| `CONSOLIDATED SCHEDULE OF INVESTMENTS` | `R6.htm`, **11.8 MB**, 38,650 table rows |
| `InvestmentIdentifierAxis` occurrences | **2,617** |
| distinct position identifiers extracted | **2,510** |
| `Spread` occurrences | 1,378 |
| Coupon, PIK | tagged |
| **Total investments as % of net assets** | **205.93%** (2024: 200.09%) |

Blue Owl Capital Corp (CIK 1655888), 10-K `0001655888-26-000010`: `R6.htm` 8.7 MB,
13,462 rows, `InvestmentIdentifierAxis` **996** occurrences.

**Every position is a tagged XBRL dimension member.** A BDC's schedule of investments
is as structured as an N-PORT; it simply arrives through the 10-K instance instead of
a fund form. Sample identifiers are clean and position-level:

```
15484880 Canada Inc. and 15484910 Canada Inc., First lien senior secured loan
15484880 Canada Inc. and 15484910 Canada Inc., First lien senior secured revolving loan
15484880 Canada Inc. and 15484910 Canada Inc., Class A2 shares
Steward Partners Global Advisory, LLC ..., First lien senior secured loan
```

Note the shape: **one borrower, several instruments.** The identifier is
`<borrower>, <instrument>`, so borrower-level aggregation requires splitting, and
"number of positions" and "number of borrowers" are different counts that must never
be conflated.

### The rendered R-file is filer-specific; the instance is not

The two BDCs render the same axis completely differently. An extractor written against
Ares' layout returned **2,510** identifiers from Ares and **0** from Blue Owl — an
artifact of the layout, not an absence, since Blue Owl carries the axis 996 times.

**Conclusion: go to the XBRL instance, not the rendered R-file.** ✅ Still right, and this is
what shipped.

> ⚠️ **CORRECTED.** The rest of this paragraph said `src/edgar_xbrl_dims.rs` already reads
> dimensional facts out of the instance, so "adding `InvestmentIdentifierAxis` is a list
> entry, not a new subsystem". **It is not.** That module filters contexts on
> `explicitMember` — a member QName the filer declared in its taxonomy, drawn from a closed
> set. `InvestmentIdentifierAxis` is a **typed** dimension whose value is free text:
>
> ```xml
> <xbrldi:typedMember dimension="us-gaap:InvestmentIdentifierAxis">
>   <us-gaap:InvestmentIdentifierAxis.domain>ACP Avenu Midco LLC, First lien senior secured loan</...>
> </xbrldi:typedMember>
> ```
>
> Ares carries 7,406 `typedMember` against 4,548 `explicitMember`, and `edgar_xbrl_dims`
> sees none of the former. Its other contract — one scene per fact plus a coherent-set
> rollup against a consolidated total — is also the wrong shape for 1,446 positions.
> The reader is `src/edgar_portfolio.rs`.
>
> **Also corrected: one 10-K carries TWO comparative schedules.** Ares FY2025 tags 1,877
> identifiers at `2025-12-31` and 1,716 at `2024-12-31`; their union, 2,580, describes a
> portfolio that never existed. Positions group by their context's instant.
>
> **And a POSITION is an identifier carrying `InvestmentOwnedAtFairValue`.** The same axis
> also keys the unfunded-commitment table: of Ares' 1,877 identifiers at 2025-12-31, 1,446
> are positions and 428 are commitment rows. Summing fair value over every identifier
> overcounts by 30%.

⚠️ Blue Owl's single schedule reports in **four currencies** (USD, GBP, EUR, AUD).
`weigh-the-m-and-a`'s currency hazard therefore lands *inside one BDC's own portfolio*,
not between two subjects. Any total must state its currency basis, and positions in
other currencies must not be silently summed. Per repo doctrine: **never convert
currency.**

> ⚠️ **CORRECTED, and the correction is the interesting part.** The currency is **not** on
> the values. All 635 of Blue Owl's fair values are `usd`, because a BDC marks its whole
> book in its own reporting currency. The instrument currencies live on
> `InvestmentOwnedBalancePrincipalAmount`: `{AUD: 1, EUR: 9, GBP: 5, USD: 272}`. A reader
> that takes the currency off the fair value reports a single-currency portfolio for every
> filer and hides the hazard completely. Two named fields now: `reporting_currency` and
> `principal_currency`.

### The classifier has a live blind spot for exactly the BDCs people care about

`N-54A` (the election to be regulated as a BDC) is a **one-time** filing:

| BDC | elected | `N-54A` in the recent block? |
|---|---|---|
| Ares Capital | 2004 | **no** (block reaches back only to 2013) |
| Main Street Capital | 2007 | **no** (block reaches back only to 2021) |
| Blue Owl Capital Corp | 2016 | yes |

Industry-wide there were **4** `N-54A` filings in all of 2026 QTR2.

`recent_forms` (`commands/edgar_fund.rs:56-62`) deliberately reads only
`filings.recent`, on the stated rationale that "a filer's *kind* is evident from what
it files now". That rationale is correct for N-MFP3 (monthly) and N-PORT (quarterly)
and **false for a one-time election**. Consequence: Ares Capital classifies as an
`OperatingCompany` and is redirected to `run-the-10k`. This is the
`project_edgar_recent_block_cap` trap landing on the entry gate.

**Fix without paying for the older-segment traversal:** use *recurring* BDC markers
alongside `N-54A`. Measured in 2026 QTR2:

| form | filings/qtr | present in all 3 BDCs sampled | note |
|---|---|---|---|
| `40-17G` | 416 | **yes** | Rule 17g-1 fidelity bond, filed annually |
| `N-2` / `N-2ASR` | — | **yes** | closed-end/BDC registration |
| `N-23C-2` | 30 | **yes** | notice of intent to redeem/repurchase |
| `40-APP` / `APP NTC` / `APP ORDR` | — | **yes** | '40 Act exemptive applications and orders |
| `N-54A` | 4 | no (1 of 3) | keep as confirmation when present |

`40-17G` and `N-23C-2` are also filed by registered investment companies, so the test
is `10-K` **and** one of those markers — a registered fund does not file a 10-K, and a
plain operating company does not file a fidelity bond under the '40 Act.

**The exemptive orders are themselves material.** A BDC generally needs SEC exemptive
relief to co-invest with affiliated funds, and both the application and the order are
public. That is a filed statement about how the adviser is permitted to allocate deals
between this BDC and its other vehicles — the adviser-conflict material that
`retort-the-n-port` could only gesture at, available here as a document.

---

## The build gate — RUN 2026-08-01, and it changes the centrepiece

> ✅ **The gate ran** as `bdc_cross_holder_overlap_gate` in `tests/edgar_live_test.rs`. Six
> BDCs, 5,691 positions, newest 10-K each. The specification below is preserved because it
> is what the gate was built to answer; here is what it answered.
>
> | | ARCC | OBDC | MAIN | HTGC | FSK | GBDC |
> |---|---|---|---|---|---|---|
> | positions | 1,446 | 635 | 747 | 365 | 735 | 1,763 |
> | USD fair value | 34.1bn | 19.4bn | 6.3bn | 5.4bn | 19.3bn | 8.9bn |
> | NAV share in a borrower another of the six also holds | **6.06%** | **11.05%** | 0.24% | ~0% | **6.58%** | **18.65%** |
>
> **2,125 distinct borrowers; 85 held by two or more.**
>
> **Borrower-level overlap: PASSES.** Real, and concentrated among the large sponsor-backed
> direct lenders. Hercules (venture) and Main Street (lower middle market) barely overlap
> anyone, which is itself a structural finding about who lends beside whom.
>
> **Instrument-level MARK COMPARISON: NOT REACHABLE.** Only 4 of the 85 shared borrowers
> produced any instrument match, one of them an undrawn revolver marked at zero on both
> sides. The cause is not extraction — the gate prints both sides of every failure so that
> can be checked — it is that **instrument descriptions are proprietary vocabulary, not a
> shared taxonomy**:
>
> ```
> [no instrument match] anaplan
>     ARCC: ["first lien senior secured loan"]
>     GBDC: ["one stop"]
>     OBDC: ["first lien senior secured loan non affiliated", "first lien senior secured revolving loan"]
> [no instrument match] apex service partners
>     FSK:  ["commercial professional services"]     <- an INDUSTRY, in the instrument slot
>     GBDC: ["one stop"]
> ```
>
> `one stop` is Golub's own word for a unitranche. It may well be the same tranche ARCC calls
> a first lien senior secured loan; **nothing in the filings says so**, and per landmine 6 a
> mark difference between different tranches is not a finding.
>
> **So stage 8 is a PRESENCE finding, not a mark comparison.** "This borrower also appears in
> N other BDCs' schedules, at $X and $Y across their positions" is filed, deterministic and
> defensible. "Ares marks it at X where Blue Owl marks it at Y" is not available. The guide
> should state that the comparison is unavailable and why — a stronger sentence than a spread
> nobody could defend.

## The gate as originally specified

`retort-the-n-port` gated its expensive CUSIP work behind a **measured resolution
rate** and the measurement made the work unnecessary. Do the same here.

**Gate:** extract borrower names from the schedules of **six** BDCs of different
strategies and sizes, normalize with the existing `edgar_identity` normalizer (which
already reached 96.8% on holdings names, and whose three fixes — truncate at the first
`/`, delete apostrophes, strip `companies`/`cos` — apply unchanged), and report:

- distinct borrowers per BDC
- borrowers appearing in ≥2 BDCs, by count and by fair value
- borrowers where two BDCs' marks on the **same instrument** differ

**Pass condition to state up front:** if a meaningful share of fair value sits in
borrowers held by at least one other BDC, the cross-holder comparison is the guide's
spine. If it does not, the spine falls back to (1) and (3) above and the guide is
smaller but still honest. **Do not build the comparison before the gate runs**, and do
not report the gate's own extraction failures as findings — the first attempt in this
round returned "zero overlap" and that was entirely the extractor.

⚠️ **The comparison must be instrument-level, not borrower-level.** Two BDCs holding
the same borrower may hold different tranches with genuinely different values. A
first-lien mark and a second-lien mark on one borrower are not a disagreement. Match
on borrower **and** instrument description, and where the instruments differ, say so
rather than reporting a spread.

---

## Engine work (`dapple-prompt-flow`)

### E1. ~~`InvestmentIdentifierAxis` in `edgar_xbrl_dims.rs`~~ → ✅ `src/edgar_portfolio.rs`

The plan wanted a third axis kind inside `edgar_xbrl_dims`. It is a separate module instead,
for the typed-vs-explicit reason in the corrected findings above. `edgar portfolio` is named
for what it produces rather than for the BDC that motivated it — `InvestmentIdentifierAxis`
is the us-gaap investment-company axis, so any 10-K filer tagging a schedule reads through it.

**Shipped per position:** identifier (opaque), fair value, cost, unrealized (subtraction),
principal, shares, interest rate, PIK rate, basis spread, % of net assets,
`reporting_currency`, `principal_currency`.

**Deliberately absent, each verified:** maturity (zero occurrences), industry (not tagged on
the axis), per-position non-accrual (aggregate only), borrower (no filer-independent split,
and the filer's own tag counts a joint venture's borrowers).

**Per period, the three buckets always partition the identifiers:** `positions` +
`commitment_rows` + `other_rows` == `identifier_count`, asserted in unit tests and against
both live filers.

### E2. ✅ `edgar bdc <ticker|cik>` — classify and pull

Shipped. Returns classification plus, per period, total investments at cost and fair value,
total assets and liabilities, net assets, NAV per share, asset coverage ratio, net and gross
investment income, distributions declared, the two aggregate non-accrual percentages, and the
schedule counts from E1. **Every figure is filed; the command does no arithmetic on them** —
distribution-versus-income is subtraction the caller performs and states.

`--any-kind` returns the record for a non-BDC rather than refusing, and the refusal names the
destination guide.

### E3. ✅ Classifier change (deliberate, with its test)

Shipped. `FilerKind::BusinessDevelopmentCompany` now returns `Some("mark-the-bdc")`, split
from the `OperatingCompany` arm, and `out_of_scope_kinds_name_their_guide` was updated in the
same edit with an added assertion that the two must not share a destination.

`classify_filer` gained `10-K AND (40-17G | N-2* | 40-APP | APP *)`, evaluated after the
N-MFP and NPORT-P arms. Measured live: **10 of 10** sampled BDCs classify correctly (only 3
still show `N-54A`); AAPL, F and KKR stay operating companies; PDI stays a closed-end fund
because the NPORT-P arm runs first. `a_bdc_whose_election_scrolled_out_still_classifies` is
the test for the actual bug, and it was demonstrated to fail when the marker arm is lesioned.

> ⚠️ **`edgar fund` REFUSES a BDC outright, and the fix already shipped (2026-08-01, from
> the `price-the-mfp` build).** `FilerKind::in_scope()` is
> `matches!(self, OpenEndFund | ClosedEndFund)` — one guide's scope rule living in a
> shared command. So `edgar fund <bdc>` hard-errors before returning anything, and stage 1
> cannot resolve its own subject.
>
> The command now takes **`--accept`**, a caller-supplied list of `FilerKind` keys.
> Omitting it preserves the historical N-PORT-only behaviour exactly, so nothing existing
> changed. This guide's stage 1 wants:
>
> ```nushell
> let identity = (edgar fund $bdc_in --accept [BusinessDevelopmentCompany])
> ```
>
> Keys are the `Debug`/`kind` tokens (`FilerKind::key()`), matched
> punctuation-and-case-insensitively. `in_scope` and `redirect_guide` are still RETURNED
> on the record; they are reported, not obeyed. Do NOT wrap the call in a `try`: the
> refusal carries the redirect an operator can act on.
>
> Note the interaction with E3: until the arm is split, a BDC's `redirect_guide()` still
> says `run-the-10k`, so `--accept` makes the fund resolvable while the redirect text is
> still wrong. Both are needed and they are independent changes.

### E4. Reuse, not rebuild

`edgar fund-sections` already reads `FilingSummary.xml` and maps named sections to
`R*.htm`; it works on a 10-K unchanged. `edgar exhibits`, `edgar filings`,
`edgar facts`, `edgar segments`, `edgar insiders`, `edgar comp` and the UPLOAD/CORRESP
path all apply to a BDC as they do to any 10-K filer. **The BDC guide inherits most of
`run-the-10k`'s data layer** — which is exactly why it must not inherit its *reading*.

Housekeeping as always: register in `commands/mod.rs`, allowlist in `engine/security.rs`,
parent help in `commands/edgar.rs`, live tests, and **rebuild `dapple-prompt-api` too**.

---

## Guide structure (`dapple-guides/public/dapple/<slug>/`)

Copy `fund-primer.md` and `accounting-primer.md`, and add a BDC primer: what the BDC
election is, the asset coverage requirement and what changed in 2018, the qualifying
assets test, RIC status and the distribution requirement, base management fee versus
incentive fee and the hurdle, PIK income, non-accrual, and the fair value hierarchy.
`CONDUITS.md` required at root.

| # | Stage | Notes |
|---|---|---|
| 1 | **Pull the Record** | auto; classify, 10-K + 10-Qs + N-2 + proxy + 8-Ks + exemptive orders |
| 2 | **Pull the Portfolio** | auto; E1. Positions, borrowers, instruments, currencies. Counts stated separately |
| 3 | Review the Promise | strategy, target borrower, stated yield and leverage policy |
| 4 | **The Marks** | cost vs fair value per position and in aggregate; Level 3 share; the auditor's critical audit matter |
| 5 | **What the Income Is Made Of** | cash interest vs **PIK** vs fees; non-accruals; the quality of the yield |
| 6 | **Is the Distribution Earned** | distributions declared against net investment income. Subtraction, no adjectives |
| 7 | **The Leverage** | asset coverage, debt maturities, unfunded commitments |
| 8 | **The Same Borrower, Elsewhere** ⚑ | the cross-holder pass. **Gate ran: a PRESENCE finding, not a mark comparison** — see the gate result above |
| 9 | **Who Runs It, and On What Terms** | external vs internal, fee structure, the exemptive orders, affiliate co-investment |
| 10 | The Fine Print | comment letters, 4.01/4.02, `40-33` legal proceedings, misfilings |
| 11 | **Weigh the Promise Against the Portfolio** | the payoff |
| 12 | **Pull the Current Period** ⚑ | fork point |
| 13 | Layer the Changes | position and mark diffs, non-accrual additions, leverage drift |
| 14 | Build the Story | connect → synthesize → HTML + PDF |

### Doctrine carried forward

- **The BDC is the scene**; borrowers are its contents. A borrower becomes a character
  by computed promotion only, `reason` always carried. With 2,510 positions this is not
  optional — repo memory is explicit that **overwhelm is cast size**, not figure count.
- **Band, do not split.** Embedded four-backtick bands, never top-level region sections.
- **Every aggregate states its denominator**, and never the whole portfolio.
- **Never convert currency** — Blue Owl makes this live inside a single filing.
- **Misfiling vs violation**; **empty-band doctrine**; **no silent caps**.
- **Deterministic before generative.** Cost-vs-fair-value, distribution coverage,
  asset coverage, PIK share and non-accrual share are all arithmetic on tagged facts.
  None of them is a prompt.

---

## Landmines

1. **`run-the-10k` will read a BDC and produce something plausible.** That is the
   failure this guide exists to prevent, and the classifier currently causes it.
2. **Positions ≠ borrowers, and there is no borrower count at all.** ⚠️ **Corrected and
   hardened.** The identifier grammar is filer-specific: Ares writes
   `<borrower>, <instrument>`, Blue Owl writes `<borrower> | <instrument> | <affiliation>`,
   and borrower names carry commas of their own
   (`Steward Partners Global Advisory, LLC, First lien senior secured loan`). 704 Ares
   identifiers end in a numeric disambiguator (`... loan 2`) whose ordinal is the filer's own
   and means nothing across filers. A structural rule was tested — "a borrower row is an
   identifier that others extend" — and finds 719 such rows on Ares (249 of which carry a
   fair value and are ordinary positions) against 36 on Blue Owl.

   The filer's own `InvestmentCompanyNumberOfBorrowers` does **not** rescue it: every live
   occurrence is dimensioned by `InvestmentCompanyNonconsolidatedSubsidiaryAxis` and counts a
   JOINT VENTURE's borrowers, reading 39 against Ares' 1,446 positions. Blue Owl does not tag
   it. So the engine reports **positions only**, and a guide that needs borrowers must say it
   does not have them.
3. **Rendered R-files differ per filer.** Read the instance. The first extraction in
   this round returned a confident zero from a layout mismatch.
4. **Multi-currency inside one portfolio.** State the basis; never convert.
5. **Non-accrual status may be footnoted rather than tagged.** ✅ **Verified 2026-08-01, and
   the answer is: not tagged per position.** Ares tags only three aggregate concepts
   (`InvestmentOwnedNonAccrualStatusPercentOfCost`, `...PercentOfFairValue`,
   `InvestmentOwnedNumberOfLoansOnNonAccrualStatus`). `InvestmentMaturityDate` occurs **zero**
   times, so maturity is not tagged either. There is no per-position non-accrual or maturity
   field anywhere in the engine, and `edgar bdc`'s help says so. The guide reports the
   aggregate and states that the per-borrower detail sits in a footnote it does not read.
6. **A mark difference between two BDCs is not automatically a finding.** Different
   tranche, different date, different fee treatment. Match on instrument, state the
   period ends, and characterize nothing.
7. **The fee structure is a filed fact and an inference trap.** Report the base fee,
   the incentive fee, the hurdle and what was actually paid. Never conclude that a fee
   caused a decision. `weigh` invariant 7 (never characterize the distance) applies.

---

## Verification

1. Gate measurement above, reported as three numbers, before stage 8 is built.
2. `cargo test -p dapple-prompt-flow`; fixtures from **both** Ares and Blue Owl so the
   layout divergence is pinned by a test.
3. **Classifier test for the actual bug**: a BDC whose `N-54A` lies outside the recent
   block must still classify as a BDC.
4. **Partition check**: positions by instrument type, by industry and by currency must
   each sum to the schedule total; cost + unrealized = fair value.
5. Probes must be **able to fail** — counters aggregated, `exit 1`, revert-to-fail
   demonstrated.
6. `dapple guide validate` at 0 warnings before ingest; `nu-check` everything; smoke
   probes that shadow backend commands so guards run.
7. End-to-end on two BDCs of opposite shape: a large externally-managed first-lien
   lender and a smaller internally-managed one.

---

## Deliberately not built

- **No valuation opinion.** The guide reports the mark, the hierarchy level, the
  cost-to-value spread and the auditor's own language. It never says a mark is wrong.
- **No credit view on a borrower.** They are private and file nothing; an opinion would
  be invention.
- **No cross-BDC ranking.** `weigh` invariant 8 (rank nothing) applies between
  holders. The comparison reports two marks side by side and stops.
- **No borrower→CIK look-through as a headline.** A minority of BDC borrowers are
  public. Where one is, `run-the-10k`'s machinery applies and the coverage number is
  reported honestly as small — it is a bonus, never the spine.
