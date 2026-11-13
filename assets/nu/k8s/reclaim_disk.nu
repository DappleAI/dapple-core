#!/usr/bin/nu
#
# reclaim_disk.nu — diagnose and recover the macOS dev microk8s cluster from
# the image-management disk leak.
#
# ROOT CAUSE (environment-independent — seen on multipass+Docker-Desktop and
# on OrbStack): every `--docker` deploy pushes the same `:dapple-latest` tag.
# The in-cluster microk8s registry never garbage-collects the orphaned blobs
# from the previous push, and containerd's overlayfs snapshotter strands
# layers that no `crictl/ctr` prune reclaims. Real disk usage climbs until
# the node crosses kubelet's image-GC high watermark and cascades: kubelet GC
# fails -> node destabilizes -> image pulls fail -> pods never schedule.
#
# This script codifies the SAFE recovery. An earlier ad-hoc version moved
# only the overlayfs snapshot dir aside, which left containerd metadata
# inconsistent ("missing parent ... bucket: not found" on every pod sandbox).
# The reset MUST remove ALL `io.containerd.*` together as one operation.
#
# Subcommands:
#   nu assets/nu/k8s/reclaim_disk.nu diagnose
#       Read-only. Cross-check `df` against `du`/`orb info` so you don't
#       chase the OrbStack phantom-df (df says ~94% while real usage is tiny).
#       Also prints the three-layer reclaim hierarchy so it's obvious which
#       command to reach for next.
#
#   nu assets/nu/k8s/reclaim_disk.nu docker-prune
#       Prune OrbStack's host-side Docker engine cache (`docker system
#       prune -af --volumes`). Layer 1 — typically the biggest reclaimable
#       bucket after a series of --docker deploys, because the host Docker
#       engine stores images/volumes/build-cache as APFS clones (NOT inside
#       data.img.raw). APFS deallocation works, so this reclaims host disk
#       IMMEDIATELY. Try this FIRST when the OrbStack data dir feels big.
#
#   nu assets/nu/k8s/reclaim_disk.nu registry-gc
#       Run the registry blob garbage-collector in the in-cluster registry
#       pod. Best-effort; safe to run anytime. Reclaims orphaned blobs.
#
#   nu assets/nu/k8s/reclaim_disk.nu containerd-reset --confirm
#       The SAFE full containerd reset (stop microk8s, wipe ALL io.containerd.*
#       consistently, restart). Wipes the local image cache (everything
#       re-pulls). The in-cluster registry PVC is on separate hostpath
#       storage and is NOT touched. Without --confirm it only prints the
#       plan. NOTE: this reclaims GUEST bytes only — it does NOT shrink
#       data.img.raw on the host. For host-side reclaim, see the three-
#       layer model printed by `diagnose`.
#
# macOS-only: the destructive/diagnostic paths drive the OrbStack VM via the
# `orb` module. On Linux microk8s runs natively as a snap and this script's
# VM abstraction does not apply — it reports that and exits.

use std/log

use ../modules/orb

# Shared reclaim helpers — also imported by deploy_dapple_prompt_pods.nu so
# both files use the same state file path and the same `du -sb` byte count.
use ../modules/dapple/reclaim.nu

# microk8s containerd state lives here inside the VM.
const CONTAINERD_ROOT = "/var/snap/microk8s/common/var/lib/containerd"
const OVERLAYFS_DIR = "/var/snap/microk8s/common/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs"
const CONTENT_DIR = "/var/snap/microk8s/common/var/lib/containerd/io.containerd.content.v1.content"
# microk8s hostpath-storage provisioner backing dir (registry PVC lives here).
const HOSTPATH_STORAGE = "/var/snap/microk8s/common/default-storage"

# Guard: every subcommand needs the OrbStack VM. On Linux there is no VM.
def _require_macos []: nothing -> bool {
    if not (orb is_macos) {
        log warning "reclaim_disk.nu is macOS-only — on Linux, microk8s runs natively as a snap and this VM abstraction does not apply. Use host `df` / `du` and `microk8s ctr` directly."
        return false
    }
    true
}

