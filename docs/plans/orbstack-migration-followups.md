# OrbStack Migration — Follow-up Cleanup

**Status:** Open
**Created:** 2026-05-08
**Parent plan:** [orbstack-migration.md](orbstack-migration.md)
**Branch:** `mac_orbstack`

This document captures the cleanup items identified while validating the OrbStack migration end-to-end through `setup_microk8s.nu`, `deploy_dapple_prompt_pods.nu`, and `deploy_lago_billing.nu`. Each item below is real (not theoretical) — every one was triggered by an actual deploy failure during the session that produced this branch.

These are not blockers for merging `mac_orbstack` into `initial-lago-integration`. They're polish items so the next contributor doesn't re-hit the same traps.

## High-priority follow-ups

### 1. `dapple lago setup` doesn't re-run migrations on a fresh postgres PVC

**Symptom:** After `kubectl delete pvc -n lago lago-postgres-pvc` and re-running `dapple lago setup`, the new postgres comes up with no schema. The `lago-migrations` Job from the previous setup is still in `Status: Complete`, so re-applying `migrations-job.yaml` no-ops. The next signup attempt fails with `PG::UndefinedTable: relation "users" does not exist`.

**Fix:**
In `assets/nu/modules/dapple/lago.nu` `setup`, before applying `migrations-job.yaml`:

```nu
# Migrations job is immutable once Complete. If it exists, delete it
# so the apply below picks up any schema changes AND re-runs against
# whatever postgres is currently provisioned.
let existing = (do { kubectl get job -n lago lago-migrations -o name } | complete)
if $existing.exit_code == 0 {
    kubectl delete job -n lago lago-migrations --wait=true
}
kubectl apply -f ($lago_dir | path join "migrations-job.yaml")
kubectl wait --for=condition=Complete job/lago-migrations -n lago --timeout=180s
```

**Acceptance:** `kubectl delete pvc -n lago lago-postgres-pvc; dapple lago setup` produces a working schema without manual intervention.

### 2. `deploy_dapple_prompt_pods.nu` should fail-fast when `lago-api` secret is missing

**Symptom:** On a cluster where Lago hasn't been bootstrapped (no `lago-api` secret in `dapple-sandbox`), the `dapple-prompt-lago-worker` pod sits in `CreateContainerConfigError` for ~90 seconds while the script's rollout-watch loop waits, then errors with a generic timeout. The actual cause (`secret "lago-api" not found`) is buried in `kubectl describe pod`.

**Fix:**
Either (a) **pre-flight check**: at the top of the lago-worker deploy block, `kubectl get secret -n dapple-sandbox lago-api` — if absent, log a clear error pointing to `deploy_lago_billing.nu` and skip the worker; or (b) **post-rollout-failure diagnostic**: when rollout times out, run `kubectl describe pod -l app=$cfg.name -n $cfg.namespace | grep -A 5 Events:` and surface that to the user.

(a) is faster to fix and gives the user actionable info immediately.

**Acceptance:** Running `deploy_dapple_prompt_pods.nu` on a cluster without Lago bootstrapped logs a clear message ("lago-api secret missing — run deploy_lago_billing.nu first") and either skips the worker or exits with that as the error.

### 3. `deploy_lago_billing.nu` Phase 4 uses a dead Lago auth endpoint

**Symptom:** Phase 4 (`setup_test_org`) calls `POST /auth/sign_up` and `POST /auth/sign_in` against the Lago API. Both return 404 on Lago v1.41.0 — those REST endpoints have been removed in favor of GraphQL mutations (`registerUser`, `loginUser`). Even the GraphQL `registerUser` returns 404 on this version (root cause unclear — see open question below).

We worked around this by manually invoking `bundle exec rails runner` inside the lago-api pod to create the test org/user/api-key directly:

```ruby
org = Organization.find_or_create_by!(name: "dapple-test")
user = User.find_or_create_by!(email: "test@dappleai.dev") { |u| u.password = "dapple-test-org-2026!" }
Membership.find_or_create_by!(user: user, organization: org)
key = ApiKey.find_or_create_by!(organization: org) { |k| k.name = "default" }
puts "LAGO_TEST_API_KEY=#{key.value}"
```

…then pasted the result into `~/.env` so Phase 4 short-circuits via the `LAGO_TEST_API_KEY` env-var check (lines 239-246).

**Fix:**
Replace Phase 4's HTTP-signup-then-fetch dance with a `kubectl exec` into `lago-api` running `rails runner` (same shape as our workaround). The advantages: works on any Lago version, doesn't depend on whichever auth endpoint Lago is shipping this month, idempotent on re-runs.

**Acceptance:** `deploy_lago_billing.nu` runs end-to-end on a fresh cluster (no `LAGO_TEST_API_KEY` pre-set) and produces a working test org + saved key.

### 4. Phase 1 reachability check is too permissive

**Symptom:** Phase 1 of `deploy_lago_billing.nu` accepts any HTTP response — including 404 — as "Lago is reachable". This masked the actual bug (Lago platform never deployed; nginx-ingress was returning 404 from its default backend).

**Fix:**
Tighten the check to require a 401 (Lago API up, auth required) on `/api/v1/organizations`, or hit a known-existing endpoint like `/health` (if Lago exposes one). 200/401 = up; anything else = error with a clear "did you run `dapple lago setup`?" message.

**Acceptance:** Running `deploy_lago_billing.nu` on a cluster where the `lago` namespace is empty produces an error like "Lago platform not running — run `dapple lago setup` first" instead of crashing inside Phase 4.

