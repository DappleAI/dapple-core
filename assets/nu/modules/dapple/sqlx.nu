# cargo sqlx commands for the dapple-prompt workspace
#
# sqlx-cli reads DATABASE_URL, which the dapple env overlay sets alongside
# DAPPLE_PROMPT_DATABASE_URL. Migrations live in crates/dapple-data/migrations/.
#
# Examples:
#   dapple sqlx migrate          # Apply pending migrations (cargo sqlx migrate run)
#   dapple sqlx info             # Show migration status (cargo sqlx migrate info)
#   dapple sqlx prepare          # Refresh the offline query cache (--workspace --all-targets)
#   dapple sqlx prepare --check  # Verify the offline cache is up to date (CI)

use std/log

# Resolve the dapple-data crate directory (holds migrations/ and the .sqlc sources)
def data-crate-dir []: nothing -> string {
    $env.DAPPLE_PROMPT_CRATES_DIR | path join "dapple-data"
}

# Guard cache-writing operations against a stale sqlx-cli. The workspace is on
# the sqlx 0.9 library + committed .sqlx (0.9 format); an older 0.8 CLI would
# silently regenerate the cache in the old format, mismatching everyone else's
# build. Errors with the exact install command instead.
export def ensure-cli-version [] {
    let v = (do { ^cargo sqlx --version } | complete | get stdout | str trim)
    if ($v !~ '0\.9\.') {
        let found = (if ($v | is-empty) { "not installed" } else { $v })
        error make { msg: $"sqlx-cli 0.9.x required [found: ($found)]. Install:
  cargo install sqlx-cli --version '^0.9' --features native-tls,postgres,sqlite" }
    }
}

# Apply pending database migrations.
#
# ALWAYS migrate via this (or `cargo sqlx migrate run`) — NEVER `psql -f`, so the
# _sqlx_migrations ledger stays correct. Run from the dapple-data crate so the
# default ./migrations source resolves.
export def migrate [] {
    let data_dir = (data-crate-dir)
    log info $"Running cargo sqlx migrate run in ($data_dir)"
    let r = (do { cd $data_dir; ^cargo sqlx migrate run } | complete)
    print (($r.stdout + $r.stderr) | str trim)
    if $r.exit_code != 0 {
        log error "migrate run failed (is DAPPLE_PROMPT_DATABASE_URL reachable?)"
        error make { msg: "sqlx migrate run failed" }
    }
}

# Show applied + pending migration status (read-only).
export def info [] {
    let data_dir = (data-crate-dir)
    do { cd $data_dir; ^cargo sqlx migrate info }
}

# Refresh (or --check) the sqlx offline query cache for the whole workspace.
#
# Run from the workspace root with --workspace so every prompt crate (api, flow,
# external-api, ...) — not just dapple-data — gets its .sqlx cache refreshed.
#
# `-- --all-targets` forwards to the underlying `cargo` build so the cache ALSO
# covers test-target inline `sqlx::query!` macros (e.g. the integration tests in
# crates/*/tests/). Without it the cache holds only lib/bin queries, and an
# offline test build (`SQLX_OFFLINE=true cargo build --tests`, used by the CI
# warnings gate) fails with "no cached data for this query". Keep this flag on
# BOTH the write and --check paths so they capture the same query set.
export def prepare [
    --check (-c)   # Verify the committed .sqlx cache is current instead of writing it
] {
    ensure-cli-version
    let ws = $env.DAPPLE_PROMPT_WORKSPACE_DIR
    let action = if $check { "Checking" } else { "Preparing" }
    log info $"($action) sqlx offline cache for the workspace"
    let r = (
        do {
            cd $ws
            if $check {
                ^cargo sqlx prepare --workspace --check -- --all-targets
            } else {
                ^cargo sqlx prepare --workspace -- --all-targets
            }
        } | complete
    )
    print (($r.stdout + $r.stderr) | lines | last 6 | str join (char nl))
    if $r.exit_code != 0 {
        log error "cargo sqlx prepare failed"
        error make { msg: "sqlx prepare failed" }
    }
    log info "Done"
}

# Show sqlx help
export def main [] {
    print "
dapple sqlx - database migrations & offline query cache

Usage: dapple sqlx <command> [options]

Commands:
  dapple sqlx migrate          Apply pending migrations (cargo sqlx migrate run)
  dapple sqlx info             Show migration status (cargo sqlx migrate info)
  dapple sqlx prepare          Refresh the offline query cache (--workspace --all-targets)
  dapple sqlx prepare --check  Verify the offline cache is up to date (CI)

Notes:
  - Migrations live in crates/dapple-data/migrations/; ALWAYS migrate, NEVER psql -f.
  - sqlx reads DATABASE_URL (set by the dapple env overlay).
  - After editing queries, prefer `dapple sqlc compose` (compose + prepare in one).
"
}
