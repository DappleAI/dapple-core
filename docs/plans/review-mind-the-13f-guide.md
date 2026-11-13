# Review guide — `mind-the-13f`, built 2026-08-02

Companion to `guide-mind-the-13f.md` (the design record). That document says what the guide
should be and why; this one says what to look at, what is verified, and **what I am least
sure of**. Written for a reviewer coming in cold.

**Location:** `dapple-guides/public/dapple/mind-the-13f/`, 43 files, untracked. Nothing is
committed in either repo.

---

## Verified, so you do not have to redo it

| check | result |
|---|---|
| `dapple guide validate . --no-build` | 4 chapters, 20 tasks, **0 warnings** |
| `nu-check`, per file, one process each (20 scripts + 2 probes) | all parse |
| `probes/probe_13f_static.nu` | **16 sections green** |
| `probes/probe_13f_render.nu` | **5 sections green**, each demonstrated red first |

⚠️ **`dapple guide validate` needs `--no-build` in this tree.** 519 `.sqlx` cache files are
deleted and unstaged (not from this work; they appeared mid-session), so every
`SQLX_OFFLINE=true` build fails. Validation ran against the existing
`target/debug/guide-validate` binary. Worth resolving before anything else.

**Every probe assertion was reverted and watched go red before being trusted.** The five
behaviour reversions are named in the probe's own comments; the reddening output is quoted
below where it quantifies a defect.

---

## Read in this order

1. `mind-the-13f.md` — the chapter. Slots, briefs, 13 stages, 20 task declarations.
2. `CONDUITS.md` — what every model call is told. The two hard rules live at the top.
3. `AGENTS.md` — the authoring contract, the engine call sheet, the three parse traps.
4. `scripts/mind-the-13f-connect.nu` and `-synthesize.nu` — the only two that assemble
   rather than find. If the shape is wrong anywhere, it shows here first.
5. `probes/probe_13f_static.nu` §4, §5, §6, §16 — the four invariants most likely to be
   silently broken by a later edit.

---

## The three structural decisions worth scrutiny

### 1. The depth gate lives in stage 4, not stage 8

The plan says stage 8 ("Depth gate runs first"). Inside one stage it cannot work: the
runtime auto-dispatches every pure-compute step once its **earlier stages** are done, so a
compute step consuming another compute step's output from the same stage races it. An
operator select cannot race.

**Rule this produced:** at most one pure-compute step per stage; a consumer goes a stage
later, or becomes an ordered task inside the same step. Stage 12 needed the second form.

⚠️ **I derived this from `CLAUDE.md`'s description of `ensure_pending_autodispatches` and
did not test it.** The evidence it is right in the permissive direction is that
`retort-the-n-port` does exactly the same compute-then-select inside one stage. If the rule
is stricter than reality the architecture is merely over-constrained; if it is looser,
stage 4 has a race.

### 2. Connect does no assignment

The sibling guides ask a model to file evidence under claims because claim-to-evidence has
no key. Here every look-through, change and routing point carries the `holding` it is about
and a tenure is keyed by that same holding, so the join is a name match over filed rows.
The `~~~ASSIGN~~~` marker and its whole misfiling surface are gone.

**Check:** is name-matching actually reliable across passes? The look-through prompts are
told to echo the `###` heading verbatim, and `weight_of` / `cite_of` drop a row whose name
does not match. A drifted name silently loses its track membership rather than misfiling it,
which is the safe direction but is worth confirming you agree with.

### 3. Multi-task steps re-emit everything

`business-generate` (task 3 of 3) emits the merged `choices` from all three reading passes;
`current-generate` (task 2 of 2) passes `current` / `current_filings` / `filings` /
`tenures` straight through.

⚠️ **I reasoned from `dapple-prompt-flow/AGENTS.md` plus retort's shape that a step's
computed outputs are extracted from the LAST task's payload, and did not test it.** If
accumulation is actually merged across tasks, the pass-through is harmless redundancy. If
it is last-task-only, it is required. Either way the guide is safe; the comment claiming
why is the thing that might be wrong.

---

## Two defects found and fixed during review prep

Both are recorded because the *way* they hid is more useful than the fix.

### Sibling-format grouping was missing

The three `build-the-story` outputs were authored from `retort-the-n-port`'s
**pre-migration-95** shape, so they carried no `group` / `format`. The pane would have
rendered one deliverable as three unrelated results. Every other financial-family guide had
already been swept; this one was written from a stale template mid-sweep.

`dapple guide validate` was clean, all probes passed, and the defect was visible **only on a
rendered pane**. Static probe **§16** now pins it.

