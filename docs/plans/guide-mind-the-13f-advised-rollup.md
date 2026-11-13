# Plan 3 — `mind-the-13f`: the funds they run, against their own words

**Status: EXECUTED 2026-08-03 (rollup AND cohort layer) — offline-verified, not
ingested, not committed.** Execution notes:
- Stages landed as 14 "Pull the Funds They Advise" (`mn_advised`, ADVISED_CAP 8,
  absence outcomes not_registered / no_advised / ambiguous_adviser /
  adv_unavailable all stated answers) and 15 "The Funds They Run, Against Their
  Own Words" (`fd` triad + `mn_funds`; ONE Opus call per fund, tagged-line format
  `character|key|basis` + `claim|verdict|claim|evidence`, unknown verdict
  defaults DOWN to unevidenced + counted, unknown key drops to none + counted).
  The rollup section renders DETERMINISTICALLY in synthesize (roster line +
  per-fund verdict table + distribution sentence + worst-claim quotes + boundary
  sentence); write-record gained a rollup-boundary guard instead of writing it.
- The cohort layer landed as stage 11 "How the Insiders of the Book Acted"
  (`ic` triad + `mn_insiders`, COHORT_CAP 12 over mn_resolved's operating
  companies, batch `edgar insiders`, stat sentences pattern-copied from 4a and
  cross-pinned in BOTH guides' static probes).
- Husk guard REFINED, not relaxed: probe §10 now pins the claim-bearer guard in
  the funds prompt, the record pass staying verdict-free, and the boundary
  sentence in the render; §19 pins vocabularies/caps/absence arms; §20 pins the
  shared cohort stat sentences. New `probe_13f_fuzz.nu` (arms A–L); 6 lesions
  demonstrated (default-verdict promotion, vocab bypass, wrap-guard removal,
  roster cap removal, boundary removal, cohort no-trading flip).
- `dapple guide validate` OK (24 tasks; the brief-pack wiring seam fired for the
  two new groups and was fixed in introduction.md — the documented seam).

Part of the promise-leveling umbrella. Needed Plan 0 (character key) + Plan E1
(`edgar advised`); the cohort layer needed Plan 4a (stat definitions copied).

## The idea, in Kenny's words

*"13-f should be an aggregate of those. Like if the 13-f has nothing but funds
that don't meet their promise that says more about the manager than the funds
themselves."*

## The doctrine constraint this must NOT break

`mind-the-13f/AGENTS.md` (§ around lines 319-329): a 13F has no promise
document; the draft "Weigh the Conduct Against the Record" stage was deleted as
"a husk" that "compares a thing to itself", with the guard *"If a build ever
emits `unfalsifiable` about a 13F, the husk is back."*

The rollup survives this because **the claim-bearers are the FUNDS, not the
13F**. Each advised fund filed a prospectus — a real promise document, owned by
the fund, adjudicable against the fund's own N-PORT positions. The 13F remains
a record of conduct and a pointer; the manager-level reading is delivered by
**juxtaposition of the funds' verdict distribution, never a manager verdict**.
AGENTS.md gains an explicit note distinguishing this from the husk: the husk
compared the 13F to itself; the rollup adjudicates OTHER filers' claims that
happen to share an adviser.

## Design

### Stage A — "Pull the Funds They Advise" (deterministic)

`scripts/mind-the-13f-advised-pull.nu`:
- Manager identity comes from the existing `mn_adv` binding (ADV roster
  name-join — a 13F carries no CRD). If that join is `not_registered` or
  ambiguous, the stage records exactly that and the rollup chapter states its
  absence — never guesses (fail-closed, absence-over-guess).
- With a CRD: `edgar advised` → roster, capped **ADVISED_CAP = 8** by net
  assets, skipped count carried and rendered.

### Stage B — "The Funds They Run, Against Their Own Words" (compact per-fund check)

`scripts/mind-the-13f-funds-generate.nu` + `prompts/mind-the-13f-funds-generate/en.md`:

Per advised fund (series):
1. `edgar fund-sections` (+ `--tagged`) for objective/strategy/risks.
2. Latest `edgar nport` — top **TOP_POSITIONS = 15** by weight + concentration
   figures (`edgar nmfp` instead for money-market series).
3. **ONE compact Opus call**: assign the character key (Plan 0 vocabulary,
   same drop rule) AND adjudicate ≤ **CLAIMS_PER_FUND = 5** claims using
   retort's EXACT four-verdict vocabulary (`supported` / `contradicted` /
   `unevidenced` / `unfalsifiable`) and its tier rule, compressed to position
   data as evidence. **No look-through into holdings' own filings** — that is a
   full retort journey, and the route stage already points there.

