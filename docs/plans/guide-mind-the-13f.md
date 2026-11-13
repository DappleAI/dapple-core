# Plan — `mind-the-13f`: the institutional manager guide

**Status: BUILD THIS ONE FIRST.** Corrected 2026-08-01 — it was written as "third of
three, deliberately last", which was a *narrative* preference ("it reads better once the
other guides exist to point at") that got read as a dependency. It has none. Its engine
layer (`edgar_thirteenf.rs`, `edgar_dera.rs`, `edgar_identity.rs`, `edgar_cusip.rs`)
already ships, neither sibling plan touches any of it, and stage 9's routing targets
(`run-the-10k`, `retort-the-n-port`, `weigh-the-m-and-a`) all exist today.
`price-the-mfp` and `mark-the-bdc` stay written and handed off; neither gates this.

**What already exists, so you do not rebuild it:**

| shipped | for |
|---|---|
| `edgar_thirteenf.rs` incl. `normalize_value` (the 2023-01-03 units cutover) | landmine 2. Its `dedup_latest` is for the DERA path ONLY — see landmine 7 |
| **`edgar manager`** — ✅ built 2026-08-01 | E1. Stages 1 and 3–7 all read it |
| `edgar_identity.rs` + `edgar resolve` + `edgar company-cik` | holdings → CIK, measured at 95.09% on operating-company names |
| `edgar nport`, `edgar ncen`, `edgar fund` (registry fixed 2026-08-01), `edgar search` | stage 8, and stage 9's fund arm |
| **`edgar adv`** — ✅ built 2026-08-01 | stage 1, stage 2's coverage ratio, stage 10. See E6 |

**The engine layer is COMPLETE.** E1, E2, E4, E5 and E6 are all built and live-verified.
Nothing in `docs/plans/` blocks the guide itself now.

---

## ✅ THE GUIDE IS BUILT — 2026-08-02

`dapple-guides/public/dapple/mind-the-13f/`. **`dapple guide validate`: 4 chapters, 21
tasks, 0 warnings.** All 21 task scripts and both probes parse. 15 static probe sections
green; 4 behaviour probe sections green, each **demonstrated red** by the reversion its own
comment names before being trusted.

**Re-laid 2026-08-02 to the composite story model — the guide as built has 14 stages, not
this plan's 13.** The stage tables below predate the re-lay: the built guide adds stage 5
"Read the Book as a Company" (`bk` triad + `mn_book` via `edgar book`), for 12 section
triads and 21 tasks, and its write family is named `write-scene`/`write-book`/`write-record`
(this plan's conduit-load section projects the retort naming). 8 prompts as planned (three
look-through generates + connect + refine + three writes), everything else deterministic
with no prompt at all. Its own `CONDUITS.md`, an agent-only 13F primer, an `AGENTS.md`, and
the accounting primer shared with `retort-the-n-port`.

**Four things the build changed, and each is a prescription here that would have shipped
broken:**

1. ⚠️ **The look-through depth gate had to move to stage 4.** This plan puts it in stage 8
   ("Depth gate runs first"), and inside one stage it cannot work: the runtime
   auto-dispatches every pure-compute step as soon as its EARLIER STAGES are done, so a
   compute step consuming another compute step's output from the same stage races it. An
   operator select cannot race, because it waits for a human. So the choices are computed on
   the "What They Held" pane, the pick happens there, and the reading passes four stages
   later find `lt_worklist` already bound. **The general rule this produced: at most one
   pure-compute step per stage, and anything consuming its output goes in a LATER stage or
   becomes an ordered task inside the SAME step.** Stage 12 needed the second form (the pull
   and its finder are two tasks in one step).
2. ⚠️ **`edgar fund --accept [InstitutionalManager]` is not enough.** This plan prescribes
   exactly that at stage 1. It hard-errors on **Berkshire Hathaway**, which the classifier
   reads as an operating company that happens to file a 13F, and on any bank holding
   company. The pull passes every `FilerKind`: by that line the filer's own 13F filings are
   already in hand, so the classification is DESCRIPTION and never a gate. `in_scope` and
   `redirect_guide` still ride on `mn_identity`, reported and not obeyed.
3. ✚ **The connect pass needs no assignment call.** The siblings ask a model to file
   evidence under claims because claim-to-evidence has no key. Here every look-through,
   change and routing point carries the `holding` it is about, and a tenure is keyed by that
   same holding, so the join is a name match over filed rows. The `~~~ASSIGN~~~` marker and
   its whole misfiling-risk surface are gone.
4. ⚠️ **Three parse traps, and only one is catchable by `nu-check`.** A leading `else` /
   `and` / `or` inside a closure parses as an EXTERNAL COMMAND and the security policy
   rejects it — `nu-check` accepts it and `dapple guide validate` does not, because the
   latter parses the way ingest does. A record-literal value needs its own parentheses
   around a concatenation (`field: (($x | into string) + "text")`). And `default -1` reads
   `-1` as a flag. **Worse: `nu-check` inside an `each` closure returns `true` for files
   that fail when checked directly**, so two batch sweeps reported all-clear on three broken
   scripts. Check per file, one process each.

⚠️ **And one defect it shipped with, caught by a cross-guide sweep rather than by anything
in the build.** The three `build-the-story` outputs were authored from `retort-the-n-port`'s
**pre-migration-95 shape**, so they carried no `group` / `format` and the pane would have
rendered one deliverable as three unrelated results. Every other financial-family guide had
already been swept; this one was written from a stale template and did not come up.
`dapple guide validate` was clean, all 19 probe sections passed, and the defect was visible
only on a rendered pane. Fixed (`group: "mn_story"` on all three, `format: "html"` / `"pdf"`
on the two visible ones, hidden member grouped and formatless, HTML authored first so it
leads). Static probe **§16** now pins all four properties and was demonstrated red.

> **The lesson for the next guide in this family:** diff the brief-output ATTRIBUTE
> VOCABULARY against **every** sibling, not just the one you copied from. A family-wide
> sweep that lands while a new guide is being written will miss it by construction.

**Not run against a live manager.** Everything above is parse, structure and fixture-level
verification. The first live journey on both Motley Fool managers (and a third on Berkshire,
for the 14-included-manager shape) is still owed, per Verification item 20.

⚠️ **Three shipped commands could not be called from a guide task script at all**, found
2026-08-01 while building E5 — see landmine 26. `edgar adv` was one of them, and stage 1
calls it. Fixed, and the scan that found them is recorded so a future command does not
repeat it.

**✅ E1 `edgar manager` IS BUILT** — 2026-08-01. `src/edgar_manager.rs` (cover +
information-table parsers, reconciliation, filing/amendment classification, the
info-table member rule) and `commands/edgar_manager.rs` (fetch, filing location,
form-scoped name resolution, nushell shape), registered in `commands/mod.rs`, allowlisted
in `engine/security.rs`, listed in the `edgar` parent help.

23 unit tests over inline fixtures from **four real filings across three filers and two
schema eras**, plus **3 live tests**. The namespace, units and cover-date probes were each
reverted and confirmed red before being restored. Live: ten Berkshire filings 2023Q4→2026Q1
all reconcile exactly (2026Q1 at 90 rows / $263,095,703,570), and a name that is not a
listed issuer resolves end to end through a guide script. Whole crate green, clippy clean
under `-D warnings`.

**✅ The tenure layer is built too** (`edgar manager --tenures`, 2026-08-01). Four pure
layers — the amendment-aware quarter fold, per-CUSIP aggregation, tenures with censoring
stated, and the closing history arithmetic. **This unblocks stages 4, 7 and 11.** 39 unit
tests + a 4th live test; the fold, aggregation, notice-exclusion and censoring probes were
each reverted and confirmed red. Whole crate green (621 tests), clippy clean on
`dapple-prompt-flow` and `dapple-prompt-api`.

⚠️ **It found one more thing the plan had wrong: a filing's row count is not its holding
count** (landmine 21). Berkshire files 90 rows over **29** securities. Every per-position
aggregate in stage 4 would have been computed on fragments.

**Building it corrected this plan in three places.** These are not refinements; each was a
prescription here that would have shipped a wrong number:

- ⚠️ **`dedup_latest` is NOT reusable, and landmine 7 is materially worse than it was
  written.** See landmine 7 — an amendment can be an *addition*, and latest-wins would
  have reported $1.1bn as Berkshire's entire 2025Q1 portfolio.
- ⚠️ **Cover dates are `MM-DD-YYYY`, which no shipped date helper parses** (landmine 18).
  It fails closed, so the symptom would have been every `period_date` silently empty.
- ⚠️ **A `13F-NT` has no information table document and no summary page at all**
  (landmine 19), so the sole-`.xml`-member rule must be gated on the filing kind or it
  fails loudly on a valid filing.

Confirmed as written: no 13F XML parser existed anywhere in the repo (`nameOfIssuer`
appears only in `edgar_nmfp.rs`, a different form), and the shared CIK resolver cannot
resolve an unlisted manager (landmine 17) — though the prescribed fix needs one amendment
of its own, landmine 20.

**Phase 2 is sketched, not designed** — see "the dark room can be lit" under stage 2. The
half of a manager a 13F structurally cannot show turns out to be reachable in bulk, fee
included.

⚠️ **`edgar fund` refuses a 13F manager unless you pass `--accept`.** Found and fixed by
the `price-the-mfp` build 2026-08-01, in the same working tree. `FilerKind::in_scope()` is
`matches!(self, OpenEndFund | ClosedEndFund)` — **retort's scope rule living in a shared
command** — so `edgar fund` hard-errored on every non-N-PORT filer *before* returning a
record, and all three follow-on guides were blocked at stage 1 by it.

```nushell
edgar fund 1512814 --accept [InstitutionalManager]
```

Omitting the flag preserves the historical N-PORT-only behaviour exactly, so no existing
call changed. Keys are the `Debug`/`kind` tokens via `FilerKind::key()`, matched
punctuation- and case-insensitively and nothing looser (`fund` must NOT match
`MoneyMarketFund`). `in_scope` and `redirect_guide` are still **returned** — reported,
never obeyed. **Never wrap the call in `try`**: the refusal carries the redirect, which is
the whole point of returning a destination rather than a bare error.

⚠️ **E1's row shape cannot reuse `HoldingRow`.** The shipped struct
(`edgar_thirteenf.rs:27`) carries manager, period, quarter, accession, filing date,
shares and value — and **none of `title_of_class`, `investment_discretion`,
`voting_authority` or `other_manager`**, because the DERA path is issuer-centric and
never needed them. Those four are the only partition keys the form natively supplies
(see the measurement below), so a manager-centric command that reuses the existing struct
leaves the guide with no scene to build.

**Story review, 2026-08-01 (Kenny).** The narrative doctrine below was a six-bullet copy
of `retort-the-n-port` with the nouns swapped, and copying is how a guide loses the story
it was going to tell. Reviewed and rewritten: see
**"The story: what the inversion does to scene, tracks and load"**. Three things changed
— the scene's furniture (retort's four partition fields do not exist on a 13F row), the
definition of a track (there was none; it is a position's tenure), and stage 11 (it
compared the conduct to the record, which are the same thing).

**Slug:** `mind-the-13f`.

✅ **The Rust side of the slug is DONE (2026-08-01) — do not redo it.**
`edgar_fund_registry.rs`'s `FilerKind::redirect_guide()` now returns `Some("mind-the-13f")`
for `InstitutionalManager`, and `out_of_scope_kinds_name_their_guide` pins **every** arm by
slug rather than by kind alone. The directory basename is also the guide slug (migration
73), so creating `public/dapple/mind-the-13f/` fixes the name on the guide side to match.

That mattered because `redirect_guide()` is **user-facing text**: it is what `edgar fund`
prints when an operator types a manager's ticker ("this is an institutional manager, go
to X"), so a stale slug sends them to a guide that does not exist. It had been stale
precisely because no test named the slug — the 13F case asserted only
`FilerKind::InstitutionalManager`, so the arm could be changed, left stale or mistyped
with nothing failing. The assertion was written and **seen to fail** against the old
string (`left: Some("read-the-13f")`) before the arm was touched; that ordering is the
only thing that proves the guard binds to the value being renamed.

> **Search by symbol, not by line.** Line numbers in this module move — the tail resolver
> shifted everything once already. Grep `redirect_guide` and
> `out_of_scope_kinds_name_their_guide`.

**Why this name.** The family's verb is what the reader does to the document, and the
best of them bridge to what the subject does: a BDC *marks* its portfolio and a reader
*marks* its work. This bridges the same way — a manager **minds** money (looks after
it) and the reader **minds** the filing (attends to it). Its caution sense also suits
this guide's hardest rule, which is never narrating *why* a position moved.

---

## Required reading before you start

This plan assumes the family's doctrine and names it by shorthand throughout. All paths
verified to exist 2026-08-01. **This guide is the hub, so read the siblings' plans too**
— stage 9 routes each holding to whichever of them can read it.

| read | for |
|---|---|
| `dapple-guides/public/dapple/retort-the-n-port/AGENTS.md` (42 kB) | **the look-through machinery this guide reuses at stage 8**, plus the evidence-tier contract, the per-claim denominator rule, band-don't-split, computed promotion, why a probe must be able to fail |
| `dapple-guides/public/dapple/retort-the-n-port/CONDUITS.md` | what the model is told on every prompt, and the banned-output vocabulary |
| `dapple-guides/public/dapple/retort-the-n-port/probes/` | the offline smoke-probe harness this guide copies, including the both-directions banned-string assertion the no-intent probe needs |
| `dapple-guides/public/dapple/run-the-10k/AGENTS.md` | misfiling vs violation, scene / cast / prominence, and the self-comparison shape stage 7 uses |
| `dapple-guides/public/dapple/weigh-the-m-and-a/AGENTS.md` | the invariants. **7 (never characterize the distance) is this guide's hardest rule** — there is no claim text to hold an inference against, so any "because" is invention. ⚠️ its three top-level region sections are a known mistake — do not copy them |
| `dapple-prompt/docs/plans/well-run-the-10-k-and-the-compressed-popcorn.md` | the four-round build record of `retort-the-n-port`, including the round where two of three probes silently could not fail |
| `dapple-prompt/CLAUDE.md`, `crates/dapple-prompt-flow/AGENTS.md` | the engine side, the sql-composer rules, and the **all shell commands go through `nu -c`** rule |

