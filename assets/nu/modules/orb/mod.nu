# OrbStack VM abstraction (macOS-only)
#
# This module is a thin wrapper around the `orb` CLI for managing the
# OrbStack-hosted Ubuntu VM that runs MicroK8s on macOS. It is the SOLE
# integration point with the OrbStack runtime — all other tooling should
# call functions exported here instead of `^orb` directly.
#
# On Linux this module's functions are intentionally non-functional. The
# microk8s cluster runs natively as a snap on Linux, so a "VM" abstraction
# does not apply. Every public function gates itself on `is_macos` and
# either returns a sensible default (`vm_state` -> "missing", `vm_exists`
# -> false) or errors with a clear message ("orb is macOS-only").
#
# Why this module exists at all (rather than scattering `^orb` calls):
# - confines the OrbStack dependency to one file (rollback story)
# - lets the install script and `setup_microk8s.nu` share a single VM-name
#   convention (env override + default)
# - documents the exact `orb` subcommand spellings in one place — useful
#   when the OrbStack CLI inevitably evolves
#
# Usage:
#   use ($nu.default-config-dir | path join 'modules' | path join 'orb')
#   orb ensure_vm
#   let ip = (orb vm_ip)
#   orb vm_run [sudo snap install microk8s --classic]

use std/log

# Parse-time-safe macOS detection. Mirrors the helper in k8s/mod.nu.
export def is_macos []: nothing -> bool {
    $nu.os-info.name | str contains "mac"
}

# Resolve the VM name. Allows env override so multiple developers can
# share the same machine without colliding (or for ephemeral test VMs).
export def vm_name []: nothing -> string {
    $env | get -o MICROK8S_VM_NAME | default "microk8s-vm"
}

# Default Ubuntu image. Pinned to noble (24.04 LTS) for stability — the
# microk8s snap is host-distro-agnostic, but pinning the host LTS gives
# us 5 years of free security updates and avoids surprise behavior shifts
# from Ubuntu development releases (questing/oracular/etc.).
#
# Override via MICROK8S_VM_IMAGE for testing alternative distros.
export def vm_image []: nothing -> string {
    $env | get -o MICROK8S_VM_IMAGE | default "ubuntu:noble"
}

# Default Linux user inside the OrbStack VM. OrbStack creates a user that
# mirrors the macOS username by default (verified empirically against
# OrbStack 2.1.1: `orb info` returns `record.config.default_username`
# matching `$env.USER`). Override via MICROK8S_VM_USER for non-default
# setups (e.g., a shared VM with a service account).
export def vm_user []: nothing -> string {
    let mac_user = ($env | get -o USER | default "ubuntu")
    $env | get -o MICROK8S_VM_USER | default $mac_user
}

# Returns true when the VM exists (running OR stopped). False if the VM
# has never been created or has been deleted.
export def vm_exists []: nothing -> bool {
    if not (is_macos) { return false }

    let name = (vm_name)
    let result = (do { ^orb info $name --format json } | complete)
    $result.exit_code == 0
}

# Returns "running" | "stopped" | "missing".
#
# JSON shape verified against OrbStack 2.1.1: `orb info <name> --format
# json` returns `{ "record": { "state": "running" | "stopped", ... },
# "ip4": "...", ... }`. The state field lives under `record.state`.
export def vm_state []: nothing -> string {
    if not (is_macos) { return "missing" }

    let name = (vm_name)
    let result = (do { ^orb info $name --format json } | complete)
    if $result.exit_code != 0 {
        return "missing"
    }

    let info = (try { $result.stdout | from json } catch { null })
    if ($info | is-empty) {
        return "unknown"
    }

    ($info | get -o record | default {} | get -o state | default "" | into string | str lowercase)
}

