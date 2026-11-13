# Guide authoring helpers
#
# `dapple guide validate <dir>` runs the exact parse -> plan -> validate
# pipeline the ingest endpoints run (structural checks + every task's nushell
# script through the EMBEDDED StepEngine parser), locally, with no database
# and no server round-trip. Catches "invalid nushell script" 422s before a
# sync. The embedded parser is stricter than an interactive system nushell:
# e.g. `str lowercase` (real command: `str downcase`) parses fine
# interactively but is rejected at ingest with a misleading
# "Extra positional argument" error.
#
# Examples:
#   dapple guide validate ~/src/dapple/dapple-guides/public/dapple/my-guide
#   dapple guide validate            # from inside the guide directory
#   dapple guide validate . -n       # reuse the already-built binary

use std/log

# Validate a guide directory locally, exactly as the ingest endpoints would.
export def validate [
    guide_dir: path = "."   # Guide directory (contains guide.toml)
    --no-build (-n)         # Skip the cargo build; use the existing guide-validate binary
] {
    let workspace = $env.DAPPLE_PROMPT_WORKSPACE_DIR
    let bin = ($workspace | path join "target" "debug" "guide-validate")

    if not $no_build {
        log info "Building guide-validate [cargo is incremental — fast when fresh]"
        let built = (
            do {
                cd $workspace
                SQLX_OFFLINE=true cargo build --quiet -p dapple-prompt-api --bin guide-validate
            } | complete
        )
        if $built.exit_code != 0 {
            log error "cargo build of guide-validate failed"
            print $built.stderr
            return
        }
    } else if not ($bin | path exists) {
        log error $"($bin) not found — run once without --no-build to build it"
        return
    }

    let result = (do { ^$bin ($guide_dir | path expand) } | complete)
    if ($result.stdout | is-not-empty) { print --no-newline $result.stdout }
    if ($result.stderr | is-not-empty) { print --no-newline $result.stderr }
    if $result.exit_code != 0 {
        log error "guide validation failed — fix the errors above before syncing"
    }
}

# Run a guide's probes, or every guide's probes.
#
# `validate` proves a script PARSES. Probes prove it RUNS, which is the question that has
# actually bitten: a guide script can parse clean, pass a thin-fixture probe, and still die
# on real data at the last stage of an expensive journey. Until now every probe was run by
# hand, so "we have probes" and "the probes ran" were different facts.
export def probe [
    guide_dir: path = "."   # A guide directory. Ignored when --all is passed.
    --all (-a)              # Run every guide's probes under DAPPLE_GUIDES_DIR
    --lint (-l)             # Also run the cross-guide nushell shape lint first
] {
    let guides_root = ($env.DAPPLE_GUIDES_DIR? | default "")

    if ($all or $lint) and ($guides_root | is-empty) {
        log error "DAPPLE_GUIDES_DIR is not set - load the dapple env overlay first"
        return
    }

    let lint_failed = if $lint {
        let lint_path = ($guides_root | path join "probes" "lint_nu_shapes.nu")
        if not ($lint_path | path exists) {
            log warning $"no shape lint at ($lint_path) [skipping]"
            false
        } else {
            print "=== shape lint ==="
            let r = (do { ^nu $lint_path } | complete)
            print $r.stdout
            if ($r.stderr | is-not-empty) { print $r.stderr }
            $r.exit_code != 0
        }
    } else { false }

    let probe_files = if $all {
        ([
            (glob ($guides_root | path join "public" "*" "*" "probes" "*.nu"))
            (glob ($guides_root | path join "internal" "*" "*" "probes" "*.nu"))
            # The cross-guide probes live at the repo root, not under a guide. Matched as
            # `probe_*` so the shared `harness.nu` and the lint are not run as probes.
            (glob ($guides_root | path join "probes" "probe_*.nu"))
        ] | flatten | sort)
    } else {
        (glob (($guide_dir | path expand) | path join "probes" "*.nu") | sort)
    }

    if ($probe_files | is-empty) {
        log warning "no probes found"
        if $lint_failed { exit 1 }
        return
    }

    print $"=== running ($probe_files | length) probes ==="
    # Sequential on purpose: each probe spawns its own `nu` per arm, and par-each here would
    # multiply that fan-out for no wall-clock win on a laptop.
    let results = ($probe_files | each {|p|
        let r = (do { ^nu $p } | complete)
        let name = ($p | path basename)
        let guide = ($p | path dirname | path dirname | path basename)
        print $"  (if $r.exit_code == 0 { 'ok  ' } else { 'FAIL' })  ($guide) / ($name)"
        if $r.exit_code != 0 {
            # The tail is where a probe prints which assertion died.
            $r.stdout | lines | last 6 | each {|l| print $"          ($l)" }
            if ($r.stderr | is-not-empty) { $r.stderr | lines | last 6 | each {|l| print $"          ($l)" } }
        }
        { guide: $guide, probe: $name, ok: ($r.exit_code == 0) }
    })

    let failed = ($results | where ok == false)
    print ""
    if ($failed | is-empty) and (not $lint_failed) {
        print $"all ($results | length) probes passed"
    } else {
        print $"($failed | length) of ($results | length) probes FAILED(if $lint_failed { ' [+ shape lint]' } else { '' })"
        exit 1
    }
}

# Show guide command help
export def main [] {
    print "
dapple guide - Guide authoring helpers

Usage:
  dapple guide validate [dir]   Validate a guide directory locally, exactly as
                                the ingest endpoints would: parse -> plan ->
                                structural checks + every task's nushell script
                                through the embedded StepEngine parser. Catches
                                ingest 422s before a sync round-trips the server.
                                [dir] defaults to the current directory.
  dapple guide validate . -n    Skip the cargo build; reuse the existing
                                target/debug/guide-validate binary.
  dapple guide probe [dir]      Run every probe in a guide's probes/ directory.
                                Probes prove a script RUNS, which validate does
                                not - validate only proves it parses.
  dapple guide probe --all      Run every guide's probes under DAPPLE_GUIDES_DIR.
  dapple guide probe --all -l   ... and run the cross-guide nushell shape lint
                                first [group-by key columns, unguarded math,
                                deprecated get -i].
"
}
