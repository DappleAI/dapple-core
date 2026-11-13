# OrbStack Migration Architecture Plan

**Status:** Draft
**Created:** 2026-05-07
**Last Updated:** 2026-05-07
**Target Version:** infra-only (no crate version impact)
**Dependencies:** none

## Executive Summary

**What.** Replace Multipass with [OrbStack](https://orbstack.dev) as the macOS host for the local-development microk8s VM. OrbStack provides a stable Apple-Virtualization.framework-backed Ubuntu VM with auto-mounted host filesystem, stable hostnames, and stable IPs across host reboots/sleeps. Multipass is removed wholesale — there is no coexistence path.

**Why.** Multipass on macOS uses qemu + qcow2 images on APFS. After repeated host sleeps and force-kills (which happen routinely during development), the qcow2 develops L2/refcount corruption. We have hit unbootable-VM scenarios four times. `qemu-img check -r all` can repair refcounts but not the underlying boot path damage. The compounding overhead — DHCP-driven VM IP drift, zombie SSHFS mounts surviving in `multipass info`, and a hand-maintained `/etc/resolver/kube.dappleai.dev` file — produces several hundred lines of recovery code (`dapple dns repair`, `dapple mount repair`, `apply_kubelet_resolv_conf_patch`) that exist solely to paper over Multipass's failure modes. OrbStack eliminates the root causes.

**How.** All Multipass call sites in our Nushell tooling are gated behind `if ($nu.os-info.name | str contains "mac")`. We swap those gated branches to call an `orb` helper (or remove them entirely where OrbStack makes them no-ops). Linux paths remain identical — microk8s installs natively as a snap, and none of the macOS-only branches ever ran on Linux. The microk8s topology inside the VM (addons, registry, dev-dns, prompt-db, etc.) is unchanged; only the host that runs the VM changes.

**Who.** Brian (and any future macOS contributor). Linux contributors are unaffected. CI is unaffected (it runs on GitHub Actions Linux runners that don't touch Multipass).

## Context and Background

### Current macOS Topology

```
macOS host
  └── Multipass (qemu/qcow2) ─── microk8s-vm (Ubuntu 22.04)
        ├── DHCP-assigned IP (drifts on reboot/sleep)
        ├── multipass mount (SSHFS) ── /Users/brian/Source/Dapple/dapple-prompt
        ├── multipass mount (SSHFS) ── /Users/brian/.cache/dapple-onnx
        └── microk8s
              ├── registry, ingress, cert-manager, hostpath-storage, dns, dashboard
              ├── prompt-db, redis, dapple-* services
              └── dev-dns NodePort 30053 ── /etc/resolver/kube.dappleai.dev → VM IP
```

### Pain Points Multipass Imposes

1. **qcow2 corruption from sleep/force-kill** — primary motivation for migration
2. **DHCP IP drift** — VM IP changes on reboot/sleep, breaking `*.kube.dappleai.dev` until `dapple dns repair` runs
3. **Zombie SSHFS mounts** — `multipass info` reports mount as healthy; daemon is dead; pods stick in `ContainerCreating` until `dapple mount repair`
4. **kubelet resolv.conf patching via `multipass exec`** — fragile sequence of `transfer`, `exec sudo cat`, `exec sudo mv`, `exec systemctl restart`
5. **No native bidirectional mount** — every host path must be explicitly registered with `multipass mount`

### Project Patterns That Apply

- All Nushell shell commands run via `nu -c` per project CLAUDE.md
- macOS-specific code is gated by `if ($nu.os-info.name | str contains "mac")` — never assumes platform
- Modules are imported via `$nu.default-config-dir` parse-time constants (per `assets/nu/AGENTS.md` cross-platform portability rules)
- The deploy scripts default to **hostPath mode** (mount source into pod) — auto-promoted to Docker mode on macOS today because Mach-O binaries can't run in the Linux VM. **This auto-promotion remains correct under OrbStack**: the host-arch / VM-arch impedance mismatch is independent of which VM runtime we use.

### OrbStack Semantics (Authoritative for This Plan)

- **macOS-only.** Uses Apple's Virtualization.framework (no qemu, no qcow2). Robust against sleep/force-kill — APFS-native snapshots and raw images.
- **Stable hostnames.** Each Linux machine is reachable as `<machine-name>@orb.local` from the macOS host without resolver-file maintenance. The VM IP is also stable across host reboots (OrbStack's internal DHCP is deterministic per-machine-name).
- **Auto-mount.** `/Users/$USER` is two-way virtiofs-mounted into every Linux VM at the same path. So `/Users/brian/Source/Dapple/dapple-prompt` is identical on host and inside the VM with **no `multipass mount` equivalent needed**.
- **CLI surface (verify exact subcommand spellings during Phase 1):**
  - `orb create ubuntu microk8s-vm` — create a new Ubuntu machine (image name and exact flags TBD in Phase 1)
  - `orb start microk8s-vm` / `orb stop microk8s-vm` / `orb delete microk8s-vm`
  - `orb info microk8s-vm` — JSON output for IP/state (verify `--format json` flag in Phase 1)
  - `orb run -m microk8s-vm -- <cmd>` — execute inside VM (analog of `multipass exec`)
  - `orb push <local> <vm>:<path>` / `orb pull` — file transfer (largely unneeded due to auto-mount)
  - `ssh microk8s-vm@orb.local` — direct SSH
- **Not using OrbStack's bundled k3s.** We continue to run microk8s inside a vanilla Ubuntu VM, mirroring the Multipass topology exactly. This keeps the in-cluster surface area (registry, addons, manifests) unchanged.
- **Paid product.** This plan assumes every macOS contributor is licensed; the onboarding doc must surface the licensing requirement.

## Database Schema Changes (DDL)

**None.** This is a host-OS-VM-runtime swap, not a data-layer change.

## Object Model Design

**None.** No Rust models or DTOs are affected.

## API Contract

**None.** No HTTP endpoints change.

## SQL Query Strategy

**None.** No SQL changes.

## Implementation Guidance

### Phased Migration

The phases are sequenced so that each phase's "done" state is independently verifiable, and so a partial migration leaves the macOS dev environment in a usable (or known-broken-with-clear-error) state.

---

### Phase 1 — Introduce `orb` Abstraction Layer

**Goal:** add a thin Nushell module that abstracts `orb` CLI calls into named functions, plus an onboarding doc, **without changing any existing call sites yet**. This phase is purely additive.

**Files to create:**

| File | Purpose |
|------|---------|
| `assets/nu/modules/orb/mod.nu` | New module exporting `vm_exists`, `vm_state`, `vm_ip`, `vm_run`, `ensure_vm`, `vm_create`, `vm_delete`, `is_macos`. Pure abstraction over the `orb` CLI — no behavioral logic. |
| `assets/nu/modules/orb/AGENTS.md` | Agent guidance for the new module: which functions exist, when each is called, naming conventions, anti-patterns (e.g. "never call `^orb` directly from deploy scripts — go through this module"). |
| `docs/books/journey-builder-guide/development/orbstack-setup.md` | Onboarding doc: install OrbStack, license note, `orb create` walkthrough, microk8s install inside the VM, common troubleshooting. Linked from `local-development.md`. |
| `assets/nu/k8s/install_orbstack_vm.nu` | One-shot script that runs `orb create ubuntu microk8s-vm` (verify exact image flag), waits for boot, then `orb run -m microk8s-vm -- sudo snap install microk8s --classic`. Idempotent — no-ops if VM already exists. Caller invokes this **before** `setup_microk8s.nu`. |

**Module surface (target signatures):**

```nushell
# assets/nu/modules/orb/mod.nu
export def is_macos []  # parity with k8s::_is_macos
export def vm_name []   # ($env | get -o MICROK8S_VM_NAME | default "microk8s-vm")
export def vm_exists []          # → bool
export def vm_state []           # → "running" | "stopped" | "missing"
export def vm_ip []              # → string (errors if not running)
export def vm_run [...args]      # wrapper for `orb run -m <vm> -- ...`
export def vm_push [src: path, dst: string]  # `orb push` wrapper
export def ensure_vm []          # creates + starts if missing
export def vm_create [--image: string = "ubuntu"]  # `orb create`
export def vm_delete []          # `orb delete --force`
```

**Setup script flow (`install_orbstack_vm.nu`):**

1. Verify `orb` is on PATH; if not, print `brew install orbstack` instruction and bail.
2. Verify OrbStack is running (`orb status` or equivalent — pin exact command in Phase 1).
3. If `vm_exists` returns false: `orb create ubuntu microk8s-vm` (use whatever the actual image flag is; verify against `orb create --help` in Phase 1).
4. Wait for VM to be reachable (`orb run -m microk8s-vm -- true` succeeds within 60s).
5. Inside VM: `sudo snap install microk8s --classic` (idempotent; no-op if already installed).
6. Inside VM: `sudo usermod -a -G microk8s ubuntu` (so non-sudo `microk8s` calls work — exact user TBD; OrbStack's default Linux user may differ).
7. Inside VM: `sudo microk8s status --wait-ready`.

**Acceptance criteria:**
- `nu assets/nu/k8s/install_orbstack_vm.nu` succeeds on a fresh macOS host with OrbStack installed.
- `orb run -m microk8s-vm -- microk8s kubectl get nodes` returns at least one Ready node.
- The new `orb` module imports cleanly via `use ($nu.default-config-dir | path join 'modules' | path join 'orb')`.
- No existing call sites have been modified yet (verify with `git diff --stat` showing only new files).

**Risks/notes:**
- OrbStack's default Linux machine username may not be `ubuntu`. Phase 1 should pin this empirically and bake the value into the install script.
- The exact `orb create` image string (`ubuntu`, `ubuntu:22.04`, `ubuntu:noble`, etc.) must be verified against `orb create --help` and the Ubuntu LTS we want to standardize on.

---

### Phase 2 — Replace Multipass Call Sites

**Goal:** replace every `multipass …` invocation in the codebase with the `orb` module, **OR** replace the `is_macos`-gated branch with a direct path access where OrbStack's auto-mount makes the operation a no-op.

**File-by-file change list:**

| File | Action | Detail |
|------|--------|--------|
| `assets/nu/modules/k8s/mod.nu` lines 55–83 (`ensure_vm_mounts`) | **Rewrite to no-op on macOS** | OrbStack auto-mounts `/Users/$USER` two-way. The function becomes a logging stub on both Linux and macOS (keep the signature and exported name — call sites in deploy scripts continue to call it; remove the function body except for an empty-paths warning). |
| `assets/nu/modules/k8s/mod.nu` lines 309–327 (`setup_host_resolver`) | **Delete** (or shrink to log-and-no-op) | OrbStack hostnames resolve via `*.orb.local` natively; we stop maintaining `/etc/resolver/kube.dappleai.dev` for VM-IP resolution. **However:** the in-cluster `*.kube.dappleai.dev` ingress hosts still need to resolve from the macOS host. See "DNS strategy under OrbStack" below — we either (a) keep a static resolver pointing to OrbStack's stable VM IP, or (b) switch ingress hosts to `*.orb.local` patterns. The Phase 2 implementer should pick one (preferred: option (a) with a one-shot resolver write at `install_orbstack_vm.nu` time, since the IP no longer drifts). |
| `assets/nu/modules/k8s/mod.nu` lines 338–398 (`configure_macos_masqed_dns`) | **Rewrite using `orb vm_run` / `orb vm_push`** | Replace `^multipass transfer` with `orb push`; replace `^multipass exec $vm_name --` with `orb run -m $vm_name --`. The semantic flow (push resolv.conf, edit kubelet args, restart kubelite) is unchanged; only the transport changes. The empty-content guard at line 357–360 must be preserved. |
| `assets/nu/k8s/setup_microk8s.nu` lines 63–69 | **Replace `multipass info` with `orb vm_ip`** | One-line swap: `let vm_ip = (orb vm_ip)`. The surrounding macOS-only gate stays (Linux has no concept of a "VM IP" for microk8s). |
| `assets/nu/k8s/deploy_dapple_prompt_pods.nu` line 16 (`use k8s ensure_vm_mounts`) | **Keep import** | The function still exists; it's just become a no-op. Removing the import would diverge unnecessarily. |
| `assets/nu/k8s/deploy_dapple_prompt_pods.nu` lines 126, 170, 203 (`ensure_vm_mounts` calls) | **Keep call sites** | Same reasoning — calls become no-ops, no behavior change needed at call site. |
| `assets/nu/k8s/deploy_dapple_web_pods.nu` lines 13, 72–75 | **Keep import and call** | Same as above. |

**DNS strategy under OrbStack (decision required in Phase 2):**

The current macOS DNS setup has two layers:

1. **`/etc/resolver/kube.dappleai.dev` → VM IP:30053** (host-side) — directs `*.kube.dappleai.dev` lookups to the dev-dns NodePort
2. **`dev-dns` CoreDNS pod** (in-cluster) — resolves `*.kube.dappleai.dev` to `status.hostIP` (baked at pod start)

Under OrbStack the VM IP is stable, so option (a) — keep both layers but write the resolver file **once** at VM creation and never touch it again — is the minimum-change path. The resolver file is written by `install_orbstack_vm.nu` after `orb vm_ip` returns; subsequent VM lifecycle events (stop/start/restart) don't change the IP, so the file stays valid.

The dev-dns pod's `status.hostIP` baking continues to work unchanged inside the VM.

**Phase 2 explicit decision:** keep option (a). Do not introduce `*.orb.local` ingress hosts — that would ripple through every k8s manifest and is out of scope for a host-runtime swap.

**Acceptance criteria:**
- `git grep -nE 'multipass|^Multipass' -- 'assets/nu/**/*.nu'` returns zero matches in non-`AGENTS.md` files.
- `nu assets/nu/k8s/setup_microk8s.nu` runs cleanly end-to-end on a fresh OrbStack VM.
- `nu assets/nu/k8s/deploy_dapple_prompt_pods.nu --svc api` succeeds; `kubectl get pods -n dapple-sandbox` shows api as Running.
- `curl -k https://dapple-prompt-api.kube.dappleai.dev/api/health_check` returns 200 from the macOS host.
- hostPath mode is auto-promoted to Docker mode on macOS as before (the auto-promotion logic in `deploy_dapple_prompt_pods.nu` lines 36–42 is independent of VM runtime).

---

### Phase 3 — Delete Now-Obsolete DNS-Drift / Mount-Repair Code

**Goal:** remove the recovery code that exists solely to paper over Multipass failure modes that don't apply to OrbStack.

**Files to delete or reduce:**

| File | Action | Detail |
|------|--------|--------|
| `assets/nu/modules/dapple/dns.nu` | **Delete the entire file** | The DNS-drift problem doesn't exist under OrbStack (stable VM IP). The `_kubectl_reachable` / kubeconfig-merge logic (lines 40–262) was specifically chasing Multipass-induced apiserver-IP drift. |
| `assets/nu/modules/dapple/mount.nu` | **Delete the entire file** | The zombie-SSHFS-mount problem doesn't exist under OrbStack (virtiofs auto-mount, no SSHFS). |
| `assets/nu/modules/dapple/mod.nu` lines 39–40 (`export module ./dns.nu` / `./mount.nu`) | **Remove exports** | Delete both lines. |
| `assets/nu/modules/dapple/mod.nu` lines 254–260 (help text for `dapple dns` / `dapple mount`) | **Remove from help** | Delete the two help-text sections. |
| `assets/nu/modules/k8s/mod.nu` line 309–327 (`setup_host_resolver`) | **Keep, but simplify** | Still called once during initial setup (`install_orbstack_vm.nu` and `setup_microk8s.nu`). The function body is correct as-is; only the *callers* shrink (no more "repair" path). |

**Acceptance criteria:**
- `git grep -nE 'dns repair|mount repair|dapple dns|dapple mount' -- 'assets/'` returns zero matches outside the deleted files.
- `dapple --help` (or `dapple` invoked bare) no longer mentions DNS or Mount management commands.
- The `dapple` module loads without error after deletion (`use ($nu.default-config-dir | path join 'modules' | path join 'dapple')`).

**Why delete rather than no-op:** these subcommands existed to recover from specific Multipass failure modes. Leaving them as no-op stubs would mislead future contributors into believing they have a recovery path for OrbStack-specific issues (which would have entirely different recovery procedures).

---

### Phase 4 — Update AGENTS.md / CLAUDE.md / Guide Docs

**Goal:** rewrite all human and agent-facing documentation that references Multipass.

**File-by-file change list:**

| File | Action |
|------|--------|
| `assets/nu/AGENTS.md` | **Major rewrite of "macOS MicroK8s Behavior" section (lines 339–431)** — see template below. |
| `assets/nu/modules/dapple/README.md` | Replace lines 349, 358, 361 (`multipass mount` / `multipass info` examples) with corresponding `orb` commands or note that auto-mount makes them unnecessary. |
| `docs/guides/journey-builder-guide/development/local-development.md` | Replace lines 22–29 (macOS section) with OrbStack-specific guidance. Drop "Host paths must be mounted into the VM" — auto-mounted. |
| `docs/guides/journey-builder-guide/kubernetes/deployment-scripts.md` | Replace lines 169–201 (Platform Behavior > macOS) with OrbStack version. |
| `docs/guides/journey-builder-guide/development/orbstack-setup.md` | New file from Phase 1. Cross-link from `local-development.md`. |
| `CLAUDE.md` (project root) | Add a one-line note in **Infrastructure Overview > K8s Setup** pointing macOS contributors to the OrbStack setup doc. No mention of Multipass. |
| **User's global `~/.claude/CLAUDE.md`** | **In scope.** Update the existing Multipass note to describe OrbStack: stable hostnames, Apple-Virtualization.framework backend, auto-mounted host filesystem, no DHCP/DNS-drift recovery commands. Preserve the surrounding semantics (e.g., the hot-reload note about `dapple-ui` source changes propagating into the pod via auto-mount remains correct — only the VM runtime changes). |

**Replacement template for `assets/nu/AGENTS.md` "macOS MicroK8s Behavior":**

```markdown
## macOS MicroK8s Behavior

On macOS, MicroK8s runs inside an OrbStack-managed Ubuntu VM. OrbStack uses
Apple's Virtualization.framework directly (no qemu/qcow2) and provides:

- A stable VM IP across host reboots and sleep/wake
- A stable hostname (`microk8s-vm@orb.local`) without `/etc/resolver` maintenance
- Two-way virtiofs auto-mount of `/Users/$USER` into every Linux VM at the same path

These properties eliminate three classes of failure that Multipass exposed
(qcow2 corruption, DHCP IP drift, zombie SSHFS mounts) and the recovery code
they required (`dapple dns repair`, `dapple mount repair`).

### Host Path Access

Because OrbStack auto-mounts the user home directory two-way, hostPath volumes
referencing any path under `/Users/$USER` work transparently — no per-mount
registration step is needed. `ensure_vm_mounts` is retained as a no-op for
call-site compatibility but performs no work.

### Initial Setup

```bash
brew install orbstack
nu assets/nu/k8s/install_orbstack_vm.nu     # create + boot microk8s-vm, install microk8s snap
nu assets/nu/k8s/setup_microk8s.nu          # configure addons, registry, certs, dev-dns
```

OrbStack is a paid product; ensure your license is active before running
`install_orbstack_vm.nu`.

### Docker Certificate Paths

(unchanged from current — keep the existing table verbatim)

### ONNX Model Provisioning

(unchanged from current — auto-download still happens; the only difference is
`DAPPLE_ONNX_CACHE_DIR` is now visible inside the VM via auto-mount rather
than via `multipass mount`)

### DNS Resolution for `*.kube.dappleai.dev`

`/etc/resolver/kube.dappleai.dev` is written **once** during
`install_orbstack_vm.nu`, pointing at OrbStack's stable VM IP. The dev-dns
CoreDNS pod inside the cluster bakes `status.hostIP` at pod start. Both
remain valid across VM lifecycle events because the IP no longer drifts.

If `*.kube.dappleai.dev` ever stops resolving from the host:

1. `orb info microk8s-vm` — confirm VM is running and note its IP
2. `cat /etc/resolver/kube.dappleai.dev` — confirm IP matches
3. If mismatched: rerun `nu assets/nu/k8s/install_orbstack_vm.nu` (idempotent)

There is no `dapple dns repair` command — that existed only for Multipass IP
drift, which OrbStack does not have.
```

**Acceptance criteria:**
- `git grep -inE 'multipass|sshfs' -- '*.md'` returns zero matches outside `docs/plans/orbstack-migration.md` (this file).
- All AGENTS.md files render correctly (no broken section anchors or stale internal links).
- `local-development.md` and `deployment-scripts.md` reflect the OrbStack workflow.

---

### Phase 5 — End-to-End Validation

**Goal:** prove the migrated stack works on a fresh OrbStack environment.

**Validation script (manual, but document in `orbstack-setup.md`):**

```bash
# 0. Prerequisite: OrbStack installed and licensed; nushell environment set up
nu -c 'dapple verify'   # should fail cleanly — VM doesn't exist yet

# 1. Create the VM
nu assets/nu/k8s/install_orbstack_vm.nu

# 2. Initial cluster setup (registry, addons, dev-dns, certs)
nu assets/nu/k8s/setup_microk8s.nu

# 3. Deploy prompt services
nu assets/nu/k8s/deploy_dapple_prompt_pods.nu

# 4. Deploy web services
nu assets/nu/k8s/deploy_dapple_web_pods.nu

# 5. Verification matrix (each line must succeed)
nu -c 'kubectl get pods -n dapple-sandbox'                           # all Running
nu -c 'curl -sk https://registry.kube.dappleai.dev/v2/_catalog'      # returns JSON catalog
nu -c 'curl -sk https://dapple-prompt-api.kube.dappleai.dev/api/health_check'  # 200 OK
nu -c 'dig @$(orb vm_ip) -p 30053 registry.kube.dappleai.dev +short' # returns VM IP

# 6. Lifecycle resilience (the failure modes we're trying to escape)
orb stop microk8s-vm
sleep 30
orb start microk8s-vm
nu -c 'kubectl get pods -n dapple-sandbox'    # all Running again, VM IP unchanged
```

**Acceptance criteria:**
- Every command in the validation script returns exit 0.
- After `orb stop` + `orb start` cycle, no manual recovery is needed (no `dapple dns repair` equivalent).
- Force-kill simulation: `orb stop --force microk8s-vm; orb start microk8s-vm` recovers without filesystem corruption.

---

## Verification Matrix

Per-phase definitions of done, suitable for a checklist:

| Phase | Verification |
|-------|--------------|
| **1** | `orb` module imports cleanly; `install_orbstack_vm.nu` produces a Ready microk8s node; no existing files modified |
| **2** | Zero `multipass` references in `assets/nu/**/*.nu`; full deploy succeeds end-to-end on OrbStack |
| **3** | `dapple dns` and `dapple mount` subcommands gone; `dapple` module loads; help text updated |
| **4** | Zero `multipass` references in `*.md` outside this plan; AGENTS.md / CLAUDE.md / guide docs reflect OrbStack workflow |
| **5** | Full validation matrix passes; stop/start cycle requires no manual recovery |

## Testing Strategy

This is infrastructure work — no Rust unit tests, no integration test trials, no `ScenarioContext` updates. Verification is the validation matrix in Phase 5, which the implementer should walk through manually on a fresh macOS environment (or a clean OrbStack VM after `orb delete microk8s-vm`).

## AGENTS.md Updates

See Phase 4 above for the full rewrite of the "macOS MicroK8s Behavior" section in `assets/nu/AGENTS.md`. New `assets/nu/modules/orb/AGENTS.md` will be created in Phase 1 to document the new module's API and usage conventions.

A new top-level subsection is **not** needed in `CLAUDE.md` (project root) — a one-line pointer to `docs/guides/journey-builder-guide/development/orbstack-setup.md` from the existing K8s Setup section is sufficient.

## No Migration Path — This Is a Clean Rewrite

**Explicit decision (Brian, 2026-05-07):** there is no migration script and no attempt to preserve any state from the Multipass VM. The macOS dev environment is being rebuilt from scratch on OrbStack. Existing contributors clean up Multipass manually:

```bash
multipass stop microk8s-vm
multipass delete --purge microk8s-vm
brew uninstall multipass    # optional
```

…then follow the OrbStack onboarding doc. **Do not write `migrate_multipass_to_orbstack.nu` or any helper that reads from the old VM.** Any code that touches `multipass …` invocations is being deleted, not refactored.

Re-bootstrapping the microk8s cluster (registry images, prompt-db, secrets, deployments) is fast and well-trodden via `setup_microk8s.nu` + `deploy_dapple_prompt_pods.nu` + `deploy_dapple_web_pods.nu`. Cross-runtime qcow2 migration would be both more code and less safe.

## Risk and Rollback

### What We Lose Permanently

- All in-cluster state in the existing Multipass VM:
  - Registry images (will be rebuilt automatically on first deploy)
  - `prompt-db` data (will be recreated by `sqlx migrate run` in `deploy_dapple_prompt_pods.nu`; **any local-only test data will be lost**)
  - Redis state (ephemeral by design)
  - All Kubernetes secrets (regenerated from `~/.env.dapple` and service-token registration on first deploy)
  - Build cache in `target/` — *not* lost; that lives on the macOS host, untouched

The user has explicitly accepted this loss. The plan is intentionally not building a state-preservation path — it would be more code than the value justifies for a single-developer migration.

### Rollback Plan (Worst Case)

If OrbStack has a regression severe enough to block development:

1. `brew install multipass` (Multipass remains free and brew-installable).
2. `git revert` the migration commit(s) — the Multipass code paths are restored verbatim.
3. `multipass launch --name microk8s-vm 22.04 --cpus 4 --memory 8G --disk 40G` (or whatever launch flags the original setup used — the implementer should capture these in a comment in `install_orbstack_vm.nu` for reference).
4. `nu assets/nu/k8s/setup_microk8s.nu`
5. `nu assets/nu/k8s/deploy_dapple_prompt_pods.nu`

The rollback is "rebuild from manifests" — same shape as the forward migration, just with Multipass instead of OrbStack on the host. The Rust workspace, sql-composer output, migrations, and k8s registry manifests are runtime-agnostic.

### Risks Specific to OrbStack

| Risk | Mitigation |
|------|------------|
| OrbStack licensing change (paid → more-paid, or feature regressions) | Documented rollback to Multipass; the abstraction in `assets/nu/modules/orb/mod.nu` confines the OrbStack dependency to one module |
| Auto-mount semantics change in a future OrbStack release | `ensure_vm_mounts` is preserved as a no-op stub — easy to re-implement against `orb push` if needed |
| `orb` CLI surface changes between versions | Pin a minimum OrbStack version in `orbstack-setup.md`; verify in Phase 1 against the version used to author the plan |
| Brian's specific OrbStack version differs from the implementer's | Phase 1 includes "verify exact subcommand spellings" as an explicit task |

## What Does NOT Change

Calling these out explicitly to scope the migration:

- **Linux paths.** Every macOS-only branch is gated by `if ($nu.os-info.name | str contains "mac")`. Linux contributors run microk8s natively as a snap and never touched Multipass; their workflow is identical post-migration. **Verification gate:** before any Phase 2 PR ships, the implementer must `git grep` every diff to confirm zero changes execute on Linux. The gate is "no Linux dev needs to read this changelog to do anything." If a change accidentally lands inside a non-gated branch, that's a bug — fix before merging.
- **CI/CD.** GitHub Actions runners are Linux; they don't run any of the macOS-gated code.
- **microk8s addon list** in `setup_microk8s.nu` (lines 22–28): `dns`, `dashboard`, `hostpath-storage`, `ingress`, `cert-manager`, `registry`. Unchanged.
- **Kubernetes manifests** in `assets/k8s/registry/` and `assets/k8s/manifests/`. Unchanged.
- **Rust workspace** (all `crates/`). Unchanged.
- **sql-composer** workflow, `.sqlc` files, generated `.sql` files, `cargo sqlx prepare`. Unchanged.
- **Docker registry** (`registry.kube.dappleai.dev`), cert-manager-issued certs, the `register_local_certs` flow. Unchanged.
- **Database schemas, migrations, query patterns.** Unchanged.
- **dapple CLI service aliases** (`api`, `conduit`, `flow`, `ui`, etc.). Unchanged.
- **Environment overlay system** (`dapple_env_profiles/`). Unchanged.
- **`~/.env.dapple` generation, service-token registration, secret management.** Unchanged.
- **hostPath-mode auto-promotion to Docker mode on macOS** (`deploy_dapple_prompt_pods.nu` lines 36–42). Unchanged — the host-arch / VM-arch impedance mismatch is independent of which VM runtime hosts the Linux kernel.

## Open Questions and Risks

1. **Exact `orb create` image flag.** Pin in Phase 1: `orb create ubuntu microk8s-vm` vs `orb create ubuntu:22.04 microk8s-vm` vs `orb create -i ubuntu microk8s-vm`. Verify with `orb create --help` and align with the Ubuntu LTS we want (22.04 or 24.04).
2. **OrbStack default Linux user.** Phase 1 must determine whether the default user is `ubuntu`, `orb`, or something else, since `microk8s` group membership is granted to that user.
3. **`orb info` JSON shape.** The `vm_ip` function in the new `orb` module needs to know the exact JSON path (e.g., `.networks[0].ip`, `.ip`, etc.). Phase 1 task.
4. **Resolver port stability.** The dev-dns NodePort is currently 30053. Confirm OrbStack's port-forwarding semantics expose NodePorts to the macOS host the same way Multipass does (typically the macOS host can hit `<vm_ip>:30053` directly because OrbStack puts the VM on a routable network). If not, a port-forward step gets added to `install_orbstack_vm.nu`.
5. **Two-way mount conflict with macOS file watching.** OrbStack's virtiofs auto-mount means inotify events from inside the VM see macOS-side filesystem changes. Confirm this doesn't cause spurious cargo rebuilds or hot-reload storms. Likely a non-issue in practice, but should be checked during Phase 5 validation.
6. **Existing developer's global CLAUDE.md mention of Multipass.** Out of scope (user-private file). The migration PR description should flag this for Brian to update manually after merge.

## Implementation Order Summary

```
Phase 1 (additive)         → orb module + install_orbstack_vm.nu + onboarding doc
Phase 2 (swap)             → Replace multipass call sites with orb / no-op
Phase 3 (delete)           → Remove dns.nu, mount.nu, related help text
Phase 4 (docs)             → Rewrite AGENTS.md sections, guide docs
Phase 5 (validate)         → Full validation matrix on fresh OrbStack VM
```

Each phase produces an independently mergeable PR. Phase 1 can ship without Phase 2; Phase 2 can ship without Phase 3 (the deleted code becomes dead code in the meantime). Phase 4 should ideally ship in the same PR as Phase 2 or Phase 3 to keep documentation in sync with code. Phase 5 is a verification gate, not a code change.
