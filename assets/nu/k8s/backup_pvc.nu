#!/usr/bin/nu
#
# backup_pvc.nu — back up / restore the microk8s-hostpath Postgres PVCs so a
# VM re-provision is not data-loss-risky.
#
# WHY: the dev-cluster disk-leak recovery runbook requires deleting and
# recreating the OrbStack VM. The in-cluster registry / containerd state is
# disposable, but the Postgres PVCs (prompt-db `_sqlx_migrations` + all app
# data) are NOT. This codifies the manual tar dance done during the incident.
#
# Backups land under the macOS user home (~/dapple-backups). That directory
# is virtiofs-mounted two-way into the OrbStack VM at the same path, so it
# SURVIVES `orb delete` — the tarball is written from inside the VM but
# physically lives on the Mac host.
#
# Subcommands:
#   nu assets/nu/k8s/backup_pvc.nu backup
#       Scale prompt-db to 0 for a consistent
#       copy, tar the PVC backing dir, scale back to 1, then verify the
#       tarball from the Mac host (exists, gzip -t OK, non-empty listing).
#
#   nu assets/nu/k8s/backup_pvc.nu restore <tarball> [--deployment prompt-db]
#       Scale the target deployment to 0, wipe the CURRENT PVC backing dir
#       (UUID differs after a re-provision), untar, fix ownership, scale back
#       to 1. Postgres runs crash-recovery on first start — that is NORMAL.
#
# Same Postgres major version assumed across backup/restore (PG18). A
# cross-major restore would require pg_upgrade and is out of scope here.
#
# macOS-only: drives the OrbStack VM via the `orb` module (the PVC backing
# path is VM-internal). On Linux microk8s runs natively and this VM
# abstraction does not apply.

use std/log

use ../modules/orb

# microk8s hostpath-storage provisioner backing root (VM-internal path).
const HOSTPATH_STORAGE = "/var/snap/microk8s/common/default-storage"
# Substring every microk8s-hostpath PVC backing dir contains
# (`<ns>-<claim>-pvc-<uid>` → for `*-pvc` claims that yields `-pvc-pvc-`).
# Used as a safety assertion before any destructive `find -delete`.
const PVC_DIR_MARKER = "-pvc-pvc-"

def _require_macos []: nothing -> bool {
    if not (orb is_macos) {
        log warning "backup_pvc.nu is macOS-only — on Linux microk8s runs natively as a snap and this VM/virtiofs abstraction does not apply. Back up the hostpath dir directly on the host."
        return false
    }
    true
}

# Resolve the host-side backup directory (under $HOME, virtiofs-mounted into
# the VM at the same path so it survives `orb delete`).
def _backup_dir []: nothing -> string {
    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    let dir = ($home | path join "dapple-backups")
    mkdir $dir
    $dir
}

# Resolve the microk8s-hostpath PV backing dir for a given PVC. Returns the
# absolute VM-internal path string, or empty string if the PVC/PV is absent.
def _pvc_backing_path [pvc: string, namespace: string]: nothing -> string {
    # Find the PV bound to this claim, then read its hostPath.path (the
    # microk8s-hostpath provisioner sets .spec.hostPath.path). jsonpath
    # can't interpolate a nu var inside the single quoted token, so query
    # all PVs as JSON and filter in nu instead (robust + readable).
    let all = (do { kubectl get pv -o json } | complete)
    if $all.exit_code != 0 {
        return ""
    }
    let parsed = (try { $all.stdout | from json } catch { null })
    if $parsed == null {
        return ""
    }
    let match = (
        $parsed.items
        | where {|it|
            let cr = ($it.spec | get -o claimRef)
            # NOTE: `and` is a BINARY INFIX operator in nu — never start a
            # continuation line with it, or the parser treats it as a command
            # call and execution explodes with `Command 'and' not found` at
            # RUNTIME (nu-check does not catch this). Keep this single-line.
            ($cr != null) and (($cr | get -o name) == $pvc) and (($cr | get -o namespace) == $namespace)
        }
    )
    if ($match | is-empty) {
        return ""
    }
    let hp = ($match | first | get spec | get -o hostPath)
    if $hp == null {
        return ""
    }
    ($hp | get -o path | default "")
}

