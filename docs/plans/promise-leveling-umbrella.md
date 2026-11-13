# Plan — Promise leveling across the fund guide family (umbrella)

**Status: THE WHOLE UMBRELLA IS EXECUTED 2026-08-03 — Plans 0, 1, 3, 4a, 4b
(both layers, incl. the run-the-10k inverse lens), 5, 6, E1, E2 and the Plan F
backfill, offline/fixture-verified. The new cohort guide is named
`sort-the-cohort` (Kenny). One deliberate deferral: the nport streamer
cargo-fuzz target (needs nightly + a fuzz/ crate; excluded from the default
suite by design). dapple-prompt engine work is on `edgar-fund-support`,
uncommitted; guide edits sit on top of Kenny's committed in-flight run-the-10k
work. Nothing ingested; Kenny regresses and ingests.**
This is the index document; each workstream below has its own plan doc and is sized
to run as its own session. Per-workstream execution notes live at the top of each
executed doc. E1 verified the DERA N-CEN linchpin against the live 2026q2 archive
and found one design correction (a quarterly set is not the roster; the command
unions a rolling year) — both unblockers for Plans 3 and 4 now exist.

## What Kenny asked for, in his words

- *"The 13-f, n-port and mfp guides are all missing a huge piece of what we know.
  Consider the BDC, they provide a 10-k and make statements... we are being brutal
  to the BDC when their forced transparency is actually really refreshing and I
  want to level the playing field."*
- *"13-f should be an aggregate of those. Like if the 13-f has nothing but funds
  that don't meet their promise that says more about the manager than the funds
  themselves."*
- On ZIP (*"the worst 10-k read I have found"*): *"If they are in a long term safe
  n-port, that's a problem. If they are in a fund about cutting edge something or
  another, that's a problem too, they just kind of added the word AI to the
  filings."*
- *"that yardstick we built will really shape the narrative of the group of
  stories that make up the 10Ks in the n-port"*
- The inverse: *"grouping insiders across things like safe bet indexes vs.
  adventurous explorative investment and how they acted as a group... there is a
  story there and there is insight in how they move as groups."* Accepted trade:
  *"this may drop how much we show about institutional movers individually, but
  that isn't a bad thing necessarily."*
- *"And then things like fuzzing style checks everywhere we can as well, I didn't
  see much of that."*

## The three ideas, stated once

