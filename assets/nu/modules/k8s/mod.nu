# modules/k8s/mod.nu
#
# Shared core for all k8s dev templates (node, rust, etc.)

use std/log

export module ./node_sandbox.nu

############################
# Basic utilities
############################
export def render [tmpl_path: path, vals: record] {
  def _to_token_key [k: string] {
    $k | str replace -r '[^A-Za-z0-9]+' '_' | str trim -c '_' | str uppercase
  }

  let tmpl_path = $env.DAPPLE_KUBE_MANIFESTS_DIR | path join $tmpl_path

  log debug $"rendering manifest from ($tmpl_path) with values: ($vals)";
  log debug $"manifest content before rendering: ($tmpl_path | open --raw)"

  let out = (
    $vals
    | transpose key value
    | reduce --fold (open --raw $tmpl_path) {|row, y|
        let key_token = (_to_token_key (($row | get key) | into string))
        let v =  ($row | get value)

        let t = $"{{($key_token)}}"
        log debug $"replacing token ($t) with value ($v)"
        if ($y | str contains $t) {
          $y | str replace -a $t $v
        } else {
          $y
        }
    }
  )
  log debug $"manifest content after rendering: ($out)"
  $out
}

export def apply_yaml [yaml: string] { $yaml | kubectl apply -f - }

def _is_macos [] { $nu.os-info.name | str contains "mac" }

# Run a microk8s subcommand. On Linux microk8s is installed natively as a
# snap and `microk8s` is on PATH. On macOS microk8s lives inside the
# OrbStack VM, so we dispatch via `orb run -m microk8s-vm sudo microk8s`.
#
# The arg list is passed through verbatim, so things like
# `microk8s enable cert-manager` and `microk8s status --wait-ready` work
# the same on both platforms.
export def microk8s [...args] {
    if (_is_macos) {
        # Lazy import so this module still loads on Linux callers that
        # never reach the macOS branch.
        use ../orb
        ^orb run -m (orb vm_name) sudo microk8s ...$args
    } else {
        ^microk8s ...$args
    }
}

# Docker Desktop is always used on macOS. On Linux, native dockerd is the
# default; set DAPPLE_DOCKER_DESKTOP=true to opt in to Docker Desktop paths.
def _uses_docker_desktop [] {
    (_is_macos) or ($env.DAPPLE_DOCKER_DESKTOP? | default "" | str trim) == "true"
}

# Ensure host paths are visible inside the MicroK8s VM.
#
# On Linux, MicroK8s runs natively and hostPath volumes work directly
# against the host filesystem — this function is a no-op.
#
# On macOS under OrbStack, every path under `/Users/$USER` is
# auto-mounted two-way via virtiofs at the same path inside the VM.
# Anything under the user home directory is therefore already visible
# without per-path registration. This function logs a warning if
# callers pass paths outside `/Users/$USER` (which would not be
# auto-mounted) but does not attempt to mount them — there is no
# explicit per-path mount step under OrbStack.
#
# The signature and exported name are preserved so existing call sites
# in the deploy scripts continue to compile. The function body is now a
# logging stub.
export def ensure_vm_mounts [paths: list<string>] {
    if not (_is_macos) {
        return
    }

    # OrbStack auto-mounts /Users/$USER. Surface a warning if a caller
    # asks us to ensure a path outside that tree, since silently
    # ignoring it could mask a real configuration mistake.
    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }

    for source_path in $paths {
        if ($source_path | is-empty) {
            continue
        }
        if not ($source_path | path exists) {
            log debug $"ensure_vm_mounts: skipping non-existent path: ($source_path)"
            continue
        }
        let expanded = ($source_path | path expand)
        if not ($expanded | str starts-with $home) {
            log warning $"ensure_vm_mounts: ($source_path) is outside ($home); OrbStack only auto-mounts the user home directory. Pods referencing this path may fail."
        } else {
            log debug $"ensure_vm_mounts: ($source_path) is auto-mounted by OrbStack [no action needed]"
        }
    }
}

