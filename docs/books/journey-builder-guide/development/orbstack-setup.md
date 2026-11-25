# OrbStack Setup (macOS)

> **Audience:** macOS Backend Developers · Platform Engineers
>
> Linux contributors: this document does not apply to you. MicroK8s installs natively as a snap on Linux. Skip ahead to [local-development.md](./local-development.md).

This document walks a fresh macOS environment through provisioning the [OrbStack](https://orbstack.dev)-hosted Ubuntu VM that runs MicroK8s for the dapple development cluster.

## Why OrbStack

OrbStack uses Apple's Virtualization.framework directly (no qemu/qcow2) and provides:

- **Stable VM IP** across host reboots, sleep/wake, and force-kills
- **Stable hostnames** — VMs are reachable as `<machine-name>@orb.local` without `/etc/resolver` maintenance for the VM hostname itself
- **Two-way virtiofs auto-mount** of `/Users/$USER` at the same path inside every Linux VM

These properties keep the dev cluster reachable across normal laptop activity (closing the lid, restarting, force-killing the VM) without manual recovery steps.

## Licensing

**OrbStack is a paid product.** Ensure your team has an active license before running the install script. Licensing is per-user, not per-machine. See the [OrbStack pricing page](https://orbstack.dev/pricing) for current details.

## Prerequisites

- macOS host (any recent Apple-Silicon or Intel Mac that meets OrbStack's system requirements)
- [Homebrew](https://brew.sh)
- A clone of this repo at `~/Source/Dapple/dapple-prompt` (or wherever your `DAPPLE_PROMPT_WORKSPACE_DIR` points)
- Nushell with the dapple tooling installed (`nu assets/nu/init/setup.nu --environment sandbox` then `exec nu`)

## Step 1 — Install OrbStack

```bash
brew install orbstack
```

Launch the OrbStack app once after install so it can request the system permissions it needs (network configuration, virtualization). Sign in with your licensed account.

## Step 2 — Provision the VM

From your dapple-prompt checkout:

```bash
nu assets/nu/k8s/install_orbstack_vm.nu
```

This script:

1. Verifies `orb` is on PATH and the daemon is running.
2. Creates an Ubuntu VM named `microk8s-vm` (or `$MICROK8S_VM_NAME` if set).
3. Waits up to two minutes for the VM to be reachable.
4. Installs microk8s inside the VM via snap (idempotent).
5. Adds the default Linux user to the `microk8s` group.
6. Waits for `microk8s status --wait-ready`.
7. Writes `/etc/resolver/kube.dappleai.dev` pointing at the VM IP.

The script is idempotent — re-running it on a healthy VM is a no-op.

## Step 3 — Configure the Cluster

```bash
nu assets/nu/k8s/setup_microk8s.nu
```

This enables the standard microk8s addons (`dns`, `dashboard`, `hostpath-storage`, `ingress`, `cert-manager`, `registry`), deploys `dev-dns`, the local registry, `prompt-db`, and Redis.

## Step 4 — Deploy Services

```bash
nu assets/nu/k8s/deploy_dapple_core.nu
```

The first invocation of `deploy_dapple_core.nu` on macOS verifies that the cross-compile toolchain (`zig`, `cargo-zigbuild`, `rustup target add aarch64-unknown-linux-gnu`) is installed — running `nu assets/nu/init/setup.nu --environment sandbox` once handles all three. From then on, the iteration loop on a Rust service is identical to Linux:

```bash
nu assets/nu/k8s/deploy_dapple_core.nu --svc api
```

The deploy script invokes `cargo zigbuild` itself on macOS (with `SQLX_OFFLINE=true` against the committed `.sqlx/` cache), then mounts the cross-compiled `target/aarch64-unknown-linux-gnu/debug/` binary into the pod via hostPath and restarts the deployment. Source changes are picked up by re-running the deploy. Conduit-family services (`conduit`, `conduit-onnx`, `conduit-llm`) auto-promote to Docker mode on macOS because their ML deps don't cross-compile cleanly.

## Validation Matrix

After provisioning, walk through each line. Every one should succeed.

```bash
# Cluster health
nu -c 'kubectl get pods -n dapple-sandbox'                                # all Running

# Registry reachable from host
nu -c 'curl -sk https://registry.kube.dappleai.dev/v2/_catalog'           # returns JSON catalog

# API reachable from host
nu -c 'curl -sk https://dapple-prompt-api.kube.dappleai.dev/api/health_check'  # 200 OK

# DNS resolution from host (uses /etc/resolver/kube.dappleai.dev -> VM:30053)
nu -c 'dig @(orb vm_ip) -p 30053 registry.kube.dappleai.dev +short'       # returns VM IP
```

### Lifecycle Resilience Test

The whole reason for the OrbStack migration was sleep/restart resilience. Validate that the VM survives a stop/start cycle without manual recovery:

```bash
orb stop microk8s-vm
sleep 30
orb start microk8s-vm

# Wait a few seconds for microk8s to come back, then:
nu -c 'kubectl get pods -n dapple-sandbox'                                # all Running again
```

The VM IP does not change across stop/start, so `/etc/resolver/kube.dappleai.dev` remains valid and no manual DNS repair step is needed.

## Auto-Mount Behavior

OrbStack mounts `/Users/$USER` two-way at the same path inside every Linux VM. This means:

- `/Users/brian/Source/Dapple/dapple-prompt` is identical on host and VM
- hostPath volumes referencing `/Users/$USER/...` work transparently with no per-mount registration
- Dev-server hot-reload (e.g., `dapple-ui`) sees host file changes via inotify automatically
- ONNX model paths under `~/.cache/dapple-onnx` are visible inside the VM at the same path

Anything outside `/Users/$USER` (system paths, `/etc`, etc.) is NOT auto-mounted and would need an explicit `orb push` if the VM needs access. The dapple stack does not currently rely on any such paths.

## Troubleshooting

### `orb: command not found`

OrbStack's CLI installs as part of the GUI app. After `brew install orbstack`:

1. Launch the OrbStack app once
2. Restart your shell

If `which orb` still fails, check `/Applications/OrbStack.app/Contents/MacOS` for the `orb` binary and add to PATH.

### VM exists but isn't reachable

```bash
orb info microk8s-vm
orb start microk8s-vm
orb info microk8s-vm
```

If `orb run -m microk8s-vm -- true` hangs, the VM is wedged. Force-recovery:

```bash
orb stop --force microk8s-vm
orb start microk8s-vm
```

If that still doesn't recover, delete and re-provision:

```bash
orb delete --force microk8s-vm
nu assets/nu/k8s/install_orbstack_vm.nu
nu assets/nu/k8s/setup_microk8s.nu
```

You will lose any cluster-local state that was inside the old VM (registry images, prompt-db data) but `setup_microk8s.nu` rebuilds everything from manifests.

### `*.kube.dappleai.dev` doesn't resolve

```bash
# Confirm the VM is running and note its IP
nu -c 'orb vm_ip'

# Compare against the resolver file
cat /etc/resolver/kube.dappleai.dev
```

If the IPs match, dev-dns inside the cluster is the likely culprit:

```bash
nu -c 'kubectl rollout restart deployment/dev-dns -n dapple-sandbox'
```

If the IPs do NOT match (very rare under OrbStack — the IP is stable), re-run the install script. It overwrites the resolver file with the current VM IP:

```bash
nu assets/nu/k8s/install_orbstack_vm.nu
```

## What Does NOT Change

- The microk8s addon list (`dns`, `dashboard`, `hostpath-storage`, `ingress`, `cert-manager`, `registry`)
- All Kubernetes manifests in `assets/k8s/`
- The Rust workspace, sql-composer outputs, migrations
- The local registry hostname `registry.kube.dappleai.dev`
- cert-manager-issued certificates and the `register_local_certs` flow
- The service deployment script (`deploy_dapple_core.nu`)
- The `dapple` CLI service aliases (`api`, `conduit`, `flow`, `ui`, etc.)
- Linux contributors' workflow (microk8s runs natively as a snap; no VM layer)

## Dev-cluster disk exhaustion & recovery

> Institutional memory from a multi-hour incident. Read this before you
> chase a "disk full" on the dev cluster — most of that time was spent
> chasing red herrings documented below.

### Root cause (environment-independent)

An **image-management leak**, not a config bug. It has reproduced on both
multipass+Docker-Desktop and OrbStack:

- Every `--docker` deploy pushes the same `:dapple-latest` tag. The
  in-cluster microk8s registry **never garbage-collects** the orphaned
  blobs from the previous push.
- containerd's overlayfs snapshotter strands layers that no
  `crictl`/`ctr` prune reclaims.
- Real disk climbs until the node crosses kubelet's image-GC high
  watermark (>85%) and **cascades**: kubelet image-GC fails → node
  destabilizes → image pulls fail → pods never schedule. Recovery
  required a full VM re-provision.

### The OrbStack phantom-df trap (do NOT chase this)

The OrbStack guest root is **not a real ext4 filesystem**. `df -h /` can
report **~94% used** while real usage is only ~24G:

| Signal | Phantom reading | Reality check |
|---|---|---|
| `df -h /` | ~94% full | misleading — ignore in isolation |
| `df -i /` | 0 inodes / `dumpe2fs` fails | confirms it's not real ext4 |
| `du -xsh /`, `orb info` | ~24G | **trust these** |

Always cross-check `df` against `du -xsh /` and `orb info` before treating
"disk full" as real. `reclaim_disk.nu diagnose` prints all of these
side-by-side with an interpretation note.

### Diagnosis traps that wasted hours

- **`iptables-save` shows "0 KUBE rules" — false.** `microk8s.iptables-save`
  / host `iptables-save` query the wrong backend. kube-proxy uses
  `/snap/microk8s/current/sbin/iptables-legacy-save`; query **that**.
- **`dig` is not installed in the OrbStack VM.** `orb run … dig …` returns
  empty (exit 127). That is **not** a DNS failure — check the CoreDNS /
  dev-dns pod logs instead (they log `NOERROR … aa` when answering).
- **A PARTIAL containerd cleanup corrupts metadata.** Moving/removing only
  `io.containerd.snapshotter.v1.overlayfs` leaves containerd inconsistent →
  `missing parent … bucket: not found` on every pod sandbox. Always do the
  full consistent reset: `reclaim_disk.nu containerd-reset --confirm`
  (removes ALL `io.containerd.*` together).

### Prevention

- `deploy_dapple_core.nu` now runs **registry GC after every
  `deploy_all`** (targeted `--svc` runs skip the cost).
- Periodically run `nu assets/nu/k8s/reclaim_disk.nu registry-gc`; run
  `containerd-reset --confirm` when real usage is high.
- **Do not lower kubelet's `--image-gc-high-threshold`.** Under the
  phantom-df it makes kubelet thrash *earlier* on a bogus percentage. The
  real fix is reducing **real** usage (registry GC + containerd reset).
  Thresholds are left at Kubernetes defaults intentionally — see the
  comment block in `k8s/mod.nu` `configure_macos_masqed_dns`.

### Automatic reclaim during deploys

`deploy_dapple_core.nu` runs an **auto-reclaim plan** at the end of
every full `deploy_all` (targeted `--svc` runs skip it):

**Auto-runs (safe, idempotent):**

1. **Registry garbage-collect** with a `--dry-run` probe — if the in-cluster
   registry has zero orphaned blobs the real GC is skipped entirely. Older
   registry images that don't recognize `--dry-run` fall back to
   unconditional GC (PR #488 behavior).
2. **`microk8s ctr -n k8s.io content prune references`** — removes
   unreferenced containerd content blobs.

**Recommended only (NEVER auto-executed):**

The destructive `containerd-reset` is intentionally **operator-gated** and
is only ever **suggested via a log warning** if a heuristic trips. The
reset wipes the local image cache and the re-pull storm that follows is
what produced the `missing parent ... bucket` corruption during PR #488's
originating incident — auto-running it would re-create that exact failure
mode. To act on the recommendation:

```bash
nu assets/nu/k8s/reclaim_disk.nu containerd-reset --confirm
```

**Heuristic (real bytes only — `df` is poisoned on OrbStack):**

| Signal | Default threshold | Env override |
|---|---|---|
| Containerd store size (`du -sb` real bytes) | 25 GiB | `DAPPLE_RECLAIM_HIGH_WATER_GIB` |
| Deploys since last reset (deterministic counter) | 25 | `DAPPLE_RECLAIM_DEPLOY_COUNT` |

Either condition triggers the warning; the message names which one(s).

**State file:** `~/.dapple/reclaim-state.json` — holds the deploys-since-
reset counter and size-at-last-reset snapshot. Reset by a successful
`containerd-reset --confirm`. The file contains no `df` metrics by design.

**Dry-run preview (no side effects):**

```bash
nu assets/nu/k8s/reclaim_disk.nu plan
```

Prints what the auto-reclaim plan would do — current containerd byte size,
state counters, threshold status, and the would-warn-or-not decision —
without executing or writing anything.

### VM disk monitoring and the reclaim hierarchy

PR #488's auto-reclaim plan keeps **guest-side** containerd bytes in check,
but that's only one of three layers. OrbStack's data dir is backed by
**two distinct storage backends**, not one, and each layer has its own
reclaim path with very different cost vs. effectiveness:

1. **`data.img.raw` (sparse VM disk)** — backs the Linux VM (microk8s,
   containerd-in-VM, in-cluster registry PVC, anything inside the guest
   filesystem). Guest TRIM does **not** pass through to host APFS
   hole-punching on observed OrbStack versions. Monotonically grows.
2. **APFS clones in other subtrees of OrbStack's data dir** — back
   OrbStack's **host-side Docker engine** (images, volumes, build cache).
   APFS clones **do** honor deallocation. `docker system prune` reclaims
   host disk **immediately** here.

Confusingly, both backends sit under the same `~/Library/Group
Containers/HUAQ24HBR6.dev.orbstack/data` tree, so `du -sk` on that path
sums them. The `--docker` deploy mode hits backend (2) every single
build, which is why **the host Docker engine is typically the largest
single reclaimable bucket** in normal dev usage.

#### Empirical evidence (May 2026)

- Guest `du` real usage after a `containerd-reset --confirm` and guest
  `fstrim -av`: ~24 GiB. The guest can be cleaned aggressively and the
  host sparse file does not shrink at all.
- Host-side `du -sk` real APFS blocks of
  `~/Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw`:
  **337 GiB** (apparent size 8.0 TB sparse, on-disk blocks 337 GiB).
  Confirms TRIM does not pass through.
- After a full recovery cycle (rm data.img.raw → fresh OrbStack →
  re-provision → restore → deploy), the OrbStack data dir sat at **61
  GiB** while the OrbStack UI showed only ~5.9 GiB of "current logical
  content" (microk8s-vm 743 MiB + Docker images 4.97 GiB + Docker volumes
  210 MiB). One `docker system prune -a --volumes` invocation reclaimed
  a **multi-tens-of-GiB delta** that the OrbStack UI did not show as
  "live content" — i.e. backend (2), the host Docker engine cache.

#### Reclaim hierarchy

**Layer 1 — Host Docker engine prune (cheap, immediate, host-side bytes).**
OrbStack's host Docker engine stores images, volumes, and build cache as
APFS clones (NOT inside `data.img.raw`). `docker system prune -af
--volumes` reclaims host disk immediately. This is typically the largest
single recoverable bucket after a series of `--docker` deploys and should
be tried **FIRST** when the OrbStack data dir feels big.

```bash
nu assets/nu/k8s/reclaim_disk.nu docker-prune
# or equivalently:
docker system prune -af --volumes
```

`deploy_dapple_core.nu` also runs this automatically at the end of
every `deploy_all`, so the auto-reclaim plan keeps this bucket trimmed
between manual interventions.

**Layer 2 — In-VM containerd content prune (cheap, immediate, GUEST-side
bytes only).** `microk8s ctr -n k8s.io content prune references` keeps
the guest's image-GC threshold from tripping kubelet thrash, but does
**NOT** shrink `data.img.raw` on the host — guest TRIM doesn't pass
through. Combined with the in-cluster registry blob GC (which removes
orphans left by `:dapple-latest` retags), this is the routine guest
hygiene. The deploy script's auto-reclaim plan runs both automatically.
Manual invocation if needed:

```bash
nu assets/nu/k8s/reclaim_disk.nu registry-gc
# (the `microk8s ctr content prune references` step is auto-run by
# deploy_dapple_core.nu; no separate subcommand for it here)
```

For the destructive in-VM containerd reset (when image-GC heuristics
trip the deploy-time warning):

```bash
nu assets/nu/k8s/reclaim_disk.nu containerd-reset --confirm
# Still GUEST-only: this does NOT shrink data.img.raw.
```

**Layer 3 — Full host-side reset (heavy, only when Layer 1 didn't help).**
The only path that actually shrinks `data.img.raw`. Quit OrbStack, `rm`
the sparse file, restart, re-provision the cluster, restore PVCs from
backup. Required when the `data.img.raw` byte figure itself has grown
unbounded AND `docker-prune` (Layer 1) did not reclaim enough host disk.
Per-VM `orb delete` does **NOT** reclaim host disk (verified May 2026:
deleting the only VM dropped the data dir from 337 to 334 GiB, because
OrbStack uses one shared `data.img.raw` for all its content).

```bash
# 1. Back up PVCs first — they live inside data.img.raw and will be wiped.
nu assets/nu/k8s/backup_pvc.nu backup

# 2. Quit OrbStack so the helper releases its handle on data.img.raw.
osascript -e 'tell application "OrbStack" to quit'

# 3. Verify no OrbStack process still holds the file open — removing a
#    file the helper still has open will not reclaim space until the
#    handle closes.
pgrep -fl OrbStack    # should print nothing

# 4. Remove the shared sparse file (the swap file is tiny; optional).
#    The path contains a space — quote it when pasting into your shell.
rm "$HOME/Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw"
rm "$HOME/Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/swap.img"

# 5. Relaunch OrbStack and rebuild the cluster from scratch.
open -a OrbStack
nu assets/nu/k8s/install_orbstack_vm.nu
nu assets/nu/k8s/setup_microk8s.nu
nu assets/nu/k8s/backup_pvc.nu restore ~/dapple-backups/<latest>.tar.gz --confirm
nu assets/nu/k8s/deploy_dapple_core.nu
```

Empirical confirmation: running this sequence dropped the data dir from
334 GiB to a few hundred MiB.

#### Why we monitor host bytes (no cap available)

**Per-VM `orb delete` does NOT reclaim host disk space.** Verified May
2026 against the installed OrbStack (verbatim terminal evidence):

```
$ orb delete --force microk8s-vm
   [success]
$ du -sh ~/Library/Group\ Containers/HUAQ24HBR6.dev.orbstack
334G    /Users/brian/Library/Group Containers/HUAQ24HBR6.dev.orbstack
```

Only ~3 GiB reclaimed (337 → 334). Listing the data dir on the
post-delete machine confirms `data.img.raw` is still live (and
OrbStack's helper updated its mtime *after* `orb delete` returned):

```
-rw-r--r-- 1 brian staff  8796093022208  May 21 15:30  data.img.raw
```

OrbStack uses **one shared `data.img.raw` file for ALL its content** —
every VM, plus OrbStack's own Docker engine, plus its k8s integration.
Per-VM delete only frees blocks *inside* that file; the host-side
allocation does not shrink.

**No programmatic per-VM disk cap exists either.** Verified May 2026:

- `orb create --help` exposes no disk flag (only `--arch`,
  `--forward-ssh-agent`, `--isolate-network`, `--isolated`, `--mount`,
  `--set-password`, `--user`, `--user-data`).
- `orb config show` exposes only `cpu` and `memory_mib` as configurable
  resource limits — no `disk`, `size`, `storage`, or `data_*_limit` key.

So **prevention is reactive, not preventative**: we monitor host-side
bytes and recommend the tiered reclaim when a threshold trips. We
cannot enforce a ceiling at VM create time.

| Knob | Default | Effect |
|---|---|---|
| `DAPPLE_RECLAIM_HOST_HIGH_WATER_GIB` | `60` | Threshold for the deploy-time host-side warning. When `du -sk` on the OrbStack data dir crosses this many GiB, the deploy script recommends Layer 1 (docker-prune) first and Layer 3 (full reset) only if Layer 1 didn't help. |

`reclaim_disk.nu diagnose` prints both backend figures side-by-side with
the delta and the three-layer hierarchy:

```
   guest containerd store: X GiB [in-VM real usage]
   host OrbStack img file: Y GiB [real APFS blocks]
   delta:                  Z GiB allocated but never reclaimed by guest TRIM

=== Reclaim hierarchy ===
Layer 1 — host Docker engine (APFS clones, reclaims immediately):
  nu assets/nu/k8s/reclaim_disk.nu docker-prune
...
```

> **Roadmap TODO:** if a future OrbStack version adds a per-VM disk-cap
> config (`orb config set <key>` or `orb create --disk`), wire it into
> `orb ensure_vm` here. As of May 2026 no such surface exists.
>
> **Future work (not in this PR):** content-hash-tag the conduit Docker
> images (e.g. `dapple-prompt-conduit:sha-<short-hash>`) so re-pushes only
> happen on real change. That reduces growth-event frequency upstream of
> the registry, attacking the cause rather than treating the symptom.

### Recovery runbook

1. `nu assets/nu/k8s/backup_pvc.nu backup` — tars prompt-db to
   `~/dapple-backups/`. The backup dir lives
   on the Mac (outside `data.img.raw`), so it survives the wipe in step 3.
   **This step is non-optional** — step 3 destroys all in-VM state.
2. `osascript -e 'tell application "OrbStack" to quit'` — quit OrbStack
   so its helper releases the handle on `data.img.raw`. Then `pgrep -fl
   OrbStack` should print nothing before continuing.
3. `rm "$HOME/Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw"`
   (and optionally `swap.img` next to it). This is the only operation
   that actually reclaims host disk — `orb delete` does not. Per-VM
   delete leaves the shared sparse file intact.
4. `open -a OrbStack` to relaunch the helper, then re-provision:
   `nu assets/nu/k8s/install_orbstack_vm.nu` →
   `nu assets/nu/k8s/setup_microk8s.nu`.
5. `nu assets/nu/k8s/backup_pvc.nu restore ~/dapple-backups/prompt-db-pgdata-<YYYYMMDD>.tar.gz --confirm`
   **before** `deploy_dapple_core.nu`, so migrations see the
   restored `_sqlx_migrations` table. (Postgres runs crash-recovery on
   first start — normal.)
6. `nu assets/nu/k8s/deploy_dapple_core.nu`.

> A VM delete+recreate **changes the VM IP** (stable only across
> reboot/sleep, not recreate). `setup_host_resolver` now verifies its write
> and **fails loud with a paste-ready manual command** if it can't write
> `/etc/resolver/kube.dappleai.dev` (e.g. non-interactive sudo), instead of
> silently leaving a stale file.
