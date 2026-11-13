#!/usr/bin/nu
#
# setup.nu — the single entry point for configuring a dapple host, in any
# environment.
#
#   nu assets/nu/init/setup.nu --environment sandbox     # a developer workstation
#   nu assets/nu/init/setup.nu --environment prod        # the production server
#
# Replaces the split where setup_nu.nu configured workstations and
# install_microk8s.nu configured the server, and neither knew the other existed.
# The prod host was consequently set up by running the DEV script on it: its nu
# fragment is headed "# Dapple development environment", and every shell on the
# production box exported the dev cluster's API host and
# `postgres://dapple:ai@prompt-db.kube.dappleai.dev:5432/dapple` — the dev
# database, with the dev password. Nothing was corrupted only because that
# hostname does not resolve from prod.
#
# Every step below declares which environments it applies to, so what a host
# gets is readable in one place instead of inferred from which script someone
# ran. Server bootstrap [microk8s, ufw, cert-manager, cluster DNS, k8s secrets]
# is NOT reimplemented here — prod delegates to install_microk8s.nu, which
# already owns it. Merging a workstation setup with a server bootstrap is where
# the risk would concentrate, and there is no shared logic to win back.
#
# Steps:
#
#   | step                    | sandbox | prod |
#   |-------------------------|---------|------|
#   | migrate legacy env files|    x    |  x   |
#   | nu modules + fragment   |    x    |  x   |
#   | generate secrets file   |    x    |      |
#   | sqlx-cli                |    x    |      |
#   | git pre-commit hook     |    x    |      |
#   | server bootstrap        |         |  x   |

use std/log
use ../modules/dapple_env_profiles/environments.nu *
use ../modules/dapple_env_profiles/env_files.nu *

# setup.nu lives at assets/nu/init/setup.nu, so FOUR dirnames reach the repo
# root [init -> nu -> assets -> root]. `path self` is parse-time-only, so this
# MUST be a const.
const WORKSPACE_ROOT = (path self | path dirname | path dirname | path dirname | path dirname)

def main [
    --environment: string,                   # Required: sandbox | prod
    --workspace-dir: string = "",
    --skip-server,                   # prod only: skip the install_microk8s.nu delegation
] {
    if ($environment | is-empty) {
        error make { msg: $"--environment is required. Known environments: ($ENVIRONMENT_NAMES | str join ', ')" }
    }
    # Fails with the valid names if this isn't one of them.
    let cfg = (environment $environment)

    let workspace = if ($workspace_dir | is-empty) { $WORKSPACE_ROOT } else { $workspace_dir | path expand }
    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }

    log info $"configuring this host as: ($environment)"

    # --- 1. Env files [all environments] ---------------------------------
    # Must run first: everything downstream reads ~/.dapple/env, and the
    # profile reads ONLY the new paths.
    log info "step: env files"
    migrate_legacy_files $home $environment
    assert_declaration_matches $home $environment

    # --- 2. Shell profile [all environments] -----------------------------
    log info "step: nu modules + conf.d fragment"
    install_shell_profile $workspace

    # --- 3. Developer-workstation steps ----------------------------------
    if $cfg.builds_from_source {
        log info "step: generated secrets file"
        generate_secrets_file $workspace

        log info "step: sqlx-cli"
        ensure_sqlx_cli

        log info "step: git pre-commit hook"
        install_pre_commit_hook $workspace
    }

    # --- 4. Server bootstrap ---------------------------------------------
    if (not $cfg.builds_from_source) and (not $skip_server) {
        log info "step: server bootstrap [delegating to install_microk8s.nu]"
        let installer = ($workspace | path join "assets" "nu" "k8s" "prod" "install_microk8s.nu")
        if not ($installer | path exists) {
            error make { msg: $"server bootstrap script not found: ($installer)" }
        }
        ^nu $installer --environment $environment --workspace-dir $workspace
    }

    log info $"done. Open a new shell; it will report itself as ($environment)."
}

# Refuse when the host's own declaration disagrees with what we were invoked as.
#
# The declaration lives in ~/.dapple/env, which is exactly the artifact that
# gets copied between machines — that is how the prod host acquired dev
# configuration in the first place. A declaration that is only ever read is
# worth very little; this is what makes it worth something.
def assert_declaration_matches [home: path, expected: string] {
    let declared = (read_key (user_env_path $home) $ENV_KEY | str trim)
    if ($declared | is-not-empty) and ($declared != $expected) {
        error make {
            msg: $"(user_env_path $home) declares ($ENV_KEY)=($declared) but this run says --environment ($expected).
Either this host is not what you think it is, or an env file was copied from another machine.
Resolve deliberately: edit the file, or re-run with --environment ($declared)."
        }
    }
}