# Pin cluster CoreDNS's upstream instead of letting it inherit /etc/resolv.conf.
#
# The node's real resolv.conf names a LOOPBACK stub — 127.0.0.1 with dnsmasq,
# 127.0.0.53 with systemd-resolved — and a pod has its own network namespace, so
# that address is dead inside one. Handing it to pods is a guaranteed
# cluster-wide DNS outage. This is what kubelet's --resolv-conf exists for, and
# it is why the override is structural rather than a workaround for any one
# host's DNS being flaky. [The previous comment blamed stale entries in the
# MicroK8s VM after DHCP lease changes — a multipass-era symptom that outlived
# its cause and hid the real reason.]
#
# Belt-and-braces, deliberately: CoreDNS runs dnsPolicy Default, so its
# /etc/resolv.conf is already whatever kubelet --resolv-conf points at, and
# configure_masqed_dns points that at assets/k8s/config/resolv.conf — which
# names these same two servers. Both exist so a cluster whose kubelet args were
# never patched still gets a working upstream. Keep the two in sync.
#
# This sets the UPSTREAM only; it adds no zone. Nothing in this repo teaches
# cluster CoreDNS about kube.dappleai.dev, so pods cannot resolve the dev-zone
# ingress hostnames and are expected to use Service names. See
# environments.nu's internal_api_url.
export def patch_coredns_upstream [] {
    log info "checking CoreDNS upstream DNS configuration"

    # Wait for the configmap to appear (microk8s enable dns is async)
    mut attempts = 0
    let max_attempts = 30
    loop {
        $attempts = $attempts + 1
        let result = (do { kubectl get configmap coredns -n kube-system } | complete)
        if $result.exit_code == 0 {
            break
        }
        if $attempts >= $max_attempts {
            log error "CoreDNS configmap not found after 30 attempts"
            error make { msg: "Timed out waiting for CoreDNS configmap" }
        }
        log debug $"waiting for CoreDNS configmap attempt ($attempts)/($max_attempts)..."
        sleep 2sec
    }

    # Get the current Corefile
    let corefile = (kubectl get configmap coredns -n kube-system -o "jsonpath={.data.Corefile}")

    if not ($corefile | str contains "forward . /etc/resolv.conf") {
        log info "CoreDNS already using direct upstream DNS, skipping patch"
        return
    }

    log info "patching CoreDNS: replacing forward . /etc/resolv.conf with forward . 1.1.1.1 8.8.8.8"
    let patched = ($corefile | str replace "forward . /etc/resolv.conf" "forward . 1.1.1.1 8.8.8.8")

    # Round-trip the full configmap to preserve all data keys (Corefile, NodeHosts, etc.)
    let cm = (kubectl get configmap coredns -n kube-system -o json | from json)
    let cm_patched = ($cm | update data.Corefile $patched)
    let tmp = (^mktemp | str trim)
    $cm_patched | to json | save -f $tmp
    let apply_result = (do { kubectl apply -f $tmp } | complete)
    if $apply_result.exit_code != 0 {
        rm $tmp
        log error $"failed to apply patched CoreDNS configmap: ($apply_result.stderr)"
        error make { msg: "failed to apply patched CoreDNS configmap", stdout: $apply_result.stdout, stderr: $apply_result.stderr }
    }
    rm $tmp

    # Restart CoreDNS pods to pick up the change
    log info "restarting CoreDNS deployment"
    let restart_result = (do { kubectl rollout restart deployment/coredns -n kube-system } | complete)
    if $restart_result.exit_code != 0 {
        log error $"failed to restart CoreDNS deployment: ($restart_result.stderr)"
        error make { msg: "failed to restart CoreDNS deployment", stdout: $restart_result.stdout, stderr: $restart_result.stderr }
    }
    # 300s, not 60s: on a freshly-provisioned cluster CoreDNS is a cold image
    # pull and 60s times out spuriously even though the pod comes healthy
    # seconds later.
    let status_result = (do { kubectl rollout status deployment/coredns -n kube-system --timeout=300s } | complete)
    if $status_result.exit_code != 0 {
        # rollout-status can time out while the deployment is actually
        # Available (slow image pull, then ready). Cross-check the real
        # availableReplicas before aborting — only error if it's genuinely
        # not available.
        let avail = (do { kubectl get deployment coredns -n kube-system -o "jsonpath={.status.availableReplicas}" } | complete)
        let avail_str = ($avail.stdout | str trim)
        let avail_n = (if ($avail_str =~ '^[0-9]+$') { ($avail_str | into int) } else { 0 })

        let desired = (do { kubectl get deployment coredns -n kube-system -o "jsonpath={.spec.replicas}" } | complete)
        let desired_str = ($desired.stdout | str trim)
        let desired_n = (if ($desired_str =~ '^[0-9]+$') { ($desired_str | into int) } else { 1 })

        if $avail_n >= $desired_n and $avail_n >= 1 {
            log warning $"CoreDNS rollout-status timed out but the deployment is fully Available [availableReplicas=($avail_n) of desired=($desired_n)] — continuing"
            return
        }
        log error $"CoreDNS rollout did not complete successfully: ($status_result.stderr)"
        error make { msg: "CoreDNS rollout did not complete successfully", stdout: $status_result.stdout, stderr: $status_result.stderr }
    }

    log info "CoreDNS upstream DNS patched successfully"
}

def _norm_common [cfg_or_name: any] {
  if ($cfg_or_name | describe | str contains "record") {
    let name  = ($cfg_or_name.name | into string)
    let ns    = (($cfg_or_name | get -o namespace) | default "dapple-sandbox" | into string)
    let base  = (($cfg_or_name | get -o resource)  | default $"($name)"    | into string)
    let lkey  = (($cfg_or_name | get -o label_key) | default "app"             | into string)
    let lval  = (($cfg_or_name | get -o label_val) | default $base             | into string)
    {
      name: $name,
      namespace: $ns,
      resource: $base,
      label_key: $lkey,
      label_val: $lval
    }
  } else {
    let name = ($cfg_or_name | into string)
    { name: $name, namespace: "dapple-sandbox", resource: $"($name)", label_key: "app", label_val: $"($name)" }
  }
}

export def delete [cfg_or_name: any] {
  let c = (_norm_common $cfg_or_name)
  # Try common workload types; ignore if not present.
  kubectl delete deploy $c.resource -n $c.namespace | ignore
  kubectl delete statefulset $c.resource -n $c.namespace | ignore
  kubectl delete daemonset $c.resource -n $c.namespace | ignore
  kubectl delete svc $c.resource -n $c.namespace | ignore
  # Optional: ConfigMaps/Secrets if your template creates named ones:
  kubectl delete configmap $c.resource -n $c.namespace | ignore
  kubectl delete secret $c.resource -n $c.namespace | ignore
}

