# OrbStack Migration — Follow-up Cleanup

**Status:** Open
**Created:** 2026-05-08
**Parent plan:** [orbstack-migration.md](orbstack-migration.md)
**Branch:** `mac_orbstack`

This document captures the cleanup items identified while validating the OrbStack migration end-to-end through `setup_microk8s.nu` and `deploy_dapple_core.nu`. Each item below is real (not theoretical) — every one was triggered by an actual deploy failure during the session that produced this branch.

They're polish items so the next contributor doesn't re-hit the same traps.

## Follow-ups

### 1. Wildcard TLS cert needs to be provisioned per-namespace

**Symptom:** The cluster's `*.kube.dappleai.dev` Certificate is hardcoded to `namespace: dapple-sandbox` in `setup_microk8s.nu`'s `issuer.yaml`. K8s TLS secrets are namespace-scoped, so any ingress in another namespace finds no matching secret and falls back to nginx-ingress's self-signed default cert.

The workaround is to add a `Certificate` resource next to that namespace's ingress. Every new namespace (e.g., another self-hosted service) needs the same treatment.

**Fix options:**
- **(a)** Move the `wildcard-kube.dappleai.dev` Certificate to the `cert-manager` namespace and use a tool like `reflector` or cert-manager's `secretTemplate` to replicate the resulting Secret into other namespaces. More infra, but DRY.
- **(b)** Document the per-namespace pattern in `assets/k8s/AGENTS.md` so the next person knows what to copy. Less DRY, more discoverable.

(b) is faster and probably sufficient given how rarely we add new namespaces.

**Acceptance:** Either the secret reflects automatically OR the convention is documented and a copy-pasteable template exists.

## What's already shipped on this branch

For reference, these were completed during the OrbStack migration validation:

- ✅ Multipass replaced with OrbStack throughout `assets/nu/`
- ✅ `install_orbstack_vm.nu` provisions VM + microk8s + group + resolver + kubeconfig + buildx
- ✅ `microk8s` wrapper in `k8s/mod.nu` dispatches per-platform
- ✅ `kubectl` wrapper made platform-aware (no longer routes through `^microk8s kubectl` on macOS)
- ✅ Host kubeconfig auto-generated and merged into `~/.kube/config`
- ✅ Multiple nushell parser traps fixed (parens-in-`$"..."`, bareword `jsonpath=`, variadic args swallowing flags, `$in`-doesn't-carry-across-statements)
- ✅ Buggy keychain-cert check that re-prompted sudo every deploy
- ✅ `nushell-usage` skill committed at `.claude/skills/nushell-usage/` for auto-discovery
- ✅ Memory file `feedback_nushell_parser_gotchas.md` updated through this session
