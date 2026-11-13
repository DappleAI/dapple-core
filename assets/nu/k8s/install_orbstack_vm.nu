#!/usr/bin/nu
#
# install_orbstack_vm.nu — One-shot provisioner for the macOS dev VM.
#
# What it does:
#   1. Verifies OrbStack is installed and reachable.
#   2. Creates the Ubuntu 24.04 LTS VM if missing (default name: `microk8s-vm`).
#   3. Waits for the VM to be reachable.
#   4. Installs microk8s inside the VM via snap (idempotent).
#   5. Adds the VM's default user to the `microk8s` group.
#   6. Waits for `microk8s status --wait-ready`.
#   7. Writes /etc/resolver/kube.dappleai.dev pointing at the VM IP
#      (one-shot — OrbStack's IP is stable across host reboots).
#
# After this script completes, run:
#   nu assets/nu/k8s/setup_microk8s.nu
# to enable addons, deploy registry/dev-dns/cert-manager, etc.
#
# This script is macOS-only. On Linux, microk8s installs natively as a
# snap and there is no VM layer — just run setup_microk8s.nu directly.

$env.NU_LOG_LEVEL = "debug"

use std/log

use ../modules/orb
use ../modules/k8s

def main [] {
    if not (orb is_macos) {
        log info "install_orbstack_vm.nu is macOS-only; on Linux, run setup_microk8s.nu directly"
        return
    }

    # 1. OrbStack present?
    let orb_check = (do { ^which orb } | complete)
    if $orb_check.exit_code != 0 {
        log error "orb CLI not found on PATH"
        log error ""
        log error "Install OrbStack first:"
        log error "  brew install orbstack"
        log error ""
        log error "OrbStack is a paid product; ensure your license is active before"
        log error "running this script. See:"
        log error "  docs/guides/journey-builder-guide/development/orbstack-setup.md"
        error make { msg: "orb CLI missing — install OrbStack and re-run" }
    }

    # 2. OrbStack daemon reachable? `orb status` returns "Running" / "Stopped"
    # for the daemon; `orb version` works regardless and is a quick liveness
    # probe. We use status because its output ("Running\n") is human-readable
    # in the failure path.
    let daemon_check = (do { ^orb status } | complete)
    if $daemon_check.exit_code != 0 {
        log error "OrbStack daemon does not appear to be running"
        log error "Open the OrbStack app (or run `orb start` if you have a stopped daemon) and re-run"
        error make { msg: "OrbStack not running" }
    }

    let name = (orb vm_name)

    # 3. Ensure the VM exists and is running.
    #
    # OrbStack exposes no CLI flag (`orb create`) and no `orb config` key
    # for capping per-VM disk size — only `cpu` and `memory_mib` are
    # configurable. The VM's sparse `.img.raw` therefore grows monotonically;
    # see `_run_reclaim_plan`'s host-side warning and
    # `docs/books/journey-builder-guide/development/orbstack-setup.md`.
    log info $"ensuring OrbStack VM ($name) exists and is running"
    orb ensure_vm

    # 4. Wait for the VM to be reachable for command execution.
    orb wait_until_ready --timeout-seconds 120

    # 5. Ensure snapd is installed. OrbStack ships a minimal Ubuntu cloud
    # image that omits snapd, so we have to install it before we can use
    # `snap`. Idempotent — apt-get install is a no-op if the package is
    # already at the latest version.
    let snap_present = (do { orb vm_run [which snap] } | complete)
    if $snap_present.exit_code != 0 {
        log info "snapd not installed in VM, installing via apt"
        orb vm_run [sudo apt-get update -qq]
        orb vm_run [sudo apt-get install -y snapd]
    } else {
        log info "snapd already present in VM"
    }

    # 6. Install microk8s. Idempotent — `snap install` no-ops on the second
    # run but emits a warning, so we check first to keep the log readable.
    let already_installed = (do { orb vm_run [snap list microk8s] } | complete)
    if $already_installed.exit_code == 0 {
        log info "microk8s already installed inside VM"
    } else {
        log info "installing microk8s inside VM"
        orb vm_run [sudo snap install microk8s --classic]
    }

    # 7. Group membership so non-sudo `microk8s ...` works. OrbStack creates
    # a Linux user matching the macOS username by default; orb::vm_user
    # resolves this (with MICROK8S_VM_USER env override).
    let user = (orb vm_user)
    log info $"adding ($user) to the microk8s group"
    orb vm_run [sudo usermod -a -G microk8s $user]

    # 8. Wait for microk8s to be ready before returning.
    log info "waiting for microk8s to become ready"
    orb vm_run [sudo microk8s status --wait-ready]

    # 9. Write the host resolver file pointing at the VM IP. OrbStack's IP
    # is stable across host reboots/sleep — but it DOES change on a VM
    # delete+recreate, so this step must re-run on every re-provision.
    # setup_host_resolver now verifies the write (readback) and fails LOUD
    # with a paste-ready manual command if it can't write (e.g.
    # non-interactive sudo), instead of silently leaving a stale file.
    let vm_ip = (orb vm_ip)
    log info $"VM IP: ($vm_ip)"
    log info $"writing /etc/resolver/kube.dappleai.dev -> ($vm_ip):30053"
    k8s setup_host_resolver $vm_ip

    # 10. Set up the host kubeconfig so `kubectl` on macOS talks to the
    # microk8s apiserver inside the OrbStack VM. We pull `microk8s config`
    # from inside the VM and merge it into ~/.kube/config, preserving any
    # other contexts the user has (for example, OrbStack's built-in k8s).
    # No server URL rewrite is needed here.
    setup_host_kubeconfig $vm_ip

    # 11. Ensure docker buildx is installed and registered as a CLI plugin.
    # OrbStack's docker CLI doesn't ship buildx by default; the dapple
    # service Dockerfiles use `RUN --mount=type=cache,...` (a BuildKit
    # feature), and `docker build` with DOCKER_BUILDKIT=1 hands off to
    # buildx. Without this step, `deploy_dapple_prompt_pods.nu` fails
    # with "BuildKit is enabled but the buildx component is missing".
    ensure_docker_buildx

    log info ""
    log info $"OrbStack VM ($name) is provisioned with microk8s installed."
    log info "Next steps:"
    log info "  nu assets/nu/k8s/setup_microk8s.nu"
    log info "  nu assets/nu/k8s/deploy_dapple_prompt_pods.nu"
    log info ""
    log info "If *.kube.dappleai.dev stops resolving:"
    log info "  1. orb info microk8s-vm — confirm VM is running"
    log info "  2. cat /etc/resolver/kube.dappleai.dev — confirm IP matches"
    log info "  3. If mismatched (the IP changes on a VM delete+recreate),"
    log info "     re-run this script. setup_host_resolver fails loud with a"
    log info "     paste-ready manual command if it cannot write the file."
}