export def logs [cfg_or_name: any, --follow (-f), --container (-c): string = ""] {
  let c = (_norm_common $cfg_or_name)
  let selector = $"($c.label_key)=($c.label_val)"
  let pod = (kubectl get pods -n $c.namespace -l $selector -o name
    | lines | first | default "" | str replace "pod/" "")
  if ($pod == "") {
    print $"No pod found for selector ($selector) in ns ($c.namespace)"
    return
  }
  if ($container != "") {
    if ($follow) { kubectl logs -f -n $c.namespace -c $container $pod } else { kubectl logs -n $c.namespace -c $container $pod }
  } else {
    if ($follow) { kubectl logs -f -n $c.namespace $pod } else { kubectl logs -n $c.namespace $pod }
  }
}

# Wait for rollout; default kind is Deployment, can be overridden in cfg
export def rollout_status [
  cfg_or_name: any,
  --kind: string = "deployment",     # deployment|statefulset|daemonset
  --timeout: string = "300s"         # 300s default: fresh clusters cold-pull every image; 90s timed out spuriously
] {
  let c = (_norm_common $cfg_or_name)
  let resource_name = $"($kind)/($c.resource)"
  let success = (try {
    kubectl rollout status $resource_name -n $c.namespace --timeout $"($timeout)" | ignore
    true
  } catch {
    false
  })
  if not $success {
    log warning $"Rollout status check for ($resource_name) in namespace ($c.namespace) timed out or failed."
    log warning $"Deployment may still be in progress. Check status with: kubectl get pods -n ($c.namespace) -l app=($c.resource)"
  }
  $success
}

export def rollout_restart [
  cfg_or_name: any,
  --kind: string = "deployment"
] {
  let c = (_norm_common $cfg_or_name)
  let resource_name = $"($kind)/($c.resource)"
  kubectl rollout restart $resource_name -n $c.namespace
}

export def register_local_certs [name: string] {
    let certs_dir = $env.KUBE_CERTS_DIR

    if not ($certs_dir | path exists) {
      mkdir $certs_dir
    } else if ($certs_dir | path type) != 'dir' {
      error make { msg: $"($certs_dir) exists but is not a directory" }
    }

    let crt = kubectl -n cert-manager get secret local-dev-root-ca -o 'jsonpath={.data.tls\.crt}' | base64 -d
    log debug $"registering local cert for ($name) from ($crt)"
    let crt_name = $env.KUBE_CERTS_DIR | path join $"($name).crt"
    log debug $"writing cert to ($crt_name)"
    $crt | save -f $crt_name

    if (_is_macos) {
        log debug "detected macos, adding cert to system keychain"
        sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain $crt_name
    } else {
        log debug "detected linux, adding cert to system CA store"
        sudo cp $crt_name /usr/local/share/ca-certificates/
        log debug "file copied, updating CA store"
        sudo update-ca-certificates

        # Linux: Chromium does NOT read the system CA store. It reads NSS
        # databases under the user's home. Snap-confined Chromium reads
        # from its own home under ~/snap/chromium. Register the cert in
        # each NSS db we find so browser TLS validation keeps working
        # after CA rotation. macOS keychain integration covers this on Mac.
        _register_nss_cert $crt_name
    }
}

# Linux-only. Install (or replace) the local-dev-root-ca in every NSS
# database we know about, so Chromium-family browsers trust the rotated CA.
def _register_nss_cert [crt_path: path] {
    let nickname = "local-dev-root-ca"
    let snap_chromium = $env.HOME | path join "snap" "chromium" "current" ".pki" "nssdb"
    let nss_paths = [
        ($env.HOME | path join ".pki" "nssdb")
        $snap_chromium
    ]

    let certutil_path = (which certutil | get path? | default [])
    if ($certutil_path | is-empty) {
        log warning "certutil not found; install libnss3-tools to register the CA with Chromium NSS dbs (browser TLS will fail until then)"
        return
    }

    for db in $nss_paths {
        if not ($db | path exists) {
            log debug $"NSS db not present at ($db), skipping"
            continue
        }
        let db_arg = $"sql:($db)"
        log debug $"registering ($nickname) in NSS db ($db)"
        # Drop any prior entry under this nickname so re-running on
        # rotation replaces the stale cert (certutil -A fails if the
        # nickname already exists).
        do --ignore-errors { ^certutil -D -d $db_arg -n $nickname } | ignore
        ^certutil -A -d $db_arg -n $nickname -t "C,," -i $crt_path
    }
}

# Make *.kube.dappleai.dev resolve on the developer's own machine.
#
# The names are served by the in-cluster dev-dns CoreDNS pod on NodePort 30053,
# which wildcards the whole zone to the node IP. Nothing in the cluster helps
# the HOST resolve them — that is per-workstation config, and it is what
# DAPPLE_PROMPT_API_URL, DATABASE_URL, the image registry and the browser all
# depend on.
#
# Both platforms are handled here. This used to be macOS-only: on Linux it
# logged "macOS-only, skipping on Linux" and returned, so a Linux workstation
# finished setup with no way to resolve any dev hostname, and every subsequent
# failure looked like a broken cluster rather than missing resolver config.
#
#   macOS  /etc/resolver/<domain>, pointing at the OrbStack VM's IP. Written
#          once by install_orbstack_vm.nu at provisioning and again here after
#          dev-dns is deployed; under OrbStack the VM IP does not drift, so the
#          file needs no ongoing maintenance.
#   Linux  systemd-resolved drop-in, pointing at 127.0.0.1 — microk8s runs
#          natively there, so the NodePort is on the host already.
#
# `vm_ip` is only meaningful on macOS; it is looked up when not supplied.
#
# PROBES BEFORE BRANCHING. If the zone already resolves there is nothing to do,
# and doing it anyway costs a sudo prompt and a resolver restart on every run of
# setup_microk8s.nu. It also keeps this honest on a host whose DNS is served by
# something we don't manage — the question that matters is "does the zone
# resolve", not "did we configure it".
export def setup_host_resolver [vm_ip: string = ""] {
    let domain = ($env | get -o KUBE_HOST | default "kube.dappleai.dev")

    if (_dev_zone_resolves $domain) {
        log info $"*.($domain) already resolves — leaving host DNS alone"
        return
    }

    if (_is_macos) {
        _setup_host_resolver_macos $vm_ip
    } else {
        _setup_host_resolver_linux
    }
}

