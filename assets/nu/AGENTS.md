# AGENTS.md — Nushell Deployment Infrastructure

This directory contains Nushell scripts and modules for deploying and managing the dapple-prompt infrastructure.

## Directory Structure

```
assets/nu/
├── config/           # Configuration files
├── deploy/           # Deployment utilities
├── init/             # setup.nu (current); setup_nu.nu (legacy, see below)
├── k8s/              # Kubernetes deployment scripts
│   ├── deploy_dapple_core.nu         # Deploy dapple-core services
│   └── setup_microk8s.nu             # Initialize microk8s cluster
├── modules/          # Reusable Nushell modules
│   ├── dapple_env_profiles/          # Environment overlay system
│   │   ├── dapple_base.nu            # Base configuration
│   │   ├── dapple_microk8s.nu        # MicroK8s-specific config
│   │   ├── dapple_sandbox.nu         # Sandbox environment config
│   │   └── dapple_sandbox_tools.nu   # Tools overlay
│   ├── k8s/                          # Kubernetes utilities module
│   │   ├── mod.nu                    # Core k8s functions
│   │   └── node_sandbox.nu           # Node.js sandbox utilities
│   └── tools/                        # CLI tools
│       ├── endpoint.nu               # API endpoint testing
│       └── service_token.nu          # Service token management
└── README.md         # Quick-start documentation
```

## Environment Profile System

Scripts use Nushell's overlay system for environment configuration. The profile hierarchy:

```
dapple_base → dapple_microk8s → dapple_sandbox → dapple_sandbox_tools
```

### Required Environment Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `KUBE_DAPPLE_NAMESPACE` | K8s namespace | `dapple-sandbox` |
| `KUBE_REGISTRY_HOST` | Docker registry hostname | `registry.kube.dappleai.dev` |
| `DAPPLE_PROMPT_WORKSPACE_DIR` | Workspace root | `~/src/dapple/dapple-prompt` |
| `DOCKER_DAPPLE_TAG_LATEST` | Docker tag for builds | `dapple-latest` |
| `DAPPLE_PROMPT_DATABASE_URL` | PostgreSQL connection string (prompt services, via `prompt-db`) | `postgres://...` |
| `DAPPLE_ONNX_CACHE_DIR` | ONNX model cache directory | `~/.cache/dapple-onnx` |
| `DAPPLE_DOCKER_DESKTOP` | Set to `true` on Linux to use Docker Desktop cert paths instead of native dockerd (`/etc/docker/certs.d/`). macOS always uses Docker Desktop. | `true` |

### Loading Environment

```nushell
use ../modules/dapple_env_profiles dapple_sandbox_tools
overlay use dapple_sandbox_tools as dapple_env
```

## K8s Module Functions

