# sql-composer commands for the dapple-data crate
#
# Authoring lives in crates/dapple-data/assets/sqlc/ (.sqlc sources); generated
# output goes to crates/dapple-data/assets/.sql/ (.sql, never hand-edited).
# Use `#` comments in .sqlc for documentation — sql-composer strips them from
# the generated SQL so they never reach the database. `--` comments are passed
# through verbatim and DO hit the database, so avoid them for documentation.
#
# Examples:
#   dapple sqlc compose          # Regenerate .sql, then cargo sqlx prepare --workspace
#   dapple sqlc compose -n       # Regenerate .sql only (skip the prepare step)
#   dapple sqlc verify           # CI drift check: does .sql match .sqlc? (read-only)

use std/log
# Reuse the sqlx-cli version guard so `compose` (which regenerates the .sqlx
# cache) can't run under a stale 0.8 CLI either.
use ./sqlx.nu [ensure-cli-version]

# Resolve the dapple-data crate directory (holds assets/sqlc + assets/.sql)
def data-crate-dir []: nothing -> string {
    $env.DAPPLE_PROMPT_CRATES_DIR | path join "dapple-data"
}

# Compose .sqlc sources into generated .sql, then refresh the sqlx offline cache.
#
# `cargo sqlc compose` is run from the dapple-data crate so the default
# --source/--target paths resolve. We pass --skip-prepare and run our own
# `cargo sqlx prepare --workspace` from the workspace root instead, because the
# generated queries are referenced from every prompt crate (api, flow,
# external-api, ...), not just dapple-data — a crate-local prepare would leave
# the other crates' offline cache stale.
export def compose [
    --no-prepare (-n)   # Only regenerate .sql; skip `cargo sqlx prepare --workspace`
] {
    let data_dir = (data-crate-dir)
    let workspace = $env.DAPPLE_PROMPT_WORKSPACE_DIR

    log info $"Composing .sqlc -> .sql in ($data_dir)"
    let composed = (
        do {
            cd $data_dir
            ^cargo sqlc compose --source assets/sqlc/ --target assets/.sql/ --skip-prepare
        } | complete
    )
    if $composed.exit_code != 0 {
        log error "sql-composer compose failed"
        return
    }

    if $no_prepare {
        log info "Skipping cargo sqlx prepare (--no-prepare)"
        return
    }

    ensure-cli-version
    log info "Running cargo sqlx prepare --workspace --all-targets"
    let prepared = (
        do {
            cd $workspace
            # --all-targets so the cache also covers test-target inline
            # `sqlx::query!` macros (mirrors `dapple sqlx prepare`); the CI
            # warnings gate builds tests offline and needs them.
            ^cargo sqlx prepare --workspace -- --all-targets
        } | complete
    )
    if $prepared.exit_code != 0 {
        log error "cargo sqlx prepare --workspace failed (is DAPPLE_PROMPT_DATABASE_URL reachable?)"
        return
    }

    log info "Compose + prepare complete"
}

# Verify generated .sql matches the .sqlc sources (read-only CI drift check).
# Exits non-zero if any generated file is missing or out of date.
export def verify [] {
    let data_dir = (data-crate-dir)
    log info $"Verifying composed .sql matches .sqlc in ($data_dir)"
    let result = (
        do {
            cd $data_dir
            ^cargo sqlc compose --verify --skip-prepare --source assets/sqlc/ --target assets/.sql/
        } | complete
    )
    print ($result.stdout + $result.stderr | str trim)
    if $result.exit_code != 0 {
        log error "Drift detected: run `dapple sqlc compose` and commit the result"
        error make { msg: "sqlc verify failed" }
    }
}

# Show sqlc help
export def main [] {
    print "
dapple sqlc - SQL composition for the dapple-data crate

Usage: dapple sqlc <command> [options]

Commands:
  dapple sqlc compose        Regenerate .sql from .sqlc, then cargo sqlx prepare --workspace
  dapple sqlc compose -n     Regenerate .sql only (skip the prepare step)
  dapple sqlc verify         CI drift check: does generated .sql match .sqlc sources?

Authoring notes:
  - Edit .sqlc sources in crates/dapple-data/assets/sqlc/ (never .sql/ directly).
  - Use `#` comments for documentation — they are STRIPPED from generated SQL.
  - Avoid `--` comments: they pass through to the database verbatim.
"
}
