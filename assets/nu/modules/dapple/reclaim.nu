# Reclaim helpers — shared between deploy_dapple_prompt_pods.nu and
# reclaim_disk.nu so both files agree on the "how big is containerd?" answer
# and the per-deploy counter state.
#
# CRITICAL DESIGN RULE (from PR #488's originating incident):
# `df` is POISONED on OrbStack — guest root reports ~94% while real usage is
# ~24 GiB (`df -i /` shows 0 inodes; `dumpe2fs` fails — guest root is not
# real ext4). kubelet's image-GC trusts `df` and that is what cascaded the
# original incident. We measure real bytes via `du -sb`; we do NOT gate any
# decision on `df %` anywhere.
#
# Safe vs destructive asymmetry:
#  - `registry garbage-collect` and `ctr content prune references` are
#    non-destructive, idempotent — safe to auto-run every deploy_all.
#  - `containerd-reset` IS destructive — it wipes the local image cache and
#    forces a re-pull storm. That exact storm produced the
#    "missing parent ... bucket" corruption mid-incident. This module never
#    triggers the reset; it only signals when the operator should consider
#    running it manually.

use std/log

use ../orb

# State file lives under the user's home dir on the MAC. The deterministic
# counter (deploys_since_reset) and the byte snapshots are all there is —
# no `df` metrics in the state file by design.
def _home []: nothing -> string {
    if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
}

export def state_file_path []: nothing -> string {
    let dapple_home = ($env | get -o DAPPLE_HOME_DIR | default ((_home) | path join ".dapple"))
    $dapple_home | path join "reclaim-state.json"
}

# microk8s containerd state locations inside the VM. Kept in sync with the
# constants in reclaim_disk.nu; if they ever drift, fix here first.
const OVERLAYFS_DIR = "/var/snap/microk8s/common/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs"
const CONTENT_DIR = "/var/snap/microk8s/common/var/lib/containerd/io.containerd.content.v1.content"

# Read the persisted reclaim state. Returns a record with the four known
# fields; missing fields are tolerated (null-safe field access via `?` is
# used at every read site).
export def read_state []: nothing -> record {
    let path = (state_file_path)
    if not ($path | path exists) {
        return {}
    }
    try {
        open --raw $path | from json
    } catch {
        log warning $"reclaim state file at ($path) is unreadable — treating as empty"
        {}
    }
}

# Persist the reclaim state. Creates the parent dir if needed.
export def write_state [state: record]: nothing -> nothing {
    let path = (state_file_path)
    let parent = ($path | path dirname)
    mkdir $parent
    $state | to json | save -f $path
}

# Real bytes used by containerd's overlayfs snapshots + content store on the
# VM. Uses `du -sb` (real bytes) — NOT `df` (poisoned on OrbStack; see the
# module header). Best-effort: returns 0 on any failure so callers never
# abort a deploy on a heuristic.
#
# The `2>/dev/null` and `awk` are INSIDE the remote bash -c string and are
# executed by the guest's bash, so they are correct in this context
# (gotcha #5 only applies to redirects at the nushell level).
export def containerd_store_bytes []: nothing -> int {
    if not (orb is_macos) {
        return 0
    }
    let cmd = $"du -sb ($OVERLAYFS_DIR) ($CONTENT_DIR) 2>/dev/null | awk '{s+=$1}END{print s+0}'"
    let result = (do { orb vm_run [sudo bash -c $cmd] } | complete)
    if $result.exit_code != 0 {
        log debug $"containerd_store_bytes: du failed \(best-effort, returning 0\): ($result.stderr | str trim)"
        return 0
    }
    let trimmed = ($result.stdout | str trim)
    if ($trimmed | is-empty) {
        return 0
    }
    try {
        $trimmed | into int
    } catch {
        log debug $"containerd_store_bytes: could not parse [($trimmed)] as int — returning 0"
        0
    }
}

# Mark a successful containerd-reset in the state file. Called by
# reclaim_disk.nu's `containerd-reset --confirm` AFTER the reset succeeds,
# so the captured size reflects the freshly-cleaned store.
export def record_reset []: nothing -> nothing {
    let now_epoch = (date now | format date '%s' | into int)
    let post_reset_bytes = (containerd_store_bytes)
    let new_state = {
        last_reset_epoch: $now_epoch
        deploys_since_reset: 0
        size_at_last_reset_bytes: $post_reset_bytes
        size_at_last_check_bytes: $post_reset_bytes
    }
    write_state $new_state
    let post_gib = (($post_reset_bytes / 1024 / 1024 / 1024) | into int)
    log info $"reclaim state reset recorded — post-reset containerd store size: ($post_gib) GiB"
}

# OrbStack's VM data directory on macOS, holding `data.img.raw` (the sparse
# disk image) plus its siblings. Defined once so callers don't duplicate
# the literal. Resolved at call time via `_home` so this remains portable
# across macOS user accounts.
def _orbstack_data_dir []: nothing -> string {
    (_home)
    | path join "Library"
    | path join "Group Containers"
    | path join "HUAQ24HBR6.dev.orbstack"
    | path join "data"
}

# Real on-disk bytes consumed by the OrbStack VM sparse file. This is the
# HOST-SIDE metric — what `du -sb` on the .img.raw file actually allocates,
# NOT the guest's view of disk usage. Critical because OrbStack's sparse
# file grows monotonically: guest TRIM does not pass through to host
# hole-punching on observed versions (verified May 2026: 337 GiB host-side
# real blocks after `containerd-reset --confirm` followed by guest
# `fstrim -av` — the trim reclaimed nothing host-side). See
# docs/books/journey-builder-guide/development/orbstack-setup.md for the
# full motivation.
#
# Best-effort: returns 0 if the file/dir is not present (non-macOS hosts,
# or OrbStack not installed). Callers should treat a 0 return as "couldn't
# measure" and skip the host-side warning — never warn on it.
export def orbstack_img_bytes []: nothing -> int {
    if not (orb is_macos) {
        return 0
    }
    let dir = (_orbstack_data_dir)
    if not ($dir | path exists) {
        return 0
    }
    # Use BSD `du`'s `-A` block-count discipline: on macOS, `du -sk` prints
    # real-allocated kilobytes (1024-byte blocks) of the directory subtree,
    # which is exactly the host-side APFS block-count we want for the
    # sparse `data.img.raw`. We do NOT use `du -sb` because that flag is
    # GNU-only — macOS `du` does not support it. We multiply by 1024 in
    # nu rather than relying on shell math.
    let result = (do { ^du -sk $dir } | complete)
    if $result.exit_code != 0 {
        log debug $"orbstack_img_bytes: du failed [best-effort, returning 0]: ($result.stderr | str trim)"
        return 0
    }
    let line = ($result.stdout | str trim)
    if ($line | is-empty) {
        return 0
    }
    # `du -sk <dir>` output: "<kilobytes>\t<path>". Take the first field.
    let kb = ($line | split row "\t" | first | str trim)
    try {
        ($kb | into int) * 1024
    } catch {
        log debug $"orbstack_img_bytes: could not parse [($kb)] as int — returning 0"
        0
    }
}