# Pull `microk8s config` from inside the VM and merge into ~/.kube/config.
#
# microk8s emits the kubeconfig with the apiserver address already set to
# the host's routable IP (verified empirically: it correctly reports the
# OrbStack VM IP rather than 127.0.0.1), so no URL rewriting is needed.
#
# We do NOT use `kubectl config view --merge --flatten` here because its
# conflict-resolution semantics are surprising — it tends to keep the
# wrong (older) cluster entry when both files define the same name.
# Instead we do a structured merge: read both YAMLs as nushell records,
# strip the old `microk8s-cluster` cluster, `microk8s` context, and
# `admin` user entries, then append the fresh ones from the VM.
#
# Preserves any other contexts the user has (OrbStack k3s, cloud
# clusters, etc.). Sets `microk8s` as the current context.
def setup_host_kubeconfig [vm_ip: string] {
    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    let kube_dir = ($home | path join ".kube")
    let main_kc = ($kube_dir | path join "config")
    mkdir $kube_dir

    log info "pulling microk8s kubeconfig from VM"
    let new_kc = (orb vm_run [sudo microk8s config] | into string | from yaml)

    let existing = if ($main_kc | path exists) {
        let backup = ($kube_dir | path join $"config.bak-(date now | format date '%Y%m%d-%H%M%S')")
        cp $main_kc $backup
        log info $"backed up existing kubeconfig to ($backup)"
        # nushell's `open` only auto-parses YAML when the file has a
        # .yaml/.yml extension; ~/.kube/config has no extension, so we
        # parse explicitly.
        open $main_kc --raw | from yaml
    } else {
        {
            apiVersion: "v1"
            kind: "Config"
            preferences: {}
            clusters: []
            contexts: []
            users: []
        }
    }

    # Filter out any existing microk8s entries (whether stale or current)
    # and append fresh ones from the VM.
    let merged = {
        apiVersion: ($existing | get -o apiVersion | default "v1")
        kind: ($existing | get -o kind | default "Config")
        preferences: ($existing | get -o preferences | default {})
        clusters: (
            ($existing | get -o clusters | default [])
            | where name != "microk8s-cluster"
            | append ($new_kc | get clusters)
        )
        contexts: (
            ($existing | get -o contexts | default [])
            | where name != "microk8s"
            | append ($new_kc | get contexts)
        )
        users: (
            ($existing | get -o users | default [])
            | where name != "admin"
            | append ($new_kc | get users)
        )
        "current-context": "microk8s"
    }

    $merged | to yaml | save -f $main_kc
    log info "merged microk8s context into ~/.kube/config"
    log info "current kubectl context: microk8s"
}

# Ensure `docker buildx` is available as a CLI plugin. OrbStack's docker
# integration provides BuildKit-capable builds, but the standalone buildx
# plugin isn't bundled — without it, `docker build` with DOCKER_BUILDKIT=1
# fails with "BuildKit is enabled but the buildx component is missing".
#
# Strategy: probe `docker buildx version`. If it works, no-op. Otherwise
# install via Homebrew and symlink into ~/.docker/cli-plugins/ where the
# docker CLI looks for plugins.
def ensure_docker_buildx [] {
    let probe = (do { ^docker buildx version } | complete)
    if $probe.exit_code == 0 {
        log info "docker buildx already installed"
        return
    }

    log info "docker buildx missing, installing via Homebrew"

    let brew_check = (do { ^which brew } | complete)
    if $brew_check.exit_code != 0 {
        log error "Homebrew not found on PATH"
        log error "Install Homebrew (https://brew.sh) and re-run this script,"
        log error "or install docker-buildx manually and symlink it to"
        log error "~/.docker/cli-plugins/docker-buildx"
        error make { msg: "buildx install requires Homebrew" }
    }

    ^brew install docker-buildx

    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    let plugin_dir = ($home | path join ".docker" | path join "cli-plugins")
    mkdir $plugin_dir

    let brew_prefix = (^brew --prefix | str trim)
    let buildx_bin = ($brew_prefix | path join "bin" | path join "docker-buildx")
    let plugin_link = ($plugin_dir | path join "docker-buildx")

    log info $"linking ($buildx_bin) -> ($plugin_link)"
    ^ln -sfn $buildx_bin $plugin_link

    # Verify the install took effect.
    let verify = (do { ^docker buildx version } | complete)
    if $verify.exit_code != 0 {
        log error "buildx install completed but `docker buildx version` still fails"
        log error $"stderr: ($verify.stderr)"
        error make { msg: "buildx install verification failed" }
    }
    log info $"docker buildx installed: ($verify.stdout | str trim)"
}