# Scale a deployment and wait for its pod to actually go away (replicas=0)
# so the on-disk Postgres data is in a consistent state for tar.
def _scale [deployment: string, namespace: string, replicas: int]: nothing -> nothing {
    let r = (do { kubectl scale $"deployment/($deployment)" -n $namespace $"--replicas=($replicas)" } | complete)
    if $r.exit_code != 0 {
        log warning $"scale ($deployment) -> ($replicas) exited non-zero: ($r.stderr | str trim)"
    }
}

def main [] {
    print "backup_pvc.nu — Postgres PVC backup/restore for safe VM re-provision"
    print ""
    print "Subcommands:"
    print "  backup                         Back up prompt-db"
    print "  restore <tarball> [--deployment prompt-db] [--confirm]   Restore a tarball into the current PVC. Without --confirm, prints a dry-run summary only; --confirm is required for the destructive wipe-and-restore."
    print ""
    print "Backups live under ~/dapple-backups (survives `orb delete` via virtiofs)."
}

# Back up one PVC: scale down (if running), tar inside the VM to the
# host-mounted backup dir, scale back up, verify from the Mac host.
def _backup_one [
    deployment: string,
    pvc: string,
    namespace: string,
    backup_dir: string,
] {
    let backing = (_pvc_backing_path $pvc $namespace)
    if ($backing | is-empty) {
        log warning $"PVC ($pvc) in namespace ($namespace) not found — skipping"
        return
    }
    log info $"($deployment): PVC ($pvc) backing dir = ($backing)"

    # Only do the scale dance if the deployment exists and has a running pod.
    let dep = (do { kubectl get $"deployment/($deployment)" -n $namespace } | complete)
    let dep_exists = ($dep.exit_code == 0)
    if $dep_exists {
        log info $"scaling ($deployment) to 0 for a consistent copy"
        _scale $deployment $namespace 0
        let waited = (do { kubectl wait --for=delete $"pod" -l $"app=($deployment)" -n $namespace --timeout=120s } | complete)
        if $waited.exit_code != 0 {
            log warning $"wait-for-delete on ($deployment) pods exited non-zero \(may already be gone\): ($waited.stderr | str trim)"
        }
    } else {
        log info $"($deployment) deployment absent — skipping scale dance, tarring backing dir as-is"
    }

    let stamp = (date now | format date "%Y%m%d-%H%M%S")
    let tarball = ($backup_dir | path join $"($deployment)-pgdata-($stamp).tar.gz")
    log info $"tarring ($backing) -> ($tarball) (inside VM, written to host-mounted path)"

    # tar runs INSIDE the VM (the backing path is VM-internal); the output
    # path is the virtiofs-mounted host dir so the artifact lands on the Mac.
    # --numeric-owner -p preserves the postgres uid/gid (999) across the
    # re-provision where the VM's /etc/passwd is freshly created.
    let tar_cmd = $"tar --numeric-owner -pczf '($tarball)' -C '($backing)' ."
    let tarred = (do { orb vm_run [sudo bash -c $tar_cmd] } | complete)
    if $tarred.exit_code != 0 {
        log error $"tar failed for ($deployment): ($tarred.stderr | str trim)"
        if $dep_exists {
            log info $"scaling ($deployment) back to 1"
            _scale $deployment $namespace 1
        }
        error make { msg: $"backup of ($deployment) failed at the tar step" }
    }

    if $dep_exists {
        log info $"scaling ($deployment) back to 1"
        _scale $deployment $namespace 1
    }

    # Verify FROM THE MAC HOST (not via orb) — the file is on the host fs.
    if not ($tarball | path exists) {
        error make { msg: $"backup verification failed: ($tarball) does not exist on the host" }
    }
    let gzt = (do { ^gzip -t $tarball } | complete)
    if $gzt.exit_code != 0 {
        error make { msg: $"backup verification failed: gzip -t rejected ($tarball)" }
    }
    let listing = (do { ^tar -tzf $tarball } | complete)
    let entry_count = (if $listing.exit_code == 0 { ($listing.stdout | lines | where {|l| ($l | str trim) != "" } | length) } else { 0 })
    if $entry_count < 1 {
        error make { msg: $"backup verification failed: ($tarball) lists 0 entries" }
    }
    let size = (ls $tarball | first | get size)
    log info $"verified backup ($tarball): gzip OK, ($entry_count) entries, size ($size)"
}

