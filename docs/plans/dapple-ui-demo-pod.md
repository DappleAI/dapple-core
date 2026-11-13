# Plan: provision `dapple-ui-demo` pod

## Context

The dapple-ui team requested a sibling pod to `dapple-ui` to host a curated,
public-facing showcase of the same product (`packages/ui-demo` in their
monorepo). The pod runs out of the same monorepo source via a different pnpm
filter and shares the existing `dapple-ui-auth` Secret so NextAuth sessions are
interchangeable across the two hosts.

Original handoff: `../dapple-ui/docs/dapple-demo-pod-handoff.md` (to be replaced
with provisioned-state notes once this work lands).

## Decisions

| Item | Value |
| --- | --- |
| Pod / service / ingress / deployment name | `dapple-ui-demo` |
| Public hostname | `dapple-ui-demo.kube.dappleai.dev` |
| Container port | `4445` |
| Source path | same monorepo as `DAPPLE_UI_SOURCE_DIR`; opted in independently via `DAPPLE_UI_DEMO_SOURCE_DIR` |
| pnpm filter / package directory | `ui-demo` / `packages/ui-demo` (workspace convention: existing siblings are `ui` and `api-auth`, no `dapple-` prefix; dapple-ui's updated handoff used `dapple-ui-demo` and needs to be brought back in line) |
| Service-token `service_id` | `dapple_ui_demo` |
| Service-token display name | `Dapple UI Demo` |
| SAT key env var | `DAPPLE_UI_DEMO_SAT_SIGNING_KEY` (Ed25519, 64 hex) |
| Service-id env var | `DAPPLE_UI_DEMO_SERVICE_ID=dapple_ui_demo` |
| Per-pod NEXTAUTH_URL value | `https://dapple-ui-demo.kube.dappleai.dev` |
| Per-pod NEXTAUTH_URL secret key | `NEXTAUTH_URL_DAPPLE_UI_DEMO` (original `NEXTAUTH_URL` unchanged) |
| SAT key generation | `ensure dapple_ui_demo --name "Dapple UI Demo"` (auto-generates, writes `~/.env.dapple`, registers DB) |
| Reused secret | `dapple-ui-auth` (extended in-place via the atomic delete+create flow) |
| Shared cookie domain | `NEXTAUTH_COOKIE_DOMAIN=.kube.dappleai.dev` — env-var-driven, read by **both** dapple-ui and dapple-ui-demo pods |

## Prerequisites (off our side)

These must land before the operator runs the deploy script:

1. `packages/ui-demo/package.json` exists in `../dapple-ui/` with package name `ui-demo` so `pnpm --filter ui-demo` resolves. Without this the pod CrashLoopBackOffs.
2. dapple-ui's NextAuth cookie scope patch (cookies scoped to `.kube.dappleai.dev`) merged in `packages/ui` and `dapple-ui` redeployed. Required for session sharing.
3. Google OAuth redirect URI `https://dapple-ui-demo.kube.dappleai.dev/api/auth/callback/google` added to the OAuth client identified by `OAUTH_GOOGLE_CLIENT_ID`. Required for first OAuth round-trip.

Operator steps on our side: none.

- `DAPPLE_UI_DEMO_SOURCE_DIR` is derived in `assets/nu/modules/dapple_env_profiles/dapple_sandbox.nu` as an alias of `DAPPLE_UI_SOURCE_DIR` (same monorepo, different pnpm filter). Convention matches every other pod's source-dir, which are all derived in the profile rather than set by hand. Operators get it automatically after running `nu assets/nu/init/setup_nu.nu` and starting a fresh shell.
- No manual SAT key generation needed — the deploy script's first run calls `ensure`, which writes `DAPPLE_UI_DEMO_SAT_SIGNING_KEY=<hex>` to `~/.env.dapple` and registers the public key in `web.service_tokens`.

## Files changed in this plan

### Added

- `assets/k8s/registry/dapple-ui-demo.yaml` — Deployment + Service + Ingress for the demo pod. Mirrors `dapple-ui.yaml` with port `4445`, host `dapple-ui-demo.kube.dappleai.dev`, pnpm filter `ui-demo`, and three swapped secret keys (`NEXTAUTH_URL_DAPPLE_UI_DEMO`, `DAPPLE_UI_DEMO_SAT_SIGNING_KEY`, `DAPPLE_UI_DEMO_SERVICE_ID`). Every other env entry and the entire bash startup script are byte-identical to dapple-ui.

### Modified

- `assets/nu/k8s/deploy_dapple_web_pods.nu` — five additions, all mirroring the existing dapple-ui plumbing:
  1. Import `ensure` alongside `register` from `service_token.nu`.
  2. `validate_web_source_paths` gains a `dapple-ui-demo` entry guarded on `DAPPLE_UI_DEMO_SOURCE_DIR`; operator-help text now mentions the new env var.
  3. `ensure_vm_mounts` call in `main` includes the new source dir (no-op under OrbStack; kept for symmetry).
  4. New deploy block in `main` after the existing dapple-ui block — applies `registry/dapple-ui-demo.yaml`, rollout-restarts, waits for rollout.
  5. `setup_dapple_ui_secret` calls `ensure dapple_ui_demo` (returns private key), and appends three `--from-literal` lines (`DAPPLE_UI_DEMO_SAT_SIGNING_KEY`, `DAPPLE_UI_DEMO_SERVICE_ID`, `NEXTAUTH_URL_DAPPLE_UI_DEMO`) to the existing `kubectl create secret generic dapple-ui-auth` invocation.
- `assets/k8s/AGENTS.md` — `dapple-ui Secrets` table extended with the three new keys; explanatory note added that `dapple-ui-auth` is now shared between two pods, each reading a distinct NEXTAUTH_URL key, while both share `NEXTAUTH_SECRET` for cookie validation.

### Intentionally NOT changed

- `assets/nu/modules/dapple_env_profiles/*.nu` — operator source-dir lives in `~/.env`, not a checked-in profile.
- `assets/nu/modules/tools/service_token.nu` — `ensure` already covers our needs (auto-generate + write to `~/.env.dapple` + DB upsert + return key).
- `assets/k8s/registry/dapple-ui.yaml` — the cookie-scope change is dapple-ui application code, not infra.

## Verification (post-deploy, after prerequisites land)

```bash
# Pod scheduled and Running
nu -c 'kubectl get pods -n dapple-sandbox | grep dapple-ui-demo'

# Startup logs clean (pnpm install + api-auth build + next dev)
nu -c 'kubectl logs -n dapple-sandbox deploy/dapple-ui-demo --tail=200'

# Ingress reachable from host
nu -c 'curl -k -o /dev/null -w "%{http_code}\n" https://dapple-ui-demo.kube.dappleai.dev/'

# Intra-cluster connectivity to dapple-prompt-api
nu -c 'kubectl exec -n dapple-sandbox deploy/dapple-ui-demo -- bash -lc "wget -qO- http://dapple-prompt-api:8001/api/health_check"'

# Service token registered (Ed25519, ACTIVE)
nu -c 'psql $env.DAPPLE_PROMPT_DATABASE_URL -c "SELECT service_id, name, key_algorithm, created_at FROM web.service_tokens WHERE service_id = $$dapple_ui_demo$$;"'

# Secret carries all three new keys
nu -c 'kubectl get secret dapple-ui-auth -n dapple-sandbox -o json | jq ".data | keys[] | select(test(\"DAPPLE_UI_DEMO|NEXTAUTH_URL_DAPPLE_UI_DEMO\"))"'
```

End-to-end SSO walkthrough (sign in on `dapple-ui.kube.dappleai.dev`, navigate
to `dapple-ui-demo.kube.dappleai.dev` and observe authenticated session) is a
dapple-ui-side concern and depends on prerequisite 2.