# Returns the VM's IPv4 address as visible from the macOS host. Errors
# if the VM is missing or stopped — callers who need a "best effort"
# value should wrap in `try`.
#
# JSON path verified against OrbStack 2.1.1: `ip4` is at the document
# root (sibling of `record`), not nested under it.
#
# Note: no return-type annotation because `error make` is not a string.
# Successful return is always a string; errors propagate via `error make`.
export def vm_ip [] {
    if not (is_macos) {
        error make { msg: "orb vm_ip is macOS-only" }
    }

    let name = (vm_name)
    let result = (do { ^orb info $name --format json } | complete)
    if $result.exit_code != 0 {
        error make { msg: $"orb info ($name) failed: ($result.stderr)" }
    }

    let info = (try { $result.stdout | from json } catch {
        error make { msg: $"could not parse JSON from `orb info ($name)`" }
    })

    let ip = ($info | get -o ip4 | default "")
    if ($ip | is-empty) {
        error make { msg: $"orb info ($name) returned no ip4 field; the VM may be stopped" }
    }

    $ip | into string | str trim
}

# Run a command inside the VM. The command is passed as a LIST to bypass
# nushell's variadic flag parsing — passing `[sudo snap install microk8s
# --classic]` works, whereas `sudo snap install microk8s --classic` (as
# spread args) would fail because nushell would try to treat `--classic`
# as a flag of `vm_run` itself.
#
# OrbStack's `orb run` does not use a `--` separator; arguments after
# `-m <name>` are forwarded directly as the command and its args.
#
# Examples:
#   orb vm_run [uname -a]
#   orb vm_run [sudo snap install microk8s --classic]
#   orb vm_run [microk8s status --wait-ready]
export def vm_run [cmd: list<string>]: nothing -> any {
    if not (is_macos) {
        error make { msg: "orb vm_run is macOS-only" }
    }
    if ($cmd | is-empty) {
        error make { msg: "orb vm_run requires a non-empty command list" }
    }

    let name = (vm_name)
    ^orb run -m $name ...$cmd
}

# Convenience wrapper that runs a shell command line inside the VM
# through `bash -c`. Use this when the command uses shell features
# (pipes, redirection, glob expansion) or when string-building is
# clearly more readable than list-building.
#
# WARNING: arguments are subject to shell quoting. For untrusted input
# or values containing whitespace / special chars, prefer `vm_run` with
# an explicit list.
#
# Examples:
#   orb vm_sh "snap list microk8s | grep -q microk8s"
#   orb vm_sh "lsb_release -d"
export def vm_sh [cmd: string]: nothing -> any {
    vm_run [bash -c $cmd]
}

# Allocate a host-side tempfile path under $HOME so it appears inside the
# OrbStack VM at the same path via auto-mount. Use this instead of system
# `mktemp` (which lives under /var/folders/... on macOS and is NOT
# auto-mounted) when you need to stage content for `vm_push`.
#
# Returns the path; caller is responsible for `save -f` and cleanup.
export def mktemp_in_home [--prefix: string = "dapple-staging"]: nothing -> string {
    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    let dir = ($home | path join ".cache" | path join "dapple-staging")
    mkdir $dir
    let name = $"($prefix)-(random uuid).tmp"
    ($dir | path join $name)
}

# Copy a host file into the VM at an arbitrary destination path, using
# sudo so writes to /etc/, /var/snap/, etc. work.
#
# Implementation note: we exploit OrbStack's auto-mount of /Users/$USER
# rather than the `orb push` CLI. Files under $HOME are visible inside
# the VM at the same path, so a plain `orb run sudo cp` works.
#
# CONSTRAINT: `src` MUST be under /Users/$USER. macOS `mktemp` writes
# to /var/folders/... which is NOT auto-mounted; use `orb mktemp_in_home`
# for ephemeral staging. We do not use the `-p` (path translation) flag
# because it translates BOTH source and destination, breaking writes to
# real Linux paths like /etc/dapple-resolv.conf.
export def vm_push [src: path, dst_in_vm: string]: nothing -> nothing {
    if not (is_macos) {
        error make { msg: "orb vm_push is macOS-only" }
    }

    let abs_src = ($src | path expand)
    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    if not ($abs_src | str starts-with $home) {
        error make {
            msg: $"vm_push requires src under ($home) for OrbStack auto-mount; got ($abs_src). Use `orb mktemp_in_home` for staging."
        }
    }

    let name = (vm_name)
    ^orb run -m $name sudo cp $abs_src $dst_in_vm
}

