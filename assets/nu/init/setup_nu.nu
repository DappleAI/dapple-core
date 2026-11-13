use std/log

$env.NU_LOG_LEVEL = "info"

const self_dir = path self | path dirname;

# Calculate paths (const for use statement, let for runtime)
const dapple_module_path = $self_dir | path join "../modules"  # assets/nu/init -> assets/nu/modules
const workspace_dir = $self_dir | path join "../../.."  # assets/nu/init -> repo root
const env_template = $workspace_dir | path join "assets/env.template"
let nushell_target_dir = $nu.config-path | path join ".."
let config_file = $nu.config-path
let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
let is_mac = ($nu.os-info.name | str contains "mac")

print "
===========================================
  Dapple Development Environment Setup
===========================================
"

# 1. Deploy modules to nushell config (per-module to avoid overwriting other projects)
log info "Step 1/7: Deploying modules..."
let modules_target = $nushell_target_dir | path join "modules"
mkdir $modules_target
ls $dapple_module_path | where type == dir | each { |entry|
    let name = ($entry.name | path basename)
    let dest = $modules_target | path join $name
    if ($dest | path exists) { rm -rf $dest }
    log debug $"  copying ($name) -> ($dest)"
    cp -r $entry.name $dest
}
log info $"  Modules installed to: ($modules_target)"

# 2. Generate ~/.env.dapple with secrets
log info "Step 2/7: Generating secrets file..."
let env_dapple_script = $dapple_module_path | path join "tools/env_dapple.nu"
nu $env_dapple_script gen --example-file $env_template
log info $"  Secrets file: ($home)/.env.dapple"

# 3. Configure config.nu with conf.d fragment (if not already configured)
log info "Step 3/7: Configuring nushell..."

# Create conf.d directory
let conf_d = $nushell_target_dir | path join "conf.d"
mkdir $conf_d

# Write dapple fragment to conf.d/dapple.nu
let fragment_content = '# Dapple development environment
use ../modules/dapple_env_profiles dapple_sandbox_tools
overlay use dapple_sandbox_tools as dapple_env

# Dapple CLI module for k8s operations
use ../modules/dapple

# Alias DATABASE_URL for sqlx CLI tooling
$env.DATABASE_URL = $env.DAPPLE_PROMPT_DATABASE_URL
'
let fragment_path = $conf_d | path join "dapple.nu"
$fragment_content | save --force $fragment_path
log info $"  Fragment written: ($fragment_path)"

# Add source line to config.nu if not already present
let source_line = 'source ($nu.default-config-dir | path join "conf.d" | path join "dapple.nu")'
let config_content = if ($config_file | path exists) { open $config_file --raw } else { "" }

if not ($config_content | str contains $source_line) {
    log info "  Adding source line to config.nu"

    # Clean up old-style inline overlay lines from config.nu
    mut cleaned = $config_content
    # Remove old dapple overlay block (inline lines that were appended directly)
    let old_patterns = [
        "# Dapple development environment"
        "# Added by setup_nu.nu - enables dapple CLI commands"
        "use modules/dapple_env_profiles dapple_sandbox_tools"
        "overlay use dapple_sandbox_tools as dapple_env"
        "# Dapple CLI module for k8s operations"
        "use modules/dapple"
    ]
    for pattern in $old_patterns {
        $cleaned = ($cleaned | lines | where { |line| ($line | str trim) != $pattern } | str join "\n")
    }
    # Collapse runs of 3+ blank lines into 2
    while ($cleaned | str contains "\n\n\n") {
        $cleaned = ($cleaned | str replace --all "\n\n\n" "\n\n")
    }
    # Trim trailing whitespace and add source line
    $cleaned = ($cleaned | str trim --right)
    $cleaned = $"($cleaned)\n\n($source_line)\n"
    $cleaned | save --force $config_file
    log info $"  Config updated: ($config_file)"
} else {
    log info "  Config already has dapple source line"
}

# Clean up orphaned modules
let orphaned = $nushell_target_dir | path join "modules" | path join "dapple_endpoints"
if ($orphaned | path exists) {
    rm -rf $orphaned
    log info "  Removed orphaned module: dapple_endpoints"
}

# 4. Ensure sqlx-cli ^0.9 (all platforms). Must track the workspace's sqlx 0.9
#    library + committed .sqlx cache format; a stale 0.8 CLI would write an
#    old-format cache. Needed for `dapple sqlx migrate`/`prepare` and
#    `dapple sqlc compose`. Placed OUTSIDE the macOS branch below — it is not
#    macOS-specific like the cross-compile toolchain.
log info "Step 4/7: Ensuring sqlx-cli (^0.9)..."
let sqlx_cli_version = (do { ^cargo sqlx --version } | complete | get stdout)
if (which cargo-sqlx | is-empty) or ($sqlx_cli_version !~ '0\.9\.') {
    log info "  installing sqlx-cli ^0.9..."
    # No --locked: it is broken for sqlx-cli as of 0.9.
    ^cargo install sqlx-cli --version '^0.9' --features native-tls,postgres,sqlite
} else {
    log info $"  sqlx-cli already on 0.9.x: ($sqlx_cli_version | str trim)"
}