# Shared with deploy_dapple_prompt_pods.nu's registry_garbage_collect (~6
# lines — duplicated intentionally rather than introducing a cross-file
# import between a k8s/ script and the deploy script).
def _registry_gc []: nothing -> nothing {
    let pod = (do { kubectl get pods -n container-registry -l app=registry -o "jsonpath={.items[0].metadata.name}" } | complete)
    if $pod.exit_code != 0 {
        log warning "registry GC skipped — could not query the container-registry registry pod (microk8s registry addon not enabled?)"
        return
    }
    let pod_name = ($pod.stdout | str trim)
    if ($pod_name | is-empty) {
        log warning "registry GC skipped — no registry pod found in the container-registry namespace"
        return
    }
    let gc = (do { kubectl exec -n container-registry $pod_name -- bin/registry garbage-collect /etc/docker/registry/config.yml } | complete)
    if $gc.exit_code == 0 {
        log info $"registry GC complete on pod ($pod_name) — orphaned blobs reclaimed"
    } else {
        log warning $"registry GC on pod ($pod_name) exited non-zero \(best-effort\): ($gc.stderr | str trim)"
    }
}

# Default: show usage.
def main [] {
    print "reclaim_disk.nu — dev-cluster disk-leak recovery"
    print ""
    print "Subcommands:"
    print "  diagnose            Read-only: cross-check df vs du/orb info + three-layer reclaim model"
    print "  plan                Read-only: print what an auto-reclaim run would do (no side effects)"
    print "  docker-prune        Layer 1 — prune host-side Docker engine cache (APFS clones; reclaims host disk IMMEDIATELY)"
    print "  registry-gc         Reclaim orphaned in-cluster registry blobs (best-effort)"
    print "  containerd-reset    SAFE full containerd reset (needs --confirm to execute; reclaims GUEST bytes only)"
    print ""
    print "Run a subcommand, e.g.:"
    print "  nu assets/nu/k8s/reclaim_disk.nu diagnose"
}