# Install the nu modules and the conf.d fragment that loads them.
#
# The fragment is environment-agnostic on purpose: it loads dapple_env, which
# reads DAPPLE_ENV from ~/.dapple/env. Before this, the fragment WAS the
# environment selector — which is why running the dev script on prod silently
# made prod a dev host.
def install_shell_profile [workspace: path] {
    let modules_src = ($workspace | path join "assets" "nu" "modules")
    if not ($modules_src | path exists) {
        error make { msg: $"nu modules not found at ($modules_src)" }
    }

    # $nu.default-config-dir, NOT a hardcoded ~/.config/nushell. On Linux they
    # are the same directory; on macOS nushell uses
    # ~/Library/Application Support/nushell, so a hardcoded path installs into
    # somewhere nu never reads — modules silently stale, fragment never
    # sourced. deploy_dapple_prompt_pods.nu's freshness check compares against
    # $nu.default-config-dir too, so the two must agree.
    let nu_dir = $nu.default-config-dir
    let modules_dst = ($nu_dir | path join "modules")
    mkdir $modules_dst
    for entry in (ls $modules_src) {
        let name = ($entry.name | path basename)
        rm -rf ($modules_dst | path join $name)
        cp -r $entry.name ($modules_dst | path join $name)
    }
    log info $"  modules installed: ($modules_dst)"

    let conf_d = ($nu_dir | path join "conf.d")
    mkdir $conf_d
    let fragment = '# Dapple environment.
#
# Environment-agnostic by design: dapple_env reads DAPPLE_ENV from
# ~/.dapple/env. Do not reintroduce a per-environment fragment — a host that
# learns its identity from which file was copied onto it is how the production
# box ended up addressing the dev cluster.
use ../modules/dapple_env_profiles dapple_env
overlay use dapple_env

# Dapple CLI module for k8s operations
use ../modules/dapple
'
    $fragment | save --force ($conf_d | path join "dapple.nu")
    log info $"  fragment written: ($conf_d | path join 'dapple.nu')"

    # nushell does not auto-source conf.d; config.nu has to pull it in.
    let config_file = ($nu_dir | path join "config.nu")
    let source_line = 'source ($nu.default-config-dir | path join "conf.d" | path join "dapple.nu")'
    let content = if ($config_file | path exists) { open $config_file --raw } else { "" }
    if not ($content | str contains $source_line) {
        $"($content | str trim --right)\n\n($source_line)\n" | save --force $config_file
        log info "  conf.d source line added to config.nu"
    }
}

def generate_secrets_file [workspace: path] {
    let script = ($workspace | path join "assets" "nu" "modules" "tools" "env_dapple.nu")
    let template = ($workspace | path join "assets" "env.template")
    if not ($script | path exists) {
        log warning $"($script) not found — skipping"
        return
    }
    ^nu $script gen --example-file $template
}

def ensure_sqlx_cli [] {
    let v = (do { ^cargo sqlx --version } | complete | get stdout | str trim)
    if ($v =~ '0\.9\.') {
        log info $"  sqlx-cli present [($v)]"
        return
    }
    log info "  installing sqlx-cli ^0.9"
    # No --locked: broken for sqlx-cli as of 0.9.
    ^cargo install sqlx-cli --version '^0.9' --features native-tls,postgres,sqlite
}

# Point git at the checked-in hooks directory rather than copying a file, so a
# hook change lands with a pull instead of needing this script re-run.
def install_pre_commit_hook [workspace: path] {
    let hook = ($workspace | path join "assets" "git-hooks" "pre-commit")
    if not ($hook | path exists) {
        log warning $"($hook) not found — skipping"
        return
    }
    # Clones carry the committed 100755 bit; ensure it anyway.
    chmod +x $hook

    let current = (do { ^git -C $workspace config --get core.hooksPath } | complete | get stdout | str trim)
    if $current == "assets/git-hooks" {
        log info "  core.hooksPath already set"
        return
    }
    if ($current | is-not-empty) {
        log warning $"  overriding existing core.hooksPath: ($current)"
    }
    ^git -C $workspace config core.hooksPath assets/git-hooks
    log info "  core.hooksPath -> assets/git-hooks"
}