# 5. Install macOS cross-compile prerequisites (Mac only — no-op on Linux)
if $is_mac {
    log info "Step 5/7: Installing macOS cross-compile prerequisites..."

    # rustup target for aarch64 Linux
    let installed_targets = (rustup target list --installed | lines | each { str trim })
    if not ($installed_targets | any {|t| $t == "aarch64-unknown-linux-gnu" }) {
        log info "  installing rustup target aarch64-unknown-linux-gnu..."
        rustup target add aarch64-unknown-linux-gnu
    } else {
        log info "  rustup target aarch64-unknown-linux-gnu: already installed"
    }

    # zig — used by cargo-zigbuild as the C cross-compiler for ring's assembly
    if (which zig | is-empty) {
        log info "  installing zig via Homebrew..."
        ^brew install zig
    } else {
        log info "  zig: already installed"
    }

    # cargo-zigbuild — wraps cargo build, routing cc-rs through zig cc
    if (which cargo-zigbuild | is-empty) {
        log info "  installing cargo-zigbuild..."
        ^cargo install cargo-zigbuild
    } else {
        log info "  cargo-zigbuild: already installed"
    }

    log info "  Cross-compile toolchain ready. Build api binaries with:"
    log info "    cargo zigbuild -p dapple-prompt-api --target aarch64-unknown-linux-gnu"
} else {
    log info "Step 5/7: Skipping cross-compile setup (Linux host — uses native cargo build)"
}

# 6. Verify kubectl connectivity
log info "Step 6/7: Verifying cluster access..."
# Check if kubectl exists in PATH first using which (returns empty on failure)
let kubectl_check = (which kubectl | get -o 0.path)
if $kubectl_check == null {
    log warning "  kubectl not found in PATH - install kubectl to manage the cluster"
    log warning "  After installing, verify with: kubectl get pods -n dapple-sandbox"
} else {
    # kubectl exists, try connecting
    let kubectl_test = (do { ^$kubectl_check get pods -n dapple-sandbox --request-timeout=5s } | complete)
    if $kubectl_test.exit_code != 0 {
        log warning "  kubectl connection failed - this is expected if not on VPN"
        log warning "  After connecting to VPN, verify with: kubectl get pods -n dapple-sandbox"
    } else {
        log info "  Cluster connection: OK"
    }
}

# 7. Install the git pre-commit hook (offline CI-parity gate).
#    core.hooksPath points git at the tracked assets/git-hooks/ dir, so the hook
#    stays in sync with the repo (no per-edit re-copy). It refuses commits that
#    would break CI — unformatted code, a stale sqlc/.sqlx cache (the recurring
#    "forgot to run prepare with --all-targets" breakage), or clippy warnings —
#    using the same offline (DB-free) checks CI runs. .git/config is
#    machine-local, so a relative path is fine and survives a repo move; git
#    resolves it against the worktree root.
log info "Step 7/7: Installing git pre-commit hook..."
let pre_commit_hook = $workspace_dir | path join "assets/git-hooks/pre-commit"
if ($pre_commit_hook | path exists) {
    # Belt-and-suspenders: clones carry the committed 100755 bit, but ensure it.
    chmod +x $pre_commit_hook
    let current_hooks_path = (do { ^git -C $workspace_dir config --get core.hooksPath } | complete | get stdout | str trim)
    if $current_hooks_path == "assets/git-hooks" {
        log info "  core.hooksPath already set to assets/git-hooks"
    } else {
        if ($current_hooks_path | is-not-empty) {
            log warning $"  overriding existing core.hooksPath: ($current_hooks_path)"
        }
        ^git -C $workspace_dir config core.hooksPath assets/git-hooks
        log info "  core.hooksPath -> assets/git-hooks"
        log info "  commits now blocked on unformatted code, a stale sqlc/.sqlx cache, or clippy warnings — offline, mirrors CI (bypass: git commit --no-verify)"
    }
} else {
    log warning $"  pre-commit hook not found at ($pre_commit_hook) — skipping"
}

# Print summary
print "
===========================================
  Setup Complete!
===========================================

Environment configured:
  - Modules:  ($nushell_target_dir)/modules/
  - Secrets:  ($home)/.env.dapple
  - Config:   ($config_file)
  - Git hook: core.hooksPath -> assets/git-hooks (pre-commit fmt/sqlc/clippy gate, offline)

Next steps:
  1. Restart nushell (or run: exec nu)
  2. Verify setup: dapple verify
  3. View available commands: dapple

Quick start after restart:
  dapple status           # Check all pods
  dapple logs api         # View API logs
  dapple quick api        # Build and deploy API

For more info: dapple --help
"
