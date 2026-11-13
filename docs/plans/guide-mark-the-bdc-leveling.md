# Plan 5 — `mark-the-bdc`: leveling the field for forced transparency

**Status: EXECUTED 2026-08-03 (guide-side session). Offline-verified, not ingested.**
Part of the promise-leveling umbrella.

> Execution note, one deviation from the spec below, deliberate: **no new
> `coverage-generate` script, stage, slot, or write prompt.** Every per-field count the
> chapter needs was already computed by the portfolio pull and sitting on `fn_marks[0]`
> (`position_count`, `cost_covered`, `rates_tagged`, `spreads_tagged`, `pik_positions`),
> so connect carries a `scene.completeness` record and synthesize renders the chapter
> deterministically — fixed thesis prose + two tables, no model call (deterministic facts
> are never model-written; a constant thesis needs no writer). Smallest possible cascade:
> no chapter-.md change, no ftl change, no pack wiring.
>
> All three bdc probes green (static §21, render arm-8 chapter assertions, new
> `probe_bdc_fuzz.nu` arms A/B with lesions LB1/LB2 demonstrated — the null-percentage
> lesion dies loudly rather than rendering). `dapple guide validate`: OK, 0 warnings.

## The problem, in Kenny's words

*"We are being brutal to the BDC when their forced transparency is actually
really refreshing and I want to level the playing field."*

Where the brutality actually lives (surveyed 2026-08-03): not in insults — the
guide's restraint is real — but in a structural asymmetry of VOICE:

- Every verdict resting on the BDC's own marks carries `basis: self` with the
  rule *"when you are unsure between `self` and `filed`, choose `self`. It
  claims less"* — systematic down-weighting imposed by the asset class (private
  borrowers file nothing), not by the filer's behavior.
- Meanwhile `price-the-mfp` INVERTS the same-source rule in its fund's favor:
  the fund's own daily four-decimal measurement is voiced as *"the strongest
  evidence in the family."*
- And the BDC files MORE per-position detail than any other filer in the
  family: cost AND fair value per position, plus rate/maturity/industry/
  control flags. An N-PORT carries fair value but no per-position cost; a 13F
  carries shares and value only; an N-MFP is amortized-cost money-market paper.
  The most-disclosing filer gets the weakest-voiced verdicts.

## What does NOT change (epistemics stay)

- The `self`/`filed`/`outside` basis discipline and the "choose `self`" rule
  are CORRECT — a mark on a private borrower is the company's own estimate and
  a verdict must say so. Leveling changes VOICING, not epistemics.
- The nine-mechanism track layer is untouched (no re-rooting; "What could go
  wrong" remains what the tracks render as). The 2026-08-01 rebuild is settled.
- Invariant 7 stands: never say a mark is wrong, never form a credit view on a
  private borrower, never rank one BDC against another.

## Change 1 — the inversion paragraph (tone + doctrine)

The price-the-mfp pattern, ported for disclosure GRANULARITY (distinct from
mark VERIFIABILITY, which stays `self`):

- `CONDUITS.md`: the basis section keeps its discipline and gains the
  inversion — per-position cost and fair value side by side is the richest
  disclosure in this guide family; where a verdict rests on data only a BDC is
  compelled to file, that is the disclosure regime WORKING, and the writing
  says so rather than merely flagging `self`.
- `prompts/mark-the-bdc-weigh-generate/en.md`: one added rule — never penalize
  granularity; when evidence exists only because the form compels it, name that
  as the reason the check was possible at all.
- `prompts/mark-the-bdc-write-verdict/en.md` and `write-mechanisms/en.md`:
  matching tone guidance (the mfp same-source paragraph is the template).

## Change 2 — the new chapter: "What Only a BDC Must Show"

The asymmetry itself becomes story. All-deterministic material:

- New `scripts/mark-the-bdc-coverage-generate.nu` (no prompt):
  - **Per-field fill rates over the already-parsed SOI rows** (the
    portfolio-pull / marks-generate output): n positions; % with cost; % with
    fair value; % with rate; % with maturity; % with industry; % with
    control/affiliation flags. These are this filer's own filed coverage
    numbers — the guide already measures coverage for the "easiest lie in this
    subject" spread rule; this generalizes that measurement into material.
  - **A fixed cross-form contrast table** (script constants, citing form
    rules, not fetched): what the 10-K SOI compels vs what N-PORT / 13F /
    N-MFP filers are permitted to omit — per-position cost basis, fair value,
    rate, maturity, non-accrual status, control flags.
- Lands as a new synthesize section + a chapter in the deliverable, placed
  after "The portfolio as filed": the reader learns early that everything the
  report is about to do is POSSIBLE because of what this form compels.
- The write prompt for the chapter states the leveling thesis in one paragraph
  and lets the two tables carry it.

**The one hard rule for the contrast table:** it states what other forms *may
omit* — NEVER that any filer omitted wrongly. Misfiling ≠ violation extends to
form-scope differences: less-disclosing filers are complying with their forms.
The table contrasts REGIMES, not filers.

## Cascade

`mark-the-bdc.md` (new coverage stage/step + expose lists) →
`scripts/mark-the-bdc-coverage-generate.nu` (new), `connect.nu` (passthrough if
the chapter's rows join the point pool — default: they don't; the chapter is
standing material, pre-fork like the record stages), `synthesize.nu` (new
section blob + chapter) → the three prompt edits above + a write prompt for the
new chapter → `CONDUITS.md` (inversion paragraph) → `probes/` static (coverage
constants + contrast-table strings pinned; the "may omit / never violation"
phrasing pinned) + render (chapter renders; zero-coverage field renders as its
number, not as absence) → `AGENTS.md` (leveling doctrine + this plan's
rationale) → `locales/en.ftl`.

## Fuzz arms (Plan F, landed this session)

- Coverage math over adversarial SOI rows: empty portfolio, all-null field
  columns, unicode borrower names, duplicate positions — fill rates always
  0-100, denominators explicit, no empty-aggregation record-vs-table trap
  (`math sum` on empty after `each` returns a record — the known nushell trap;
  guard shape asserted).
- Contrast table is constant: probe pins its exact rows so a "helpful" edit
  that adds an editorializing column reddens.

## Landmines

- Do not let the inversion bleed into mark verifiability: "richest disclosure"
  is about FIELD COVERAGE the form compels; a verdict on a mark still rests
  `self` and still says so. Two sentences apart, never merged.
- The banned-adjective list in write-mechanisms ("high, low, safe, risky,
  comfortable, aggressive, conservative, thin, stretched") applies to the new
  chapter too — coverage percentages are stated, never characterized.
- SUMMARY.md likely unchanged (book skeleton pages only) — verify during the
  session rather than assuming a new primer is needed.