> **Lesson:** diff the brief-output ATTRIBUTE VOCABULARY against **every** sibling, not just
> the one you copied from.

### "TRUST" in a company name made it a fund

The coverage pass matched one marker set against the issuer name and the class together.
Measured on real 13F issuer names:

```
NORTHERN TRUST CORP    would_be_called_a_fund: true
TRUSTMARK CORP         would_be_called_a_fund: true
```

Stage 2's headline is the number every later percentage is read against, so on a book with a
few large financials this overstated how much of it is opaque by whole percentage points.
The reddened probe quantifies it: **3 fund positions instead of 1, 75% instead of 25%.**

Fixed by splitting strong markers (`ETF`, `ISHARES`, `SPDR`, `IBONDS`, …) from weak ones
(`TRUST`, `FUND`, `PORTFOLIO`); a weak marker now counts only in `title_of_class`, which is
the filer's own instrument hint, never in the issuer name alone. Behaviour probe **§E** pins
it in both directions.

**The trade runs one way on purpose:** this now misses a closed-end fund whose name carries
nothing but "FUND". Understating the opaque bucket is the safe direction, the number is
labelled provisional, stage 4 corroborates against resolution, and stage 9 settles each
position by CUSIP. A number overstating how much of a book is unreadable is a claim; one
understating it is a floor.

---

## What I am least sure of

Ranked by how much I would want a second opinion.

1. **Nothing has run against a live manager.** Everything is parse, structure and
   fixture-level. Verification item 20 of the plan (both Motley Fool managers separately,
   plus Berkshire for the 14-included-manager shape) is entirely owed.

2. **Stage 9 is probably the real request cost, not stage 8, and I capped it by guessing.**
   `FUND_CAP = 40` `edgar security` calls, each one an EFTS search plus up to 4 N-PORT
   fetches, so up to **~200 requests**; plus `MERGER_CAP = 15` companies × 2 forms = 30.
   Stage 8 is ~100 and stage 1 about 48. SEC rate-limits hard (two 429s during the engine
   build). Both caps report their shortfall, so nothing is silent, but neither number is
   measured.

3. **Stage 1 fetches the window twice** — once for the filing table, once for `--tenures`.
   The two are different returns of the same command with no combined mode, so ~50 KB per
   quarter becomes ~100 KB. Fine at 12 quarters, less fine at the 48 cap.

4. **No diagram.** `lanes` would suit a set of tenures, but its lane cap drops whichever
   lanes sort last rather than whichever matter least, and the caller is required to gate
   rather than lean on the cap. I did not verify the spec shape, so I emitted charts only
   and said so in the script. A reviewer may want the diagram; it needs the gate written
   first.

5. **The merger arm is shallow.** S-4 / DEFM14A on the top 15 operating-company positions.
   A deal on a smaller position is unlooked-for rather than absent, and the point says so,
   but it is the thinnest of the three routing arms.

6. **The `13F-NT` prose path is untested against a real notice.** `additional_information`
   is read and rendered in the organization stage, but no fixture in this build carries real
   notice prose. The engine's own tests cover the parse; the guide's rendering of it does
   not.

---

## Things that are deliberate and will look wrong

- **Ten of thirteen stages have no prompt.** A 13F contains no prose: it is rows plus seven
  cover scalars. A model pass over the manager's own record would be arithmetic done in
  prose. This is the smallest conduit surface in the family (8 prompts against retort's 13)
  and it is a property of the form.
- **`edgar fund` is called with every `FilerKind` in `--accept`.** Naming only
  `InstitutionalManager` hard-errors on Berkshire, an operating company that files a 13F. By
  that line the filer's 13F filings are already in hand, so the classification is
  description and never a gate.
- **Stage 11 adjudicates nothing.** "Weigh the conduct against the record" compares a thing
  to itself. Static probe §10 fails on `unfalsifiable` / `unevidenced` / `contradicted`
  appearing in code (comments exempt, since the record pass carries a comment naming the
  vocabulary as the thing to watch for).
- **Every aggregate is gross of fund contents**, stated everywhere rather than computed net.
  Nothing expands what an ETF holds.

---

## Repo state

| repo | state |
|---|---|
| `dapple-guides` | `public/dapple/mind-the-13f/` untracked, 43 files. Nothing else touched. |
| `dapple-prompt` | `docs/plans/guide-mind-the-13f.md` (build record appended) and this file. **519 deleted `.sqlx` files and one modified `.sqlc` are NOT from this work.** |

Nothing committed. Guide sync and ingest belong to Kenny.
