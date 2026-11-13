# AGENTS.md — orb Module

A thin Nushell wrapper around the [OrbStack](https://orbstack.dev) CLI. macOS-only — every public function gates on `is_macos` and either no-ops or errors with a clear message on Linux.

## Why This Module Exists

OrbStack hosts the Ubuntu VM that runs MicroK8s for macOS contributors. This module is the **single integration point** with the `orb` CLI; deploy scripts and `setup_microk8s.nu` should never invoke `^orb` directly. Reasons:

- Confines the OrbStack dependency to one file (clean rollback story to any future VM runtime)
- Pins the VM-name convention (`MICROK8S_VM_NAME` env override, default `microk8s-vm`) in one place
- Documents the exact `orb` subcommand spellings — useful when the OrbStack CLI evolves

## Public API

| Function | Purpose |
|----------|---------|
| `is_macos` | Parse-time-safe macOS detection (mirrors `k8s::_is_macos`) |
| `vm_name` | Resolve VM name (env override + default) |
| `vm_user` | Default Linux user inside the VM (env override + default) |
| `vm_exists` | Returns `true` if the VM exists in any state |
| `vm_state` | Returns `"running" \| "stopped" \| "missing" \| "unknown"` (lowercased) |
| `vm_ip` | Returns the VM IP visible from the macOS host (errors if missing/stopped) |
| `vm_run ...args` | Run command inside VM |
| `vm_push src dst` | Push file from host into VM |
| `vm_create --image` | Create a new Ubuntu VM |
| `vm_delete` | Delete the VM (idempotent) |
| `ensure_vm` | Create + start if missing/stopped (idempotent) |
| `wait_until_ready --timeout-seconds` | Block until `orb run -- true` succeeds |

## When to Use Each Function

- **Initial provisioning** (`install_orbstack_vm.nu`): `ensure_vm` -> `wait_until_ready` -> `vm_run sudo snap install microk8s --classic`
- **Reading VM IP** (`setup_microk8s.nu`, downstream callers): `vm_ip` — always inside `if (orb is_macos)`
- **Pushing config files** (e.g., `/etc/dapple-resolv.conf`): `vm_push` — but prefer auto-mounted paths under `/Users/$USER`
- **Executing commands inside the VM**: `vm_run` — for snap, systemctl, etc.

## Anti-Patterns

- **Never call `^orb` directly from deploy scripts.** Add a wrapper here instead.
- **Never gate on the previous VM runtime.** The legacy gate is being removed wholesale; do not introduce a runtime-detection branch.
- **Never assume the VM name is `microk8s-vm` literally.** Always go through `vm_name` so the env override works.
- **Never call `vm_ip` without `is_macos` gate.** Linux callers should not invoke the function at all.
- **Do not introduce a generic "VM runtime" abstraction here.** This module is specifically for OrbStack; rollback is a code revert, not a polymorphic dispatch.

## CLI Behaviors Verified Against OrbStack 2.1.1

The module's empirical assumptions are pinned against this version. If a future OrbStack upgrade breaks any of these, this section is the place to update first:

1. **Default Linux user** — OrbStack creates a user named after the macOS username (`$env.USER`). `vm_user` returns that with `MICROK8S_VM_USER` as override.
2. **`orb info --format json` shape** — `record.state` for state, `ip4` (top-level) for IPv4. `vm_state` and `vm_ip` extract these directly with no fallback paths.
3. **`orb create` flag form** — positional: `orb create <distro>[:<version>] <name>`. `vm_create` defaults to `ubuntu:noble` (24.04 LTS).
4. **`orb push` semantics** — `orb push` writes to a destination relative to the Linux user's home directory and is therefore unsuitable for our `/etc/...` writes. `vm_push` instead exploits OrbStack's auto-mount: it requires src under `/Users/$USER` and copies via `orb run sudo cp`. Use `mktemp_in_home` for ephemeral staging because system `mktemp` returns paths under `/var/folders/...` which is NOT auto-mounted.
5. **`orb run` argument forwarding** — no `--` separator is used; arguments after `-m <name>` are forwarded directly.
6. **`-p` (path translation) flag** — translates BOTH source and destination from macOS paths to `/mnt/mac/...` Linux paths. Avoid for `cp` operations where dst is a real Linux path; use auto-mount staging instead.

## Cross-Module Conventions

- macOS detection: `is_macos` (this module) and `_is_macos` (k8s/mod.nu) must remain in sync. Both check `$nu.os-info.name | str contains "mac"`.
- VM name resolution: `vm_name` here is the canonical version. Other modules should import or duplicate the env-override pattern (`$env | get -o MICROK8S_VM_NAME | default "microk8s-vm"`) consistently.
