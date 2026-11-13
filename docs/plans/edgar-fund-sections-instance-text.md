# Section text from the XBRL instance (`edgar fund-sections`) — completed

**Opened 2026-08-01** as a handoff; **closed 2026-08-01**. This is now a record rather than a
plan. It is kept because most of what it cost was *measurement* against real filings, and none
of that is recoverable from the diff.

Read [`edgar-fund-sections-hardening.md`](./edgar-fund-sections-hardening.md) first: it
establishes that **roles are the substrate and `FilingSummary.xml` is only one place they are
written down**. This pass applies the same reframing one level down.

---

## What was wrong, and what it reads like now

| # | Defect | Before | After |
|---|---|---|---|
| 1 | Vanguard (no `FilingSummary.xml`) had no prose at all | 12 funds listed, every `html_file` null, `--fetch` returned nothing | **95 sections with prose, still 0 fragments** |
| 2 | `--fetch` cost one HTTP request per section, capped at 24 | up to 24 requests, silently truncated past the cap | ONE instance fetch for the whole filing |
| 3 | Fund rows carried no series id, so `funds` could not be joined to `tagged` | 1290: 8 funds with `series_id: null` beside a `tagged` record keyed by 8 series ids | all 8 carry their series |
| 4 | A fund that matched no requested key returned nulls silently | indistinguishable from a fund that disclosed nothing | a warning naming the fund AND the keys the filing does have |

Guarded by `fund_sections_works_across_three_fund_families` and
`instance_prose_is_the_same_disclosure_without_the_viewer_chrome` (both live, `#[ignore]`d)
plus 16 new unit tests. Lib suite 565 → 603.

---

## The measurement that settled the design

The handoff proposed instance text as a **fallback** for filings with no fragments, on the
reasoning that a fragment is *rendered from* the instance and so its text should be a superset.

Measured through the same `html_to_text` on 1290's `InvestmentStrategy`, that is backwards:

| source | characters |
|---|---|
| instance (`StrategyHeading` + `StrategyNarrativeTextBlock`) | **4,737** |
| rendered `R5.htm` fragment | **35,127** |

The fragment is 7.4× larger and **none of the excess is disclosure**:

```
XML 163 R5.htm IDEA: XBRL DOCUMENT v3.25.4
────────┬──────────────────────────────────────────────────────────  (full-width table frame)
function toggleNextSibling (e) { if (e.nextSibling.style.display=='none') …   (renderer JS)
Prospectus [Line Items]   Strategy [Heading]   Strategy Narrative [Text Block]   (XBRL labels)
&lt;span style="color:#000000;font-family:Arial;…">Investments, Risks, and Performance
                                                          (heading markup, DOUBLE-escaped)
```

So the instance became the **primary** source and the fragment the fallback. That inversion is
the whole reason this pass is worth more than a fetch-count optimization.

---

## The design decision, and the one that was rejected

The handoff proposed a `section_key -> element names` table
(`InvestmentStrategy -> [StrategyHeading, StrategyNarrativeTextBlock]`, …). **Rejected.** Such
a table is filer-observed, must be re-verified against every new filer, and silently drops
whatever it has not seen.

**The presentation linkbase already states it.** Verbatim locator order of 1290's
`/1/role/InvestmentStrategy`:

```
FormN1aAbstract  ProspectusTable  DocumentInformationDocumentAxis  DocumentDomain
LegalEntityAxis  EntityDomain  ProspectusLineItems
StrategyHeading  StrategyNarrativeTextBlock  StrategyPortfolioConcentration
```

The first seven are scaffolding (suffixes the EDGAR Filer Manual reserves — `Abstract`,
`Table`, `Axis`, `Domain`, `LineItems`); the last three are the section, in reading order. So
the join is:

```
linkbase:  role     ->  section key + fund + the concepts the section presents, in order
instance:  concept + context  ->  the text, and the series the context scopes it to
```

No vocabulary is maintained anywhere, filer-specific elements work, and the **N-CSR role set —
documented nowhere** — is carried for free.

---

## Facts worth not re-deriving

**1. Prose must be decided from the VALUE, never from the element name.** A role's line items
are not all narrative. 1290's `OperatingExpensesData` presents `ManagementFeesOverAssets`,
`ExpensesOverAssets`, `NetExpensesOverAssets` — a fee table whose facts are bare numbers — plus
`OperatingExpensesColumnName`, whose value is `Class A`. Emitting those as the section's text
would put `0.0055` where a reader expects a table AND suppress the fragment fallback that
renders it properly. The test is: not a number, date or bool; then markup present, or the
element ends `TextBlock`/`Heading`/`Narrative`.

**2. `RiskTextBlock` is not 1:1 with a fund and its order is content.** It is dimensioned by
`RiskAxis` as well as `LegalEntityAxis` (1290: 174 blocks over 8 funds; Vanguard: 409 over 47),
so a fund's risks section is a CONCATENATION. It must be assembled in **document order** — a
`BTreeMap` keyed by the risk member returns the section alphabetized by QName, which is a
different document. Same reason `RoleContent::line_items` is a `Vec` and not a `BTreeSet`.

**3. Text must be gated on `--fetch` even though the instance is parsed whole.** One parse
yields every section, so the natural code returns them all — which turned a two-key request
into an eleven-section payload. Caught only by a live run (`text=48` where 16 was the maximum
possible), never by a unit test.

**4. Most filers' role URIs name an ordinal, not a series.** 1290 writes
`http://www.1290funds.com/1/role/InvestmentStrategy`. So on the `FilingSummary` path every
section came back `series_id: None` while `edgar_fund_tags` reported risks and benchmarks KEYED
BY SERIES — eight funds with no ids beside eight ids with no funds.
`FilingSections::attribute_series_from_linkbase` closes it through the same `group_series`
resolution attribution already needed.