# diagnose — read-only. The whole point is to NOT chase a bogus 'disk full'.
def "main diagnose" [] {
    if not (_require_macos) { return }

    let vm = (orb vm_name)
    log info $"diagnosing disk usage for OrbStack VM ($vm)"

    print "=== df -h / (GUEST — UNRELIABLE under OrbStack; see note) ==="
    let df_h = (do { orb vm_run [bash -c "df -h /"] } | complete)
    print ($df_h.stdout | str trim)

    print ""
    print "=== df -i / (inodes — OrbStack guest root often reports 0; not real ext4) ==="
    let df_i = (do { orb vm_run [bash -c "df -i /"] } | complete)
    print ($df_i.stdout | str trim)

    print ""
    print "=== du -sh containerd overlayfs snapshotter (REAL usage) ==="
    let du_overlay = (do { orb vm_run [sudo bash -c $"du -sh ($OVERLAYFS_DIR) 2>/dev/null || echo 'not present'"] } | complete)
    print ($du_overlay.stdout | str trim)

    print ""
    print "=== du -sh containerd content store (REAL usage) ==="
    let du_content = (do { orb vm_run [sudo bash -c $"du -sh ($CONTENT_DIR) 2>/dev/null || echo 'not present'"] } | complete)
    print ($du_content.stdout | str trim)

    print ""
    print "=== du -sh hostpath-storage (registry PVC + DB PVCs — REAL usage) ==="
    let du_storage = (do { orb vm_run [sudo bash -c $"du -sh ($HOSTPATH_STORAGE) 2>/dev/null || echo 'not present'"] } | complete)
    print ($du_storage.stdout | str trim)

    print ""
    print "=== du -xsh / (REAL whole-root usage, one filesystem) ==="
    let du_root = (do { orb vm_run [sudo bash -c "du -xsh / 2>/dev/null"] } | complete)
    print ($du_root.stdout | str trim)

    print ""
    print "=== orb info (host-side OrbStack view of VM disk) ==="
    let info = (do { ^orb info $vm } | complete)
    print ($info.stdout | str trim)

    print ""
    print "=== GUEST vs HOST byte accounting ==="
    # Guest-side: real bytes used by containerd's overlayfs + content stores.
    # Host-side: real APFS blocks consumed by the OrbStack sparse data dir.
    # These two figures should be close in a fresh VM, but the host figure
    # is a MONOTONIC HIGH-WATER MARK of everything the VM has ever
    # allocated. Guest TRIM does not pass through to host hole-punching on
    # observed OrbStack versions (verified May 2026: 337 GiB host-side
    # post-`containerd-reset` + guest `fstrim`).
    let guest_bytes = (reclaim containerd_store_bytes)
    let host_bytes = (reclaim orbstack_img_bytes)
    let guest_gib = (($guest_bytes / 1024 / 1024 / 1024) | into int)
    let host_gib = (($host_bytes / 1024 / 1024 / 1024) | into int)
    print $"   guest containerd store: ($guest_gib) GiB [in-VM real usage]"
    if $host_bytes > 0 {
        let delta_gib = ((($host_bytes - $guest_bytes) / 1024 / 1024 / 1024) | into int)
        print $"   host OrbStack img file: ($host_gib) GiB [real APFS blocks]"
        print $"   delta:                  ($delta_gib) GiB allocated but never reclaimed by guest TRIM"
        print "   note: guest cleanup cannot shrink the host file on observed OrbStack versions."
    } else {
        print "   host OrbStack img file: unmeasured [OrbStack data dir not present or non-macOS]"
    }

    print ""
    print "--- INTERPRETATION ---"
    print "OrbStack's guest root is NOT a real ext4 fs. `df -h /` can report"
    print "~94% used while `du -xsh /` and `orb info` show only ~24G real."
    print "`df -i /` showing 0 inodes / dumpe2fs failing confirms the phantom."
    print "Trust du/orb-info. Do NOT lower kubelet's image-gc threshold:"
    print "under phantom-df that thrashes EARLIER."
    print ""
    print "OrbStack uses TWO storage backends, not one:"
    print "  • host Docker engine — stored as APFS clones in the OrbStack data"
    print "    dir, OUTSIDE data.img.raw. APFS deallocation works; `docker"
    print "    system prune` reclaims host disk immediately."
    print "  • data.img.raw — sparse VM disk backing microk8s and everything"
    print "    inside the guest. Guest TRIM does NOT pass through to host"
    print "    APFS hole-punching on observed OrbStack versions, so this"
    print "    file grows monotonically. Only `rm data.img.raw` (while"
    print "    OrbStack is quit) actually shrinks it."
    print ""
    print "=== Reclaim hierarchy ==="
    print "Layer 1 — host Docker engine (APFS clones, reclaims immediately):"
    print "  nu assets/nu/k8s/reclaim_disk.nu docker-prune"
    print "  ^^ try this FIRST when the OrbStack dir feels big"
    print ""
    print "Layer 2 — in-VM containerd unreferenced content (does NOT shrink"
    print "  the host data.img.raw, but keeps the guest's image-GC threshold"
    print "  from tripping kubelet thrash):"
    print "  nu assets/nu/k8s/reclaim_disk.nu registry-gc"
    print "  (`microk8s ctr -n k8s.io content prune references` is invoked"
    print "  automatically at the end of every deploy_all — no manual"
    print "  subcommand needed for that one.)"
    print ""
    print "Layer 3 — full reset (only when the sparse file itself has grown"
    print "  unbounded AND Layer 1 did not help):"
    print "  Guest-side (does NOT shrink the host file):"
    print "    nu assets/nu/k8s/reclaim_disk.nu containerd-reset --confirm"
    print "  Host-side (the ONLY path that actually shrinks data.img.raw):"
    print "    nu assets/nu/k8s/backup_pvc.nu backup"
    print "    osascript -e 'tell application \"OrbStack\" to quit'"
    print "    rm \"$HOME/Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw\""
    print "    open -a OrbStack"
    print "    nu assets/nu/k8s/install_orbstack_vm.nu"
    print "    nu assets/nu/k8s/setup_microk8s.nu"
    print "    nu assets/nu/k8s/backup_pvc.nu restore ~/dapple-backups/<latest>.tar.gz --confirm"
    print "    nu assets/nu/k8s/deploy_dapple_prompt_pods.nu"
    print "  See docs/books/journey-builder-guide/development/orbstack-setup.md."
}

# registry-gc — best-effort blob GC in the in-cluster registry pod.
def "main registry-gc" [] {
    log info "running in-cluster registry garbage-collect"
    _registry_gc
}

