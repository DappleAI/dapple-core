# Kubernetes Infrastructure Guide

This document describes the local Kubernetes development environment used by dapple-prompt.

## Overview

The development environment uses **MicroK8s** as the local Kubernetes distribution, with an in-cluster CoreDNS pod (`dev-dns`) serving `*.kube.dappleai.dev` and a per-workstation resolver entry pointing at it.

## Environment Variables

The Nushell environment provides these key variables (from `assets/nu/modules/dapple_env_profiles/dapple_microk8s.nu`):

| Variable | Value | Description |
|----------|-------|-------------|
| `KUBE_HOST` | `kube.dappleai.dev` | Base domain for K8s services |
| `KUBE_REGISTRY_HOST` | `registry.kube.dappleai.dev` | Local Docker registry |
| `KUBE_DAPPLE_NAMESPACE` | `dapple-sandbox` | Primary namespace for dapple services |
| `KUBE_ROOT_CA_CERT_NAME` | `local-dev-root-ca.crt` | Root CA certificate filename |
| `KUBE_CERTS_DIR` | `$DAPPLE_HOME_DIR/certs` | Certificate storage directory |
| `DOCKER_DAPPLE_TAG_LATEST` | `dapple-latest` | Standard Docker tag for local builds |

## Shell Aliases

```nushell
# kubectl is aliased to MicroK8s kubectl
alias kubectl = ^microk8s kubectl

# Short alias for convenience
alias k = kubectl
```

## MicroK8s Addons

The setup script (`assets/nu/k8s/setup_microk8s.nu`) enables these addons:

| Addon | Purpose |
|-------|---------|
| `dns` | CoreDNS for in-cluster DNS resolution |
| `dashboard` | Kubernetes dashboard UI |
| `hostpath-storage` | Local persistent volume provisioner |
| `ingress` | NGINX ingress controller |
| `cert-manager` | TLS certificate management |
| `registry` | Local Docker registry (port 32000) |

## Network Architecture

### DNS Resolution

**Three** separate concerns, often confused because the first two setup functions sit next to
each other. They solve different problems and none substitutes for another.

| direction | mechanism | resolves `*.kube.dappleai.dev`? |
|---|---|---|
| host → cluster | `setup_host_resolver` | **yes** — that is its whole job |
| pod → outside world | `configure_masqed_dns` + `patch_coredns_upstream` | no |
| pod → cluster service | in-cluster CoreDNS, nothing to configure | **no** — use Service names |

#### Host → cluster (`setup_host_resolver`)

`*.kube.dappleai.dev` is served by the in-cluster `dev-dns` CoreDNS pod, which wildcards the
zone to the node IP (NodePort 30053/UDP). Nothing in the cluster teaches the *host* to ask it —
that is per-workstation config, and it is what `DAPPLE_PROMPT_API_URL`, `DATABASE_URL`, the
image registry and the browser all depend on.

`k8s setup_host_resolver` (`assets/nu/modules/k8s/mod.nu`), called by `setup_microk8s.nu`:

- **Probes first.** If the zone already resolves, it logs that and returns — so a re-run is a
  no-op and a host whose DNS is served some other way is left alone.
- **macOS:** writes `/etc/resolver/kube.dappleai.dev` (`nameserver <vm-ip>`, `port 30053`).
- **Linux:** writes a systemd-resolved drop-in at
  `/etc/systemd/resolved.conf.d/dapple-dev-dns.conf` with `DNS=127.0.0.1:30053` and
  `Domains=~kube.dappleai.dev`. The tilde makes it a *routing* domain — only that suffix goes
  to dev-dns; everything else keeps using your normal resolvers.

If the zone still doesn't resolve afterwards it warns and continues rather than aborting
setup; only a failed *write* is fatal.

> Some workstations still resolve this zone through a hand-configured **dnsmasq** forwarder.
> That is a legacy arrangement being retired. The probe-first behaviour above leaves it working
> untouched in the meantime — no dapple tooling reads or writes dnsmasq config.

#### Pod → outside world (`configure_masqed_dns`)