# Create a new Ubuntu VM. Defaults to `ubuntu:noble` (24.04 LTS); pass
# --image for a specific tag.
#
# `orb create` is positional: `orb create <distro>[:<version>] <name>`.
# Verified against OrbStack 2.1.1. Note: OrbStack exposes no CLI flag for
# capping per-VM disk size (verified May 2026: `orb create --help` lists
# only --arch / --forward-ssh-agent / --isolate-network / --isolated /
# --mount / --set-password / --user / --user-data, and `orb config show`
# exposes only `cpu` and `memory_mib` as resource limits — no disk key).
export def vm_create [
    --image: string = "ubuntu:noble"
]: nothing -> nothing {
    if not (is_macos) {
        error make { msg: "orb vm_create is macOS-only" }
    }

    let name = (vm_name)
    log info $"creating OrbStack VM ($name) from image ($image)"
    ^orb create $image $name
}

# Delete the VM. Idempotent — does nothing if the VM doesn't exist.
export def vm_delete []: nothing -> nothing {
    if not (is_macos) {
        log info "orb vm_delete is macOS-only, skipping on Linux"
        return
    }

    if not (vm_exists) {
        log info $"VM (vm_name) does not exist, nothing to delete"
        return
    }

    let name = (vm_name)
    log info $"deleting OrbStack VM ($name)"
    ^orb delete --force $name
}

# Ensure the VM exists and is running. Idempotent. Creates the VM if
# missing, starts it if stopped, no-ops if already running.
#
# This function does NOT install microk8s — the caller (typically
# `install_orbstack_vm.nu`) handles that after `ensure_vm` returns,
# because microk8s installation is a separate concern (and the install
# script needs to wait for boot before invoking snap).
export def ensure_vm []: nothing -> nothing {
    if not (is_macos) {
        log info "orb ensure_vm is macOS-only, skipping on Linux"
        return
    }

    let name = (vm_name)
    let state = (vm_state)

    match $state {
        "running" => {
            log info $"VM ($name) is already running"
        }
        "stopped" => {
            log info $"VM ($name) is stopped, starting"
            ^orb start $name
        }
        "missing" => {
            log info $"VM ($name) does not exist, creating"
            vm_create
            log info $"VM ($name) created, starting"
            ^orb start $name
        }
        _ => {
            log info $"VM ($name) is in state ($state), attempting start"
            # Best-effort start covers transient states.
            do { ^orb start $name } | complete | ignore
        }
    }
}

# Wait until `orb run -- true` succeeds against the VM, indicating
# the guest OS is reachable for command execution. Used by the install
# script after `ensure_vm` to gate the microk8s snap install on actual
# boot completion (vs. just "the VM record exists").
export def wait_until_ready [--timeout-seconds: int = 60]: nothing -> nothing {
    if not (is_macos) {
        log info "orb wait_until_ready is macOS-only, skipping on Linux"
        return
    }

    let name = (vm_name)
    let deadline = ((date now) + ($timeout_seconds * 1sec))

    log info $"waiting up to ($timeout_seconds)s for VM ($name) to be reachable"
    loop {
        let probe = (do { ^orb run -m $name true } | complete)
        if $probe.exit_code == 0 {
            log info $"VM ($name) is reachable"
            return
        }
        if (date now) >= $deadline {
            error make { msg: $"VM ($name) did not become reachable within ($timeout_seconds)s" }
        }
        sleep 2sec
    }
}
