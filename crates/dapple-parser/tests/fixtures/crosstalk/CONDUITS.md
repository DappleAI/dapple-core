# Conduit Context — Crosstalk (fixture)

A tiny conversation flow: the `director` prompt writes dialogue, a voice
synthesizes each line, and `av mix` overlaps the clips.

## Bindings

This fixture defines no value slots; the `orchestrator` task's payload carries
the mixed audio directly.

## Reading retrieved prose

Any retrieved excerpt is parser-fixture filler; the real Crosstalk guide lives
in dapple-guides.