Aggregation is deterministic script work: verdict counts per fund and across
funds. The write layer states the distribution — *"of 8 funds, 5 carry at least
one contradicted claim"*, with each fund's worst-supported claim quoted and
figures cited — and stops. No roll-up adjective, no manager grade.

Cost envelope: 1 bulk N-CEN dataset (persisted once by E1) + 8 × 2 throttled
EDGAR fetches + 8 compact Opus calls ≈ bounded minutes. All three caps carry
skipped counts.

### The cohort layer (after Plan 4a exists)

`scripts/mind-the-13f-cohort-generate.nu` — deterministic finder, NO prompt:
top resolved operating-company holdings from `mn_resolved` (cap **12**) → E2's
batch `edgar insiders` → the cohort aggregate block (breadth, direction by
classification, timing) rendered beside "What the Holdings Filed". Stat
definitions are pattern-copied from the new cohort guide (4a) so the two agree.
Accepted trade documented in AGENTS.md: this shrinks the per-institution movers
cast — *"that isn't a bad thing necessarily"* (Kenny, 2026-08-03); groups
moving together are the insight.

### Report placement

Two new sections in the deliverable, after "The record itself":
- **"The funds they run, against their own words"** — roster line (n advised,
  n read, n skipped), per-fund character + verdict summary rows, the
  distribution sentence.
- Cohort triad beside "What the Holdings Filed" (when the layer lands).

## Cascade

`mind-the-13f.md` (two new stages + binding docs + expose lists; mind the
pure-compute dependency note around lines 94-99 — advised-pull must sit a stage
BEFORE funds-generate so the consumed-computed-dep gate covers the handoff) →
`scripts/` advised-pull (new), funds-generate (new), cohort-generate (new,
later), connect.nu + synthesize.nu (new payload passthroughs + section blobs) →
`prompts/` funds-generate (new), write-record (rollup section framing) →
`CONDUITS.md` (character key block per Plan 0 + the compact-verdict vocabulary
with its tier rule) → `probes/probe_13f_static.nu` / `probe_13f_render.nu`
(key-line pins; roster-cap lesions; verdict-parse lesions; absent-CRD
degradation renders) → `AGENTS.md` (not-the-husk note; movers-cast trade) →
`locales/en.ftl`.

## Fuzz arms (Plan F, landed this session)

- Verdict/character parse: unknown verdict, unknown key, wrong field count,
  empty reply — dropped + counted, each lesion shown to fail.
- Roster handling: empty roster, single fund, >8 funds (skip count), duplicate
  series ids, zero-net-assets rows — deterministic, order-stable.
- Absent-CRD path: the whole rollup degrades to one stated-absence line; probes
  assert the chapter renders the absence rather than vanishing.

## Landmines

- The husk guard: if any output emits `unfalsifiable` ABOUT THE 13F (rather
  than about an advised fund's claim), the design has drifted — probe arm
  asserts the rollup section never names the 13F as a claim-bearer.
- `mn_adv` is a name-join; homonym managers must fail closed (existing
  behavior — do not weaken it to make the rollup fire more often).
- Money-market series route to `edgar nmfp`, not `edgar nport` — a wrong-form
  fetch burns rate limit and returns nothing.
- EFTS fallback in E1 is rate-limited; the guide never retries the roster fetch
  in a loop.