### 5. Wildcard TLS cert needs to be provisioned per-namespace

**Symptom:** The cluster's `*.kube.dappleai.dev` Certificate is hardcoded to `namespace: dapple-sandbox` in `setup_microk8s.nu`'s `issuer.yaml`. K8s TLS secrets are namespace-scoped, so any ingress in another namespace (we hit this with `lago`) finds no matching secret and falls back to nginx-ingress's self-signed default cert.

We worked around it by adding a `Certificate` resource to `assets/k8s/registry/lago/ingress.yaml`. Future namespaces (e.g., if we add another self-hosted service) will need the same treatment.

**Fix options:**
- **(a)** Move the `wildcard-kube.dappleai.dev` Certificate to the `cert-manager` namespace and use a tool like `reflector` or cert-manager's `secretTemplate` to replicate the resulting Secret into other namespaces. More infra, but DRY.
- **(b)** Document the per-namespace pattern in `assets/k8s/AGENTS.md` so the next person knows what to copy. Less DRY, more discoverable.

(b) is faster and probably sufficient given how rarely we add new namespaces.

**Acceptance:** Either the secret reflects automatically OR the convention is documented and a copy-pasteable template exists.

## Lower-priority follow-ups

### 6. Investigate why Lago `registerUser` mutation 404s

**Symptom:** Even on a fresh `lago` namespace with migrated schema and `LAGO_DISABLE_SIGNUP: "false"` in the api configmap, `POST /graphql` with the `registerUser` mutation returns `{"status":404,"error":"Not Found","code":"resource_not_found"}`. The Rails log shows the request reaches `GraphqlController#execute` cleanly (no schema errors, no exceptions) but emits 404 anyway. Bypassing GraphQL via `rails runner` works fine.

**Hypotheses to investigate:**
- Lago v1.41 may require a paid license to enable signup (community-edition gate)
- A hidden middleware checks `LAGO_LICENSE` or similar that we're not setting
- The mutation may be permission-gated and our anonymous request is missing required headers/cookies
- Possibly a CSRF or origin gate beyond the basic CORS

**Why it matters:** If we ever switch to fully-automated Lago bootstrap (without `kubectl exec`), we need a working signup. For now `rails runner` is fine.

### 7. `deploy_lago_billing.nu` reads stale `LAGO_API_KEY` from somewhere

**Symptom:** Even after updating `~/.env` with a fresh API key, the script's Phase 3 logged `LAGO_API_KEY is set (ThereIsN...)` — the old placeholder value. The override-load order in the env profile may be reading from a different file than expected, or `~/.env.dapple` is masking `~/.env`.

**Fix:** Trace the env load order in `dapple_env_profiles/dapple_base.nu` (or wherever LAGO_API_KEY originates) and confirm `~/.env` overrides take precedence. Add a debug log in Phase 3 showing the source of the value (e.g., "loaded from ~/.env" vs "loaded from env.template default").

### 8. Lago configmap's `LAGO_API_URL` points at the in-cluster service

```yaml
LAGO_API_URL: "http://lago-api:3000"
```

This is correct for in-cluster callers (lago-front, lago-worker). But the dapple-prompt-lago-worker references `LAGO_API_URL: http://lago-api.lago:3000` (different — uses FQDN with namespace). Worth confirming both are pointed correctly and the FQDN form is necessary across-namespace.

### 9. `dapple lago setup` shouldn't require a separate test org bootstrap

**Symptom:** The split between "user's primary org" (created via UI signup) and "test org" (created by `deploy_lago_billing.nu` Phase 4) feels awkward. A first-time contributor has to do both. Could `deploy_lago_billing.nu` create both via `rails runner` (using the fix in #3)?

**Why it matters:** Reduces onboarding to a single command. Lower priority because today's UX (sign up via UI, then run script) is documented.

## What's already shipped on this branch

For reference, these were completed during the OrbStack migration validation:

- ✅ Multipass replaced with OrbStack throughout `assets/nu/`
- ✅ `install_orbstack_vm.nu` provisions VM + microk8s + group + resolver + kubeconfig + buildx
- ✅ `microk8s` wrapper in `k8s/mod.nu` dispatches per-platform
- ✅ `kubectl` wrapper made platform-aware (no longer routes through `^microk8s kubectl` on macOS)
- ✅ Host kubeconfig auto-generated and merged into `~/.kube/config`
- ✅ Wildcard TLS cert added to `lago` namespace (workaround for cross-namespace cert issue, see #5)
- ✅ Multiple nushell parser traps fixed (parens-in-`$"..."`, bareword `jsonpath=`, variadic args swallowing flags, `$in`-doesn't-carry-across-statements)
- ✅ Buggy keychain-cert check that re-prompted sudo every deploy
- ✅ `nushell-usage` skill committed at `.claude/skills/nushell-usage/` for auto-discovery
- ✅ Memory file `feedback_nushell_parser_gotchas.md` updated through this session

## Recommended next-session order

1. (#1) `dapple lago setup` migration-job idempotency — 5 lines, biggest UX win.
2. (#3) Phase 4 rewrite using `rails runner` — removes the dead-endpoint dependency.
3. (#2) Pre-flight check for `lago-api` secret in prompt-pods deploy — ~10 lines.
4. (#4) Tighten Phase 1 reachability check — ~3 lines.
5. (#5) Document the per-namespace wildcard-cert pattern — doc edit.
6. (#6) Investigate Lago `registerUser` 404 — research; may not need a fix if (#3) lands.
7. (#7-#9) Lower priority, address as time allows.