Unrelated to the above, despite the similar name — "masqed" is *masquerade*, not dnsmasq. This
patches **kubelet's** resolv.conf so that pods get working upstream DNS instead of inheriting
the host's systemd-resolved stub (`127.0.0.53`), which is unreachable from inside a pod:

```nushell
# From k8s/mod.nu — configure_masqed_dns
# Patches /var/snap/microk8s/current/args/kubelet with:
#   --resolv-conf=$DAPPLE_KUBE_ASSETS_DIR/config/resolv.conf
# That file pins public upstreams (1.1.1.1, 8.8.8.8) — see assets/k8s/config/resolv.conf
```

The override is structural, not a workaround for one flaky host. *Any* local stub resolver puts
a loopback address in the node's `/etc/resolv.conf` — `127.0.0.53` under systemd-resolved,
`127.0.0.1` under dnsmasq — and a pod has its own network namespace, so that address is dead
inside one. Handing it to pods is a guaranteed cluster-wide DNS outage. This is exactly what
kubelet's `--resolv-conf` exists for.

**A useful consequence: changing the host's resolver cannot break pod DNS.** Kubelet is pinned
to a file in this repo, so migrating the workstation between dnsmasq and systemd-resolved does
not touch what pods resolve against.

`patch_coredns_upstream` sets the same two servers in the cluster CoreDNS Corefile. That is
**belt-and-braces, not a second decision**: CoreDNS runs `dnsPolicy: Default`, so its own
`/etc/resolv.conf` is already the kubelet-pinned file. It exists so a cluster whose kubelet args
were never patched still gets a working upstream. Keep the two in sync; don't remove one alone.

#### Pod → cluster service (nothing to configure)

**Pods cannot resolve `*.kube.dappleai.dev`.** Nothing in this repo adds that zone to the
cluster CoreDNS — `dev-dns` is authoritative for it but reachable only on NodePort 30053, which
cluster CoreDNS never forwards to. A pod asking for `dapple-prompt-api.kube.dappleai.dev` gets
NXDOMAIN, and even where a shared cluster happens to have a forwarder installed by another
tenant, no CA bundle is mounted into the web pods, so the HTTPS ingress cert would fail next.

**So any value a pod reads must be an in-cluster address**, not an ingress URL:

- `http://dapple-prompt-api:8001` — short Service name, same namespace.
- `http://dapple-prompt-api.dapple-sandbox.svc.cluster.local:8001` — FQDN, needed when the
  *node* must resolve it too (a routing-domain drop-in supplies no search list, so the short
  form works in a pod and fails in a shell).

This is why `environments.nu` pairs every service address with an `internal_*` variant —
`api_base` / `internal_api_url`, `database_url` / `internal_postgres_url`. Putting an ingress
URL where a pod reads it fails twice over and is not obvious from either end.

### Service Hostnames

Services are accessible via pattern: `{service}.kube.dappleai.dev`

Examples:
- `registry.kube.dappleai.dev` → Local Docker registry
- `prompt-db.kube.dappleai.dev` → PostgreSQL database (prompt services)
- `redis.kube.dappleai.dev` → Redis cache

### Registry Configuration