# One lookup against the dev zone.
#
# dev-dns wildcards the zone, so ANY name under it answers — this does not
# depend on a particular Service existing.
#
# Platform-dispatched, because the probe has to go through the SAME resolver an
# application would use:
#
#   Linux  `getent hosts` — glibc's NSS path. It does NOT exist on macOS; the
#          `getent` an interactive zsh appears to have is a shell FUNCTION that
#          greps /etc/hosts, so it is both invisible to nu and wrong for this.
#          Probing with it unconditionally aborted setup_microk8s.nu on every
#          Mac at `setup_host_resolver`, before the cert issuer, the registry,
#          prompt-db and redis were ever applied.
#   macOS  `dscacheutil`, which asks mDNSResponder — the only thing that honours
#          /etc/resolver. `dig` / `host` / `nslookup` query the configured
#          nameservers directly and IGNORE per-domain resolver files, so they
#          report failure on a host where curl and the browser resolve the zone
#          perfectly. dscacheutil exits 0 either way, so the OUTPUT is the
#          signal: a resolved name prints an `*_address:` line, a failed one
#          prints nothing.
#
# Both branches guard on `which` so a missing tool reads as "inconclusive" — we
# fall through and configure, which is idempotent — instead of raising. `complete`
# does not catch a command-not-found; that is a nu-level error, not an exit code.
def _dev_zone_resolves [domain: string]: nothing -> bool {
    let name = $"dev-dns-probe.($domain)"

    if (_is_macos) {
        if (which dscacheutil | is-empty) {
            return false
        }
        let probe = (do { ^dscacheutil -q host -a name $name } | complete)
        return ($probe.stdout | str contains "_address:")
    }

    if (which getent | is-empty) {
        return false
    }
    (do { ^getent hosts $name } | complete).exit_code == 0
}

# Route just this suffix to the dev-dns NodePort, leaving every other lookup on
# the machine's normal resolvers.
#
# `Domains=~<suffix>` is what makes the global `DNS=` line safe. Without the
# tilde-prefixed routing domain it would capture ALL DNS, and the symptom would
# be image pulls and crates.io failing — nothing that points back here.
def _setup_host_resolver_linux [] {
    let domain = ($env | get -o KUBE_HOST | default "kube.dappleai.dev")
    let drop_in_dir = "/etc/systemd/resolved.conf.d"
    let drop_in = ($drop_in_dir | path join "dapple-dev-dns.conf")

    # Only reached when the zone does NOT already resolve — setup_host_resolver
    # probed first — so this is a genuine "you have no dev DNS" state, not a
    # host that simply resolves it some other way.
    let resolved_active = ((do { ^systemctl is-active systemd-resolved } | complete).exit_code == 0)
    if not $resolved_active {
        log warning $"*.($domain) does not resolve, and systemd-resolved is not active on this host"
        log warning "  so there is no drop-in for dapple to write. Either enable it:"
        log warning "    sudo systemctl unmask --now systemd-resolved"
        log warning $"  or point your own resolver at 127.0.0.1:30053 for *.($domain)."
        log warning "  Until one of those, dev hostnames [registry, api, ui] will not resolve."
        return
    }

    let content = $"# Managed by dapple setup_microk8s.nu — *.($domain) via the in-cluster
# dev-dns NodePort. `Domains=~` is a ROUTING domain: only this suffix is sent
# here, everything else keeps using your normal resolvers.
[Resolve]
DNS=127.0.0.1:30053
Domains=~($domain)
"

    # A correct drop-in that still doesn't resolve means dev-dns is down, not
    # that the drop-in is wrong — rewriting it would change nothing. Report and
    # return rather than restarting resolved for no reason.
    let existing = (do { ^cat $drop_in } | complete)
    if $existing.exit_code == 0 and ($existing.stdout == $content) {
        log warning $"($drop_in) is already correct, but *.($domain) does not resolve"
        log warning $"  check the dev-dns pod: kubectl get pods -n ($env.KUBE_DAPPLE_NAMESPACE) -l app=dev-dns"
        return
    }

    # Probe non-interactive sudo the way the macOS branch does. Three sudo
    # calls follow; with no TTY [a hook, CI, a backgrounded deploy] sudo cannot
    # prompt, and a partial run leaves the drop-in written but resolved never
    # restarted — which then fails the DNS probe below and blames the cluster.
    if ((do { ^sudo -n true } | complete).exit_code != 0) {
        log warning "non-interactive sudo unavailable — the resolver write may prompt or fail; verifying after the attempt"
    }

    log info $"configuring systemd-resolved for *.($domain) via 127.0.0.1:30053"
    ^sudo mkdir -p $drop_in_dir
    $content | ^sudo tee $drop_in out+err> /dev/null
    ^sudo systemctl restart systemd-resolved

    # Read the drop-in back before blaming DNS. A failed sudo write and an
    # unreachable dev-dns pod produce the same probe failure, and only one of
    # them is fixed by looking at the cluster.
    let readback = (do { ^cat $drop_in } | complete)
    if $readback.exit_code != 0 or ($readback.stdout != $content) {
        log error $"could not write ($drop_in). Write it by hand with exactly this content:"
        print $content
        log error $"  then: sudo systemctl restart systemd-resolved"
        error make { msg: $"resolver drop-in ($drop_in) was not written — see log" }
    }

    _verify_dev_zone_resolves $domain
}