def "main backup" [] {
    if not (_require_macos) { return }

    let backup_dir = (_backup_dir)
    log info $"backup directory: ($backup_dir) [host-side, virtiofs-mounted into the VM]"

    let dapple_ns = ($env | get -o KUBE_DAPPLE_NAMESPACE | default "dapple-sandbox")

    # prompt-db is always expected.
    _backup_one "prompt-db" "prompt-db-pvc" $dapple_ns $backup_dir

    log info "backup complete"
}

def "main restore" [
    tarball: string,                       # path to a *.tar.gz produced by `backup`
    --deployment: string = "prompt-db",    # target deployment
    --namespace: string = "",              # defaults to KUBE_DAPPLE_NAMESPACE
    --pvc: string = "",                    # defaults: <deployment>-pvc
    --confirm,                             # required for the destructive wipe-and-restore — without it, dry-run summary only
] {
    if not (_require_macos) { return }

    if not ($tarball | path exists) {
        error make { msg: $"restore: tarball ($tarball) does not exist on the host" }
    }
    let abs_tarball = ($tarball | path expand)

    let ns = if ($namespace | is-not-empty) {
        $namespace
    } else {
        ($env | get -o KUBE_DAPPLE_NAMESPACE | default "dapple-sandbox")
    }
    let claim = if ($pvc | is-not-empty) { $pvc } else { $"($deployment)-pvc" }

    # Resolve the CURRENT backing dir — after a re-provision the
    # microk8s-hostpath UUID is different from the one in the backup.
    let backing = (_pvc_backing_path $claim $ns)
    if ($backing | is-empty) {
        error make { msg: $"restore: could not resolve backing dir for PVC ($claim) in namespace ($ns) — is the PVC bound? Deploy prompt-db first so the PVC exists." }
    }

    # SAFETY: never run `find -delete` on a path that doesn't look like a
    # microk8s-hostpath PVC dir. An empty/garbage path here would otherwise
    # delete the wrong tree.
    if not ($backing | str contains $PVC_DIR_MARKER) {
        error make { msg: $"restore: resolved backing dir ($backing) does not contain the expected microk8s-hostpath marker '($PVC_DIR_MARKER)' — refusing to wipe it" }
    }
    log info $"restore target: ($deployment) [ns ($ns), PVC ($claim)] backing dir ($backing)"

    if not $confirm {
        print "DRY RUN — pass --confirm to actually wipe and restore:"
        print $"  deployment: ($deployment) [ns ($ns)]"
        print $"  PVC: ($claim)"
        print $"  backing dir to be WIPED then populated: ($backing)"
        print $"  tarball: ($abs_tarball)"
        print "  steps: scale deployment to 0; find -mindepth 1 -delete the backing dir; untar with --strip-components=1; chown -R 999:999; chmod 700; scale deployment to 1"
        print ""
        print "Pass --confirm to proceed."
        return
    }

    log info $"scaling ($deployment) to 0"
    _scale $deployment $ns 0
    let waited = (do { kubectl wait --for=delete $"pod" -l $"app=($deployment)" -n $ns --timeout=120s } | complete)
    if $waited.exit_code != 0 {
        log warning $"wait-for-delete on ($deployment) pods exited non-zero \(may already be gone\): ($waited.stderr | str trim)"
    }

    # Wipe the current contents (NOT the dir itself — the PV mount expects it
    # to exist), then untar. --strip-components=1 drops the leading `./` the
    # backup's `-C <dir> .` produced. --numeric-owner -p restores the
    # postgres uid/gid; we re-chown anyway for safety.
    log info $"clearing ($backing) and untarring ($abs_tarball)"
    let restore_cmd = (
        $"find '($backing)' -mindepth 1 -delete"
        + $" && tar --numeric-owner -pxzf '($abs_tarball)' --strip-components=1 -C '($backing)'"
        + $" && chown -R 999:999 '($backing)'"
        + $" && chmod 700 '($backing)'"
    )
    let restored = (do { orb vm_run [sudo bash -c $restore_cmd] } | complete)
    if $restored.exit_code != 0 {
        log error $"restore data step failed: ($restored.stderr | str trim)"
        error make { msg: $"restore of ($deployment) failed — deployment is scaled to 0; inspect ($backing) before scaling back up" }
    }

    log info $"scaling ($deployment) back to 1"
    _scale $deployment $ns 1

    log info $"restore complete. ($deployment) Postgres will run crash-recovery on first start — this is NORMAL and expected. If restoring prompt-db, run `backup_pvc.nu restore` BEFORE deploy_dapple_core.nu so migrations see the restored _sqlx_migrations table."
}
