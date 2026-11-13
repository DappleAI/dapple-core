# `edgar fund-sections` hardening — completed

**Opened 2026-08-01** as a handoff for two open pieces; **closed the same day**. This is now a
record rather than a plan. It is kept because most of what it cost was *measurement* against
real filings, and none of that is recoverable from the diff.

Branch: `edgar-fund-support` (dapple-prompt) / `new-guide/retort-the-n-port` (dapple-guides).

---

## What was wrong, and what it reads like now

| # | Defect | Before | After |
|---|---|---|---|
| A1 | Benchmarks unattributable to a fund | 13 of 13 unattributed on the 1290 prospectus; `benchmarks` empty for every fund | 16 attributions, 0 unattributed, no warnings |
| A2 | Extension members named by splitting their QName | `Cboe Sand P 500Buy Write Index`, `One 290Multi Asset Income Index` | `Cboe S&P 500 BuyWrite IndexSM`, `1290 Multi-Asset Income Index` |
| B1 | Vanguard 485BPOS ships no `FilingSummary.xml` | command **errored outright** | 12 funds, all named, `html_file: null` + a warning |
| B2 | Fidelity role URIs carry no `/N/` ordinal | every fund collapsed into one `fund_index: None` group; `--fund` failed against an empty name list | 7 funds, 6 named, `--fund` works |

Guarded by `fund_sections_works_across_three_fund_families` (live, `#[ignore]`d) plus 27 unit
tests. Lib suite 538 → 565.

---

## The five facts that cost the most to establish

**1. A role is declared in SEVERAL `presentationLink` elements and they must be UNIONED.**
On the 1290 filing: 192 elements over 96 roles — *every* role appears exactly twice, and the
split is not even. For `.../1/role/PerformanceTableData` the first element holds only
`oef:AfterTaxes*` boilerplate and the second holds all the benchmarks. **Reading only the
first returns zero benchmarks while looking like a successful parse.** Pinned by
`union_is_required_a_role_spans_several_presentation_links`.

**2. A fund's series member is in a DIFFERENT role from its benchmarks.** This was found by a
live run, not by reading: after wiring the linkbase in, every *name* resolved and every
*attribution* still failed. `PerformanceTableData` carries benchmarks + class members;
the series member sits in the fund's other roles. Attribution must therefore group by FUND
(the ordinal, or the series id in the key), never by role. Pinned by
`a_fund_whose_series_is_declared_in_another_of_its_roles_still_attributes`.

**3. `verboseLabel` outranks the standard `label` role — the standard one is the degenerate
one.** Real rows from 1290's label linkbase:

| element | `role/label` | `role/verboseLabel` |
|---|---|---|
| `eqat_S000052767Member` | `S000052767 [Member]` | `1290 Avantis US Large Cap Growth Fund` |
| `eqat_C000165722Member` | `C000165722 [Member]` | `Class A` |

Preferring the standard role *because* it is standard would have swapped a garbled name for an
identifier — worse than the QName split it was meant to improve on.

**4. A fund is a SERIES, not a prospectus variant.** Vanguard writes one role group per
variant (Investor / Admiral / Institutional prospectuses of one fund), so grouping on the role
ordinal reported **47 funds for a trust of 12**, with names repeating. `--fund` then matches
two groups and fail-closes, so the duplicates are not cosmetic. Fidelity has the same shape
one level down (`-ComboPRO` / `-RetailPRO` / `-AMCIZPRO`). Both grouping paths merge on the
series id.

**5. Vanguard's missing `FilingSummary.xml` is a filing-pipeline change, not a Vanguard
property.** Measured across its last five 485BPOS filings:

| filed | accession | FilingSummary |
|---|---|---|
| 2026-04-28 | `0000036405-26-000181` (self-filed) | **absent** |
| 2025-04-29 | `0001683863-25-004078` (agent) | present |
| 2024-04-26 … 2022-04-29 | agent | present |

So the command broke on precisely the filing a caller gets by default — the latest. Its
linkbase carries the identical `/N/role/SectionKey` vocabulary (565 roles), which is the
entire recovery.

---

## The reframing that made both pieces one job

**Roles are the substrate. `FilingSummary.xml` is one place they are written down.** It is the
only source of the role→`R<n>.htm` map, and nothing else. Everything else a caller wants —
which funds, which sections, series ids, and (from the label linkbase) the funds' real names —
is in the linkbases, which are fetched anyway for `--tagged`.

That is why piece B stopped needing an HTML-scraping or an instance-text fallback: the same
parse serves both. `html_file` became `Option<String>`; `None` means "this filing ships no
FilingSummary", and `--fetch` reports nothing **with a warning** rather than returning empty
prose — which would read exactly like a fund that disclosed nothing.

