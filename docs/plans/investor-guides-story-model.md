# The investor-guide story model — one model, four substrates

**Status: BUILT 2026-08-02** across `mind-the-13f`, `retort-the-n-port`, `price-the-mfp`,
`mark-the-bdc`, with two localized fixes to the reference models (`run-the-10k`,
`weigh-the-m-and-a`) and one new engine command (`edgar book`). Each guide's AGENTS.md
carries its own instantiation; this document is the model stated once, and the record of
why.

## Why (Kenny's diagnosis, 2026-08-02 session)

The four investor guides were each built by adapting a sibling, and the copies drifted the
same way: **the scene wasn't right and the tracks didn't hang off it.** The audited state:
zero of the four emitted a figure with a date axis, while the time series existed in every
guide's bindings and were collapsed everywhere (13f joined a per-quarter movements series
into one semicolon sentence; mfp computed daily NAV series and reduced them to extremum
sentences; retort's scene was five partitions of one instant).

Three corrections, in the order they arrived:

1. **The scene is the PRACTICE.** The subject of an investor guide is a decision-maker,
   and what a decision-maker does is build a book over time. Reading a manager is like
   reading a 10-K: the scene is pieced together across the period. A static aggregate is
   a photograph of a thing whose identity is movement.
2. **"A ton is said about their investments, by the very people they are investing in."**
   A holding is not a name in a list; it is a stack of filed time series (its own
   fundamentals, its 8-Ks, its insiders) on the same axis as the position series. The
   guides were reading holdings as routing targets or one-shot documents.
3. **The book is judgeable like a company.** Weight each holding's own filed values by
   the position and the portfolio has revenue, margins, leverage, trajectories — written
   by the holdings' own filers. Decompose the motion into a COMPANY effect and a
   MANAGER/FUND effect and the subject's hand is isolated in filed dollars, with no
   prices, no returns, no motive.

## The reader frame

**"I use a fund so I don't have to read all of this."** The operator DELEGATED the
reading; watching the holdings' filings is the service being bought. The series stack is
therefore a delegation audit: everything the holdings published, laid beside what the
manager did while it landed. Never "the manager should have known" — knowing is their
job, so the adjacency needs no commentary. The reader checks the STEWARD, not the stocks.

**The unquantified trust index.** The sibling guides already compute each company's
record-integrity layer deterministically (misfilings, restatements, comment letters,
auditor changes). The investor guides compose it: that record beside the position series
of the professionals paid to have read it. NEVER quantified, never a score — the moment
it becomes a number it becomes the kind of unfounded claim the family refuses.

## The model

| layer | content |
|---|---|
| **Frame** | what the form can show at all: coverage, exclusions, censoring walls |
| **Scene** | the practice: the book over time + what kind of shop + the look-through composite |
| **Cast** | promoted holdings/issuers/borrowers; promotion computed, reason carried |
| **Track** | per-guide root (below) with SERIES STACKS beneath: the subject's own series beside the holdings' filed series on one axis, plus 8-K event pins at their filing DATES |
| **Record** | the filing itself: timeliness, amendments, reconciliation |

**Track roots are per-guide and were already right — they were NOT re-rooted:** tenure
(13f), claim (retort — the fund genuinely wrote a promise), the four named characters +
issuer exposures (mfp), mechanism (bdc). The re-lay is what hangs off them.

## The four substrates

| guide | book series from | rich layer written by | composite |
|---|---|---|---|
| mind-the-13f | quarterly 13F folds | the holdings (all file — Official List) | `edgar book` on the depth-gated worklist; new stage 5 |
| retort-the-n-port | per-filing N-PORT periods | the holdings (~96.5% resolve) | `edgar book`; prior periods re-resolved free (registry-memoized); signals pass now READS the book payload instead of fetching per company |
| price-the-mfp | monthly N-MFP (daily series inside) | the ISSUERS the exposures roll over on — paper matures in days, exposure persists; each maturity is a fresh decision to lend again | `edgar book` over persistent private-credit issuers; government paper is `no_filer` BY CONSTRUCTION and that partition is itself scene |
| mark-the-bdc | its own schedule of investments | the BDC itself (borrowers file nothing) | native form: per-borrower series + a MARK effect vs ORIGINATION effect split, own schedule only |