# Confirm the dev zone answers after we wrote the drop-in, with a bounded retry.
#
# WARNS, does not raise. This mirrors install_microk8s.nu's configure_cluster_dns
# — the copy of this logic that has actually run on Ubuntu 24.04 — which logs a
# warning and continues. Raising here kills setup_microk8s.nu two-thirds of the
# way through, before the cert issuer, register_local_certs, the registry,
# prompt-db and redis; a host with no dev DNS is a nuisance, a half-built
# cluster is worse. A failed WRITE still raises: that is a different condition,
# it is the operator's to fix, and the macOS branch treats it the same way.
#
# The retry is not politeness. A Deployment reporting Ready is necessary but not
# sufficient: the NodePort's iptables rules and systemd-resolved's own cache
# each settle a beat later, and a one-shot probe raced them.
def _verify_dev_zone_resolves [domain: string] {
    mut attempt = 0
    while $attempt < 10 {
        if (_dev_zone_resolves $domain) {
            log info $"host resolver verified — *.($domain) resolves"
            return
        }
        $attempt = $attempt + 1
        if $attempt < 10 { sleep 2sec }
    }

    log warning $"*.($domain) still does not resolve after configuring systemd-resolved."
    log warning "  the drop-in was written and verified, so this is most likely the cluster:"
    log warning $"    kubectl get pods -n ($env.KUBE_DAPPLE_NAMESPACE) -l app=dev-dns"
    log warning "    resolvectl status | grep -A2 'DNS Servers'"
    log warning "  continuing — setup finishes, but dev hostnames will not resolve until this is fixed."
}

def _setup_host_resolver_macos [supplied_ip: string] {
    # Look the VM IP up when the caller did not supply one, so this is callable
    # with no arguments on either platform.
    let vm_ip = if ($supplied_ip | is-not-empty) {
        $supplied_ip
    } else {
        use ../orb
        (orb vm_ip)
    }

    let resolver_dir = "/etc/resolver"
    let resolver_file = $"($resolver_dir)/kube.dappleai.dev"
    let content = $"nameserver ($vm_ip)\nport 30053\n"

    # Fast path: if the file already exists and is byte-identical to the
    # desired content, there is nothing to do (and no sudo needed).
    if ($resolver_file | path exists) {
        let existing = (do { ^cat $resolver_file } | complete)
        if $existing.exit_code == 0 and ($existing.stdout == $content) {
            log info $"resolver already correct [($vm_ip)]"
            return
        }
    }

    # Ensure /etc/resolver/ exists
    if not ($resolver_dir | path exists) {
        log info $"creating ($resolver_dir)"
        ^sudo mkdir $resolver_dir
    }

    # Probe whether sudo can run non-interactively. With no TTY (background /
    # CI) sudo can't prompt; the old `| ignore` swallowed the failure and the
    # script continued with a STALE resolver — and after a VM delete+recreate
    # the VM IP changes, so the host silently loses *.kube.dappleai.dev. We
    # now verify the write and fail LOUD with a paste-ready manual command.
    let sudo_ok = ((do { ^sudo -n true } | complete).exit_code == 0)
    if not $sudo_ok {
        log warning "non-interactive sudo unavailable — the resolver write may prompt or fail; verifying after the attempt"
    }

    log info $"writing resolver file ($resolver_file) with nameserver ($vm_ip)"
    let w = (do { $content | ^sudo tee $resolver_file } | complete)

    # Read back and verify the file actually contains the current VM IP.
    let readback = (do { ^cat $resolver_file } | complete)
    let wrote_ok = ($w.exit_code == 0)
    let verified = ($readback.exit_code == 0 and ($readback.stdout == $content))

    if (not $wrote_ok) or (not $verified) {
        let fix = $"sudo mkdir -p /etc/resolver && sudo bash -c 'printf \"nameserver ($vm_ip)\\nport 30053\\n\" > /etc/resolver/kube.dappleai.dev'"
        log error $"Could not write /etc/resolver/kube.dappleai.dev — run manually: ($fix)"
        error make { msg: "host resolver write failed (non-interactive sudo?) — see log for the manual command" }
    }

    log info $"resolver file written and verified [($vm_ip)]"
}

export def configure_masqed_dns [] {
    #TODO: might as well add in the pieces where we setup /etc/resolv.conf and turn off resolvd
    if (_is_macos) {
       configure_macos_masqed_dns
    } else {
       configure_linux_masqed_dns
    }
}