The `k8s` module (`<nushell-config-dir>/modules/k8s/mod.nu` — `~/.config/nushell/` on Linux, `~/Library/Application Support/nushell/` on macOS; see [Module Setup](#module-setup)) provides:

| Function | Description |
|----------|-------------|
| `render` | Render manifest template with token substitution |
| `apply_yaml` | Apply YAML via kubectl |
| `deploy` | Render + apply + wait for rollout |
| `apply_manifest` | Apply rendered manifest without waiting |
| `delete` | Delete workload and related resources |
| `logs` | Stream pod logs |
| `rollout_status` | Wait for rollout completion |
| `rollout_restart` | Trigger rolling restart |
| `setup_docker_registry_cert` | Configure Docker to trust registry CA |
| `setup_k8s_registry_cert` | Configure K8s nodes to trust registry CA |
| `setup_host_resolver` | Make `*.kube.dappleai.dev` resolve on the host. **Probes first and returns if the zone already resolves**, whatever is serving it — so it is a no-op on a re-run and never fights a resolver it doesn't manage. Otherwise: `/etc/resolver` on macOS, a systemd-resolved drop-in on Linux. Warns rather than aborting when the zone still doesn't resolve; raises only on a failed write |
| `configure_masqed_dns` | **Pod** DNS, unrelated to `setup_host_resolver` despite sitting next to it — "masqed" is *masquerade*, not dnsmasq. Pins kubelet `--resolv-conf` to the checked-in `assets/k8s/config/resolv.conf` (1.1.1.1 / 8.8.8.8). Structural, not a workaround: any local stub resolver puts a loopback address in the node's resolv.conf (`127.0.0.53` resolved, `127.0.0.1` dnsmasq) and that address is dead inside a pod's netns. **Changing the host resolver therefore cannot break pod DNS** |
| `patch_coredns_upstream` | Sets the same two upstreams in the cluster CoreDNS Corefile. **Belt-and-braces, not a second decision** — CoreDNS runs `dnsPolicy: Default`, so its `/etc/resolv.conf` is already the kubelet-pinned file. Exists for a cluster whose kubelet args were never patched. Sets the UPSTREAM only; adds no zone, so **pods cannot resolve `*.kube.dappleai.dev`** and in-pod config must use Service names (see `environments.nu`'s `internal_*` fields) |
| `ensure_vm_mounts` | Mount host paths into MicroK8s VM (macOS only) |

## Deployment Script Pattern

`deploy_dapple_core.nu` defaults to **hostPath mode** — it mounts the local source/binary directory into the pod. No Docker build is needed for local development.

### Deployment Modes

| Mode | Flag | Behavior |
|------|------|----------|
| **hostPath** (default) | none | Builds the service, mounts `target/<triple>/debug/` into pod, runs local binary directly |
| **hostPath, no build** | `--no-build` | Skips the build and mounts whatever is already in `target/<triple>/debug/` |
| **Docker** | `--docker dev` | Builds Docker image, pushes to registry, deploys from image |

On Linux, `<triple>` is implicit (just `target/debug/`). On macOS, hostPath mode mounts the cross-compiled `target/aarch64-unknown-linux-gnu/debug/` instead — see [macOS hostPath via cross-compile](#macos-hostpath-via-cross-compile) below. Conduit-family services (`conduit`, `conduit-onnx`, `conduit-llm`) auto-promote to Docker mode on macOS because their third-party native deps (`mistralrs`, `hf-hub`, `ort`) can't cross-compile cleanly.

**Dirty tree guard:** Docker builds refuse to run if `git status` shows uncommitted changes. This prevents content-hash cache poisoning (where a dirty build gets tagged with the clean hash, causing future clean deploys to silently use stale code). Use hostPath mode for iterating on uncommitted code.

### Quick Deploy (single service, hostPath)

```bash
# Identical workflow on Linux and macOS — the deploy script picks the right
# build command for the host (cargo build on Linux, cargo zigbuild on Mac).
nu assets/nu/k8s/deploy_dapple_core.nu --svc api
```

**The deploy builds the binary it mounts, on both platforms.** In hostPath mode the mounted artifact *is* the deployment, so a stale `target/` dir means the rollout succeeds while your change never runs — the pod comes up healthy running the last binary you happened to build. `setup_dapple_svc` therefore compiles each hostPath service before applying the patch: `cargo zigbuild -p <svc> --target aarch64-unknown-linux-gnu` on macOS, plain `cargo build -p <svc>` on Linux. Cargo's freshness check makes this a fast no-op when you already built by hand, so there is no reason to skip it.

The two builds differ in one respect: the macOS cross-compile sets `SQLX_OFFLINE=true` (it uses the committed `.sqlx/` cache instead of needing a live database connection), while the Linux build leaves it unset and compiles against the live DB through the overlay's `DATABASE_URL` — identical to the `cargo build` you would have typed yourself. A query change that outran `dapple sqlx prepare` therefore fails the same way it fails in your own terminal.

`--no-build` opts out and mounts whatever is already in `target/`. Use it to redeploy a known-good artifact, or when you just built by hand and want to skip even the freshness check. If the binary is missing, `apply_hostpath_patch` fails loudly with the manual build command rather than mounting an empty directory (which crashloops the pod with an opaque "no such file").

To redeploy code to an already-deployed hostPath service (no manifest changes), the CLI shortcut is:

```bash
dapple patch api --build      # compile (cross-compiles on macOS) + restart
dapple patch api --restart    # restart only, if the binary is already built
```

**Do not use `dapple patch`/`dapple quick` without `--build` for a hostPath service.** Those only update the pod's *image*, which a hostPath pod never executes — the pod keeps running the last binary you built locally, so the deploy looks successful while your change is nowhere. `patch` detects a hostPath mount and refuses with these commands rather than no-op'ing silently.

### Docker Deploy (for CI/staging)

```bash
# Build and deploy via Docker (requires clean git tree)
nu assets/nu/k8s/deploy_dapple_core.nu --docker dev --svc api
```

### macOS hostPath via cross-compile

Mac binaries are Mach-O and can't run in the Linux microk8s VM. The deploy script handles this by cross-compiling the binary for `aarch64-unknown-linux-gnu` itself and mounting that output instead of the native `target/debug/`. Three pieces enable this:

1. The Cargo workspace uses `rustls-tls-native-roots` (not `native-tls`) for `reqwest` and `runtime-tokio-rustls` for `sqlx`, eliminating `openssl-sys` from the dep tree of cross-compile-eligible binaries (`api`, `external-api`, `flow`). The conduit family still pulls `openssl-sys` transitively via `mistralrs`/`hf-hub`/`rig-core`, so it stays on Docker.
2. `cargo zigbuild` (built atop [zig](https://ziglang.org)) routes `cc-rs` through `zig cc` to handle the C cross-compile of `ring`'s assembly bits.
3. The deploy script invokes `cargo zigbuild -p <svc> --target aarch64-unknown-linux-gnu` automatically on Mac before applying the hostPath patch — Mac contributors don't need to memorize the cross-compile command.

**One-time setup** (handled by `assets/nu/init/setup.nu`):

```bash
rustup target add aarch64-unknown-linux-gnu
brew install zig
cargo install cargo-zigbuild
```

The deploy script runs `ensure_cross_compile_prereqs` up front on Mac and fails with explicit install instructions if any of these are missing.

### Configuration Record

```nushell
setup_dapple_svc {
  name: "dapple-prompt-api"       # Service name (used for image, manifest)
  profile: "dev"                  # Cargo build profile
  namespace: $env.KUBE_DAPPLE_NAMESPACE
  source_path: $env.DAPPLE_PROMPT_WORKSPACE_DIR
  template: dapple-prompt-service.yaml  # Template from manifests/
  port: "8001"                    # Container port
  node_port: $env.DAPPLE_WEB_LANDING_PAGE_PORT
}
```

### Template Selection

| Template | Use Case |
|----------|----------|
| `dapple-prompt-service.yaml` | HTTP services with health checks |
| `dapple-prompt-engine.yaml` | Background workers (no health checks) |

## Common Operations

### Deploy All Services

```bash
cd $DAPPLE_PROMPT_WORKSPACE_DIR
nu assets/nu/k8s/deploy_dapple_core.nu
```

### Check Deployment Status

```nushell
use k8s
k8s rollout_status { name: "dapple-prompt-api", namespace: "dapple-sandbox" }
```

### View Logs

```nushell
use k8s
k8s logs "dapple-prompt-api" -f  # Follow logs
```

### Restart Deployment

```nushell
use k8s
k8s rollout_restart "dapple-prompt-api"
```

**IMPORTANT**: Kubernetes does not automatically reload secrets into running pods. After updating a secret (e.g., via `kubectl create secret`), you must restart the deployment to pick up the new values:

```bash
kubectl rollout restart deployment/dapple-ui -n dapple-sandbox
```

## Nushell Version Compatibility

**CRITICAL**: Nushell APIs change between versions. Common pitfalls:

| Old API | New API (0.80+) | Notes |
|---------|-----------------|-------|
| `$nu.home-path` | `$nu.home-dir` | Renamed in Nushell 0.80 |

Use the version-compatible pattern for home directory:
```nushell
let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
```
This works with both old (<0.80) and new (0.80+) Nushell versions.

## Anti-Patterns

### Docker Image Parsing (CRITICAL)

**NEVER** parse `docker images` output using column-based string splitting:

```nushell
# BAD - Breaks with Docker Desktop's variable-width columns
let image = (
    ^docker images -f $"reference=($tag)" |
    split row "\n" |
    get 1 |
    split column --regex "\\s+"
)
let image_id = $image.column2 | get 0  # Gets SIZE instead of ID!
```

**WHY IT BREAKS**: Docker Desktop uses variable-width columns, and the "IMAGE ID" header is two words, which skews column indices. You end up with size (`195MB`) instead of image ID (`0bd40bd470b1`).

**ALWAYS** use Docker's `--format` flag for machine-readable output:

```nushell
# GOOD - Reliable across all Docker versions
let image_id = (^docker images -f $"reference=($tag)" --format "{{.ID}}" | str trim)

if ($image_id | is-empty) {
    error make { msg: $"No image found matching ($tag)" }
}
```

Available format placeholders: `{{.ID}}`, `{{.Repository}}`, `{{.Tag}}`, `{{.Size}}`, `{{.CreatedAt}}`

### Variable Interpolation in Command Arguments (CRITICAL)

**NEVER** use bare variables in command arguments:

```nushell
# BAD - Passes literal string "$nextauth_url", not the value
kubectl create secret generic my-secret --from-literal=NEXTAUTH_URL=$nextauth_url
```

**WHY IT BREAKS**: In nushell, `--flag=$var` passes the literal string `$var`, not its value. This is different from bash behavior.

**ALWAYS** use nushell's `$"..."` string interpolation:

```nushell
# GOOD - Properly interpolates the variable value
kubectl create secret generic my-secret $"--from-literal=NEXTAUTH_URL=($nextauth_url)"
```

The `$"..."` syntax with `($var)` inside ensures the variable is expanded before being passed to the command.

### Other Anti-Patterns

- **Hardcoded paths**: Use environment variables from profiles
- **Skipping registry manifest**: If a service needs custom config (probes, env vars), create `assets/k8s/registry/{service}.yaml`
- **Using `dapple-prompt-engine.yaml` for HTTP services**: Engine template lacks health probes
- **Ignoring push failures**: The script checks for certificate errors and provides guidance
- **Using `$nu.home-path` or `$nu.home-dir` directly**: Use the version-compatible pattern (see Nushell Version Compatibility section)

### Module Imports (CRITICAL)

**NEVER** use runtime environment variables in `use` statements:

```nushell
# BAD - $env.* is a runtime value, not available at parse time
use ($env.DAPPLE_PROMPT_WORKSPACE_DIR | path join "tools" | path join "service_token.nu")
```

**WHY IT BREAKS**: Nushell's `use` statement is evaluated at parse time, before any code runs. Environment variables don't exist yet at parse time.

Module paths are resolved at parse time, so they must be literals or
parse-time constants. Prefer a path RELATIVE TO THE FILE — nushell resolves it
against the source file, not the working directory, so scripts load the modules
from the checkout they ship with rather than whatever happens to be installed
in the user's nushell config:

```nushell
# GOOD - relative to this file; always the matching checkout copy
# Import specific functions with [func_name] syntax
use ../modules/tools/service_token.nu [ensure]

# Then call directly
let secret = (ensure dapple_ui --env-var DAPPLE_UI_API_SECRET --name "Dapple UI")
```

Parse-time constants: `$nu.default-config-dir`, `$nu.home-dir`, `$nu.config-path`, `path self`

**NOTE**: When importing functions from a module file, use the `[function_name]` syntax to import specific exports. The subcommand pattern (`main ensure`) used for CLI doesn't work well with module imports.

## Module Setup

Modules must be copied into your nushell config directory's `modules/` before
scripts can import them:

```bash
nu assets/nu/init/setup.nu --environment sandbox
```

The nushell config directory is `$nu.default-config-dir`, which is
**platform-dependent** — the setup script targets it via `$nu.config-path`, so
it lands in the right place automatically:

| Platform | nushell config dir | Modules land in |
|----------|--------------------|-----------------|
| Linux    | `~/.config/nushell/` | `~/.config/nushell/modules/` |
| macOS    | `~/Library/Application Support/nushell/` | `~/Library/Application Support/nushell/modules/` |

(`$nu.default-config-dir` resolves to whichever applies; prefer it over a
hardcoded path when you need to reference the location.)

This copies all directories from `assets/nu/modules/` to the nushell config directory. Run this after:
- Cloning the repo for the first time
- Adding new modules
- Updating existing modules

## Cross-Platform Portability (CRITICAL)

**The `dapple_env_profiles/` modules are used across multiple environments and platforms.** Never hardcode absolute paths in these files.

### Portable Import Patterns

**Within modules** — Use `path self` for sibling imports:
```nushell
# GOOD - Works on any machine
use (path self dapple_base.nu) *
export use (path self dapple_microk8s.nu) *
```

**In user's env.nu** — Use parse-time constants:
```nushell
# GOOD - Uses config directory, not hardcoded path
const modules_dir = $nu.default-config-dir | path join "modules" | path join "dapple_env_profiles"
use ($modules_dir | path join "dapple_sandbox.nu") *
```

### Anti-Pattern: Hardcoded Paths

```nushell
# BAD - Only works on one developer's machine
use /home/kenny/src/dapple/dapple-prompt/assets/nu/modules/dapple_env_profiles/dapple_base.nu *
```

### Environment Targets

These modules are deployed to:
- **Local development** — Developer workstations (Linux, macOS)
- **CI/CD pipelines** — GitHub Actions runners
- **Kubernetes pods** — Container environments with nushell

Any hardcoded path will break deployments on other machines.

## macOS MicroK8s Behavior

On macOS, MicroK8s runs inside an OrbStack-managed Ubuntu VM. OrbStack uses
Apple's Virtualization.framework directly (no qemu/qcow2) and provides:

- A stable VM IP across host reboots and sleep/wake
- A stable hostname (`microk8s-vm@orb.local`) without `/etc/resolver` maintenance for the VM hostname itself
- Two-way virtiofs auto-mount of `/Users/$USER` into every Linux VM at the same path

These properties eliminate three classes of failure that the previous
VM-runtime exposed (qcow2 corruption, DHCP IP drift, zombie filesystem
mounts) and the recovery code those failures required.

### Host Path Access

Because OrbStack auto-mounts the user home directory two-way, hostPath volumes
referencing any path under `/Users/$USER` work transparently — no per-mount
registration step is needed. The `ensure_vm_mounts` function is retained as a
no-op for call-site compatibility but performs no real work; it logs a warning
if a caller passes a path outside `/Users/$USER` (which would not be
auto-mounted).

```nushell
use k8s
k8s ensure_vm_mounts [
    $env.DAPPLE_ONNX_CACHE_DIR
    $env.DAPPLE_PROMPT_WORKSPACE_DIR
]
```

### Initial Setup

```bash
brew install orbstack
nu assets/nu/k8s/install_orbstack_vm.nu     # create + boot microk8s-vm, install microk8s snap
nu assets/nu/k8s/setup_microk8s.nu          # configure addons, registry, certs, dev-dns
```

OrbStack is a paid product; ensure your license is active before running
`install_orbstack_vm.nu`. See
[`docs/guides/journey-builder-guide/development/orbstack-setup.md`](../../docs/books/journey-builder-guide/development/orbstack-setup.md)
for the full onboarding walkthrough.

### Docker Certificate Paths

Certificate installation depends on which Docker daemon is in use:

| Platform | Docker Daemon | Cert Path | Restart Needed? |
|----------|--------------|-----------|-----------------|
| macOS | Docker Desktop (always) | `~/.docker/certs.d/{registry}/ca.crt` + system keychain | Yes — restart Docker Desktop |
| Linux (default) | Native dockerd | `/etc/docker/certs.d/{registry}/ca.crt` (via `sudo`) | No — dockerd reads certs on each TLS handshake |
| Linux + `DAPPLE_DOCKER_DESKTOP=true` | Docker Desktop | `~/.docker/certs.d/{registry}/ca.crt` | Yes — restart Docker Desktop |

The `_uses_docker_desktop` helper in `k8s/mod.nu` encapsulates this logic. Error messages in `deploy_dapple_core.nu` and `build_image.nu` are platform-aware.

### ONNX Model Provisioning

The deploy script (`deploy_dapple_core.nu`) automatically downloads pre-converted ONNX models from HuggingFace when deploying the conduit service. No Python/torch/optimum dependencies are needed.

**Auto-downloaded models:**

| Model | HuggingFace Repo | Files |
|-------|------------------|-------|
| embedding | `lightonai/modernbert-embed-large` | `onnx/model.onnx`, `tokenizer.json` |
| mlm | `answerdotai/ModernBERT-base` | `onnx/model.onnx`, `tokenizer.json` |

**Not auto-downloaded:**
- **T5 summarization** — No pre-converted ONNX on HuggingFace (encoder-decoder architecture). Requires manual Python conversion via `scripts/onnx_convert/convert_t5.py`.
- **TTS (VITS)** — Downloaded from GitHub releases via `convert_onnx_models.nu tts`.

**Manual usage:**
```bash
nu assets/nu/k8s/convert_onnx_models.nu                  # Download all models
nu assets/nu/k8s/convert_onnx_models.nu --model embedding # Download specific model
nu assets/nu/k8s/convert_onnx_models.nu verify            # Check model status
nu assets/nu/k8s/convert_onnx_models.nu --force            # Re-download
```

### ONNX Model Path

The deployment script patches ONNX paths in Kubernetes manifests to use the
host path. Under OrbStack the host path is visible inside the VM at the same
location via auto-mount, so no per-path registration is needed. Set
`DAPPLE_ONNX_CACHE_DIR` in your environment or let it default to
`~/.cache/dapple-onnx`.

### DNS Resolution for `*.kube.dappleai.dev`

`/etc/resolver/kube.dappleai.dev` is written **once** during
`install_orbstack_vm.nu`, pointing at OrbStack's stable VM IP. The dev-dns
CoreDNS pod inside the cluster bakes `status.hostIP` at pod start. Both
remain valid across VM lifecycle events because the IP no longer drifts.

If `*.kube.dappleai.dev` ever stops resolving from the host:

1. `orb info microk8s-vm` — confirm the VM is running and note its IP
2. `cat /etc/resolver/kube.dappleai.dev` — confirm the IP matches
3. If mismatched: rerun `nu assets/nu/k8s/install_orbstack_vm.nu` (idempotent)

There is no `dapple dns repair` command — that existed only to recover from
DHCP-driven VM IP drift, which OrbStack does not have.

**Caveat:** the IP IS stable across reboot/sleep but DOES change on a VM
delete+recreate. `k8s setup_host_resolver` now verifies its write
(readback) and fails LOUD with a paste-ready manual command if it can't
write the file (e.g. non-interactive sudo) — no more silent stale resolver.

### Disk exhaustion / image-management leak

Environment-independent leak (seen on multipass+Docker-Desktop and
OrbStack): `--docker` deploys keep pushing `:dapple-latest`; the in-cluster
registry never GCs orphaned blobs and containerd overlayfs strands layers,
until kubelet's image-GC watermark (>85%) cascades the node.

**The OrbStack phantom-df trap:** `df -h /` reports ~94% while real usage
(`du -xsh /`, `orb info`) is ~24G — the guest root is not real ext4
(`df -i /` shows 0 inodes). **Cross-check `du`/`orb info` before believing
`df`.** Lowering kubelet's image-GC threshold is **counterproductive** here
(it thrashes earlier on a bogus %); thresholds stay at k8s defaults
(comment in `k8s/mod.nu` `configure_macos_masqed_dns`).

**Tooling:**

| Script | Use |
|---|---|
| `reclaim_disk.nu diagnose` | read-only; cross-checks df vs du/orb-info + prints three-layer reclaim hierarchy |
| `reclaim_disk.nu plan` | read-only dry-run of the auto-reclaim plan |
| `reclaim_disk.nu docker-prune` | Layer 1 — prune host-side Docker engine cache (APFS clones; reclaims host disk IMMEDIATELY) |
| `reclaim_disk.nu registry-gc` | reclaim orphaned registry blobs (best-effort) |
| `reclaim_disk.nu containerd-reset --confirm` | SAFE full containerd reset (wipes local image cache; PVCs untouched; GUEST bytes only) |
| `backup_pvc.nu backup` / `restore` | Postgres PVC backup/restore around a VM re-provision |

`deploy_dapple_core.nu` now runs an **auto-reclaim plan** at the
end of every `deploy_all`: safe actions (registry GC with `--dry-run`
probe, `ctr content prune`) run automatically; the destructive
`containerd-reset` is only ever **recommended via a log warning** when a
heuristic on real bytes (`du -sb`, never `df`) or a deploys-since-reset
counter trips. Knobs: `DAPPLE_RECLAIM_HIGH_WATER_GIB` (default 25 GiB),
`DAPPLE_RECLAIM_DEPLOY_COUNT` (default 25). State file:
`~/.dapple/reclaim-state.json`. Recovery runbook (backup → re-provision →
restore **before** deploy → deploy) and the diagnosis red-herrings
(`iptables-legacy-save`, `dig` absent in VM, partial-containerd-cleanup
corruption) are documented in
`docs/books/journey-builder-guide/development/orbstack-setup.md` →
"Dev-cluster disk exhaustion & recovery".

**Host-side disk monitor (no cap available).** Guest TRIM does not pass
through to host APFS hole-punching on observed OrbStack versions, so the
host `data.img.raw` grows monotonically regardless of guest cleanup
(empirical: 337 GiB host-side after a full guest reset + `fstrim`).
OrbStack exposes no programmatic disk cap — `orb create` has no disk
flag and `orb config show` only surfaces `cpu` and `memory_mib`. The
deploy reclaim plan therefore monitors rather than caps: it checks
host-side bytes via `du -sk` on the OrbStack data dir and emits a
SEPARATE warning when it crosses `DAPPLE_RECLAIM_HOST_HIGH_WATER_GIB`
(default 60). **Per-VM `orb delete` does NOT reclaim host disk** —
verified May 2026: deleting the only VM dropped the data dir from 337 to
334 GiB. OrbStack uses one shared `data.img.raw` for ALL its content
(every VM + Docker engine + k8s integration), and per-VM delete only
frees blocks *inside* that file. The actual reclaim is `backup_pvc.nu
backup` → `osascript … quit OrbStack` → `rm data.img.raw` → `open -a
OrbStack` → re-provision → restore. See the same doc section above.

**Reclaim hierarchy (three layers).** OrbStack uses **two** storage
backends, not one: (1) `data.img.raw` (sparse VM disk; guest TRIM does
not pass through — only `rm`'ing the file shrinks it), and (2) APFS
clones in other subtrees of the OrbStack data dir (host-side Docker
engine cache; APFS deallocation works — `docker system prune` reclaims
host disk immediately). After a series of `--docker` deploys, backend
(2) is typically the largest reclaimable bucket; one
`docker system prune -a --volumes` reclaims a multi-tens-of-GiB delta
the OrbStack UI does not show as "live content". The three layers,
in order of cost/effectiveness: **Layer 1**
`reclaim_disk.nu docker-prune` (host Docker engine, cheap, immediate
— try FIRST; also auto-run by every `deploy_all`); **Layer 2**
`reclaim_disk.nu registry-gc` + the auto-run `ctr content prune`
(in-VM containerd, keeps guest image-GC honest but does NOT shrink
`data.img.raw`); **Layer 3** the full host-side reset (`backup_pvc.nu`
→ quit OrbStack → `rm data.img.raw` → re-provision → restore) — the
only path that actually shrinks `data.img.raw`.