## Rules added to CONDUITS.md (per guide, as applicable)

- **A value change is price plus trade.** Movement status decides on SHARES, weights on
  VALUE; the gap between the two series is the subject's hand. Adjacent, no verb.
- **A filed value per share is a PATH, never a difference** (13f; Kenny's explicit call).
  No script or prompt computes a change/ratio/endpoints comparison of the implied mark —
  one subtraction turns it into a return. The reader who subtracts is exercising their
  own judgment on filed facts.
- **An exit has several causes and nothing filed says which** (sold / acquired / delisted
  from the Official List / de-minimis omission) — the 13F sibling of BDC's "gone is the
  only word"; retort got its snapshot version.
- **Two clocks, never blended** (mfp): daily fund series inside quarterly issuer facts.
  The daily NAV mark beside a dated 8-K is the family's one same-resolution pair.
- **The composite states its coverage on every number**; gross margin is the
  weight-average of the holdings' own margins (the definition that decomposes); the
  company/manager split is arithmetic attribution, never motive.
- **CORRECTED rule:** "the scene is not furnished by the holdings' sectors" became **"an
  aggregate over the book is a fact about the subject (they assembled it); a narrative
  about one holding belongs to the guide that reads it."** SIC partitions stay out — as
  material nobody filed, not as holding-touching.

## `edgar book` (engine)

`crates/dapple-prompt-flow/src/edgar_book.rs` (pure: concept map with spelling unions,
duration-classified alignment, TTM/annual flows, USD-only, staleness, per-field coverage,
exact company/manager/interaction decomposition — 22 unit tests) +
`commands/edgar_book.rs` (per-cik companyfacts fetch with warn-and-continue
`fetch_failed`, IFRS fallback, `--fields/--top/--min-weight` with `gated_weight_pct`
stated, per-holding `aligned` values emitted for the stacks). Live-verified (2 tests incl.
StepEngine + decomposition identity on live data — RELATIVE tolerance; revenue-scale f64
needs it). One command, dual output: the composite AND the per-holding series it was
computed from — tracks read stacks from the payload, nothing fetches twice, and the
concept spellings live in ONE place (retort's guide-side list is gone).

Input contract: `{period?, period_end, cik?, name, weight_pct (0-100, never re-scaled),
value_usd?, reason?}` — identity resolution stays guide-side (it differs per form).
Cost: ~1 fetch/covered company, sequential ~10 req/s: **minutes, not seconds** (~90 ≈
1.5-4.5 min). No dispatch timeout exists to trip; gates + coverage keep budgets honest.

## Conduit load

Every write call that received a flat pile now receives GROUPS: 13f write-book (125 lines
→ per-tenure nesting), retort write-portfolio (160 → per-holding), mfp write-record (125 →
per-character). Deterministic series NEVER travel through a prompt — they render as
tables/charts directly (the family's first date-axis figures: line charts in 13f and mfp;
periods-as-columns tables everywhere). Chart axes are categorical: one label per present
period, absence stated in captions; daily series sample evenly and always keep first+last.

## Verification state

- Engine: 678 lib tests green, clippy `-D warnings` clean, 2 live tests green, api crate
  rebuilt (guide script parse).
- All six guides `dapple guide validate` clean (13f 21 tasks / retort 21 / mfp 19 / bdc 18
  / 10k 19 / weigh 24; 0 warnings each).
- Probe suites green in all six; each NEW probe section demonstrated RED via its named
  reversion before being trusted (one vacuous assertion caught exactly this way in bdc
  §19 and tightened to the wiring lines).

## Owed (not done here)

- **Live runs of all four investor guides** on their standing subjects (13f: both Motley
  Fool managers + Berkshire; retort: RBB trust ETFs; mfp + bdc: standing subjects) —
  Kenny runs these; nothing generative was fired.
- **weigh `write-arrangements` re-measure** — the starved-conduit fix is in and asserted
  (`signals.arrangements_unevidenced`) but has not been re-measured on a live run.
- **MFP name-path hit rate** for issuer resolution — measure live; N-MFP's lei/isin/cusip
  keys fit no existing resolver and a resolver extension was deliberately out of scope.
- `dapple guide validate` still needs `--no-build` in this tree (519 deleted `.sqlx`
  files, unrelated to this work).
