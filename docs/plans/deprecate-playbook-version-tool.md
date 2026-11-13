# Future ticket — Tool: mark a specific playbook *version* as unusable (soft delete)

**Status:** Proposed / backlog
**Filed:** 2026-05-29

## Context / motivation

A lot of guide (playbook) versions have been loaded into the environment that
we do not intend to keep — only a handful are "real." We want a way to
**explicitly retire a specific playbook version** so resolution and listings
skip it, *without* hard-deleting it (keep the audit trail; don't break in-flight
journeys). This is the clean alternative to leaning on system fallbacks /
strict-default behavior to paper over un-curated content: instead of the
platform guessing, an operator marks the junk versions unusable.

This complements value-slot-overlay authoring: the keepers get overlays
authored (real per-stage gating); the rejects get deprecated and disappear.

## What already exists (reuse, don't rebuild)

- **Soft-delete machinery:** `rev.deprecation_events` (event-sourced),
  `rev.deprecation_levels` (keyed levels, read via
  `select_deprecation_level_by_key`), and the shared CTEs
  `shared/deprecate_latest_versions.sqlc` + `shared/deprecate_events.sqlc`
  (write side) and `shared/select/latest_deprecation.sqlc` (read side).
- **Playbook deprecation, path-level:** `deprecate_playbooks(args:
  DeprecatePlaybooksArgs { path_ids, reasons, to_level_id, deprecated_by_user_id })`
  in `crates/dapple-data/src/query/playbooks.rs`, exposed via
  `DELETE /api/playbooks` (batch) and `DELETE /api/playbooks/{id}` (single) in
  `crates/dapple-prompt-api/src/handler/playbooks.rs` with a selectable
  `?level=` query param.
- **Soft-deprecation continuity guarantee** (see `crates/dapple-data/AGENTS.md`,
  "Journey ↔ Guide version pinning"): a journey pinned to a now-deprecated
  version keeps resolving against it; deprecation only blocks *new* use. This is
  exactly the behavior we want here.

## The gap

The existing path deprecation operates on **`path_ids`** and (via
`deprecate_latest_versions`) the **latest** version at each path. We need
**version-level** granularity: mark *one specific* `versioned_object` of a
playbook unusable — e.g. retire v2 while v3 exists, or retire a known-bad
version — leaving sibling versions untouched.

## Proposed work

1. **Data layer — version-targeted deprecation.** Add a `deprecate_playbook_versions`
   query that writes a `deprecation_events` row for an explicit
   `versioned_object_id` (or `(path_id, major, minor, patch)` selector) instead
   of "latest per path". Mirror the existing append-only event shape so the
   `latest_deprecation` CTE picks it up. Append-only + reversible (a later event
   at a lower/clear level restores it).
2. **An "unusable" level.** Confirm/seed the `deprecation_levels` key that means
   "do not use at all" (vs. a softer "deprecated but still selectable"), and use
   it as the tool's default `to_level`.
3. **Resolution must honor per-version deprecation.** Audit every place that
   resolves or lists playbook versions so the deprecated version is excluded:
   - `select_latest_guide_id_for_playbook` / journey auto-pin
   - playbook SELECT/list variants
   - the per-stage views (`/stages`) and brief-form derivation
   Confirm each filters on the `latest_deprecation` level per *version*, not
   just per path.
4. **The tool.** A nushell tool under `assets/nu/modules/tools/` wrapping a
   version-targeted endpoint (e.g. `DELETE /api/playbooks/{id}/versions/{ver}`
   or a `?version=` param on the existing handler), so an operator can run
   something like:
   `nu assets/nu/... deprecate-playbook-version <slug> <version> --level removed --reason "junk load"`.
   Consider a `--dry-run` that lists what would be retired.

## Semantics (intended)

- **Soft + append-only:** writes a `deprecation_events` row; never hard-deletes.
- **Reversible:** an un-deprecate/restore event clears it.
- **Per-version:** only the named version is affected; siblings untouched.
- **In-flight safe:** journeys already pinned to the retired version keep
  running (soft-deprecation continuity); the retirement only blocks new pins,
  new journeys, and listings.

## Open questions

- **Guide vs playbook.** Journeys pin to a *guide* version (`guide_id`) and the
  playbook is reached through it. Retiring just the playbook version may not be
  enough to make a junk *guide* undiscoverable — decide whether the tool also
  (or instead) deprecates the guide version, and whether it cascades.
- **Granularity of the selector.** `versioned_object_id` (precise, opaque) vs
  `slug + semver` (human-friendly). Probably support both.
- **Which level = "unusable".** Exact `deprecation_levels` key and whether we
  need a new one distinct from existing "deprecated".
- **Listing UX.** Do deprecated versions vanish from lists, or show with a
  retired badge? (Backend should make exclusion the default and offer an
  `include_deprecated` flag for admin/audit.)

## Acceptance criteria

- Running the tool against a specific playbook version writes a
  `deprecation_events` row at the "unusable" level for *that* version only.
- After retirement: new journey creation / auto-pin will not select it; it is
  excluded from default playbook listings and from `/stages` / brief-form
  resolution.
- A journey already pinned to the retired version continues to resolve and run
  unchanged.
- The action is reversible via a restore event, and the full history is visible
  in `deprecation_events` (audit trail).
- Test (corrupt-then-verify): pin journey A to version v; retire v; assert a
  *new* journey won't auto-pin v while journey A still resolves v; restore;
  assert v is selectable again.