Two house rules that are easy to violate on day one:

- **No em-dashes or en-dashes in rendered guide content.** They read as an AI tell.
- **Never `git commit`.** Working-tree edits only; stage only when asked. Guide
  sync and ingest belong to Kenny.

Sibling plans: `guide-price-the-mfp.md`, `guide-mark-the-bdc.md`.

---

## Why this guide is different from every sibling

| Guide | The claim | The record |
|---|---|---|
| `run-the-10k` | the company's narrative | its own filings over time |
| `weigh-the-m-and-a` | none; two peers | two records side by side |
| `retort-the-n-port` | the fund's stated strategy | the holdings' own filings |
| `price-the-mfp` | a price, $1.00 | the fund's own daily measurement |
| **`mind-the-13f`** | **there is no claim** | **a record of conduct** |

A 13F filer has **no promise document on EDGAR**. There is no prospectus, no objective,
no principal-strategies section, no benchmark. A 13F is a quarterly statement of what
a manager *held*, filed 45 days after the fact. It records **conduct, not intent.**

So the spine cannot be promise-vs-portfolio, and trying to force it there is the
mistake to design against. The three honest spines available:

1. **Conduct against its own prior conduct.** Position changes across quarters, with
   the existing `status` vocabulary (`new` / `added` / `trimmed` / `exited`) and
   `streak`. This is `run-the-10k`'s self-comparison shape, applied to actions.
2. **Conduct against what the holdings filed.** Did the manager add to a position
   through a quarter in which the company filed an 8-K Item 4.02 non-reliance, or drew
   a comment letter? This is `retort-the-n-port`'s look-through, and it is the strongest
   material here for exactly the same reason: the evidence was written by someone who
   did not know it would be read this way.
3. **Conduct against what the manager filed about itself elsewhere.** Form 4s if the
   manager is an insider, SC 13D/G if it crosses 5%, and the cover page's own
   declarations.

### It is the hub, and that is a design consequence