# docker-prune — Layer 1 reclaim. Prunes OrbStack's host-side Docker engine
# cache, which stores images/volumes/build-cache as APFS clones in the
# OrbStack data dir (NOT inside data.img.raw). APFS deallocation works
# here, so this reclaims host disk IMMEDIATELY — typically the largest
# single reclaimable bucket after a series of --docker deploys.
#
# `-a` removes ALL unused images (not just dangling); `-f` skips the
# interactive confirmation; `--volumes` includes orphaned volumes. The
# deploy script's --docker mode rebuilds-and-retags on every run, so any
# unused image really is unused.
#
# Operates on whichever Docker engine the `docker` CLI is talking to —
# OrbStack-Docker on macOS, Docker Desktop on macOS or Linux, native
# Linux Docker. The accumulation pattern is not OrbStack-specific, so
# Linux contributors using `--docker dev` benefit from the same prune
# as Mac contributors. Skipped only when `docker` is not on PATH.
def "main docker-prune" [] {
    let docker_check = (do { ^which docker } | complete)
    if $docker_check.exit_code != 0 {
        print "docker CLI not present on PATH — nothing to prune"
        return
    }

    print "Running `docker system prune -af --volumes` against the host Docker engine..."
    let result = (do { ^docker system prune -af --volumes } | complete)
    if $result.exit_code == 0 {
        # Echo Docker's own report (it includes the deleted-image list and
        # the "Total reclaimed space: X" footer, which is the figure the
        # operator wants to see).
        print ($result.stdout | str trim)
        let lines_with_match = ($result.stdout | lines | where { |l| ($l | str lowercase | str contains "total reclaimed space") })
        let reclaimed = if ($lines_with_match | is-empty) { "" } else { $lines_with_match | first | str trim }
        if ($reclaimed | is-not-empty) {
            log info $"docker system prune complete — ($reclaimed)"
        } else {
            log info "docker system prune complete — host Docker engine cache reclaimed"
        }
    } else {
        log error $"docker system prune failed: ($result.stderr | str trim)"
        error make { msg: "docker-prune: see stderr above" }
    }
}

# containerd-reset — the SAFE full reset. ALL io.containerd.* removed as ONE
# consistent operation. A PARTIAL cleanup (moving/removing only the
# overlayfs snapshotter dir) corrupts containerd metadata and every pod
# sandbox then fails with "missing parent ... bucket: not found" — that is
# WHY this wipes everything together.
def "main containerd-reset" [--confirm] {
    if not (_require_macos) { return }

    let vm = (orb vm_name)

    print "=== containerd-reset PLAN ==="
    print $"  Target VM:        ($vm)"
    print "  1. sudo microk8s stop"
    print $"  2. rm -rf ($CONTAINERD_ROOT)/io.containerd.* ($CONTAINERD_ROOT)/tmpmounts"
    print $"  3. mkdir -p ($OVERLAYFS_DIR)/snapshots"
    print "  4. sudo microk8s start"
    print ""
    print "  WARNING: this WIPES the local image cache. Every image re-pulls"
    print "  on next deploy (cold pulls — expect a slow first rollout). The"
    print "  in-cluster registry PVC and DB PVCs live on SEPARATE hostpath"
    print "  storage and are NOT touched by this reset."

    if not $confirm {
        print ""
        log warning "dry-run: pass --confirm to actually execute the reset"
        return
    }

    print ""
    log info "stopping microk8s"
    let stop = (do { orb vm_run [sudo microk8s stop] } | complete)
    if $stop.exit_code != 0 {
        log warning $"microk8s stop exited non-zero \(continuing — it may already be stopped\): ($stop.stderr | str trim)"
    }

    log info "removing ALL io.containerd.* and tmpmounts (single consistent operation)"
    # One shell invocation so the wipe is atomic from our side — never remove
    # only a subset (that is the metadata-corruption footgun documented above).
    let wipe_cmd = $"rm -rf ($CONTAINERD_ROOT)/io.containerd.* ($CONTAINERD_ROOT)/tmpmounts && mkdir -p ($OVERLAYFS_DIR)/snapshots"
    let wipe = (do { orb vm_run [sudo bash -c $wipe_cmd] } | complete)
    if $wipe.exit_code != 0 {
        log error $"containerd wipe failed: ($wipe.stderr | str trim)"
        error make { msg: "containerd-reset: wipe step failed — microk8s is stopped; inspect the VM before starting it" }
    }

    log info "starting microk8s"
    let start = (do { orb vm_run [sudo microk8s start] } | complete)
    if $start.exit_code != 0 {
        log error $"microk8s start failed: ($start.stderr | str trim)"
        error make { msg: "containerd-reset: microk8s start failed — inspect the VM" }
    }

    log info "containerd reset complete — images will re-pull on next deploy. Run: nu assets/nu/k8s/deploy_dapple_prompt_pods.nu"

    # Reset the shared reclaim state file so the deploy script's
    # deploys-since-reset counter starts fresh, the post-reset size is
    # captured, and the last_reset_epoch reflects this run.
    reclaim record_reset
}

