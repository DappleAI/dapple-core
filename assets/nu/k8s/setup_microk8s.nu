#!/usr/bin/nu

$env.NU_LOG_LEVEL = "debug"

use std/log
use std/assert

use ../modules/dapple_env_profiles dapple_sandbox_tools
overlay use dapple_sandbox_tools as dapple_env

# Load k8s module - use local version if default config dir doesn't have it
# The module should be synced via setup.nu, but we can also load from local assets
use ../modules/k8s
use k8s *

# Compatible with both old ($nu.home-path) and new ($nu.home-dir) nushell versions
let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
mkdir ($home | path join ".dapple" | path join "dev" | path join "certs")

configure_masqed_dns

microk8s enable dns
patch_coredns_upstream
microk8s enable dashboard
microk8s enable hostpath-storage
microk8s enable ingress
microk8s enable cert-manager
microk8s enable registry

# --- Ensure namespace exists ---
log info "ensuring dapple-sandbox namespace exists"
kubectl create namespace dapple-sandbox --dry-run=client -o yaml | kubectl apply -f -

# --- Decommission deprecated resources ---
# Idempotent cleanup of stale resources from prior PG 16 (dapple-postgres) setup.
# Add future decommissions here in the same pattern.

let deprecated_resources = [
    "deployment/dapple-postgres"
    "service/dapple-postgres"
    "daemonset/dapple-postgres-gateway"
    "serviceaccount/dapple-postgres-gateway"
    "role/dapple-postgres-gateway"
    "rolebinding/dapple-postgres-gateway"
    "configmap/dapple-postgres-init-sql"
    "secret/dapple-postgres-secret"
    "secret/dapple-db"
    "pvc/dapple-postgres-pvc"
]

for res in $deprecated_resources {
    log info $"decommissioning ($res)"
    kubectl delete $res -n dapple-sandbox --ignore-not-found
}

# --- Registry infrastructure (needed before pushing custom images) ---

log info "starting dev-dns deployment"
kubectl apply -f assets/k8s/registry/dev-dns.yaml

# `kubectl apply` returns when the objects are CREATED, not when the pod serves
# DNS. That was harmless while setup_host_resolver only verified a file write
# [macOS]; the Linux branch verifies live resolution, so without this wait the
# probe races the pod and aborts the rest of this script — before the cert
# issuer, the registry, prompt-db and redis are ever applied.
log info "waiting for dev-dns to become ready"
kubectl rollout status deployment/dev-dns -n dapple-sandbox --timeout=120s

# Teach the HOST to resolve *.kube.dappleai.dev via the dev-dns pod deployed
# above. Both platforms — see setup_host_resolver for how each is wired.
#
# macOS writes /etc/resolver pointing at the OrbStack VM, which the function
# looks up itself; under OrbStack that IP is stable across reboots, so the write
# is one-shot rather than drift recovery. install_orbstack_vm.nu also writes it
# during provisioning, and calling it again here is idempotent.
#
# Linux previously got nothing at all, which left a workstation unable to
# resolve any dev hostname after a clean setup.
log info "setting up host resolver for dev hostnames"
setup_host_resolver

log info "starting cert issuer deployment"
kubectl apply -f assets/k8s/registry/issuer.yaml

log info "waiting for cert-manager to create the root CA certificate"
kubectl wait --for=condition=Ready certificate/local-dev-root-ca -n cert-manager --timeout=60s

register_local_certs "local-dev-root-ca"

log info "starting registry deployment"
kubectl apply -f assets/k8s/registry/registry.yaml

log info "configuring Docker to trust registry certificate"
setup_docker_registry_cert $env.KUBE_REGISTRY_HOST $env.KUBE_ROOT_CA_CERT_NAME

log info "configuring Kubernetes nodes to trust registry certificate"
setup_k8s_registry_cert

# --- Custom images (pushed to registry) ---

log info "building and pushing prompt-db image (PG18 + PostGIS + pgvector)"
let prompt_db_dockerfile = ($env.DAPPLE_PROMPT_WORKSPACE_DIR | path join "assets" | path join "docker" | path join "prompt-db")
let prompt_db_tag = $"($env.KUBE_REGISTRY_HOST)/prompt-db:($env.DOCKER_DAPPLE_TAG_LATEST)"
^docker build -t $prompt_db_tag $prompt_db_dockerfile
^docker push $prompt_db_tag

# --- Infrastructure services ---

log info "starting dapple-sandbox prompt-db (PG18) deployment"
kubectl apply -n dapple-sandbox -f assets/k8s/registry/prompt-db.yaml

log info "starting dapple-sandbox redis setup"
kubectl apply -n dapple-sandbox -f assets/k8s/registry/redis.yaml

log info "creating HuggingFace cache PVC for model storage"
kubectl apply -f assets/k8s/registry/dapple-hf-cache-pvc.yaml
