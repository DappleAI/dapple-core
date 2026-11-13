# Conduit Context — VOD Recap (fixture)

A tiny reel-cutting flow: analyze a VOD, let the `editor` prompt propose beats,
assemble the recap reel.

## Bindings

- `recap` (asset) — the assembled recap reel video. Computed: bound by the
  `assemble` step from `/recap` in the task payload; rendered to the operator
  as "The recap".

## Reading retrieved prose

Any retrieved excerpt is parser-fixture filler; the real gaming-VOD guide lives
in dapple-guides.