# plan — read-only dry-run of the auto-reclaim plan the deploy script runs
# at the end of every `deploy_all`. Computes containerd byte size, reads
# the state file, and prints what WOULD happen (registry GC: would-skip or
# would-run; ctr prune: would-run; reset recommendation: yes/no + reason).
# Executes nothing and writes nothing. Useful for "is my containerd store
# getting big?" without side effects.
def "main plan" [] {
    if not (_require_macos) { return }

    let bytes = (reclaim containerd_store_bytes)
    let state = (reclaim read_state)
    let bytes_gib = (($bytes / 1024 / 1024 / 1024) | into int)

    # Thresholds (env-overridable — mirrors deploy_dapple_prompt_pods.nu).
    let high_water_override = ($env.DAPPLE_RECLAIM_HIGH_WATER_GIB? | default "")
    let high_water = if ($high_water_override | is-empty) {
        25 * 1024 * 1024 * 1024
    } else {
        try { ($high_water_override | into int) * 1024 * 1024 * 1024 } catch { 25 * 1024 * 1024 * 1024 }
    }
    let deploy_count_override = ($env.DAPPLE_RECLAIM_DEPLOY_COUNT? | default "")
    let deploy_limit = if ($deploy_count_override | is-empty) {
        25
    } else {
        try { $deploy_count_override | into int } catch { 25 }
    }
    let high_gib = (($high_water / 1024 / 1024 / 1024) | into int)

    let deploys_since = ($state.deploys_since_reset? | default 0)
    let last_reset = ($state.last_reset_epoch? | default 0)
    let size_at_last_reset = ($state.size_at_last_reset_bytes? | default 0)
    let last_reset_gib = (($size_at_last_reset / 1024 / 1024 / 1024) | into int)

    let state_path = (reclaim state_file_path)
    print "=== reclaim plan (dry-run — no side effects) ==="
    print $"  state file:                  ($state_path)"
    print $"  containerd store size:       ($bytes_gib) GiB [real bytes via du -sb]"
    print $"  size threshold:              ($high_gib) GiB"
    print $"  deploys since last reset:    ($deploys_since)"
    print $"  deploy-count threshold:      ($deploy_limit)"
    print $"  last reset epoch:            ($last_reset) [0 = never]"
    print $"  size at last reset:          ($last_reset_gib) GiB"

    print ""
    print "  Would auto-run (safe):"
    print "    1. registry garbage-collect  (with --dry-run probe; skipped if no orphaned blobs)"
    print "    2. microk8s ctr content prune references"

    let over_size = ($bytes > $high_water)
    let over_count = ($deploys_since >= $deploy_limit)
    let recommend = ($over_size or $over_count)

    print ""
    if $recommend {
        let size_reason = $"containerd store at ($bytes_gib) GiB [threshold ($high_gib) GiB]"
        let count_reason = $"($deploys_since) deploys since last reset [threshold ($deploy_limit)]"
        let reason = if $over_size and $over_count {
            $"($size_reason); ($count_reason)"
        } else if $over_size {
            $size_reason
        } else {
            $count_reason
        }
        print "  Would WARN — recommend containerd-reset (NOT auto-executed):"
        print $"    reason: ($reason)"
        print "    to reclaim: nu assets/nu/k8s/reclaim_disk.nu containerd-reset --confirm"
    } else {
        print "  No reset recommendation — both thresholds clear."
    }
}
