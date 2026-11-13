# fluent-syntax-unicode

Workspace-local fork of [`fluent-syntax`](https://crates.io/crates/fluent-syntax)
v0.11.1 (upstream `projectfluent/fluent-rs`). The only behavioural change is in
the identifier scanner: Fluent message and attribute identifiers now accept any
Unicode codepoint that satisfies `char::is_alphabetic` (start) or
`char::is_alphanumeric` (continuation), in addition to the upstream ASCII set.

## Why

Migration 42 in `dapple-data` made `rev.slugify()` Unicode-preserving so that
authored guides like `esboçar-o-restaurante` survive the slug derivation.
The remaining ASCII chokepoint was upstream's identifier grammar
(`Identifier ::= [a-zA-Z][a-zA-Z0-9_-]*`), which rejected every Unicode FTL
message key and so failed customer guide ingest at HTTP 422. This crate is the
minimum-surface fix: the AST, serializer, errors and slice plumbing are
byte-for-byte identical to upstream, only `helper.rs::is_identifier_start`,
`core.rs::get_identifier{,_unchecked}`, `core.rs::skip_to_next_entry_start` and
the two call sites in `expression.rs` change.

When upstream lands the same change (PR worth filing), this crate can be
deleted and the consumers swapped back to `fluent-syntax`.

## Modified files

- `src/parser/helper.rs` — `is_identifier_start`, `skip_to_next_entry_start`
- `src/parser/core.rs` — `get_identifier`, `get_identifier_unchecked`
- `src/parser/expression.rs` — message-ref and term-ref start-byte handling

## Removed files

- `src/unicode.rs` — upstream's `unescape_unicode{,_to_string}` helpers. Removed
  because nothing in dapple calls them (parsing handles `\u`/`\U` escape
  *sequences* separately in `parser/helper.rs`), and the `_to_string` variant
  carried an upstream bug (`s.push(*b as char)` mangles multi-byte UTF-8 that
  follows the first escape). Deleting the unused module avoids shipping that
  latent bug; the module returns automatically if consumers swap back to
  upstream `fluent-syntax`.

Every other file is an unmodified copy of upstream.

## Licensing

Apache-2.0 OR MIT, mirroring upstream. See `LICENSE-APACHE` and `LICENSE-MIT`.