**5. A filer may put the standard keys on TRUST-level roles.** See the gap below.

---

## Known gap — Fidelity does not partition its prospectus into sections at all

Not a regression; it predates this work and is **not fixed here**. Handed off in
[`edgar-fund-sections-fidelity.md`](./edgar-fund-sections-fidelity.md), which carries the full
probe output.

The short version, and it is not what the shape first suggested: Fidelity's trust-level
`http://xbrl.sec.gov/rr/role/InvestmentStrategy` role carries the standard section KEY but is a
**stub** — its only real concept is `dei:DocumentType`. All 131 of a fund's concepts
(`ObjectiveHeading`, `ObjectivePrimaryTextBlock`, `ExpenseNarrativeTextBlock`, …) sit in ONE
per-fund role keyed `DisclosureDetailS000067930_FidelityAgricultural…-ComboPRO`.

So the prose is fully present and fully attributed — it is simply **not divided into sections**
anywhere in the filing, and no join between the two role families can recover the division,
because the trust-level side has nothing to join to.

What this pass adds is that the command now **says so** — the `--fetch matched no text for
fund(s) …` warning names the fund and lists the keys the filing actually has. Silence there
read as "this fund disclosed nothing", which is the inverted answer this guide family has been
bitten by twice.

---

## Verification

```bash
nu -c 'cargo test -p dapple-prompt-flow --lib'
nu -c 'cargo clippy -p dapple-prompt-flow --tests -- -D warnings'
nu -c 'cargo fmt --check'

# live — UA is the flow pod's own, in assets/k8s/registry/dapple-prompt-flow.yaml
nu -c 'cargo test -p dapple-prompt-flow --test edgar_live_test fund_sections_works_across -- --ignored --nocapture'
nu -c 'cargo test -p dapple-prompt-flow --test edgar_live_test instance_prose_is_the_same -- --ignored --nocapture'
```

Live results, run individually:

```
1290 (control)   funds=  9 named=  8 series=  8 fragments=  89 text= 16
Vanguard         funds= 12 named= 12 series= 12 fragments=   0 text= 95
Fidelity         funds=  7 named=  6 series=  6 fragments=  20 text=  2   (+ starvation warning)
```

> **Run the live EDGAR tests ONE AT A TIME, or a few at a time.** The whole file is ~39 live
> tests and running them together trips EDGAR's ~10 req/s fair-access limit; the failures then
> surface as ordinary assertion failures (`no readable 10-K instance`, `got 1 filer`,
> `returned no prose`) rather than as anything resembling a rate-limit error, and stay tripped
> for minutes afterwards. Confirmed by the one place it does surface honestly:
> `GenericError { error: "SEC rate limit exceeded" }`.

## Guides

Neither guide that calls `edgar fund-sections` needs an edit: the emitted shape is unchanged
(`sections[].text`), and both readers take candidate lists and take the FIRST match, so
Vanguard's per-prospectus-variant duplicate sections cost payload rather than correctness.

- **`retort-the-n-port`** — `pull.nu`, `current-pull.nu`, and the readers in
  `promise-generate.nu`, `fees-generate.nu`, `story-generate.nu`
- **`price-the-mfp`** — `pull.nu`

`guide_prospectus_fetch_keys_match_the_role_vocabulary` mirrors `pull.nu`'s key list verbatim
and asserts the objective and expense reads are non-empty. It passes, with
`objective_chars: 147`, `strategy_chars: 2426`, `expense_chars: 55284` — the last being the
numeric-table sections still coming from their fragments, chrome and all, exactly as intended.
Its objective threshold was re-calibrated; see below.

## A threshold that had to go DOWN because the text got better

`guide_prospectus_fetch_keys_match_the_role_vocabulary` asserted
`objective_chars > 200`. It now asserts `> 80`, and the number fell for a good reason: the
old bar was calibrated when `--fetch` returned the rendered fragment, whose length is mostly
chrome. Measured across all eight funds of the 1290 filing, `ObjectivePrimaryTextBlock` runs
**33 to 119 characters** —

| fund | objective | chars |
|---|---|---|
| S000045911 | "Seeks to maximize current income." | 33 |
| S000052767 | "Seeks to provide long-term growth of capital." | 45 |
| S000052766 | "Seeks to provide income and total return through principally investing in…" | 119 |

A fund's stated objective is ONE SENTENCE; that is the disclosure, not a truncation. The whole
`RiskReturn` section with its headings measures 147. A 200-character floor was asserting that
the filing pads its objective.

## Still open

- Fidelity's trust-level standard keys (above).
- `edgar_fund_resolves_unit_investment_trusts` — QQQ resolves `series` where the test asserts
  `unitary`. Pre-existing, unrelated (`edgar fund` → `edgar_fund_registry.rs`).

## Files

- `crates/dapple-prompt-flow/src/edgar_fund_text.rs` — **new**, the linkbase↔instance join
- `crates/dapple-prompt-flow/src/edgar_fund_linkbase.rs` — `RoleContent::line_items` (ordered)
- `crates/dapple-prompt-flow/src/edgar_fund_sections.rs` — `attribute_series_from_linkbase`
- `crates/dapple-prompt-flow/src/commands/edgar_fund_sections.rs` — one shared instance/linkbase
  fetch, `section_text`, the starvation warning
- `crates/dapple-prompt-flow/tests/edgar_live_test.rs` — the measurement + the extended guard