export def configure_macos_masqed_dns [] {
    log info "macOS detected, applying kubelet resolv.conf patch via orb"

    # Lazily import the orb module so this file can still load on Linux
    # callers that never reach this branch.
    use ../orb

    let vm_name = (orb vm_name)
    let resolv_src = $env.DAPPLE_KUBE_ASSETS_DIR | path join "config" | path join "resolv.conf"
    let vm_resolv_path = "/etc/dapple-resolv.conf"
    let kubelet_args = "/var/snap/microk8s/current/args/kubelet"
    let new_line = $"--resolv-conf=($vm_resolv_path)"

    # Copy project resolv.conf directly into /etc/. The source lives under
    # the project (which is under /Users/$USER, auto-mounted into the VM),
    # so `orb vm_push` resolves to a single `orb run sudo cp src dst`.
    log info $"copying resolv.conf into ($vm_name):($vm_resolv_path)"
    orb vm_push $resolv_src $vm_resolv_path

    # Read current kubelet args from inside the VM (requires sudo)
    let content = (orb vm_run [sudo cat $kubelet_args] | into string)

    # Guard: refuse to proceed if the file read returned empty — that would
    # destroy the existing kubelet args on write-back.
    if ($content | str trim) == "" {
        log error $"kubelet args file ($kubelet_args) appears empty; aborting to avoid data loss"
        error make { msg: $"Failed to read kubelet args from ($vm_name):($kubelet_args)" }
    }

    let patched = match ($content | str contains "--resolv-conf=") {
        true => {
            log info "kubelet args already contain --resolv-conf, updating it"
            $content | str replace -r '(?m)^--resolv-conf=.*$' $new_line
        }
        false => {
            log info "kubelet args do not contain --resolv-conf, appending it"

            let base = if not ($content | str ends-with (char nl)) {
              $content + (char nl)
            } else {
              $content
            }

            $base + $new_line + (char nl)
        }
    }

    # Skip write and restart if nothing changed
    if $patched == $content {
        log info "kubelet args already up to date, skipping restart"
        return
    }

    # NOTE (kubelet image-GC threshold — deliberate non-change):
    # We intentionally do NOT add `--image-gc-high-threshold` /
    # `--image-gc-low-threshold` to the kubelet args here. Under OrbStack's
    # phantom-df the guest root reports ~94% used while real usage (du -x /,
    # `orb info`) is a fraction of that. kubelet's image GC keys off that
    # bogus percentage, so LOWERING the high-threshold makes kubelet thrash
    # EARLIER on a false signal — counterproductive. The real prevention is
    # reducing REAL disk usage: registry GC + a full containerd reset. See
    # assets/nu/k8s/reclaim_disk.nu and the "Dev-cluster disk exhaustion &
    # recovery" section of docs/books/journey-builder-guide/development/
    # orbstack-setup.md. Thresholds are left at Kubernetes defaults on purpose.

    # Stage the patched content under $HOME so it's visible inside the VM
    # via OrbStack's auto-mount, then `orb vm_push` (which is sudo cp from
    # the auto-mount path to the real Linux destination).
    let tmp = (orb mktemp_in_home --prefix "kubelet-args")
    $patched | save -f $tmp
    orb vm_push $tmp $kubelet_args
    rm $tmp
    orb vm_run [sudo chown root:microk8s $kubelet_args]
    orb vm_run [sudo chmod 660 $kubelet_args]

    # Restart kubelet so it picks up the change
    log info "restarting kubelite in VM"
    orb vm_run [sudo systemctl restart snap.microk8s.daemon-kubelite.service]
}

export def configure_linux_masqed_dns [] {
    log info "linux detected, applying kubelet resolv.conf patch"

    # patch-kubelet-resolv.nu
    let kubelet_args = "/var/snap/microk8s/current/args/kubelet"
    let resolv_path  = $env.DAPPLE_KUBE_ASSETS_DIR | path join "config" | path join "resolv.conf"
    let new_line     = $"--resolv-conf=($resolv_path)"

    # read file (empty if it doesn't exist yet)
    let content = (try { open $kubelet_args | into string } catch { "" })

    let patched = match ($content | str contains "--resolv-conf=") {
        true => {
            log info "kubelet args already contain --resolv-conf, updating it"
            $content | str replace -r '(?m)^--resolv-conf=.*$' $new_line
        }
        false => {
            log info "kubelet args do not contain --resolv-conf, appending it"

            let base = if ($content == "" or not ($content | str ends-with (char nl))) {
              $content + (char nl)
            } else {
              $content
            }

            $base + $new_line + (char nl)
        }
    }

     # write atomically with sudo
     let tmp = (mktemp -t kubelet-args.XXXXXX)
     $patched | save -f $tmp
     ^sudo mv $tmp $kubelet_args

     # restart kubelet so it picks up the change
     ^sudo systemctl restart snap.microk8s.daemon-kubelite.service
}

