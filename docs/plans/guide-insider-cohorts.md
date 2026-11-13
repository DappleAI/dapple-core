# Plan 4 — Insider cohorts: a new guide + two layers

**Status: FULLY EXECUTED 2026-08-03 (4a + BOTH 4b layers) — offline-verified,
not ingested; run-the-10k edits land on top of Kenny's committed in-flight work.**
run-the-10k lens execution notes:
- Substrate correction vs the sketch: 13F holders are MANAGERS, so "registered-fund
  holders" is a two-step chain — `tk_holders` top 6 by value → `edgar adv` name join
  (fails closed) → `edgar advised` by CRD → top 2 funds per holder → one compact
  character call per fund (`run-the-10k-fundchar-generate` prompt, Opus). Most holders
  land `unassigned` with the rung named; that is coverage by design.
- Landed as post-fork stage "The Funds Behind the Holders" (`fh` triad + `tk_fundchar`),
  invariant-14 point shape so connect needed only a registry row + the `annual_years`
  exclusion; the juxtaposition (company's classified P/S insider trades beside keyed
  holders' quarterly moves, adjacent, no ordering verb) is deterministic in the script;
  write-current gained the fund-character boundary paragraph.
- `probes/probe_10k_fundchar.nu` is run-the-10k's FIRST probe — pins the family key
  (closing the Plan 0 gap), caps (6/2/4), the classified-only filter, full wiring, plus
  behavioural arms (full chain / no holders / all unassigned). Lesions demonstrated:
  unclassified direction (sells counted as non-P) reddened §2+§3; vocabulary bypass
  reddened §3 ×4. `dapple guide validate` OK (20 tasks, 0 warnings).

Earlier execution notes (4a + the 13f layer):
- 4a landed as `dapple-guides/public/dapple/sort-the-cohort/` (named
  `sort-the-cohort` by Kenny 2026-08-03; the rename sweep from the working name
  `muster-the-form-4s` is done). Full kit: 7 stages
  (Muster the Cohort → Pull the Trades → three deterministic finders → one Opus
  conduct read → Build the Story), 9 scripts, 4 prompts, 3 probes, en.ftl,
  CONDUITS.md with the family character-key block + the no-motive / no-concert /
  indirection doctrine. Deviations, documented in its AGENTS.md: FUND_CAP 4 seed
  funds; no separate connect task (no track machinery at this scale — synthesize
  renders deterministic chapters + one write call); conduct-generate has a
  PARSE-TIME banned-vocabulary guard (drops + counts) on top of the prompt rules.
- Membership is deterministic (floor 1% of a seed fund's own portfolio,
  `edgar company-cik` fail-closed, COHORT_CAP 12 counted); a model names only
  each FUND's character (drop rule; no-material short-circuit).
- Probes: static (11+1 sections incl. the cross-guide stat-sentence pin), render
  (full / empty-cohort / no-trading arms), fuzz (A–F: all-compensation renders
  NO direction, empty cohort stated in every finder, company-granular breadth,
  banned-line drop + wrap guard + width guard, event-date range validation —
  which caught a live looseness in the date regex — and the pull cap/union/drop
  arm). 6 lesions demonstrated. `dapple guide validate` OK (9 tasks, 0 warnings).
- The mind-the-13f layer landed per `guide-mind-the-13f-advised-rollup.md`
  (stage 11, `ic` triad, shared sentences cross-pinned in both probes).

## The idea, in Kenny's words

*"The 10-k guide has a bunch of insiders, so the inverse... is grouping insiders
across things like safe bet indexes vs. adventurous explorative investment and
how they acted as a group... there is a story there and there is insight in how
they move as groups."* Accepted trade: *"this may drop how much we show about
institutional movers individually, but that isn't a bad thing necessarily."*

Why it's the INVERSE of the yardstick: the yardstick reads DOWN (fund promise →
how its holdings' stories are narrated); this reads UP (company insiders →
cohorted by the promise-character of the funds holding them → group conduct).
Shared substrate: the Plan 0 character key.

## 4a — The new guide (`sort-the-cohort`, named by Kenny 2026-08-03)

Family pattern-copy from `mind-the-13f` (the closest shape: deterministic root,
external pulls, one model read). Full file kit: `guide.toml`, `SUMMARY.md`,
`CONDUITS.md` (character key block per Plan 0 + the cohort-stat vocabulary),
`AGENTS.md`, main stage `.md`, `scripts/`, `prompts/`, `probes/`,
`locales/en.ftl`.

**Scene = the cohort itself.** Not one company, not one fund: the set of
issuers that share a fund-character, and the window of time in view.