A 13F position may be an operating company (`run-the-10k`'s subject), an ETF or
closed-end fund (`retort-the-n-port`'s subject), or a company in the middle of a merger
(`weigh-the-m-and-a`'s subject). The guide should **route and name**, exactly as
`edgar fund` already refuses out-of-scope filers by naming the right guide, rather than
absorb all three. Build the routing as a first-class output: a position list annotated
with which sibling guide can read each name is a genuinely useful artifact and costs
one classification call per promoted holding.

#### The fund arm is not an edge case, and the money-market arm does not exist

Measured live 2026-08-01. The SEC's own 13F FAQ settles the scope in two sentences:

> "primarily includes U.S. exchange-traded stocks…, shares of **closed-end investment
> companies**, and shares of **exchange-traded funds (ETFs)**"
>
> "shares of **open-end investment companies**, *i.e.*, mutual funds, are not included on
> the list and, therefore, should not be reported on Form 13F"

So **ETFs and closed-end funds route to `retort-the-n-port`** (both file N-PORT), and
**`price-the-mfp` is structurally unreachable from a 13F** — a money market fund is an
open-end company, so one can never appear. Do not wire that arm. BDCs are exchange-traded
and do appear, but `run-the-10k` already reads a BDC's 10-K, so `mark-the-bdc` only
sharpens a route that is already correct.

How much of a real book this is, on the first subject:

| | MFAM (asset mgmt) | MFWM (wealth mgmt) |
|---|---|---|
| ordinary shares | 234 · 99.18% | 91 · 72.98% |
| ADRs | 3 · 0.82% | 5 · 1.95% |
| **ETFs** | **0** | **28 · 25.07% of value** |

MFWM's **#5 position is Vanguard's S&P 500 ETF**, and its bond sleeve is 14 ETFs at
16.11% of value built as defined-maturity ladders (BulletShares 2030/2031, iBonds Dec
2026/2029/2031). A quarter of that book is invisible to a guide that only knows how to
read operating companies, and the two managers differ so completely on this axis that
running the guide twice exercises both the empty case and the loaded one.

⚠️ **A fund position is MORE transparent than a company position, not less.** The
company files financials; the fund files a position-level portfolio quarterly. The fund
arm of stage 9 is the only route where the destination guide can read the holding's own
holdings, which is why it deserves to be built rather than bucketed as unresolved.

---

## Verified findings (live, 2026-08-01)

### Scale, and the notice problem

2026 QTR2 `form.idx`:

| form | filings | share |
|---|---|---|
| `13F-HR` | 9,307 | 80.0% |
| **`13F-NT`** | **1,985** | **17.1%** |
| `13F-HR/A` | 318 | 2.7% |
| `13F-NT/A` | 23 | 0.2% |
| total | 11,633 | |

**A `13F-NT` is a notice with no holdings** — the manager states that its positions are
reported by another manager. One filing in six carries no portfolio at all. That is not
an error case to skip quietly; it is a fact about how the manager is organized, and the
guide must say so plainly rather than reporting an empty portfolio.

`edgar_thirteenf.rs` already distinguishes them (`is_holdings: stype.contains("HR")`).

### The cover page declares things worth reading

Berkshire Hathaway (CIK 1067983), `13F-HR` `0001193125-26-226661`, period 2026-03-31,
filed 2026-05-15 (**45 days**, the rule):

```
reportType                  13F HOLDINGS REPORT
isAmendment                 false
isConfidentialOmitted       false
otherIncludedManagersCount  14
tableEntryTotal             90
tableValueTotal             263,095,703,570
form13FFileNumber           028-04545
```

- **`isConfidentialOmitted`** is a filed admission that positions were withheld under a
  confidential treatment request. When true, the portfolio on the page is knowingly
  incomplete and the guide must say the manager said so. This is the cleanest
  coverage-is-the-ceiling statement in the family: it is not our inference, it is a
  checkbox the filer ticked.

  ⚠️ **It is an OPTIONAL element and it models THREE states, not two.** Berkshire's
  cover carries it; **both Motley Fool covers omit it entirely** (verified on
  `0001104659-26-059230` and `…-059237`). So `false` and absent are different
  statements — "the manager said nothing was withheld" versus "the manager said
  nothing" — and a parser defaulting absence to `false` asserts something never filed.
  Given the field's whole value is that it is *the filer's own admission*, inventing
  one is the specific thing that ruins it.
- **`otherIncludedManagersCount`** plus the per-row `otherManager` sequence numbers
  describe **who inside the organization actually holds what** — a structure fact.
  Berkshire declares 14; both Motley Fool managers declare **0** while sharing a brand
  and a street address, which is itself the finding (see "The subject" below).

### The filing states an arithmetic identity about its own attachment, and it closes

```
infoTable rows          90            declared tableEntryTotal   90
sum of <value>          263,095,703,570   declared tableValueTotal   263,095,703,570
```

Exact. This is the family's scene-sum check, except **the filer states both sides**. A
mismatch is a misfiling detectable with no outside data at all, and it should be the
first check the pull runs.

### The holdings table

```xml
<infoTable>
  <nameOfIssuer>ALLY FINL INC</nameOfIssuer>
  <titleOfClass>COM</titleOfClass>
  <cusip>02005N100</cusip>
  <value>498992850</value>
  <shrsOrPrnAmt><sshPrnamt>12719675</sshPrnamt><sshPrnamtType>SH</sshPrnamtType></shrsOrPrnAmt>
  <investmentDiscretion>DFND</investmentDiscretion>
  <otherManager>4</otherManager>
  <votingAuthority><Sole>12719675</Sole><Shared>0</Shared><None>0</None></votingAuthority>
</infoTable>
```

- **No ISIN, no ticker — CUSIP and name only.** So the guide inherits
  `retort-the-n-port`'s identifier problem *without* the ticker shortcut that made it
  cheap there. The mitigation is already proven: `edgar_identity`'s normalized
  issuer-name path reached **96.8% of lookable NAV** on N-PORT holdings, and its three
  fixes (truncate at the first `/`, delete apostrophes, strip `companies`/`cos`) apply
  unchanged. ✅ **Measured on 13F names — it transfers.** See E3 below.
- **`votingAuthority` Sole / Shared / None** — a manager can hold shares it cannot vote.
  That is a character fact available nowhere else and worth its own pass.
- **`investmentDiscretion`** (`SOLE` / `DFND` / `OTR`) says whether the manager decided.

⚠️ **The example above is ONE filer's conventions, and nearly every field in it varies.**
Berkshire and Motley Fool Asset Management, same form, same quarter:

| field | Berkshire | Motley Fool AM |
|---|---|---|
| `titleOfClass` | **9 distinct values**, all share classes: `COM` (69), `CAP STK CL A` (4), `COM SER A` (4), `COM SHS SER C` (4), `CL A` (4), `COMMON STOCK` (2), `CAP STK CL C`, `COM NEW`, `CL B` | `Ordinary Shares` (234), `American Depository Receipts` (3) |
| `nameOfIssuer` | `ALLY FINL INC` | `NVIDIA Corp.`, `Amazon.com, Inc.`, `JPMorgan Chase &amp; Co.` |
| `investmentDiscretion` | `DFND` on **all 90** | `SOLE` on all 237 |
| `otherManager` | present on **90 of 90** | absent entirely |

⚠️ Berkshire spells the same instrument `COM` 69 times and `COMMON STOCK` twice **in one
filing**, which is the shortest available proof that this field carries no vocabulary.

Two consequences, both of which will bite a parser written against one filer:

- **`titleOfClass` has no closed vocabulary.** Do not build an enum from `COM`.
- **`nameOfIssuer` casing and entity references differ**, and `&amp;` is the live one.
  **This is the exact trap that silently truncated `Johnson &amp; Johnson` to
  `"Johnson"` in the N-PORT parser** and dropped every mega-cap with an ampersand out
  of the look-through. `JPMorgan Chase &amp; Co.` is Motley Fool's 8th largest position
  at $62.7m. Decode entities before normalizing, and assert it on a `&`-bearing name.

### The form's three native partitions, measured on three filers

A 13F row carries **no asset category, no country, no issuer category and no fair-value
level.** `retort-the-n-port`'s scene is built from exactly those four fields, so none of
its partition machinery has anything to run on here. The only partition keys the form
itself supplies are the three that describe **the manager's relationship to the
security** rather than the security: who decided, who can vote, who inside the
organization holds it. Measured live 2026-08-01 on the same quarter (2026-03-31):

| | Berkshire (90) | MFAM (237) | MFWM (124) |
|---|---|---|---|
| `investmentDiscretion` | `DFND` ×90 | `SOLE` ×237 | `SOLE` ×124 |
| `votingAuthority` | Sole on all 90 | Sole on all 237 | **None on all 124** |
| `otherManager` | 90 of 90 | 0 | 0 |
| `titleOfClass` used for | share class | instrument type | instrument type |

Four things follow, and they shape the scene more than anything else in this plan.

**1. Every native partition is degenerate on every filer measured.** Not sparse, not
mixed: one value for the whole book, on all three. A partition with one bucket is a
**one-line fact, never a table and never a chart.** The guide must detect that and
render it as a sentence, or it ships three empty figures per report.

**2. Degenerate is not null, and the strongest fact in the sample is a degenerate one.**
MFWM reports `<Sole>0</Sole><Shared>0</Shared><None>10801</None>` on every row: it holds
$884m of stock and **can vote none of it**, while declaring `SOLE` investment discretion
on all of it. It decides every trade and holds no proxies. MFAM, same brand, same
address, votes all 237 solely. That contrast is filed, structural, and available nowhere
else. Sit it beside the ADV account counts (MFAM 0 clients / 6 accounts; MFWM 1,808 /
2,112) and the two facts explain each other with nobody attributing anything, which is
`weigh` invariant 7 working exactly as intended.

**3. `otherManager` is present-or-absent, never sparse** — 90 of 90 or 0 of N. It is a
fact about how the filing is organized, so treat it as a mode the whole filing is in.

**4. `titleOfClass` means different things to different filers, and for one of them it
is the fund partition.** Berkshire uses it for share class; MFWM uses it for instrument
type and files `Equity ETFs` ×14 + `Bond ETFs` ×14 — **exactly the 28 fund positions
measured independently at 25.07% of value.** So *that* those positions are funds was
filed all along; only *which* funds needs E5. Read the field as a filer-supplied hint,
corroborate it against resolution, and never build a vocabulary from it.

### Two traps already solved in the repo, and one not

**Solved — the units cutover.** `normalize_value(raw, filing_iso)`
(`edgar_thirteenf.rs:56`) multiplies by 1,000 for filings before `2023-01-03`, when
13F values were reported in thousands. Any multi-year manager history crosses that
boundary; use the helper, never the raw column.

**Solved — amendment supersession.** The existing dedup keeps one row per
(manager, quarter) taking the latest-filed accession.

**Not solved — the info table has no fixed filename.** Berkshire's is `53405.xml`. The
document must be located through the filing's `index.json`, never by guessing
`infotable.xml`.

⚠️ **And the trap is WORSE than it reads, because the guess usually works.** Both Motley
Fool filings name theirs `infotable.xml`, exactly as a hardcoded path would expect.
Berkshire is the exception. So a parser that guesses passes on most filers, ships, and
fails later on a specific manager — the failure mode that survives review. `index.json`
is not defensive coding here; it is the only correct route.

### The DERA route, sized

| measure | value |
|---|---|
| available quarterly zips | 53 |
| naming | **rolling 3-month FILING windows**, e.g. `01mar2026-31may2026_form13f.zip` |
| size | **99.4 MB** each |

`edgar_dera.rs` already handles the index page, the basename regex and the window
selection (`select_zip`, `quarter_ends_in_window`), and streams `INFOTABLE.tsv` through
a filter rather than loading it.

---

## The first subject: both Motley Fool managers, run separately

Two managers file 13Fs from the same Alexandria address under one brand. Verified live,
period 2026-03-31:

| | Asset Management | Wealth Management |
|---|---|---|
| CIK | 1512814 | 1607978 |
| 13F filings | **63**, 2011-02-11 → 2026-05-12 | **47**, 2015-02-13 → 2026-05-12 |
| positions | 237 | 124 |
| of which funds (ETFs) | **0** | **28 · 25.07% of value** |
| voting authority | **Sole on all 237** | **None on all 124** |
| investment discretion | SOLE on all 237 | SOLE on all 124 |
| declared value | $2,326,025,992 | $884,214,118 |
| reconciles to the table | **exactly, to the dollar** | check on build |
| `otherIncludedManagersCount` | 0 | 0 |
| 13F file number | 028-14177 | 028-16557 |
| also files | — | **N-PX** (2024-10-31, 2025-09-17) |
| ADV CRD / SEC# | 149688 / 801-69975 | 165689 / 801-77616 |
| ADV latest filing | 12/23/2025 | 04/20/2026 |
| employees (advisory) | 14 (12) | 39 (28) |
| clients / accounts | 0 / 6 | 1,808 / 2,112 |
| fee basis (ADV 5E) | percentage of assets only | percentage of assets only |
| ADV Item 11 disciplinary | all N | all N |
| **regulatory AUM** | **$2,603,671,290** | **$1,595,324,821** |

**The guide reads ONE manager. Run it twice.** Pairing them is the operator's act, never
the guide's — which keeps the "no cross-manager ranking" rule intact rather than
negotiating with it on the first subject.

Four facts here are already findings, and all four are structure rather than intent:

- **MFWM decides every trade and holds no votes.** `SOLE` investment discretion on all
  124 rows, `<Sole>0</Sole><Shared>0</Shared><None>N</None>` on all 124. Put that beside
  its ADV account structure (1,808 clients, 2,112 accounts) against MFAM's (0 clients,
  6 accounts, and sole voting on all 237) and the two filed facts sit adjacent with
  nobody attributing anything. This is the guide's best scene material and it comes from
  a field the earlier draft expected to be a *split* rather than a *constant*.

- **They are separate filers, not co-filers.** `otherIncludedManagersCount: 0` on both.
  One brand, one address, two books, two file numbers.
- **Only Wealth Management files `N-PX`**, the proxy voting record. That pairs directly
  with stage 5 (what they cannot vote): one manager's voting is on the record and the
  other's is not.
- **Both span the 2023-01-03 units cutover** — 15 and 11 years respectively. Landmine 2
  is live on both, so this subject exercises it rather than merely documenting it.

### The coverage ratio, and why it is the frame for the whole guide

Form ADV reports regulatory AUM; the 13F reports the value of §13(f) securities. Both are
filed by the same entity to the same regulator under different rules, so the ratio is
subtraction and attributes nothing:

| | ADV regulatory AUM | 13F reported value | **13F as % of AUM** |
|---|---|---|---|
| MFAM | $2,603,671,290 | $2,326,025,992 | **89.3%** |
| MFWM | $1,595,324,821 | $884,214,118 | **55.4%** |

This answers the one question a 13F structurally cannot answer about itself: **how much of
the manager are you actually looking at?** A reader of MFWM's 13F is seeing a bit over
half the business, and nothing in the 13F says so. The missing 45% is mutual funds, bonds
held directly, cash and non-US listings — everything outside the Official List.

That makes it stage 2's headline number rather than a footnote. It is the same evidentiary
move as the family's best material, one level up: `retort-the-n-port` holds a fund's claim
against filings the fund did not write; here we hold a 13F against the **same filer's own
other form**, which it cannot retroactively align because the two have different scopes,
thresholds and deadlines.

⚠️ **It is a fourth clock and the dates must ride on the number.** MFAM's AUM is as of a
12/23/2025 filing against a 2026-03-31 holdings date. Stating the ratio without both dates
implies a simultaneity that is not there — `retort-the-n-port`'s three-clock problem with
one more hand on the dial.

Concentration, for cast sizing: Asset Management's top 1 is **7.2%**, top 5 **30.2%**,
top 10 **43.8%**, top 25 **62.6%**. **98 of 237 positions sit under 0.1% of value and
combine to 2.95%.** That tail is what will kill the report if promotion is not strict.

## The flip, stated precisely

`holdings_for_cusip(submission_tsv, coverpage_tsv, infotable_tsv, target_cusip)`
answers **"who holds this company"**. The manager-centric question is **"what does this
manager hold"**, and the two are not symmetric in one specific way:

> **`INFOTABLE` carries `ACCESSION_NUMBER`, not `CIK`.**

So the CUSIP filter is a single streaming pass over one column, while a manager filter
is **two passes**: resolve manager CIK → its accessions from `SUBMISSION.tsv`, then
filter `INFOTABLE.tsv` by that accession set. `read_member_filtered` takes a single
needle today and needs a set.

**And for a single manager the DERA route is usually the wrong one.** One manager's own
filing is two documents totalling ~50 KB; the DERA zip for the same quarter is 99.4 MB.
Use DERA only where the question is genuinely cross-sectional (this manager's position
against all reported institutional value that quarter — the existing `pct_of_value`
field). Default to the manager's own filings.

---

## Engine work (`dapple-prompt-flow`)

### E1. `edgar manager <cik|name> [--quarters N]` — PHASE 1, elaborated 2026-08-01

**Status: OWED, and it is the gate on everything else here.** Stages 1 and 3–7 all read this
command's output, so no part of the guide can be built before it. Elaborated to build quality
below; nothing in this section is designed, all of it is either measured or cited to a line
already in the tree.

Reads the manager's **own** 13F filings over the Archives route (~50 KB/quarter). Not the DERA
route — landmine 5.

#### It should be recognizably `edgar adv`'s sibling

`edgar adv` (E6) is the most recently built command in the family and it is the standard this
one follows, point for point. Each row is a convention already in the tree, not a preference:

| # | The convention | Where it is written |
|---|---|---|
| 1 | Two files — pure logic in `src/edgar_manager.rs`, fetch + memo + nushell shape in `src/commands/edgar_manager.rs` — and the module doc says so out loud | `commands/edgar_adv.rs:10-11` |
| 2 | A module doc naming the question this command answers that its neighbours cannot | `commands/edgar_adv.rs:3-8` |
| 3 | `ShellError::GenericError` with `error`/`msg`/`span`/`help`, where `help` names the source URL and what probably changed upstream | `commands/edgar_adv.rs:34-42` |
| 4 | Absent is `Value::nothing`, never a default — the `opt_str`/`opt_int`/`opt_float` helpers. This is landmine 8 expressed in code | `commands/edgar_adv.rs:112-122` |
| 5 | Explicit outcome states over null-or-value | `commands/edgar_adv.rs:164-185` |
| 6 | Never match a varying filename by fixed string: take the sole member matching a shape and *"say so if that assumption ever breaks, rather than silently reading the first thing in the file"* | `commands/edgar_adv.rs:83-99` |
| 7 | Counted skips and staleness surface as `log::warn!`, and the same condition is an assertion in the live test | `commands/edgar_adv.rs:44-58` |
| 8 | Comments record the measured trap, not the mechanism | throughout |

#### Return shape

One row per filing, newest first — the `edgar nport` / `edgar nmfp` shape (a period record with
nested positions), not a flat holdings table. **One row per FILING, never per quarter**: an
amendment is its own row, so a consumer folding a quarter does it with the amendment type
visible rather than behind its back (landmine 7).

The pure layer's types are built and carry all of this; ✚ marks a field added while
building against real filings.

| group | fields |
|---|---|
| identity | `manager`, `cik`, `accession`, `form`, `filing_date`, `period_date`, `quarter` |
| outcome | `outcome` ∈ `holdings_reported` \| `notice_only` \| `no_filings` \| ✚`unrecognized_form` |
| cover | `confidential_omitted` (three states, landmine 8), `other_managers_count`, `other_managers[]` (✚ each `{sequence, name, file_number, cik}`; `sequence` is null for cover-page entries — the form carries two disjoint lists), `report_type`, ✚`form13f_file_number`, ✚`instruction_5`, ✚`additional_information` (the relationship prose on a notice — landmine 19), ✚`signature_date` |
| ✚ amendment | null unless `is_amendment`; else `{number, kind ∈ restatement \| new_holdings \| unspecified, reason, conf_denied_expired, date_denied_expired, date_reported}` (landmine 7) |
| reconciliation | `reported_entry_total`, `parsed_entry_count`, `reported_value_total`, `parsed_value_total`, `reconciles`, ✚`entries_match`, ✚`value_match` — all nullable, because a notice states no totals and *nothing to check is not checked-and-passed* |
| positions[] | `name_of_issuer`, `cusip`, `title_of_class`, `value_usd`, `shares_or_prn`, `sh_prn_type`, `put_call`, `investment_discretion`, `voting_sole`, `voting_shared`, `voting_none`, `other_manager`, ✚`other_manager_seqs[]` (the cell is a comma-separated list of *sequence numbers into the cover roster*, e.g. `"2,4,11"` — not names) |
| | `warnings[]` |

✚ **`holdings[]` + `holding_count`** alongside `positions` — one entry per
`(cusip, sh_prn_type, put_call)` with `row_count` and `pct_of_portfolio`. Landmine 21:
`position_count` is filed ROWS and is not the number of things held.

**`--tenures` returns a different shape** — `{ manager, cik, window, quarters[], tenures[],
history }` — because a tenure spans the window rather than sitting inside one filing. It is
the same fetch; without the flag the output is unchanged.

| group | fields |
|---|---|
| `quarters[]` | the per-period fold: `fold` ∈ `clean` \| `restated` \| `amended` \| `ambiguous` \| `notice_only` \| `no_base`, `observable`, `base_accession`, `accessions[]`, `holdings[]`, `total_value_usd` (landmines 7, 23) |
| `tenures[]` | `cusip`, `name_of_issuer`, `first_quarter`, `last_quarter`, `quarters_held`, `quarters_spanned`, `gaps[]`, `peak_pct`, `peak_quarter`, `latest_pct`, **`entered_before_window`**, **`is_open`**, **`fully_observed`**, `exited_quarter` (landmine 22) |
| `movements[]` | `quarter`, `quarter_end`, `shares`, `value_usd`, `pct_of_value`, `delta_shares`, `delta_pct`, `status` ∈ `opening` \| `new` \| `added` \| `trimmed` \| `held` \| `exited` \| `unknown`, `streak` — field names match `edgar holders` |
| `history` | `quarters_observed`, `distinct_positions`, `fully_observed_tenures`, `censored_tenures`, `median_quarters_held` (nullable), `held_entire_window`(`_pct_of_value`), `in_and_out_within_two_quarters`, `new_reaching_top_ten`, `turnover_by_quarter[]` |

**`turnover_pct` is snapshot difference, not trading.** A position opened and closed inside
one quarter appears in neither snapshot, so it is a lower bound on activity — the same
91-day-window discipline as landmine 15, and it must never be described as trading turnover.

**Movements are reachable BOTH ways** (added 2026-08-01). `tenures[].movements[]` is
position-first; a top-level **`movements[]`** carries the same readings quarter-first with
`cusip` and `name_of_issuer` folded in, so stage 7's "what changed in 2025Q3" is a
`where`, not a per-guide flatten. Verified live: 341 movements, flat count == nested count.

⚠️ **Each `quarters[]` entry carries `changes` (counts by status), and it is NULL when the
period reported nothing.** This is deliberate and load-bearing: a bare filter over a
flattened movement list returns **empty for both** "the manager filed and nothing moved"
and "the manager filed a notice", so a guide would report "no changes" for a quarter with
no data behind it. All-zero on an `observable: true` quarter is the first; `changes: null`
with `observable: false` is the second. Berkshire's own 12-quarter window contains one, so
this is exercised rather than theoretical.

**`--persist`** writes the folded holdings to `rev.institutional_holdings` for SQL — see E2.

#### What is reusable, and what is not

Reuse unchanged from `edgar_thirteenf.rs`: `normalize_value` (:56), `to_iso_date` (:66),
`quarter_label` (:110), `normalize_cusip` (:118), `iso_to_date` (:471). The pure layer as
built uses the first three.

⚠️ **`dedup_latest` (:271) was on this list and has been REMOVED (2026-08-01).** It keeps
the latest-filed accession per (manager, quarter), which is correct for the per-CUSIP DERA
question it was written for and catastrophic here: Berkshire's 2025Q1 `NEW HOLDINGS`
amendment would replace a $263bn quarter with the $1.1bn of positions it *adds*. See
landmine 7. E1 returns one row per filing and leaves the fold to the consumer, which is
why it needs neither this nor a corrected replacement.

⚠️ **`compute_movements` (:306) is NOT reused either, and this plan's suggested workaround
does not survive.** The text above proposed "a projection from the new row type"; building
the tenure layer showed two requirements it cannot meet:

- **It drops no-change rows by design** (`edgar_thirteenf.rs:301-305`) because its
  deliverable is how a holder base *moves*. A tenure is a **span** and needs continuity —
  dropping `held` quarters turns every span into a set of holes.
- **It emits nothing for the first period**, which is right when the baseline is not a
  reading. A tenure must report that quarter, and must call it `opening` rather than `new`
  because we cannot see before the window (landmine 22).

Plus both need the fold to exist first. So `edgar_manager` has its own movement/tenure
layer, reusing the **vocabulary** (`new`/`added`/`trimmed`/`exited`, and the wire field
names from `commands/edgar_holders.rs:314`) and not the function. `edgar_thirteenf.rs` is
untouched: it is correct for the per-CUSIP DERA question it serves, and `edgar holders`
depends on exactly its current behaviour.

⚠️ **`HoldingRow` (:27) cannot be reused, and the code confirms what this plan already said.**
It carries manager/period/quarter/accession/filing-date/shares/value and none of
`title_of_class`, `investment_discretion`, `voting_authority` or `other_manager`. E1 needs its
own row type. `parse_tsv`, `holdings_for_cusip` and `select_zip` are DERA-shaped and irrelevant
here. `compute_movements` takes `&[HoldingRow]`, so feeding it wants a projection from the new
row type rather than a signature change — that keeps the DERA path untouched.

#### The parser is the work — ✅ BUILT (`src/edgar_manager.rs`, 2026-08-01)

⚠️ **No 13F XML parser existed anywhere in the repo** (verified 2026-08-01). `nameOfIssuer`
appears only in `edgar_nmfp.rs`, which is a different form. Two documents per filing:

- `primary_doc.xml` — the cover page declarations. `parse_cover`.
- the information table — **no fixed filename** (landmine 4). Resolve through edgarkit's
  existing `filing_directory` helper, already used by `edgar_exhibits`, `edgar_filings` and
  `edgar_fund_sections`, then take the **sole non-`primary_doc` `.xml` member** and fail loudly
  on 0 or 2+, per convention 6 above — `info_table_member`, kept pure so the rule is testable
  without a network. **Gate it on `filing_kind` first**: a notice has no such member and would
  otherwise fail loudly on a valid filing (landmine 19).

Use `roxmltree`, as `edgar_nmfp.rs` and `edgar_fund_linkbase.rs` already do.

⚠️ **Match on the LOCAL element name, never a qualified one, and never via
`descendants()`.** Two independent traps, both live:

- **Namespaces vary by era.** A 2026 Berkshire information table declares none; the same
  filer's 2019 table declares a default `xmlns=".../informationtable"`; a 13F-NT cover
  declares a default namespace *and* puts `ns1:` on its address children. `roxmltree`'s
  `has_tag_name` compares only the local name when handed a bare `&str` (0.21
  `lib.rs:1030-1033`), so the house idiom is already namespace-agnostic — **by accident of
  the API**. A well-meant "correctness" fix to a fully qualified name silently drops every
  pre-2023 filing. Proven: qualifying it turns 4 of the 21 tests red.
- **`name` is not unique within the cover.** It occurs under `filingManager`,
  `signatureBlock` and every `otherManager`. A `descendants()` search returns whichever
  comes first in document order, which is the manager today and the signatory the moment a
  filer reorders. Every lookup is path-scoped through direct children.

#### ⚠️ The front door is broken for the guide's own first subject

**Found 2026-08-01, and it invalidates the `<cik|name>` half of this command's own signature.**
`resolve_cik` (`commands/edgar_common.rs:50`) is digits-or-**ticker**, and `edgar.company_cik`
resolves against `company_tickers.json`, which holds only **listed companies**. But a 13F filer
is a *manager*, not necessarily an issuer. So:

| input | resolves today | why |
|---|---|---|
| `edgar manager 1067983` | ✅ | all-digits, taken as a CIK |
| `edgar manager BRK-B` | ✅ | Berkshire is a listed issuer |
| `edgar manager "Motley Fool Asset Management"` | ❌ | not a listed issuer, so not in `company_tickers.json` |

The command would pass on Berkshire and fail on both first subjects. Resolve instead through
`browse-edgar?action=getcompany&company=<name>&type=13F&output=atom` — the same endpoint
`locate_fund_filings` (`edgar_common.rs:76`) already uses, with `company=` in place of `CIK=`.
Scoping the query to the form makes it self-disambiguating: only actual 13F filers can match.

**Measured 2026-08-01, and the prescription needs one amendment.** The premise holds —
`company=vanguard` returns 100+ companies unscoped and **12** with `type=13F`; `apple` goes
100+ → **3**. Use `type=13F`, not `13F-HR`: it prefix-matches, so `HR`, `HR/A`, `NT` and
`NT/A` all come back and the notices are not silently dropped. Three response shapes, and
the discriminator is clean:

| outcome | response | how you know |
|---|---|---|
| one filer | the FILINGS feed, with the company block | `<conformed-name>` present |
| several filers | the company-list feed | entries with `<cik>` but **no** `<conformed-name>` |
| none | an empty feed titled `Company Search Feed` | no entries |

⚠️ **The multi-match feed carries no names** — `name="ARRAY(0x5589e41ff350)"`, a leaked
Perl array ref (landmine 20). So `ambiguous` returns bare CIKs unless E1 spends one small
request per candidate to name them. Recommend doing so and capping it, since this is an
error path a human is about to read. The single-match case is free either way: it already
returns the filings, so resolution and location are one request.

#### Probes (each must be able to fail — revert the fix and watch it go red)

✅ **Built and demonstrated red, 2026-08-01** — 21 unit tests over inline fixtures from four
real filings across three filers and two schema eras (`edgar_manager.rs` tests). Three
reversions were run to prove the probes bite, not merely pass:

| reversion | tests reddened |
|---|---|
| qualify element matching with a namespace | 4 — every namespaced document |
| swap month/day in `cover_date_to_iso` | 4 — including the derived quarter |
| drop `normalize_value` from reconciliation | 1, reporting `735158` for `735158000` |

Covered: `reconciliation` (both sides close on Berkshire; a seeded corruption reports
rather than errors, and names *which* side broke) · `units` (pre-cutover values scale on
**both** sides, so the cutover cannot manufacture a mismatch) · `notice-only` (states no
totals; `reconciles` is null, not true) · `filename` (all three observed names; 0 and 2+
fail with the offenders named) · `ampersand` (`JOHNSON & JOHNSON` survives) ·
`optional-cover-field` both directions (present on Berkshire, absent on Motley Fool) ·
`degenerate-partition` (`SOLE` against `None` voting, neither flattened) · `amendment`
(`NEW HOLDINGS` distinguished from a restatement; an unnamed type is a third state) ·
`namespace` (a default namespace does not hide the rows) · `dangling-reference` (an
`otherManager` sequence the cover does not define is surfaced).

✅ **Live probes added and passing** (`tests/edgar_live_test.rs`, 2026-08-01) — three,
where E6 has one, because the command's riskiest behaviour is the part a fixture cannot
vouch for:

1. `manager_filings_reconcile_across_a_live_multi_year_window` — ten Berkshire filings,
   2023Q4→2026Q1. **All ten reconcile exactly**, including 2026Q1 at 90 rows /
   $263,095,703,570. Asserts the window spans **more than one** information-table
   filename, so the sole-member rule is exercised against names the test never states; it
   saw ten. Also prints the amendments, which is how the 20% figure in landmine 7 was
   measured.
2. `edgar_manager_runs_inside_step_engine_and_resolves_an_unlisted_manager` — the whole
   command through a guide script, so registration and the security allowlist are tested
   too, not just the parser. `edgar manager "Motley Fool Asset Management"` and
   `edgar manager 1512814` reach the same filer (237 positions, reconciles), which is the
   landmine-17 fix proven end to end. Asserts `confidential_omitted` is **null**, not
   false, on the filer that omits it.
3. `manager_name_resolution_answers_ambiguous_and_absent_without_erroring` — the other
   two rows of the resolution table. `"motley fool"` returns `ambiguous` with **both**
   candidates *named* (`Motley Fool Asset Management LLC`, `Motley Fool Wealth
   Management, LLC`) even though the search feed carries no names at all; a needle
   matching nothing returns `no_filings`. Both are records a script branches on, never
   errors it must catch.
4. `manager_tenures_hold_across_a_live_window` — the tenure layer over 12 Berkshire
   filings. Measured payload: **9 periods, 8 observable** (the 9th is a 2023Q4 amendment
   whose original is older than the window — correctly a fragment, not a 1-position
   quarter); **29 holdings** from ~90 rows; every observable quarter's weights sum to
   **99.98%**; the one `amended` quarter totals **$259,807,694,872** — the landmine-7
   assertion, which fails loudly at ~$1.1bn if the fold ever regresses to latest-wins;
   `APPLE INC` is the largest position by peak weight and reports
   `entered_before_window: true`; **41 censored tenures against 7 fully observed**, with
   `median_quarters_held` computed over the 7.

Unit tests on inline fixture XML, as `edgar_nmfp.rs:1184` does, plus one live test — matching
E6's 13 unit + 1 live. Then the five-item housekeeping list from E4.

### E2. Persisting a manager's holdings — ✅ BUILT 2026-08-01, and NOT as the DERA flip

**This section used to read "the DERA-side flip in `edgar_thirteenf.rs`, two-pass;
`read_member_filtered` grows a set-valued needle."** That was written before `edgar manager`
existed. Re-examined when the work was scheduled, and the DERA route turned out to be the
wrong tool for what E2 is actually for:

| | DERA route | Archives route (`edgar manager`) |
|---|---|---|
| cost per quarter | **99.4 MB** | **~50 KB** |
| amendments | `dedup_latest`, latest-wins (landmine 7) | folded, `new_holdings` vs `restatement` |
| rows already in hand? | no — a fresh bulk download | **yes**, `--tenures` has already folded them |

The DERA flip only wins for managers you have **not enumerated**, which is `edgar holders`'
CUSIP-first job and already built. So E2 shipped as **`edgar manager --persist`**, writing
what the command already holds into the same `rev.institutional_holdings` cache. No bulk
download, no reader refactor. `read_member_filtered` is untouched.

⚠️ **A manager-first write must never mark a `(cusip, quarter)` slice ingested.** That
marker means "every manager holding this security this quarter is stored", and
`edgar holders` trusts it to SKIP its download (`commands/edgar_holders.rs:188-198`).
Claiming one from here would make `edgar holders <cusip>` return the one manager we
happened to store **and present it as the whole holder base** — every row genuine, the
coverage claim false, and nothing else in the suite would catch it. Migration 94 adds a
separate `(manager_cik, quarter_end)` marker; deletes are manager-scoped, never by cusip.
The negative assertion is pinned in both `tests/manager_holdings_persist_test.rs` and the
live test, and was demonstrated red.

⚠️ **Both writers must agree what a row means.** `holdings_for_cusip`
(`edgar_thirteenf.rs:229-233`) stores only plain long share positions — no `putCall`, no
`PRN`. `persist_plan` applies the identical filter and REPORTS what it dropped
(`skipped_derivative` / `skipped_non_share` / `skipped_bad_cusip`), so the same table
cannot come to mean two things depending on which command filled it. A period that reported
nothing persists **nothing**, never an empty set — an empty set would record "held nothing"
for a quarter that reported nothing.

Verified live on Motley Fool Asset Management: 480 rows over 2 quarters, equal to the
aggregated holding count (that filer never repeats a CUSIP, so any aggregation slip would
show); no CUSIP marked; cleanup restored the table exactly, **leaving untouched three
single rows in 2025 Q1–Q3 that an earlier CUSIP-first ingest had left for the same
manager** — which is the property that lets the two writers share one table.

### E3. Name resolution rate — measured, and it SPLITS by position type

Run `edgar_identity`'s normalizer over the `nameOfIssuer` corpus of six managers of
different styles and report resolved / no-filer / unresolved as a **partition**, not a
rate. Repo memory is explicit that collapsing `no_filer` into `unresolved` hides a real
distinction and would report a bond-heavy manager as unreadable.

**Measured live on Motley Fool Asset Management's 237 names** (2026-08-01), against
`company_tickers.json` with the normalizer's three fixes applied unchanged:

| bucket | n | by value |
|---|---|---|
| resolved | 219 | **95.09%** |
| ambiguous, failed closed | 1 | — |
| unresolved | 17 | 4.91% |

Against N-PORT's 96.8%, **the technique transfers** and the look-through stages are not
blocked on new identifier work.

⚠️ **One manager is not the measurement, and this was the easy one.** A large-cap US
long-only book is where issuer names are most standard. The five still owed should be
chosen for where it should *break*: a small-cap manager (thinner names, more recent
registrants), an ADR-heavy one (Motley Fool holds 3 — `Relx PLC`, `Rentokil Initial
PLC`, `Taiwan Semiconductor Manufacturing Co., Ltd.`), and one holding closed-end funds
or trusts. Report the partition per manager; **do not report a family average**, which
would hide exactly the manager the number matters for.

⚠️⚠️ **The gate was measured on a book with no funds in it, and fund positions score
zero.** MFAM is 99.18% ordinary shares. Running the same normalizer over MFWM, which is
25.07% ETFs:

| corpus | resolved |
|---|---|
| MFAM, 237 positions, no funds | 95.09% of value |
| MFWM, 124 positions, 28 of them funds | 56.5% of value |
| **the 28 fund positions alone** | **0 of 28** |

This is not a matcher-quality problem and no amount of normalization fixes it.
`company_tickers.json` is a map of exchange-listed **operating-company** filers; it holds
no fund series at all. What it does hold is worse than nothing — `iShares Gold Trust`,
`iShares Bitcoin Trust ETF`, `DoubleLine Yield Opportunities Fund`, `VanEck Solana ETF`.
A looser matcher lands `ISHARES CORE MSCI EUROPE` on a gold trust and every downstream
judgement inherits it. **Keep failing closed.**

And the deeper reason it cannot work: **`nameOfIssuer` on a fund position is the TRUST,
not the fund.** Two MFWM positions are both named `DOUBLELINE ETF TRUST`, differing only
by CUSIP. `iShares Trust` is 367 distinct series under one CIK. Name resolution cannot
separate them even in principle, so the fund arm needs its own instrument — see E5.

**So E3's real deliverable is a partition per POSITION TYPE, not one rate per manager.**
A blended 56.5% describes neither half of MFWM's book.

### E4. Classifier

✅ **Done 2026-08-01.** `InstitutionalManager` redirects to `mind-the-13f`, and all three
`redirect_guide` arms are now asserted by slug in `out_of_scope_kinds_name_their_guide`
(443 unit tests green). Nothing is owed here.

Housekeeping for any NEW engine command this guide adds: register in `commands/mod.rs`,
allowlist in `engine/security.rs`, parent help in `commands/edgar.rs`, live tests, and
**rebuild `dapple-prompt-api` too** — guide script parse happens API-side. The redirect
change needed none of that: it added no command.

> **Already done, so nobody re-derives it (2026-08-01).** `edgar fund`'s series registry
> was pinned to a decade-old snapshot and could not resolve any fund registered after
> ~2016. Fixed: the URL now points at the 2024 snapshot, `company_tickers_mf.json` is a
> tail source for anything newer, and a miss above the snapshot ceiling is a distinct
> loud error. **This does not touch `redirect_guide()`** — E4 is still owed. It is
> mentioned here because a 13F team will run `edgar fund` on a manager, and it is worth
> knowing the redirect it prints is now the only stale thing in that path.

> ⚠️ **`edgar fund` REFUSES a 13F manager outright, and the fix already shipped
> (2026-08-01, from the `price-the-mfp` build).** `FilerKind::in_scope()` is
> `matches!(self, OpenEndFund | ClosedEndFund)` — one guide's scope rule living in a
> shared command. So `edgar fund <manager>` hard-errors with "is not a filer this caller
> can read" before returning anything, and stage 1 cannot resolve its own subject.
>
> The command now takes **`--accept`**, a caller-supplied list of `FilerKind` keys.
> Omitting it preserves the historical N-PORT-only behaviour exactly, so nothing existing
> changed. This guide's stage 1 wants:
>
> ```nushell
> let identity = (edgar fund $manager_in --accept [InstitutionalManager])
> ```
>
> Keys are the `Debug`/`kind` tokens (`FilerKind::key()`), matched
> punctuation-and-case-insensitively, so `InstitutionalManager`, `institutional_manager`
> and `institutional-manager` all work and nothing looser does. `in_scope` and
> `redirect_guide` are still RETURNED on the record; they are reported, not obeyed. Do
> NOT wrap the call in a `try`: the refusal carries the redirect an operator can act on.
>
> `mark-the-bdc` needs the identical treatment with `[BusinessDevelopmentCompany]`.

### E5. Fund-position resolution — ✅ BUILT 2026-08-01 as `edgar security`

E3 established that a fund position cannot be resolved by name. It **can** be resolved,
by a bootstrap the family already owns half of. Verified live on three of MFWM's CUSIPs:

1. **A US ISIN embeds the CUSIP.** `US9229083632` contains `922908363`. Round 1 specified
   this as cascade step 1 of `edgar_identity` and it was **never built**, because name
   matching hit 96.8% without it. It is a substring, not a service.
2. **Any fund that HOLDS the ETF files an N-PORT naming it properly**, and `edgar search`
   (built in round 2) finds those filings by CUSIP.

| the 13F's mangled name | recovered from someone else's N-PORT |
|---|---|
| `VANGUARD INDEX FDS S&P 500 ETF` | `Vanguard S&P 500 ETF` |
| `DOUBLELINE ETF TRUST` (25861R303) | `DoubleLine Commercial Real Est` + **ticker `DCRE`** |
| `ISHARES IBONDS DEC 2029` | `iShares iBonds Dec 2029 Term Corporate ETF` |

Full path: **13F CUSIP → US ISIN prefix match in any N-PORT → the fund's real name and
sometimes its ticker → the series/class registry or `company_tickers_mf.json` → trust CIK
plus series id → `edgar nport`.** Every link ships today except the ISIN substring.
Cost is one `edgar search` call per fund position — 28 for MFWM, which is cheap.

⚠️ **The registry route reaches the TRUST, and the trust is not the answer.** Matching
`VANGUARD INDEX FDS` to entity `VANGUARD INDEX FUNDS` gives CIK 36405 and 12 series;
`ISHARES TRUST` gives one CIK and **367**. Only the recovered name or ticker pins the
series, which is why the N-PORT bootstrap is the load-bearing step rather than a
fallback. The registry carries no CUSIP column, so there is no shortcut.

**Shipped as `edgar security <cusip>`** (`src/edgar_security.rs` pure + its command),
accepting a list on the pipeline so a whole 13F's positions resolve in one pass. It stops
at IDENTITY and does not resolve the series — `edgar fund <ticker>` is that step and
already exists, so the two compose rather than duplicating the registry load:

```nushell
edgar manager $mgr --quarters 1 | first | get holdings | get cusip
  | edgar security | where ticker != null | get ticker | each { edgar fund $in }
```

Live-verified on `25861R303`: recovers `DoubleLine Commercial Real Estate ETF`, ticker
`DCRE`, ISIN `US25861R3030`, from **2 filers across 4 index hits**.

**Three things the build measured that this section did not anticipate** — landmines 24-26
below. The first two changed the design; the third was a defect in already-shipped code.

✚ **It works for ordinary equities too, which was not the intent.** `037833100` returns
`Apple Inc.` — thousands of funds hold Apple and every one names it. So `edgar security`
is a general CUSIP-to-name cleaner for any 13F position, not just the fund arm, and
`not_found` means *no registered fund reported holding it at all* rather than "this is an
operating company".

### E6. `edgar adv` — ✅ BUILT 2026-08-01, use it

**Shipped and live-verified.** `crates/dapple-prompt-flow/src/edgar_adv.rs` (pure) +
`src/commands/edgar_adv.rs` (fetch, memo, nushell). Registered, allowlisted, in the
parent help, `dapple-prompt-api` rebuilt. 13 unit tests + 1 live test; 407 unit tests
green overall, clippy clean under `-D warnings`.

```nushell
edgar adv "Motley Fool Asset Management"   # by name (the route a 13F can take)
edgar adv 149688                            # by CRD
edgar adv 149688 --raw | get "9A(1)(a)"    # escape hatch; refetches, not memoized
```

Returns `outcome` ∈ `registered` | `not_registered` | `ambiguous`, plus `crd`,
`sec_number`, `cik_self_reported`, `employees`, `advisory_employees`, `clients`,
`accounts_*`, `aum_*` (Item 5F regulatory AUM), `fee_bases` (Item 5E, labelled),
`related_person_items` (Item 7A keys), `disciplinary_items` (Item 11 keys; empty means
clean), `discipline_is_clean`, `brochure_url`, `snapshot`.

Live figures, reproduced by the test: **16,935 advisers, 0 rows skipped**, roster
decoded as windows-1252, newest filing 2026-06-30 against the 2026-07-01 snapshot.
MFAM 89.3% and MFWM 55.4% of AUM covered by the 13F.

⚠️ **Item 7A and 11 come back as raw item keys, deliberately.** The roster carries a
`Form Version` column because item numbering is version-dependent, so a baked label map
would silently mislabel older rows — a wrong name on a filed fact. Item 5E is the one
exception (its seven options are stable) and is labelled. For Item 11 the signal is
presence and count, not which sub-item, so an empty list *is* "clean record".

⚠️ **The fee RATE is not in this command.** Item 5E gives the basis; the rate is prose
in the Part 2A brochure at `brochure_url`, and nothing in the repo reads a PDF. Report
the basis and name the brochure. Do not estimate.

The research behind it, kept because it is what the command defends against:

Two halves, both measured 2026-08-01:

| what | where | size |
|---|---|---|
| Part 1A, all registered advisers, 448 columns keyed by ADV item number | `sec.gov/files/investment/data/other/information-about-registered-investment-advisers-exempt-reporting-advisers/ia{MMDDYYYY}.zip` | 5.2 MB zip → 42 MB CSV, 16,935 rows |
| Part 2A brochure, where the fee **rate** lives | `reports.adviserinfo.sec.gov/reports/ADV/{CRD}/PDF/{CRD}.pdf` | ~2 MB PDF |

**This is the same snapshot-staleness family as the fund series registry** — same
`sec.gov/files/investment/data/` tree, same periodic-snapshot publishing, same
inconsistent naming (the path moved between January and February 2026, and
`ia020226-exemptzip.zip` carries the SEC's own typo). Pin one URL, assert a freshness
invariant that can actually fail, and never build a candidate list that falls back
silently. That lesson is already paid for; do not pay again.

Four traps, all measured:

1. **The CSV is Windows-1252, not UTF-8** — invalid byte at row 11, field 216. Worse,
   `grep -i motley` returned **nothing** on a file that contains it, so a text search
   silently reports absence rather than erroring. Same family as the CSV-comma trap:
   a naive tool answering confidently and wrongly.
2. **`CIK#` is self-reported and is wrong for one of the two subjects.** MFWM's row says
   `160797`; its real CIK is `1607978`. It fails closed here only because `160797` is not
   an EDGAR entity, and a length check cannot detect it — 4-digit CIKs are legitimate
   (Invesco is 5100).
3. **Only 25.5% of rows carry a CIK at all** (4,320 of 16,935). So **CRD is the reliable
   key**, and a 13F does not carry one. The join is name → roster → CRD, with the CIK as
   a *confirmation* when present rather than the primary key.
4. **Not every 13F filer is a registered adviser.** Berkshire (CIK 1067983) has zero
   roster rows, correctly — it is an operating company that files 13F. Model
   `registered` / `not_registered` / `unresolved` and never collapse the last two, the
   same partition discipline `edgar resolve` already enforces.

**Deferred, and say so rather than approximating:** the fee **rate** is in the Part 2A
PDF, and nothing in the repo reads a PDF (`dapple-render` writes them). Part 1A gives the
fee **basis** structurally and for free (`5E(1)=Y`, percentage of assets), which is a
real fact. Report the basis; do not estimate the rate.

---

## The story: what the inversion does to scene, tracks and load

> Added 2026-08-01 after Kenny's review. Everything above this line was written by
> reading the *record*. This section is written by reading the *report*, and it exists
> because the earlier draft carried its narrative doctrine as a six-bullet list copied
> from `retort-the-n-port` with the nouns swapped. Copying is how a guide loses the
> story it was going to tell.

### The scene survived; its furniture did not, and nobody noticed

The one-line doctrine "the manager is the scene, positions are its contents" is true and
almost useless, because it says nothing about **what furnishes the scene** — and in both
shipped guides the furniture comes from the subject's own structured self-description:

| guide | the scene is | furnished by |
|---|---|---|
| `run-the-10k` | the company | its segments: Item 1 prose + segment XBRL |
| `retort-the-n-port` | the fund | `asset_cat`, `inv_country`, `issuer_cat`, `fair_val_level`, filed **per position** |
| `mind-the-13f` | the manager | **none of the above exist on a 13F row** |

That is the inversion, and it is not a detail of emphasis. A 13F row describes the
manager's *relationship to* a security, not the security. So the four fields retort
partitions on are absent, the three that remain are degenerate on every filer measured,
and a guide that copies retort's scene builder gets four empty tables.

The trap waiting there is worse than emptiness: the obvious repair is to resolve each
CUSIP to a filer, read its SIC code, and partition by sector. **That builds the scene
out of material the subject never filed**, which is precisely what run-the-10k's
"an invented scene is dropped" rule forbids. A sector breakdown of a 13F is a fact about
the companies, not about the manager, and the manager is the subject.

**What actually furnishes it, all filed by the subject about itself:**

1. **The organizational facts, from Form ADV** — employees, advisory employees, clients,
   accounts, fee basis, discipline, regulatory AUM. This is the room the conduct happens
   in, and `edgar adv` (E6, shipped 2026-08-01) makes it free. MFAM at 14 employees and
   6 accounts is a fund adviser; MFWM at 39 employees and 1,808 clients is a retail
   wealth business. Two completely different rooms, and the 13F alone never says so.
2. **The form's three native partitions**, rendered as sentences when degenerate and as
   figures only when they are not. See the measurement above: this is where MFWM's
   "votes none of it" lives.
3. **The coverage arithmetic**, which is the scene's numbers.

### The scene sum check ports, and it grows a second level

run-the-10k's cheapest and most productive check is that scene revenue must equal
consolidated revenue. Retort ports it as: every partition totals the portfolio. Here it
has **two levels, and they are different kinds of statement** — which must be said
plainly or somebody builds a check that fails spuriously and gets deleted.

**Inner level: an identity, and the filer states both sides.**

```
sum of <value> over the info table  ==  declared tableValueTotal
count of infoTable rows            ==  declared tableEntryTotal
```

Berkshire closes to the dollar (90 / $263,095,703,570); MFAM closes to the dollar
(237 / $2,326,025,992). A mismatch is a misfiling detectable **with no outside data at
all**, which no sibling guide's scene check can claim. Every partition of the book must
also total `tableValueTotal`, exactly as retort's total the portfolio.

**Outer level: a difference, NOT an identity.**

```
13F reported value  +  what the form structurally cannot show  ≈  ADV regulatory AUM
```

MFWM: $884m visible against $1,595m of regulatory AUM, so **$711m of the room is dark.**
This is the guide's headline number and the dark bucket is a first-class part of the
scene rather than a caveat about it.

⚠️ **It can never be made to close, and pretending otherwise is the error.** The two
forms have different scopes, different thresholds, different deadlines and **different
dates** (MFAM's AUM is as of a 12/23/2025 filing against 2026-03-31 holdings). So the
outer level is reported as a difference with both clocks on it, never asserted as an
identity and never used as a validity check. If the 13F value *exceeds* AUM, that is
itself worth stating and is not a bug in the arithmetic.

### A track is a position's tenure

The plan carried no definition of a track at all, which is how a guide ends up with
tracks that are just its section headings.

The shipped definition is run-the-10k invariant 33: **a track is a cross-section
through-line, grouped by scene** — it exists so the segment, the revenue it earns, the
risks written against it and the claims made for it stop being four unrelated entries in
one flat pool. Retort's root is a **claim**, with evidence nested beneath. There are no
claims here, so that root does not exist and something has to take its place.

**The root is a position's tenure**, and it is the right answer for four reasons:

- **It is the only unit in a 13F with a shape over time**, and time is the only axis
  this record has. A single 13F is 90 rows and no narrative; the story exists across
  quarters or it does not exist.
- **It fixes exactly the fragmentation tracks were invented for.** A position's entry
  sits in stage 4, its movements in stage 7, its issuer's filings in stage 8, its voting
  status in stage 5 and its routing in stage 9. **Five stages, one subject.**
- **It needs no model to mint.** Positions are already enumerated and already promotable
  by a computed test, so unlike retort's closed claim vocabulary there is no planner
  call deciding what the tracks are.
- **It lands the family's span/cue vocabulary natively.** A tenure is a span (entered,
  held N quarters, exited); every movement and every issuer filing beneath it is a cue.
  `HolderMovement` already carries `status` and `streak`, so the shape is half-built.

Root headline is the tenure, not the position: *"NVIDIA Corp., held 14 quarters, 2022 Q3
to 2026 Q1, peaked at 7.2%."* Children are the movements, the issuer's own filings timed
against them, the voting status and the routing.

**Material belonging to no position is scene, not track** — the coverage ratio, the NT
relationship, the other-manager structure, the voting split, the ADV facts, the filing
lag. That means this guide's scene carries proportionally more than either sibling's,
which follows directly from the record having no prose.

**Grouping, never filtering** ports unchanged: an unpromoted position still appears in
every aggregate and simply never gets named.

### The rule that makes the intent rule enforceable: a trade date is never filed

The plan's hardest rule is that a holding is an action and an action has no stated
reason. It needs a technical partner, because the tempting sentence is not the obvious
"the manager sold because of the restatement" but the subtler **"the manager sold after
the restatement"** — which asserts an ordering the record does not carry.

> **A 13F reports the position AT a quarter end, and is filed 45 days later. A trade
> date is never filed.** A position that went from 100,000 shares to zero between two
> quarter ends was sold at an unknown moment inside a **91-day window**, and the public
> did not learn it for another 45. Every timed statement is at quarter resolution.

So the honest form of a look-through beat is three filed facts and no verb joining them:
the position was N at date A and M at date B; the company filed X on date C; C falls
inside / before / after that window. Retort has a weaker version of this already
("never present a quarterly position change as though it happened in a particular
month"); here it is sharper, because there is no finer data anywhere in the record.

### Stage 11 was a husk, and this is what should be there

**"Weigh the Conduct Against the Record" compares a thing to itself** — the 13F *is* the
record of the conduct. The stage is retort's "Weigh the Promise Against the Portfolio"
with the nouns swapped, and its four-way verdict vocabulary
(`supported`/`contradicted`/`unevidenced`/`unfalsifiable`) attaches to a claim that does
not exist here. Shipping it would produce a payoff stage that adjudicates nothing.

The payoff of this guide **is the assembled tenures**, so the closing stage should do the
one thing a track cannot: report the conduct pattern that is only visible at the scale of
the whole filing history. MFAM has 63 filings and MFWM 47; the current draft uses one of
each for composition and the sequence only for per-position diffs.

Arithmetic over the sequence, all self-comparison, none of it ranking or attributing:
median holding period; share of the book held for the entire window; how many positions
entered and exited within two quarters; how often a new position reached the top ten;
turnover by value per quarter against its own history. Every one is subtraction over
filed rows, every one fails empty on a young manager, and none needs a claim to test.

The **distinguishability measure** (set intersection against a tracker fund's own filed
N-PORT, already a named candidate above) is the natural closing number if it is built,
and the same section is where it belongs.

### Conduit load: this guide reads almost nothing, and should be built that way

Measured across the three shipped guides:

| guide | prompts | scripts | deterministic section passes |
|---|---|---|---|
| `run-the-10k` | 21 | 19 | — |
| `weigh-the-m-and-a` | 19 | 24 | — |
| `retort-the-n-port` | 13 | 20 | **5 of 12** |

Retort's every prompt is **40 to 72 lines, 1.5 to 5.8 kB**. Nothing is a monolith, and
the architecture behind that uniformity is worth stating because it is the thing to
copy: **one prompt per corpus for reading, one prompt per document region for writing,
one prompt for planning. No prompt does two jobs.** Retort's writing is four prompts
(`write-arc`, `write-portfolio`, `write-short-story`, `write-verdict`), not one
synthesize call; run-the-10k's is six.

**The fact that changes everything here: a 13F contains no prose.**

- run-the-10k feeds a pass ~12,000 characters of Item 1 and asks it to read.
- Retort feeds a pass strategy prose, a shareholder letter, a 497 supplement.
- A 13F is N rows of {name, class, CUSIP, value, shares, discretion, voting, other
  manager} plus a cover of seven declared scalars. MFWM's entire info table is 58,826
  bytes of XML that reduces to 124 rows. **There is nothing to read.**

So every pass over the manager's own record should be **deterministic, with no prompt at
all**. Asking a model to classify structured rows is asking it to do arithmetic in
prose, which is where it invents. That covers stages 2, 3, 4, 5, 6, 7, 9 and most of 10
— roughly **eight deterministic passes, more than any sibling**, and that is a property
of the form rather than a thin build.

A model is genuinely needed for exactly three jobs:

1. **Reading the holdings' documents** (stage 8): an 8-K, a comment letter, Item 1
   prose. This is run-the-10k's job ported, and it is the only real reading in the guide.
2. **Planning** (connect).
3. **Writing** (the `write-*` family).

Projected surface: **≈3 look-through generate prompts + connect + refine + 3 write-\* =
8 prompts**, against retort's 13 and run-the-10k's 21. Do not mint a generate/finalize/
refine triad per stage by reflex because the siblings have them; a deterministic pass is
a finder plus an operator keep-and-place, with no generate prompt and no refine.

**The load hazard here is the opposite shape to the siblings', and the depth gate is not
optional.** The sibling risk is one prompt receiving a lot of text about one thing. This
guide's risk is one prompt receiving a little text about **237 things** — and that is the
shape that overwhelms, per `feedback_narrative_overwhelm_is_cast_size`. Look-through on
MFAM's whole book at three filings each is over 700 documents. Retort solved this with
`fn_depth_choices` / `fn_worklist` and a refuse-loudly threshold; the same gate is
required here and must run **before** any look-through prompt. The natural bound is
tighter than retort's: 98 of MFAM's 237 positions sit under 0.1% of value and combine to
2.95%.

**And one load rule native to the hub role.** A position is not a subject of this report;
it is a **pointer at another guide's subject**. Told nothing, a model handed a holding's
filings will start writing about the company's business, which is run-the-10k's job and
which this guide will do worse because it holds one document instead of a decade. The
look-through prompt must be told it is writing about the manager's *relationship to* the
company. That belongs in `CONDUITS.md`, alongside the trade-date rule and the
no-intent rule.

---

## Guide structure (`dapple-guides/public/dapple/mind-the-13f/`)

`CONDUITS.md` required at root. Add a 13F primer covering what §13(f) securities are,
what the form does and does not contain, what 45 days means for staleness, confidential
treatment, and what voting authority is.

The **model** column is the conduit-load budget from the section above: `—` is a
deterministic finder with **no prompt at all**, and the default is `—`.

### Engine call sheet — written 2026-08-01 while the commands were fresh

Every engine call the guide needs, with the one trap for each. Full field lists live in
each command's `--help`; these are the invocations and the things a field name will not
tell you.

| stage | call | the trap |
|---|---|---|
| 1 | `edgar fund $mgr --accept [InstitutionalManager]` | **omitting `--accept` hard-errors** before returning anything. Never wrap in `try` — the refusal carries the redirect |
| 1 | `edgar manager $mgr --quarters 12` | one row per **filing**, not per quarter; an amendment is its own row |
| 1 | `edgar adv $manager_name` | join by NAME. `cik_self_reported` is present on ~25% of rows and is WRONG for MFWM. `not_registered` is an answer (Berkshire) |
| 2 | `$h.declared_value / $a.aum_total` | the coverage ratio. State both denominators |
| 3 | `… | get reconciliation` | `reconciles: null` ≠ `false`. A notice states no totals, and "nothing to check" is not "checked and passed" |
| 4 | `… | get holdings` | **never `positions`** for any aggregate — 90 rows, 29 holdings (landmine 21). `row_count` shows the fold |
| 5 | `… | get positions` | here `positions` IS right: the per-`otherManager` voting structure is what the stage reads. Degenerate ⇒ one sentence, not a figure |
| 6 | `… | get other_managers` | two disjoint lists in one array — `sequence: null` marks the cover-page (unnumbered) entries. On an NT, `additional_information` carries the relationship in prose |
| 7 | `edgar manager $mgr --tenures | get movements` | quarter-first and flat. Check `quarters[].changes` for null before saying "no changes" (landmine 23) |
| 9 | `edgar security $cusip` (list on pipeline) | a NAME reaches the trust; only `ticker` pins a series. Null ticker ⇒ honest unpinned bucket. Cite `filers`, not `hits` |
| 9 | `edgar security … | get ticker | edgar fund $in` | the pinning step. `is_pinned` distinguishes series from trust |
| 10 | `… | get amendment`, `confidential_omitted` | `confidential_omitted: null` is the filer making no admission — never render it as `false` |
| 11 | `edgar manager $mgr --tenures | get history` | `median_quarters_held` covers `fully_observed_tenures` ONLY, and it is null when none qualify. **State the censored count** (landmine 22) |
| — | `edgar manager … --persist` | optional; writes the shared cache for SQL. Never marks a CUSIP complete |

⚠️ **Every one of these runs inside a task script, where `$in` is the dispatch record.**
Landmine 26: a command whose signature lacks an `(Any, Any)` input arm is rejected at parse
time there. All three known offenders are fixed; a NEW command needs the arm plus a
StepEngine test, because a live test calls from a shell where the input really is nothing.

| # | Stage | model | Notes |
|---|---|---|---|
| 1 | **Pull the Manager's Record** | — | auto; N quarters of 13F-HR/NT, the manager's SC 13D/G and Form 4s, **and its Form ADV row (E6)** |
| 2 | **What This Form Cannot Show** | — | the coverage stage, and it comes early. **The ADV ratio is its headline number**, and it is the outer scene sum. See below |
| 3 | **Reconcile the Filing** | — | declared entry and value totals against the table: the inner scene sum, and the filer states both sides. Misfilings |
| 4 | **What They Held** | ✅ engine ready | positions, concentration, turnover. `titleOfClass` as the filer's own instrument hint. **Partition operating companies from funds first** — see the double-count landmine. ⚠️ Compute every aggregate from `holdings`, never `positions` — landmine 21 |
| 5 | **How They Hold It** | — | voting authority, discretion. Renamed from "What They Cannot Vote": on every filer measured this partition is **degenerate**, so it is a sentence and the stage must say *how* rather than presuppose a split |
| 6 | **Who Else Reports Inside the Organization** | — | `otherManager` (present-or-absent, never sparse), sequence numbers, NT relationships |
| 7 | **What Changed** | ✅ engine ready | new / added / trimmed / exited, streaks, across quarters — `edgar manager --tenures` emits `movements[]` per position. ⚠️ NOT `compute_movements` (see E1's reuse note): a quarter with `observable: false` is skipped, never read as an exit |
| 8 | **What the Holdings Filed** ⚑ | **yes, ×3** | the only real reading in the guide. Deterministic XBRL finders **plus** prompted passes over 8-Ks, comment letters and issuer prose, timed against the quarter. **Depth gate runs first** |
| 9 | **Which Guide Reads Each Name** | — | the hub output: per promoted holding, the sibling guide that can read it. Arms: operating company → `run-the-10k`, ETF or closed-end fund → `retort-the-n-port` (via E5), pending merger → `weigh-the-m-and-a`. **No `price-the-mfp` arm** — an open-end fund cannot appear |
| 10 | The Manager's Own Fine Print | — | filing lag as misfiling, amendments, confidential omissions, **plus the ADV facts: staffing, client and account counts, fee basis, Item 11 discipline** |
| 11 | **The Shape of the Whole Record** | ✅ engine ready | ⚠️ **replaces "Weigh the Conduct Against the Record", which compared a thing to itself.** Arithmetic over all N filings — `--tenures` returns it as `history`: median holding period, share held the whole window, in-and-out inside two quarters, new positions reaching the top ten, turnover against its own history. Self-comparison only; the distinguishability measure lands here if built. ⚠️ **State the censoring** — landmine 22: 41 of Berkshire's 48 tenures predate a 3-year window, so the median covers 7 of them and the stage must say so |
| 12 | **Pull the Current Quarter** ⚑ | — | fork point |
| 13 | Build the Story | **connect + 3 write** | connect plans tracks and scene; the writing splits by document region, never one synthesize call |

### Stage 2 comes second on purpose

`retort-the-n-port` learned that **coverage is the ceiling on evidence strength, not a
completeness caveat**, and that it is **a number per claim, not one number**. A 13F is
the most partial record in the family and the limits are structural, not incidental:

- **The 13F covers only part of the manager, and Form ADV says how much.** MFWM: 55.4%.
  This is the one limit in the list that is a **filed number rather than a description**,
  so it leads. Every later percentage in the report is a percentage of this.
- **Long US-listed equity and related instruments only.** No shorts, no cash, no bonds,
  no non-US listings, no private positions.
- **45 days stale at best**, and a quarter-end snapshot in between.
- **Confidential treatment** can remove positions, declared on the cover.
- **17% of filings are notices** with no holdings at all.
- **Fund positions are opaque at this level and transparent one level down.** Report the
  fund share of the book here (MFWM: 25.07%), because until stage 9 resolves them through
  E5 those positions are a quarter of the value and none of the detail.

Putting this at stage 2 rather than in a closing caveat is the whole lesson of round 3
of `retort-the-n-port`: a limitation that arrives late reads as an apology, and a
limitation that arrives early is the frame every later number is read through.

#### PHASE 2: the dark room can be lit, and it is all bulk (measured 2026-08-01, not designed)

Stage 2 currently measures the gap and then has nothing to say about what is inside it —
MFWM's 13F covers 55.4% of regulatory AUM, so **$711m of the room is dark** and the stage
stops there. That is a dead end only because nothing here reads the *other* half of a
manager: the registered funds it advises. Those file continuously, and the SEC publishes
all of it in bulk. Every join below was closed live before being written down.

| source | size | key | what it gives |
|---|---|---|---|
| N-CEN DERA `ADVISER.tsv` | 8.4 MB/qtr | CRD → series | every registered fund the manager advises **or sub-advises** |
| N-CEN `FUND_REPORTED_INFO.tsv` | same zip | series | `IS_ETF`/`IS_MONEY_MARKET`/`IS_INDEX`/`IS_FUND_OF_FUND`/`IS_TARGET_DATE`, lending flags, `IS_INDEX_AFFILIATED` |
| N-CEN `DIRECTOR.tsv` | same zip | registrant | trustees, interested/independent, interlock across the family |
| Prospectus Risk/Return DERA `num.tsv` + `txt.tsv` | 81.6 MB/qtr | series/class | the stated objective and strategy, **and the expense ratio** |
| N-PORT DERA | 440 MB/qtr | series | `FUND_REPORTED_HOLDING.tsv` (910 MB, every position) and `MONTHLY_TOTAL_RETURN.tsv` |

Closed end to end on a large family: **Fidelity's four legal entities → 617 primary-advised
series → 258 joined to a prospectus fee** in a single quarter, median gross expense ratio
0.58%. Four quarters of N-CEN cover the whole universe: **14,243 series, 1,947 advisers.**

⚠️ **The fee rate is reachable after all, and not from a PDF.** E6 defers it because Item 5E
gives only the basis and the rate is prose in the Part 2A brochure. But every registered fund
tags its own fee table in XBRL, and the prospectus bulk set carries it — 17,649
`ExpensesOverAssets` rows in one quarter. `grep -rn "expense_ratio"` over the crate returns
zero hits today. This does not contradict E6; it routes around it.

Traps already measured, so phase 2 starts from them rather than rediscovering them:

- **N-CEN and prospectus data are ANNUAL filings sampled quarterly.** One quarter is a partial
  universe and the quarters are wildly uneven — 2,418 / 2,437 / **7,058** / 2,330 series, Q1
  running ~3× the rest because most funds have a Dec 31 fiscal year end. Four quarters, or
  state the fraction covered.
- **CRD disagrees across all three sources.** The N-CEN XML pads (`"000105958"`,
  `edgar_ncen.rs:614`); the DERA TSV coerces to int under `from tsv` (`105496`). A literal
  comparison silently returns zero rows. This is landmine 12's join problem in a new place.
- **`IS_AFFILIATED` is blank on 62% of rows** (15,010 of 24,083). Landmine 8 again.
- **`ADVISER_TYPE` has four values, not two:** `Advisor`, `Subadvisor` (one word), `Terminated
  Advisor`, `Terminated Subadvisor`. Filtering to `Advisor` silently drops the sub-advised
  book — measured on Fidelity, 603 `Advisor` rows against **784 `Subadvisor`**.
- **A group is not a filed entity.** "Fidelity" is four legal entities with four CRDs, two of
  them offshore sub-advisers sitting on ~392 series each. Never group by name prefix.
- **`txt.tsv` is 484 MB uncompressed.** The series filter is mandatory, not an optimization.

**Not designed here, deliberately.** Phase 2 gets its own pass once E1 is real and we have
seen its output on live subjects. It changes no doctrine: the guide still reads one manager,
still ranks nothing, and stage 2 still leads with the filed coverage number — it just stops
being the end of the sentence.

### Doctrine carried forward

- **The manager is the scene, and it is furnished by ADV plus the form's own three
  partitions** — never by the holdings' sectors, which the manager did not file. See
  "The scene survived; its furniture did not" above; the one-line version of this
  doctrine is what let the earlier draft ship with an unbuildable scene.
- **A track is a position's tenure**, with its movements, its issuer's filings, its
  voting status and its routing nested beneath. Computed promotion only, `reason` always
  carried. Berkshire files 90 positions and a large manager files thousands — cast size
  is what kills the report.
- **A degenerate partition is a sentence, not a figure.** One bucket is not a partition.
  Detect it and render a line; the alternative is three empty tables per report.
- **A trade date is never filed.** Quarter resolution on every timed statement, and no
  verb joining a position change to a dated issuer filing.
- **Band, do not split**; embedded four-backtick bands only.
- **Every aggregate states its denominator.**
- **Misfiling vs violation.** The reconciliation check produces misfilings; only a
  comment letter or enforcement produces a violation.
- **Rank nothing between holdings**; **never characterize the distance**.
- **No silent caps.**

### The rule this guide needs written down first

**A holding is an action, and an action has no stated reason.** The guide reports that
a position was opened, added to, trimmed or exited, in which quarter, and what the
company filed around it. It **never** reports why. There is no document in which a 13F
filer explains itself, so any "because" is invention — and unlike the other guides,
there is no claim text to hold the inference against. This is `weigh` invariant 7 in
its strictest form: the columns sit adjacent and the reader draws the conclusion.

### The question that rule makes hard, and the arithmetic that answers it anyway

A manager's public identity is often a claim — Motley Fool's whole brand is the licensed
fool who dares tell the king the truth. Its filed portfolio is led by NVIDIA, Alphabet,
Apple, Microsoft, Amazon and Meta, which is the most consensus book available. The
temptation to write a sentence about that gap is enormous, and the rule above forbids
every version of it: the brand is not on EDGAR, so there is no filed claim to test, and
"contrarian in name only" is a motive attributed to an actor who never stated one.

**But there is a measurable form, and the family already invented the technique.**
`retort-the-n-port` round 1 established that **an index's constituents are reachable
through public filings even though the index itself is licensed**, because a fund that
tracks the index files its own N-PORT. Applied to a manager:

> *"78% of this manager's reported value also appears in SPY's most recent filed
> portfolio."*

Both sides are filed documents, the operation is set intersection, and the sentence
attributes nothing. It measures **how distinguishable this book is from a passive one**
without ever asking why — which is precisely the shape this guide can carry.

⚠️ **The caveats ride on the label or the number is dishonest**, and they are already
written in `retort-the-n-port`'s deferred section: it is the tracker FUND's portfolio,
not the index (trackers sample and optimize); it carries N-PORT's lag of up to ~60 days
against 13F's 45; it includes the tracker's own cash and futures overlay; and the index
name is trademarked, so *"measured against SPY's filed portfolio, which tracks the S&P
500"* is fine while *"the S&P 500 constituents"* is not.

**A named candidate, not a commitment.** It needs a manager→benchmark choice that the
guide has no filed basis for making, which is itself the reason to leave it explicit
rather than let it arrive looking as settled as the reconciliation.

---

## Landmines

1. **The temptation to narrate intent.** The single biggest risk in this guide. A
   quarter of purchases followed by a stock move is a sequence, not a motive.
2. **Values in thousands before 2023-01-03.** Use `normalize_value`; a three-year
   history crosses the boundary and a raw sum is wrong by 1,000× on the early quarters.
3. ~~**Name resolution is unmeasured on this corpus.**~~ **Measured, and it splits** (E3).
   Operating-company names pass at 95.09% by value on the first manager — the technique
   transfers from N-PORT. **Fund positions score 0 of 28** and need a different
   instrument entirely (E5, landmine 10). So the gate passes for one half of the problem
   and does not exist for the other, which is why E3's deliverable is a partition per
   position type rather than a rate per manager. The 95.09% was also taken on the
   *easiest* shape; keep the per-manager partition and never average it.
4. **The info table filename is arbitrary — and "arbitrary" was an understatement.**
   ✅ **Handled** in `edgar_manager::info_table_member`. This landmine was written as
   "the filename varies by filer"; the live run proves it varies **per filing**.
   **Berkshire's own last ten 13Fs produced ten distinct names** (measured 2026-08-01):
   `32498.xml`, `34725.xml`, `36917.xml`, `39042.xml`, `43977.xml`, `43981.xml`,
   `46994.xml`, `50240.xml`, `53405.xml`, `form13fInfoTable.xml`. It is effectively a
   per-submission serial. Motley Fool writes `infotable.xml` throughout, which is why a
   fixture built on that filer alone would have made a hardcoded path look reasonable —
   and it would then have been wrong on **nine of Berkshire's ten**.
   Resolve through edgarkit's `filing_directory` and take the **sole non-`primary_doc`
   `.xml` member**, failing loudly on 0 or 2+ — the house already solved this exact shape
   for the ADV roster's serial-suffixed zip member (`commands/edgar_adv.rs:83-99`).
   **`index.json` verified working** on ten Berkshire filings spanning 2019–2026, on
   Motley Fool AM 2026 and on a 13F-NT. *Amended 2026-08-01: this landmine used to say
   "use `index.json`", then was rewritten to warn against it on the strength of
   `edgar_fund_linkbase.rs:201`, which records index.json coming back empty on a real
   Fidelity filing. That note is narrower than it reads — that filing had a
   standards-defined alternative (the instance names its own schema) and a 13F has none,
   so the directory listing is the only route AND it works. Carry the lesson, which is to
   validate what comes back and fail loudly, not the conclusion.*
5. **99.4 MB per DERA zip.** Do not reach for it to answer a single-manager question.
6. **`13F-NT` is not an empty portfolio.** Report the relationship, not a blank.
7. **An amendment can ADD, and a latest-wins fold destroys the quarter when it does.**
   ⚠️ **Rewritten 2026-08-01 against a real amendment; the earlier text understated this
   and pointed at the wrong field.** Berkshire's `0000950123-25-008361` (2025Q1, filed
   2025-08-14) carries `amendmentType = NEW HOLDINGS`, `tableEntryTotal = 4`,
   `tableValueTotal = $1,106,550,356` and
   `reasonForNonConfidentiality = "Confidential Treatment Expired"` — **four positions
   ADDED** to the original's ~$260bn, released because their confidential treatment
   lapsed. It is not a restatement and it is not the quarter.

   Consequences, all three of which contradict what this plan said before:

   - **`dedup_latest` (`edgar_thirteenf.rs:271`) CANNOT be reused for E1** and must come
     off the reuse list. It keeps the latest-filed accession per (manager, quarter),
     which here swaps a $263bn portfolio for $1.1bn of it — a 99.6% loss reported as a
     complete quarter. It remains correct for the DERA path it was written for, where
     the question is per-CUSIP.
   - **The distinguishing field is `coverPage/amendmentInfo/amendmentType`, not
     `reportType`.** `reportType` reads `13F HOLDINGS REPORT` on the amendment and on the
     original alike; it separates holdings from notice, not restatement from addition.
   - **An amendment that names no type is a third state.** Neither "replace" nor "add"
     can be assumed, and guessing either way is wrong in one direction (drop the
     original) or the other (double-count). `AmendmentKind::Unspecified` + a warning.

   ✅ Handled: E1 returns **one row per filing, never per quarter**, each carrying
   `is_amendment`, the amendment block and its own reconciliation — so the fold is the
   consumer's decision, made with the amendment type in hand rather than behind its back.

   ⚠️ **And it is not rare.** Berkshire's last ten 13F filings contain **two**
   `new_holdings` amendments (2025Q1, 4 rows / $1.1bn; 2023Q4, 1 row / $4.5bn) — 20% of
   the window, so a latest-wins fold would have destroyed two of the eight quarters it
   returned, not one edge case. Both are confidential-treatment expiries, which is a
   *recurring* filing habit of this manager rather than an accident. Note also that the
   feed is ordered by **filing date, not period**, so an amendment sorts among later
   quarters than the one it amends and a naive "newest first" read misattributes it.
8. **Absent is not `false`.** `isConfidentialOmitted` is optional and both Motley Fool
   covers omit it. Model three states. The field's entire worth is that it is the
   filer's own admission, so supplying one they did not make destroys it.
9. **One filer's conventions are not the form's.** `titleOfClass`, name casing, entity
   references and `otherManager` all vary between two filings of the same form in the
   same quarter. Build fixtures from **two** managers before trusting any field shape.
10. **Fund positions resolve at zero, and `nameOfIssuer` names the trust.** 0 of MFWM's
    28. `company_tickers.json` holds no fund series, and the fund-shaped entries it does
    hold are traps that a looser matcher would land on. Two positions share the name
    `DOUBLELINE ETF TRUST`; `iShares Trust` is 367 series under one CIK. Route them
    through E5 or report them as an explicit bucket — **never let them fall into
    `unresolved` alongside genuinely unreadable names**, because they are the most
    readable positions in the book once the right instrument is used.
11. **Look-through double-counts, silently.** MFWM's #5 position is Vanguard's S&P 500
    ETF and it also holds Amazon directly — so Amazon appears twice the moment a fund
    position is expanded. And "top 10 = 43.8%" with a broad-market ETF in it is not ten
    companies, it is roughly five hundred. Any concentration or look-through aggregate
    must state whether it is gross or net of fund contents. This is round 2's invariant
    II landing here: **state the denominator, and the denominator is never the whole
    portfolio.**
12. **Form ADV's join key is CRD, and a 13F does not carry one.** The roster's `CIK#` is
    self-reported, present on only 25.5% of rows, and wrong for one of the two first
    subjects (`160797` for a real CIK of `1607978`). Join by name, confirm by CIK, and
    model `registered` / `not_registered` / `unresolved` — Berkshire is legitimately
    absent from the roster, which is a fact about Berkshire rather than a lookup failure.
    Also: the bulk CSV is **Windows-1252**, and a `grep` over it silently reports absence
    rather than erroring.
13. **Copying `retort-the-n-port`'s scene builder yields four empty tables**, because a
    13F row has no asset category, country, issuer category or fair-value level. The
    tempting repair — resolve to a filer and partition by SIC sector — builds the scene
    out of material the subject never filed and is forbidden by run-the-10k's
    "an invented scene is dropped". Furnish it from ADV and the form's own three fields.
14. **All three native partitions are degenerate on all three filers measured**, so a
    build that renders them as figures ships three empty visuals per report. And
    degenerate does not mean uninformative: MFWM's `None` on 124 of 124 is the strongest
    single fact in the sample. Render a degenerate partition as one sentence.
15. **The 45-day lag plus quarter-end snapshots means a trade date does not exist in the
    record.** The failure is not the obvious "sold because of the restatement" but the
    quiet "sold **after** the restatement", which asserts an ordering nothing filed
    supports. The change happened somewhere in a 91-day window and the public learned it
    45 days after that window closed. Quarter resolution, always.
16. **Stage 11 as originally drafted compared the conduct to the record, which are the
    same thing.** Retort's verdict vocabulary needs a claim and there is none here. If a
    build finds itself emitting `unfalsifiable` about a 13F, that is the husk still in
    place.
17. **A 13F filer is a manager, not necessarily an issuer, and the shared CIK resolver
    only knows issuers.** Found 2026-08-01. `resolve_cik` (`commands/edgar_common.rs:50`)
    is digits-or-ticker against `company_tickers.json`, which lists **public companies**.
    Berkshire resolves because it happens to be one; neither Motley Fool manager does.
    The shape of the failure is the dangerous part — E1 would pass its Berkshire fixture
    and fail on both of the subjects this plan actually names. Resolve a manager name
    through `browse-edgar` scoped to `type=13F-HR` instead (E1). The same trap waits for
    any future command whose subject is a filer rather than an issuer.
    **Premise verified 2026-08-01:** scoping by form really does disambiguate —
    `company=vanguard` returns 100+ companies unscoped and **12** with `type=13F`;
    `company=apple` returns 100+ and **3**. But see landmine 20 for what the scoped feed
    does *not* give back.
18. **13F cover dates are `MM-DD-YYYY` and no shipped helper parses them.** Found
    2026-08-01. `periodOfReport`, `reportCalendarOrQuarter`, `signatureDate` and every
    amendment date are written `03-31-2026`. `to_iso_date` (`edgar_thirteenf.rs:66`)
    knows ISO and DERA's `31-MAR-2025` and **neither matches** — its ISO test needs
    dashes at positions 4 and 7 where this form has digits, and its `DD-MON-YYYY` arm
    needs an alphabetic month. It therefore **fails closed**, returning `None` rather
    than a wrong date, which is the good news; the bad news is that the symptom is every
    `period_date` and `quarter` silently empty, on every filing, with nothing raised.
    ✅ `edgar_manager::cover_date_to_iso` adds the arm and delegates the other two forms
    first. The three formats provably cannot collide.
19. **A `13F-NT` is structurally a different document, not a holdings report with the
    holdings removed.** Found 2026-08-01 on Syncona Ltd (`0001193125-26-222801`). Four
    differences, each of which breaks something written for an HR:
    - **No information table document exists.** The accession holds `primary_doc.xml` and
      nothing else, so landmine 4's sole-`.xml`-member rule reports "found 0" on a
      perfectly valid filing unless it is gated on the submission type first.
    - **No `summaryPage` element at all** — so `tableEntryTotal` and `tableValueTotal`
      are *absent*, not zero, and there is nothing to reconcile. "Nothing to check" must
      not render as "checked and passed" (`reconciles: null`, not `true`).
    - **The other-manager list is the OTHER one** (`coverPage/otherManagersInfo`,
      unnumbered) rather than the numbered `summaryPage/otherManagers2Info`.
    - **`additionalInformation` carries the relationship in prose**, naming who actually
      holds the securities and through what chain of entities. This is the content that
      makes landmine 6 ("report the relationship, not a blank") actionable rather than
      aspirational — it is *there*, in the filing, in the filer's own words.
20. **The name-search feed returns no names.** Found 2026-08-01, and it constrains E1's
    `ambiguous` outcome. On a **multi**-match, `browse-edgar`'s company-list feed emits
    `<company-info name="ARRAY(0x5589e41ff350)">` and `title="ARRAY(0x...)"` — the SEC's
    CGI leaking a stringified Perl array reference where the company name belongs. The
    feed carries CIKs and nothing else identifying. So a candidate LIST cannot be built
    from one request, and E1 must either return bare CIKs or spend one small request per
    candidate to name them.
    The one piece of luck: the bug is unmistakable, so there is no plausible-looking
    wrong name to accidentally trust. **The discriminator is clean** —
    `<conformed-name>` is present *iff* the search resolved to exactly one filer (a
    single match returns the FILINGS feed, which carries the company block; zero matches
    return an empty feed titled `Company Search Feed`).
21. **A filing's ROW count is not its HOLDING count, and the gap is enormous.** Found
    2026-08-01. A filer may report one security **once per `otherManager` combination
    that holds it**: Berkshire's 2026Q1 filing is **90 rows over 29 distinct CUSIPs**,
    with **Apple on 12 of them**; its 2019Q3 filing is 166 rows over 48. Motley Fool
    Asset Management files 237 rows over 237 CUSIPs and never repeats one — so the two
    counts are *identical* on that filer and a fixture built there would never surface
    this. Landmine 9 for the third time.
    Consequences: `tableEntryTotal` answers "how many rows did you file", never "how many
    things do you hold"; stage 4's concentration and top-ten are computed on **fragments**
    unless aggregated; and a subsidiary re-shuffling which entity holds a share changes
    the ROWS without changing the position, which reads as a trade.
    ✅ Handled: `edgar manager` returns `positions` (as filed, for the `otherManager` and
    voting structure stage 5 reads) **and** `holdings` (one row per
    `(cusip, sh_prn_type, put_call)`, each with `row_count` and `pct_of_portfolio`), plus
    both counts. **A PUT on X is not a holding of X** and `SH` is not `PRN`, so those are
    separate grouping keys, not merged.
22. **Both ends of a tenure can be censored, and a median over censored data is an
    artifact of the window.** Found while building the tenure layer, 2026-08-01. A
    position present in the window's FIRST quarter was entered at an unknown date — not
    at the window start — and one present in the last has not been exited *yet*.
    **Measured on Berkshire over 12 quarters: 41 tenures censored, 7 fully observed.**
    So a "median holding period" taken over all 48 would be measuring where the pull
    happened to start on 85% of its input.
    ✅ Handled: `entered_before_window` / `is_open` / `fully_observed` are explicit
    fields, `quarters_held` is documented as a lower bound whenever either flag is set,
    and `median_quarters_held` counts fully-observed tenures ONLY — reporting `null`
    rather than a number when none qualify, alongside `fully_observed_tenures` and
    `censored_tenures` so the denominator is visible.
    ⚠️ **Stage 11 must state this.** On a three-year window of a long-lived manager the
    honest sentence is "seven holding periods are measurable; forty-one positions predate
    what this window can see" — not a median presented as the manager's holding period.
23. **A quarter the manager did not report is not a quarter in which nothing was held.**
    A `13F-NT` period, or an amendment whose original falls outside the window, yields no
    portfolio. Reading either as an absence exits **every** position and re-enters it the
    following quarter, manufacturing a total-turnover event out of a filing habit. Both
    are marked `observable: false` and skipped by the tenure walk entirely. Live
    confirmation: a 12-filing Berkshire pull yields 9 periods of which **8 are
    observable** — the ninth is a 2023Q4 amendment whose original is older than the
    window, correctly reported as a fragment rather than as a 1-position quarter.
24. **Filers TRUNCATE a security's name, and the truncated form can be the more common
    one.** Found 2026-08-01 recovering `25861R303`. Tidal Trust III files `DoubleLine
    Commercial Real Est` — cut at 30 characters. Collaborative Investment Series Trust
    files `DoubleLine Commercial Real Estate ETF`. **Taking the first hit, or the most
    frequent spelling, returns the truncation** — it appeared in 3 of the 4 index hits.
    ✅ `edgar security` takes the **longest** variant, because a longer string is never a
    truncation of a shorter one, so it is the only candidate that cannot be somebody's
    field width. It is still a heuristic, so every variant is returned in `name_variants`
    with its filer count and the disagreement is stated. This is the family's
    *"position truncation is a reading"* pattern with the truncator being a FILER rather
    than our own code — which means it cannot be fixed at the source, only detected by
    asking more than one filer and noticing they differ.
25. **Index hits are not independent sources.** The same `25861R303` search returned **4
    hits from 2 filers** — one trust filing three quarters running. Citing the hit count
    would have called it "confirmed by four sources". ✅ `edgar security` dedupes by CIK
    **before fetching** (fewer requests AND better evidence — the rare case where the
    correct thing is also the cheap one) and reports `hits` / `filers` / `filings`
    separately. **Cite `filers`.**
26. **A command can be perfect from a shell and uncallable from a guide.** Found
    2026-08-01. A guide task script runs with the dispatch **record** on `$in`, so every
    command it calls is type-checked against a record input *whether it reads one or not*.
    A signature declaring only `(Nothing, …)` and `(List<String>, …)` is rejected **at
    parse time**: `expected nothing and list<string>, got record`. The command is fine;
    the call is unparseable — and it fails at INGEST, far from the command.
    **Three commands shipped with this**: `edgar adv` (E6, and stage 1 calls it),
    `edgar company-cik`, and `edgar security` before it was caught. All three had live
    tests, which call from a shell where the input really *is* nothing, so nothing caught
    it. ✅ Fixed by adding an `(Any, Any)` arm, and covered by
    `optional_arg_commands_are_callable_from_a_guide_task_script`.
    **Any new command taking an optional arg plus a list on the pipeline needs the same
    arm, and a StepEngine test — a live test alone does not prove a guide can call it.**

---

## Verification

**Fixtures come from TWO managers, not one.** Berkshire and Motley Fool Asset Management
disagree on `titleOfClass`, name casing, entity references, `otherManager` and whether
`isConfidentialOmitted` appears at all. A single-filer fixture set cannot catch any of
that, and every one of those differences is a live parser hazard. Capture both.

1. **Reconciliation probe**: the Berkshire fixture must reconcile exactly (90 / 263.1 bn)
   and a seeded corruption must fail it. Motley Fool AM is the second case
   (237 / $2,326,025,992, verified exact) — two filers, so the check is not tuned to one.
2. **Units probe**: a pre-2023 filing's values must be scaled and a post-2023 filing's
   must not. Revert-to-fail. Both Motley Fool managers span the cutover, so this is
   exercised by the real subject rather than only by a fixture.
3. **`13F-NT` probe**: a notice fixture must produce the relationship statement and
   **not** an empty holdings table.
4. **Resolution partition** reported as three buckets, never one rate, and **never
   averaged across managers**.
5. **Ampersand probe.** Parse `JPMorgan Chase &amp; Co.` and assert the normalized name
   is not `"JPMorgan Chase"`. This exact bug shipped in the N-PORT parser and silently
   dropped every mega-cap with an `&` out of the look-through; assert it here rather
   than rediscovering it. Revert-to-fail by disabling entity decoding.
6. **Optional-cover-field probe.** A cover with no `isConfidentialOmitted` must report
   *absent*, and a cover with `false` must report *false*, and the two must not compare
   equal. Both directions — asserting only the absent case passes on a parser that
   returns absent for everything.
7. **Filename probe.** Locate the info table for both a filer that names it
   `infotable.xml` and one that does not (`53405.xml`). A single-fixture test here
   passes with the filename hardcoded, which is the failure this is meant to prevent.
8. **No-intent probe**: assert the rendered report contains none of a closed list of
   motive words (`because`, `in anticipation`, `betting`, `conviction`, `signals that`).
   Both directions — banned string absent **and** the neutral replacement present,
   since absence alone passes on an empty render.
9. **Fund-partition probe.** MFWM's 28 fund positions must land in their own bucket, and
   the bucket must be named in the output. Assert that `unresolved` does **not** absorb
   them — a blended "56.5% resolved" that hides a clean 0-of-28 is exactly the failure
   `edgar resolve`'s three-bucket partition exists to prevent. Revert-to-fail by merging
   the buckets and asserting the probe goes red.
10. **E5 bootstrap probe.** CUSIP `922908363` must recover `Vanguard S&P 500 ETF`, and
    `25861R303` must recover a name **and** the ticker `DCRE`. Offline-fixture the N-PORT
    responses so the probe does not depend on which fund happens to hold what this
    quarter. Assert the ISIN substring separately: `US9229083632` → `922908363`.
11. **ADV join probe, three states.** MFAM must resolve to CRD 149688; MFWM must resolve
    **despite** its roster CIK being wrong; Berkshire must come back `not_registered`
    rather than `unresolved`. All three, or the partition is untested. Also assert the
    Windows-1252 decode on a row that contains a non-UTF-8 byte.
12. **Coverage-ratio probe.** The rendered ratio must carry **both** dates (ADV filing
    date and holdings period end). Assert the number and the dates together; a ratio
    without its clocks is the defect, not a formatting preference.
13. **Double-count probe.** A look-through aggregate over a book containing both a direct
    position and a fund holding the same name must state gross or net. Assert the label
    is present, not merely that the arithmetic runs.
14. **Degenerate-partition probe.** A book whose voting authority is one value across
    every row must render **one sentence and zero figures**; a book with a genuine split
    must render the figure. Both directions — asserting only the degenerate case passes
    on a build that never renders a figure at all. MFWM (`None` ×124) and MFAM (`Sole`
    ×237) are the live degenerate cases and a synthetic split is the control.
15. **Scene-sum probe, both levels.** Inner: every partition of the book totals the
    declared `tableValueTotal`, and a seeded dropped row fails it. Outer: the ADV
    difference is rendered **as a difference with both dates**, and the probe asserts it
    is never used as a pass/fail check — a 13F value exceeding regulatory AUM must render
    rather than raise.
16. **Quarter-resolution probe.** Assert no rendered sentence joins a position change to
    a dated issuer filing with an ordering verb. Closed banned list (`after`, `before`,
    `following`, `ahead of`, `in response to`, `once`) in the specific construction, and
    the neutral replacement present. This is the enforcement arm of landmine 15 and it
    is the one probe most likely to be written so it cannot fail.
17. **Track-root probe.** Every promoted position produces exactly one track whose root
    is its tenure, and its movements, issuer filings, voting status and routing appear
    **beneath that root and nowhere else**. Assert an unpromoted position appears in the
    aggregates and in no track: grouping, never filtering.
18. Probes must be **able to fail**; revert-to-fail at least one new assertion per
    section.
19. `dapple guide validate` at 0 warnings before ingest; `nu-check` everything.
20. End-to-end on the two Motley Fool managers, run separately. They are deliberately
    NOT opposite shapes — same brand, same address, both solo filers — so a difference
    the report surfaces between them is a difference in the *books*, not in the filing
    style. A third run on a manager that files alongside others (Berkshire, 14 included
    managers) covers the shape these two cannot.

---

## Deliberately not built

- **No performance calculation.** A 13F has no cost basis and no cash, so any return
  number would be fabricated. Report positions and changes; never a return.
- **No intent, no thesis, no "conviction".** See landmine 1.
- **No cross-manager ranking or copying.** The guide reads one manager.
- **No short or derivative inference from absence.** A 13F not showing something is the
  form's scope, not evidence about the manager.
- **No absorption of the sibling guides.** Stage 9 names which guide reads each holding
  and stops there.
- **No `price-the-mfp` routing arm.** Open-end funds including money market are not on
  the Official List, so one cannot appear in a 13F. Building the arm would be a road
  nothing drives on, and its presence would imply the guide had checked for something it
  cannot encounter.
- **No fee-rate extraction from the Part 2A brochure.** The rate is in a PDF and nothing
  in the repo reads one. Report the fee **basis** from Part 1A (`5E`), which is
  structured and free, and say the rate was not read rather than estimating it.
- **No commercial data intermediary.** Every source here is a file the SEC publishes.
  A third-party API (sec-api.io and similar) is a paid, normalized copy of exactly these
  files, and routing through one would put the guide's citations behind something the
  reader cannot open. This was considered and declined on 2026-08-01; the record is here
  so it is not re-litigated as a new idea.