export def setup_docker_registry_cert [host_name: string, crt_name: string] {
    # Refresh the local CA first when the cluster has rotated it. Copying a
    # stale file into Docker's trust dir is worse than not running at all: it
    # looks like the trust was configured.
    _ensure_local_ca_current
    # The node's containerd trusts its own copy of the CA. A rotation that
    # refreshes only the host leaves every image pull failing x509 while the
    # deploy itself reports success.
    _ensure_node_ca_current

    let crt_path = $env.KUBE_CERTS_DIR | path join $crt_name
    let registry_host = if $host_name == "" { $env.KUBE_REGISTRY_HOST } else { $host_name }

    if (_uses_docker_desktop) {
        # Docker Desktop path: ~/.docker/certs.d/{registry}/ca.crt
        let user_target_dir = $env.DOCKER_DIR | path join "certs.d" | path join $registry_host
        let user_target_crt = $user_target_dir | path join "ca.crt"

        if not ($user_target_dir | path exists) {
          mkdir $user_target_dir
        } else if ($user_target_dir | path type) != 'dir' {
          error make { msg: $"($user_target_dir) exists but is not a directory" }
        }
        cp $crt_path $user_target_crt

        log info $"Docker Desktop certificate configured at ($user_target_crt)"

        # macOS-only: ensure cert is in the system keychain. The previous
        # implementation was buggy — it ran `security … | complete` then
        # tried to read `$in.exit_code`, but `$in` inside a try block
        # refers to the block's piped input, not the previous statement's
        # result. So the check always errored, was caught, and returned
        # false — meaning every deploy run re-prompted for sudo to add a
        # cert that was already there. Fixed by binding the complete
        # record to a let.
        if (_is_macos) {
            let cert_check = (do { ^security find-certificate -c "local-dev-root-ca" -a /Library/Keychains/System.keychain } | complete)
            let cert_in_keychain = ($cert_check.exit_code == 0)

            if not $cert_in_keychain {
                log warning "Certificate not found in system keychain, attempting to add it"
                try {
                    sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain $crt_path
                    log info "Certificate added to system keychain"
                } catch { |err|
                    log warning $"Failed to add certificate to system keychain: ($err)"
                }
            }
        }

        log warning "IMPORTANT: Docker Desktop may need to be restarted for the certificate to take effect"
        log warning "If you continue to see certificate errors, please restart Docker Desktop"
        log info "The certificate is configured in:"
        if (_is_macos) {
            log info "  - System keychain: /Library/Keychains/System.keychain"
        }
        log info $"  - User Docker certs: ($user_target_crt)"
    } else {
        # Native dockerd path: /etc/docker/certs.d/{registry}/ca.crt
        let system_target_dir = $"/etc/docker/certs.d/($registry_host)"
        let system_target_crt = $system_target_dir | path join "ca.crt"

        # Content comparison first: the common case changes nothing and never
        # prompts for sudo, so a non-interactive deploy does not die here.
        let current = if ($system_target_crt | path exists) {
            open $system_target_crt --raw | str trim
        } else {
            ""
        }
        if $current == (open $crt_path --raw | str trim) {
            log debug $"Docker registry certificate at ($system_target_crt) is current"
            return
        }

        if not ($system_target_dir | path exists) {
            ^sudo mkdir -p $system_target_dir
        }
        ^sudo cp $crt_path $system_target_crt

        # Native dockerd reads certs on each TLS handshake — no restart needed
        log info $"Docker registry certificate configured at ($system_target_crt)"
        log info "Native dockerd picks up new certificates automatically — no restart needed"
    }
}

# Keep the locally-trusted copy of the dev root CA in step with the cluster's.
#
# cert-manager reissues local-dev-root-ca whenever the Certificate is recreated
# — a cluster rebuild, a deleted secret, an expiry — and the replacement keeps
# the SAME common name. A stale copy therefore does not look stale. Docker finds
# a CA called local-dev-root-ca, fails the signature check, and reports
#
#     x509: certificate signed by unknown authority (possibly because of
#     "crypto/rsa: verification error" ... candidate authority certificate
#     "local-dev-root-ca")
#
# which reads like a broken machine rather than a rotated key. Nothing detected
# it: the deploy failed on a registry HEAD request, and recovering meant knowing
# to call register_local_certs and setup_docker_registry_cert by hand.
#
# Content comparison first, so the common case costs one kubectl call, changes
# nothing, and never prompts for sudo — only an actual rotation reaches the
# keychain / trust-store write.
#
# Sandbox-only in practice: prod pulls from ghcr.io over public TLS and has no
# in-cluster registry, so KUBE_REGISTRY_HOST is empty there.
#
# Deliberately NOT exported. Scripts import this module by NAME
# [`use k8s setup_docker_registry_cert`], so a new export is unresolvable
# against an already-installed older copy and the script dies at PARSE time
# with "Export not found" — before _check_installed_modules_fresh can run and
# say "your modules are stale, reinstall". Folding the behaviour into an
# existing exported function keeps that useful error intact.
def _ensure_local_ca_current [] {
    let cluster = (do {
        kubectl -n cert-manager get secret local-dev-root-ca -o 'jsonpath={.data.tls\.crt}'
    } | complete)
    if $cluster.exit_code != 0 or ($cluster.stdout | str trim | is-empty) {
        log warning "could not read the local-dev-root-ca secret — skipping CA freshness check"
        return
    }
    let cluster_pem = ($cluster.stdout | str trim | decode base64 | decode | str trim)

    let local_path = ($env.KUBE_CERTS_DIR | path join $env.KUBE_ROOT_CA_CERT_NAME)
    let local_pem = if ($local_path | path exists) {
        open $local_path --raw | str trim
    } else {
        ""
    }

    if $local_pem == $cluster_pem {
        log debug "local dev root CA matches the cluster"
        return
    }

    if ($local_pem | is-empty) {
        log info "local dev root CA not installed yet — installing"
    } else {
        log warning $"local dev root CA at ($local_path) differs from the cluster's [CA rotated] — refreshing trust"
    }

    # Only re-registers the CA itself. The Docker-side copy is done by the
    # caller, which is setup_docker_registry_cert — calling it here would
    # recurse.
    register_local_certs "local-dev-root-ca"
}