1. **Level the field: every filer is judged against ITS OWN stated promise,
   never an absolute disclosure yardstick.** The BDC's forced per-position
   granularity (cost AND fair value — the family's richest disclosure) must read
   as the regime working, not get silently down-weighted while opaque filers
   escape judgment by disclosing less. The `self`/`filed`/`outside` basis
   discipline is epistemically right and stays; what changes is the *voicing*
   (price-the-mfp already made exactly this inversion for its own same-source
   case — that's the pattern).

2. **The promise is the narrative organizer, not just a verdict input.** A fund's
   holdings are a group of company stories; the fund's stated promise casts them
   into roles — embody / strain / contradict. A holding's own filing character is
   evidence in BOTH directions (the ZIP rule): a hollow thematic label contradicts
   a safe fund AND indicts the thematic fund that bought the label.

3. **Aggregation runs both directions.**
   - DOWN→UP for managers: a 13F manager's advised funds each made promises; the
     distribution of their promise-vs-holdings verdicts is a fact about the
     manager (delivered by juxtaposition — never a manager grade).
   - UP across cohorts (the inverse): insiders of held companies, grouped by the
     character of the funds holding them, read as group conduct.

## Decisions already made (do not re-litigate)

| Question | Decision |
|---|---|
| Where does the insider-cohort story live? | All three: a NEW guide + a mind-the-13f layer + a run-the-10k layer |
| How is fund character assigned? | Opus + closed vocabulary (retort-style assignment; unknown keys drop) |
| 13F rollup path | ADV-join: manager → advised registered funds → their verdicts roll up |
| BDC leveling depth | Tone + doctrine inversion PLUS a new disclosure-completeness chapter |
| Movers detail | Cohort lens may shrink per-institution enumeration; accepted |

## The linchpin fact (verified 2026-08-03)

`edgar adv` parses only the ADV Part 1A bulk roster — it does NOT expose the
funds a manager advises. The manager→funds join must invert **N-CEN** (per-series
`investmentAdviserCrdNo`, `src/edgar_ncen.rs`). So the 13F rollup has an engine
prerequisite: a new `edgar advised` command (Plan E1). Traps already known: CRD
zero-padding differs between N-CEN and the ADV roster (`000105958` vs `105958`),
and a 13F carries no CRD at all — manager identity is name-joined and fails
closed, so the rollup must degrade to "absent, reported".

## Workstreams

| Plan | Doc | Blocks / blocked by |
|---|---|---|
| 0 — Fund-character key (shared spec) | `fund-character-key.md` | blocks 1, 3, 4, 6 |
| E1 — `edgar advised` (engine) | `edgar-advised.md` | blocks 3 |
| E2 — Batch insiders + cohort aggregates (engine) | `edgar-insiders-cohorts.md` | blocks 4 |
| 1 — retort-the-n-port: role casting | `guide-retort-the-n-port-role-casting.md` | after 0 |
| 3 — mind-the-13f: advised rollup + cohort layer | `guide-mind-the-13f-advised-rollup.md` | after 0 + E1 (+4a for the cohort layer) |
| 4 — Insider cohorts: new guide + two layers | `guide-insider-cohorts.md` | after 0 + E2 |
| 5 — mark-the-bdc: leveling | `guide-mark-the-bdc-leveling.md` | independent |
| 6 — price-the-mfp: character key adoption | `guide-price-the-mfp-character-key.md` | after 0 |
| F — Fuzz/property checks | `fuzz-property-checks.md` | woven through all + one backfill session |

```
Session 0 ──┬─→ Session 1 (retort)
            ├─→ Session 6 (mfp)
            ├─→ Session E1 ─→ Session 3 (13f rollup)
            └─→ Session E2 ─→ Session 4a (new guide) ─→ Session 4b (layers, 13f cohort)
Session 5 (bdc) — independent, any time
Plan F — every session carries its own fuzz arms; one backfill session at the end
```

## Family doctrine guards (every session re-reads this list)

- Verdicts and roles are never totaled into an opinion of a fund (retort's
  "NEVER GRADE THE FUND" survives everything here).
- No manager grade, and the deleted husk stage does not return
  (mind-the-13f/AGENTS.md:319-329) — the rollup's claim-bearers are the FUNDS.
- No motive verbs in cohort conduct: "sold", "bought", "filed" — never "dumped",
  "fled", "piled in".
- Misfiling ≠ violation extends to form-scope differences: the BDC contrast table
  states what other forms *may omit*, never that omission is a violation.
- Basis discipline intact (bdc `self` down-weighting stays); mfp's same-source
  inversion untouched.
- All cohort math consumes CLASSIFIED Form 4 aggregates (trading vs
  compensation), never raw rows — naive disposal counting overstates ~12x.
- Caps are never silent: every cap carries and renders its skipped count.
- Guides share no code. The character key stays identical across guides the way
  the family keeps everything identical: probes pin the exact lines.

## Track-layer blast radius (why several designs deliberately avoid tracks)

Any change to a track root or track payload cascades:
`scripts/<guide>-connect.nu` → `scripts/<guide>-synthesize.nu` →
`prompts/*-write-*/en.md` + `-connect/en.md` → `probes/probe_*_static.nu` /
`probe_*_render.nu` (line-pinned; written to redden on dropped passthroughs) →
`CONDUITS.md` closed vocabularies → `AGENTS.md` → main `.md` expose lists →
`locales/en.ftl`.

Standing decisions that respect this:
- retort's roles hang off the CAST, not the claim tracks (tracks stay
  claim-rooted; region/tier/basis machinery untouched).
- mark-the-bdc's nine-mechanism tracks are NOT re-rooted; its promise layer
  already lives beside them.
- price-the-mfp's four fixed characters keep their daily series; the character
  key rides the existing promise pass.
- mind-the-13f's tenure tracks are untouched; the rollup and cohort material are
  new layers, not re-roots.

## Verification posture

Per session: guide probes green INCLUDING the new fuzz arms, each demonstrated
able to fail first (lesion-tested, the mark-the-bdc precedent); engine
`cargo test -p dapple-prompt-flow` on fixtures + proptest (live EDGAR tests
excluded — rate-limited, whole-file runs fail deceptively); `dapple guide
validate` on touched guides. No live generative dispatches fired to verify —
Kenny runs those.