**Track root = the cohort definition** — character key + the deterministic
membership rule that built it (e.g., "issuers held ≥1% of NAV by funds assigned
`explorative-growth`"). Membership is deterministic like 13f's tenure root:
name-matched, never model-assigned.

**Stages (sketch — the build session finalizes against the family cadence):**

1. **Pull the Cohort** — operator supplies the seed (a fund, a manager, or a
   character key + fund list); resolve holdings → CIK set via `edgar resolve`;
   **COHORT_CAP = 12** issuers by weight, skipped count carried. The character
   key of each seed fund comes from its own character pass (Plan 0 machinery,
   pattern-copied).
2. **Pull the Trades** — E2 batch `edgar insiders` over the CIK set; the
   engine's cohort aggregate block is the guide's raw material. `unread`
   issuers render as coverage, never vanish.
3-5. **Three deterministic finders** (scripts, no prompts):
   - breadth — how many issuers had classified-trading activity in the window;
   - direction-by-classification — buys/sells from TRADING rows only,
     compensation counted separately and never netted;
   - timing — the monthly series, aligned to quarter ends and any
     operator-supplied event dates.
6. **Read the Cohort's Conduct** — the ONE model call. Neutral narration over
   the finders' rows only. **No motive verbs**: "sold", "bought", "filed" —
   never "dumped", "fled", "piled in", "rushed". No coordination language
   ("acted in concert") — breadth and timing are facts; concert is an
   accusation.
7. **Build the Story** — deliverable chapters:
   - "Who the Cohort Is" (membership rule, the funds and their quoted character
     bases, the issuer roster with weights);
   - "How Broadly They Acted" (breadth);
   - "Which Way, By Classification" (direction; compensation in its own band);
   - "When They Acted" (timing series);
   - "Where every point came from" (family-standard sourcing chapter).

**Doctrine block for CONDUITS.md/AGENTS.md:** cohort statistics are counts,
shares, and dates; the story voice interprets breadth and timing but never
imputes motive or coordination; a cohort is a lens, not an allegation; the
12x trap note (all math consumes E2's classified aggregates, never raw rows).

**Probes:** static (vocabulary pins incl. the character key line, cap
constants, stat-definition strings) + render (each chapter renders; empty
cohort degrades to a stated absence; unread band renders) + fuzz arms (Plan F):
adversarial aggregate blocks — zero-activity cohort, single-issuer cohort,
all-compensation cohort (direction must render as "no classified trading", not
as a direction) — each lesion shown to fail.

## 4b — The two layers (after 4a)

**mind-the-13f layer** — specified in `guide-mind-the-13f-advised-rollup.md`:
deterministic cohort finder over the manager's top resolved holdings, stat
definitions copied verbatim from 4a (duplication by design; probes pin the
shared definition strings in both guides).

**run-the-10k layer — the inverse lens on one company:**
- New `scripts/run-the-10k-fundchar-generate.nu`: from the existing holders
  material, identify registered-fund holders of the subject company; assign
  each fund's character via one compact prompt (Plan 0 vocabulary, drop rule);
  output the character MIX of who holds the company (n funds per key, weight
  where known, unassigned counted).
- Juxtaposition finder: the company's own insider conduct (existing `edgar
  insiders` material, classified) laid against the character mix and timing —
  *"insiders sold in the same window that explorative-growth funds increased"*
  — deterministic date arithmetic, neutral voice, one paragraph added to the
  ownership write prompt.
- This is where the accepted trade lands: the per-institution movers
  enumeration may shrink to make room; cohorts shrink the cast without
  rationing the material (band, don't split).
- Cascade discipline: run-the-10k's connect/synthesize/probes are the family's
  largest; the layer adds cast-side fields only, never touches track roots, and
  re-pins only the probes for what it adds.

## Landmines

- **The 12x trap is the whole game.** Any code path that computes direction
  from unclassified rows is a defect regardless of output plausibility. Probe
  arms in every home assert compensation never moves direction.
- **No motive verbs** is a hard vocabulary rule in prompts AND probes (grep-arm
  the rendered output for the banned verb list, the tone-cleanup pattern).
- Caps (COHORT_CAP, fund-list caps) always render skipped counts.
- Live EDGAR is rate-limited: fixtures first; the batch command's cache means
  re-runs are cheap, but the FIRST cohort pull is n fetches — cap n.
- The character key describes the FUND's promise; a cohort must never be
  described as "safe companies" or "adventurous companies" — the companies are
  cohorted by who HOLDS them, and the writing must keep that indirection
  explicit (it is the entire point of the story).