# Keep the node-side copy of the dev root CA in step with the cluster's.
#
# containerd on every node trusts the CA that the registry-ca-trust DaemonSet
# copied out of the kube-system/registry-root-ca ConfigMap. Only
# setup_microk8s.nu refreshed that ConfigMap, so after cert-manager rotated the
# root CA the node kept trusting the old one: once it expired, every image pull
# failed with "x509: certificate has expired" while deploys still reported
# success, and pods sat in ContainerCreating / ImagePullBackOff indefinitely.
#
# Content comparison first — one kubectl call when nothing changed. No sudo:
# the DaemonSet does the privileged copy. Not exported, for the same reason as
# _ensure_local_ca_current.
def _ensure_node_ca_current [] {
    let cluster = (do {
        kubectl -n cert-manager get secret local-dev-root-ca -o 'jsonpath={.data.tls\.crt}'
    } | complete)
    if $cluster.exit_code != 0 or ($cluster.stdout | str trim | is-empty) {
        log warning "could not read the local-dev-root-ca secret — skipping node CA freshness check"
        return
    }
    let cluster_pem = ($cluster.stdout | str trim | decode base64 | decode | str trim)

    let node = (do {
        kubectl -n kube-system get configmap registry-root-ca -o 'jsonpath={.data.ca\.crt}'
    } | complete)
    let node_pem = if $node.exit_code == 0 { $node.stdout | str trim } else { "" }

    if $node_pem == $cluster_pem {
        log debug "node registry CA matches the cluster"
        return
    }

    log warning "node registry CA differs from the cluster's [CA rotated] — refreshing containerd trust"
    setup_k8s_registry_cert
    # Re-applying an unchanged DaemonSet does not restart it, and the copy
    # happens at pod start — force a restart so the new CA reaches the node.
    kubectl -n kube-system rollout restart daemonset/registry-ca-trust
    kubectl -n kube-system rollout status daemonset/registry-ca-trust --timeout=300s
}

# Configure Kubernetes nodes to trust the registry certificate
export def setup_k8s_registry_cert [] {
    log info "configuring Kubernetes nodes to trust registry certificate"
    
    # Get the root CA certificate
    let root_ca_crt = (kubectl -n cert-manager get secret local-dev-root-ca -o 'jsonpath={.data.tls\.crt}' | base64 -d)
    
    # Create a temporary file with the certificate
    let tmp_file_raw = (^mktemp)
    let tmp_file = ($tmp_file_raw | to text | str trim)
    
    log debug $"created temp file: ($tmp_file)"
    
    try {
        # Write certificate to temp file
        $root_ca_crt | save -f $tmp_file
        
        # Verify file was created
        if not ($tmp_file | path exists) {
            error make { msg: $"temp file was not created: ($tmp_file)" }
        }
        
        log debug $"temp file exists: (($tmp_file | path exists)), size: (($tmp_file | open --raw | bytes length))"
        
        # Create or update the ConfigMap with the root CA
        # Use absolute path for kubectl
        let tmp_file_abs = ($tmp_file | path expand)
        log debug $"creating ConfigMap with cert from: ($tmp_file_abs)"
        kubectl create configmap registry-root-ca -n kube-system --from-file=ca.crt=($tmp_file_abs) --dry-run=client -o yaml | kubectl apply -f -
        
        # Apply the DaemonSet to trust the certificate on all nodes. Resolve it
        # from the workspace so a deploy launched from any directory finds it;
        # fall back to the project-root-relative path setup_microk8s.nu uses.
        let rel_path = "assets/k8s/registry/trust-registry-ca.yaml"
        let workspace = ($env | get -o DAPPLE_PROMPT_WORKSPACE_DIR | default "")
        let daemonset_path = if ($workspace | is-not-empty) and ($workspace | path join $rel_path | path exists) {
            $workspace | path join $rel_path
        } else {
            $rel_path
        }
        log debug $"applying DaemonSet from: ($daemonset_path)"
        
        if not ($daemonset_path | path exists) {
            error make { msg: $"DaemonSet file not found: ($daemonset_path)" }
        }
        
        kubectl apply -f $daemonset_path
    } catch { |err|
        if ($tmp_file | path exists) {
            rm $tmp_file
        }
        error make { msg: $"failed to configure registry trust: ($err)" }
    }

    if ($tmp_file | path exists) {
        rm $tmp_file
    }
    
    log info "waiting for registry-ca-trust DaemonSet to be ready on all nodes"
    # Wait for the DaemonSet to report all desired pods as available (covers all nodes)
    kubectl rollout status daemonset/registry-ca-trust -n kube-system --timeout=300s
    
    log info "Kubernetes nodes configured to trust registry certificate"
    log info "Note: You may need to restart containerd on nodes if certificate trust doesn't take effect immediately"
}

# Deploy = render + apply + wait for rollout (Deployment by default)
export def deploy [
  cfg: record,
  --wait,
  --timeout: string = "300s",    # 300s default: fresh clusters cold-pull every image; 90s timed out spuriously
  --kind: string = "deployment"  # override if your template uses another workload
] {
  apply_manifest $cfg

  if $wait {
    # Reuse shared selector defaults by passing a small cfg map
    let resource = ($cfg | get -o resource | default $cfg.name)
    let timeout_val = ($cfg | get -o timeout | default $timeout)
    rollout_status { name: $cfg.name, namespace: $cfg.namespace, resource: $resource } --kind $kind --timeout $timeout_val
  }
}

# Just apply the rendered manifest (no wait)
export def apply_manifest [cfg: record] {
  let vals = {
    name: $cfg.name,
    namespace: $cfg.namespace,
    source_path: $cfg.source_path,
    template: $cfg.template,
    image_name: ($cfg | get -o image_name | default $cfg.name),
    port: ($cfg | get -o port | default ""),
    node_port: ($cfg | get -o node_port | default ""),
  }

 #TODO: build the path to the template from env
  log debug $"render call: manifest from ($cfg.source_path) with values: ($vals)"
  let y = (render $cfg.template $vals)

  log debug $"rendered manifest: {$y}"

  log debug $"apply_yaml call from ($cfg.source_path) with values: ($vals)"
  apply_yaml $y
}