Discovery chain (standards-based, no directory listing needed on the `--tagged` path):
instance → `<link:schemaRef>` → `.xsd` → either inline linkbases (1290 1.1 MB, Vanguard 5.9 MB)
or `<link:linkbaseRef>` siblings (Fidelity's `.xsd` is 35 KB and holds none). Siblings are
selected by `xlink:role`, not by the `_pre`/`_lab` filename habit, which skips `_cal`/`_def`.

---

## Other facts worth not re-deriving

- **`instance` on the fund-sections record is an HTML filename**, not an XBRL instance. The
  real one comes from `instance_candidates()` / `fetch_instance()` in `commands/edgar_common.rs`.
- **`edgarkit` 0.3.0 has no RR/OEF taxonomy** (`Taxonomies` = `us_gaap` + `dei`), so
  `edgar facts` can never reach fund risk/benchmark facts. The instance is the only route.
- **The benchmark discriminator is structural, not lexical**: a `PerformanceMeasureAxis` member
  is a benchmark iff no context carrying it also carries a share-class member. A namespace rule
  fails — Vanguard and the rr era tag after-tax returns as *filer extensions*.
- **Boilerplate risks drop by declaring-namespace URI, never by prefix** —
  `us-gaap:CreditRiskMember` / `InterestRateRiskMember` are real fund risks.
- **Filings before the 2024q3 `oef` taxonomy carry no risk axis at all.** `saw_risk_axis` keeps
  that apart from "this fund tagged no risks".
- **Series/class ids are `S`/`C` + 9 digits** (`S000052767` = `S000` + 6 digits). A regex of
  `S0000\d{5}` matches some of them by luck and silently misses the rest.
- **A degenerate-label rule must NOT compare squashed stems.** `Russell 1000® Index` squashes
  to the same letters as `Russell1000IndexMember` minus its suffix — that is the SUCCESS case
  (filers name elements after labels). An early draft rejected it and silently threw away every
  correctly-labelled benchmark. Only two shapes are degenerate: the local name repeated
  verbatim, and a bare `S`/`C` id.
- **`index.json` is not a reliable discovery route.** A Fidelity filing lists its `.xsd`, but a
  naive read came back empty; `schemaRef` always works.

### Toolchain traps

- **`dapple guide validate --no-build` runs a STALE binary** and reports phantom parse failures.
- **Ingest parses with embedded nu 0.109; the local shell is 0.114.** `str downcase` and
  `get -i` warn locally but **must stay**.
- **`else` must share a line with the closing `}`** in nushell.
- **A raw Rust string holding XML needs `r##"…"##`** — `xlink:href="#…"` contains `"#`, which
  terminates `r#"…"#`.

---

## Verification

```bash
nu -c 'cargo test -p dapple-prompt-flow --lib'
nu -c 'cargo clippy -p dapple-prompt-flow --tests -- -D warnings'
nu -c 'cargo build --workspace'

# live (needs DAPPLE_EDGAR_USER_AGENT; the flow pod's own value is in
# assets/k8s/registry/dapple-prompt-flow.yaml)
nu -c 'cargo test -p dapple-prompt-flow --test edgar_live_test -- --ignored'

# parses exactly as ingest would — never pass --no-build after an engine change
nu -c 'use assets/nu/modules/dapple; dapple guide validate \
       /home/kenny/src/dapple/dapple-guides/public/dapple/retort-the-n-port'
```

Re-ingest is **`PATCH /api/guides/{path_id}/ingest`**, not POST. Deploy `api` **before**
syncing the guide.

---

## Still open (unrelated to this work)

- `edgar_fund_resolves_unit_investment_trusts` fails live: QQQ resolves `structure: "series"`
  (1 series) where the test asserts `"unitary"`. That path (`edgar fund` →
  `edgar_fund_registry.rs`) references none of the modules changed here, and a "unitary"
  resolution just means *no series* — so this is SEC registry drift or the parallel BDC work,
  not this change.
- The memory note "guide citations to accounting/legal rules are inert" is about guide-block
  `cite:` attributes to ASC/CFR authorities. Unrelated to the `/data/0/` citation-URL bug fixed
  earlier, and still open.
- ~~Reading section TEXT from the instance's narrative text blocks.~~ **Done the same day** —
  see [`edgar-fund-sections-instance-text.md`](./edgar-fund-sections-instance-text.md). It went
  further than sketched here: the presentation linkbase turns out to state which elements each
  section is made of, so no `section_key -> element` vocabulary was needed, and the rendered
  fragment measured 7.4× LARGER than the instance text with none of the excess being
  disclosure — so the instance became the primary source rather than a fallback.

## Files

- `crates/dapple-prompt-flow/src/edgar_fund_linkbase.rs` — **new**, presentation + label
  linkbase parsing, role/schema discovery
- `crates/dapple-prompt-flow/src/edgar_fund_tags.rs` — linkbase-aware entry point, name-ladder
  rung 2, degenerate-label veto, disclaimer stripping
- `crates/dapple-prompt-flow/src/edgar_fund_sections.rs` — `series_id`, series partitioning,
  `FilingSections::from_linkbase`, `html_file: Option`
- `crates/dapple-prompt-flow/src/commands/edgar_fund_sections.rs` — linkbase fetch, the
  no-FilingSummary fallback, `series_id` on fund rows
- `crates/dapple-prompt-flow/tests/edgar_live_test.rs` — the cross-filer guard