The local registry runs at `localhost:32000` internally but is accessible via `registry.kube.dappleai.dev` once the host resolver is configured (see [Host → cluster](#host--cluster-setup_host_resolver)).

Docker must be configured to trust the local registry certificate:
```nushell
# From k8s/mod.nu - setup_docker_registry_cert
# Copies cert to ~/.docker/certs.d/registry.kube.dappleai.dev/ca.crt
```

## Certificate Management

### cert-manager

TLS certificates are managed by cert-manager with a local CA:

1. **Root CA**: Self-signed CA stored as `local-dev-root-ca.crt`
2. **Issuer**: ClusterIssuer named `local-dev-issuer`
3. **Certificates**: Automatically provisioned for services

### System Trust

Certificates must be registered with the system to avoid TLS warnings:

**macOS:**
```nushell
# Adds to system keychain
sudo security add-trusted-cert -d -r trustRoot \
  -k /Library/Keychains/System.keychain $cert_path
```

**Linux:**
```nushell
# Copies to CA store and updates
sudo cp $cert_path /usr/local/share/ca-certificates/
sudo update-ca-certificates
```

## Deployed Services

The setup script deploys these infrastructure services:

| Service | Namespace | Description |
|---------|-----------|-------------|
| `prompt-db` | `dapple-sandbox` | PostgreSQL database (prompt services) |
| `dapple-redis` | `dapple-sandbox` | Redis cache |
| `dev-dns` | `default` | DNS configuration |
| `local-dev-issuer` | cluster-wide | cert-manager issuer |

## Common Operations

### Check Cluster Status

```bash
# Using the k alias
k get nodes
k get pods -n dapple-sandbox

# Check addon status
microk8s status
```

### View Logs

```bash
k logs -n dapple-sandbox -l app=dapple-prompt-api -f
```

### Port Forwarding

```bash
# Forward PostgreSQL
k port-forward -n dapple-sandbox svc/prompt-db 5432:5432

# Forward a service for local testing
k port-forward -n dapple-sandbox svc/dapple-prompt-conduit 8081:8081
```

### Build and Push Images

```bash
# Build image
docker build -t localhost:32000/dapple-prompt-api:dapple-latest .

# Push to local registry
docker push localhost:32000/dapple-prompt-api:dapple-latest

# Or using the registry hostname
docker tag myimage registry.kube.dappleai.dev/myimage:dapple-latest
docker push registry.kube.dappleai.dev/myimage:dapple-latest
```

### Apply Manifests

```bash
k apply -f assets/k8s/registry/dapple-prompt-api.yaml
```

## Troubleshooting

### DNS Not Resolving

First decide which of the two DNS paths is broken — they fail in different places.

**A hostname doesn't resolve from your shell / browser** (host → cluster):

1. Confirm the symptom: `getent hosts dapple-prompt-api.kube.dappleai.dev`
2. Check `dev-dns` is actually serving: `kubectl get pods -n dapple-sandbox -l app=dev-dns`
3. Check the host resolver entry:
   - Linux — `resolvectl status | grep -A2 'DNS Servers'`, and
     `cat /etc/systemd/resolved.conf.d/dapple-dev-dns.conf`
   - macOS — `cat /etc/resolver/kube.dappleai.dev`
4. Re-run `nu assets/nu/k8s/setup_microk8s.nu`, or call `k8s setup_host_resolver` directly. It
   probes first, so it is safe to run when you are unsure.

**A pod can't reach the internet** (pod → outside world):

1. Verify the kubelet resolv.conf patch was applied:
   `grep resolv-conf /var/snap/microk8s/current/args/kubelet`
2. Restart MicroK8s: `microk8s stop && microk8s start`

### Registry Connection Refused

1. Verify registry addon: `microk8s status | grep registry`
2. Check certificate is trusted by Docker
3. Test connectivity: `curl -k https://registry.kube.dappleai.dev/v2/`

### Pod Can't Pull Image

1. Check imagePullPolicy is `Always` for local development
2. Verify image was pushed: `curl localhost:32000/v2/_catalog`
3. Check pod events: `k describe pod <pod-name> -n dapple-sandbox`

### Certificate Errors

1. Re-export certificates: `register_local_certs`
2. Restart Docker daemon
3. For browsers, clear certificate cache

## Setup Scripts Reference

| Script | Purpose |
|--------|---------|
| `assets/nu/k8s/setup_microk8s.nu` | Main setup, enables addons, deploys services |
| `assets/nu/modules/k8s/mod.nu` | Utility functions for k8s operations |
| `assets/nu/modules/dapple_env_profiles/dapple_microk8s.nu` | Environment variables and aliases |

## GPU Support (for NIM)

GPU support requires enabling the NVIDIA addon:

```bash
microk8s enable gpu
```

Verify GPU is available:
```bash
k describe nodes | grep -A 10 "Capacity:"
# Should show: nvidia.com/gpu: 1
```

See `docs/plans/nvidia-nim/01-k8s-prerequisites.md` for full GPU setup instructions.
