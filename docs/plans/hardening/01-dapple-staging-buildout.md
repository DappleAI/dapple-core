# Hardened Staging Namespace (`dapple-staging`)

**Status:** Draft
**Created:** 2026-05-17
**Last Updated:** 2026-05-17
**Target Version:** N/A (operations plan)
**Dependencies:** Existing microk8s cluster (`dapple-sandbox` operational), cert-manager wildcard cert for `*.kube.dappleai.dev`, local registry at `registry.kube.dappleai.dev`
**Scope sentence:** Stand up a fresh `dapple-staging` namespace alongside `dapple-sandbox` that is production-shaped (TLS, NetworkPolicies, role-separated database, image-hygiene, no developer affordances) so the public surface — `dapple-prompt-external-api`, `dapple-ui`, `dapple-web-landing-page` — can be validated end-to-end before customer go-live.

## 1. Executive Summary

`dapple-sandbox` is the engineering namespace. It carries `*.kube.dappleai.dev` ingresses for every HTTP service, host-network socat gateways for Postgres/Redis, a single PG superuser role used by every app pod, mutable `:dapple-latest` image tags, and a no-NetworkPolicy floor. Those properties are dev affordances, not security defects in context: nothing in dapple-sandbox is reachable from the public internet (no public DNS points at the cluster), and the workflow depends on them.

`dapple-staging` is a new sibling namespace built to be reachable from the public internet eventually. Every dev affordance is dropped. The namespace contains exactly the pods needed to serve customer traffic, fronted by exactly the three Ingresses customers will hit, with NetworkPolicies that refuse every other path. It uses its own `prompt-db` Deployment, its own JWT signing keys, its own image-pin policy, its own customer-onboarding rows. Tearing it down is `kubectl delete namespace dapple-staging`; standing it back up is the deploy script + a fresh customer keypair.

Until we point real DNS at it, `dapple-staging` is reachable inside the cluster on `*.staging.kube.dappleai.dev` (matched by the existing wildcard cert). Promotion to production swaps the hostnames to `*.dapple.x` and switches the `ClusterIssuer` from `local-dev-ca` to Let's Encrypt; everything else stays the same.

## 2. Resolved Decisions

| # | Decision | Resolution |
|---|---|---|
| D1 | Same namespace vs parallel namespace? | **Parallel namespace.** `dapple-sandbox` untouched; `dapple-staging` greenfield. Sibling in the same microk8s cluster (D2 of the previous draft). |
| D2 | Hostnames | **`*.staging.kube.dappleai.dev` for now.** Real hostnames (`external-api.dapple.x`, `app.dapple.x`, `www.dapple.x`) at promote-to-prod time, in a separate change. |
| D5 | DB role separation? | **Yes.** `dapple_admin` (DDL) and `dapple_app` (DML). Admin only mounted by the migration Job's ServiceAccount. |
| D7 | Mock-pod gate | **N/A.** `dapple-mock-openai` deleted entirely (separate change, already landed). |
| D9 | `dev-dns` | **Not deployed to dapple-staging.** |
| D10 | `rust-docs` | **Not deployed to dapple-staging.** Stays in dapple-sandbox only. |
| D11 | Socat gateway DaemonSets | **Not deployed to dapple-staging.** |

Decisions still open (defer until implementation):

| # | Decision | Default |
|---|---|---|
| D3 | Rate limit on `POST external-api.dapple.x/api/get-service-token` | 5 rps / 20 burst per IP |
| D4 | Rate limit on the rest of `/api/*` | 30 rps per JWT |
| D6 | Conduit egress allow-list strategy | Explicit FQDN allow-list (anthropic.com, huggingface.co), no forward proxy |
| D8 | dapple-ui prod runtime (`pnpm dev` → `next start`) | Tracked separately; staging starts with `pnpm dev` against hostPath, hardened image is a follow-up plan |

## 3. Namespace Inventory

Exactly ten pods. Each carries the minimum NetworkPolicy needed for its job.

| # | Pod | Public? | Source | Probes | Secrets it mounts | Egress need |
|---|---|---|---|---|---|---|
| 1 | `dapple-prompt-external-api` | **Yes** | this repo | TCP 8081 | `dapple-prompt-external-auth-staging` | conduit (in-NS) only |
| 2 | `dapple-ui` | **Yes** | external repo | none today (see D8) | `dapple-ui-auth-staging` | dapple-prompt-api (in-NS), Google OAuth, PostHog |
| 3 | `dapple-web-landing-page` | **Yes** | external repo | none today (see D8) | `dapple-ui-auth-staging` subset | dapple-prompt-api (in-NS), PostHog |
| 4 | `dapple-prompt-api` | No (in-NS only) | this repo | HTTP `/api/health_check` | `dapple-prompt-auth-staging`, `prompt-db-staging-app` | conduit, flow (in-NS) |
| 5 | `dapple-prompt-conduit` | No | this repo | HTTP `/api/health_check` | `dapple-llm-keys-staging`, `dapple-prompt-auth-staging`, `prompt-db-staging-app` | api.anthropic.com (443 only), conduit-onnx, conduit-llm (in-NS) |
| 6 | `conduit-onnx` | No | this repo | HTTP `/api/health_check` | `conduit-internal-auth-staging` | none |
| 7 | `conduit-llm` | No | this repo | HTTP `/api/health_check` | `dapple-llm-keys-staging` (HF), `conduit-internal-auth-staging` | huggingface.co (443 only, pre-warm before deploy) |
| 8 | `dapple-prompt-flow` | No | this repo | HTTP `/api/health_check` | `dapple-prompt-auth-staging`, `prompt-db-staging-app` | conduit (in-NS) |
| 9 | `prompt-db` | No | this repo | TCP 5432 | `prompt-db-staging-admin` (Job-only), `prompt-db-staging-app` | none |
| 10 | `dapple-redis` | No | this repo | TCP 6379 | `dapple-redis-secret-staging` | none |

Explicitly NOT in dapple-staging: `dapple-mock-openai` (deleted), `dapple-prompt-mock-api`, `dev-dns`, `rust-docs`, `prompt-db-gateway`, `dapple-redis-gateway`, Flagsmith.

## 4. Cluster Prerequisite

Once, at the cluster level:

```bash
microk8s enable network-policy
```

This activates Calico's NetworkPolicy enforcement. **dapple-sandbox is unaffected** because no NetworkPolicy objects exist there — the addon only enforces policies that are defined. Add this line to `assets/nu/k8s/setup_microk8s.nu` next to the other `enable` calls so a fresh cluster picks it up automatically.

Verification: `microk8s status | grep network-policy` shows `enabled`.

## 5. Namespace Bootstrap

Stand up an empty `dapple-staging` with a default-deny floor before any pods land. Doing this in this order means pods deploy into an already-policy-enforced namespace and can be tested as the policies are added.

1. `Namespace dapple-staging` with label `kubernetes.io/metadata.name: dapple-staging` (used by sibling-namespace selectors).
2. `ResourceQuota` capping CPU/memory/storage so a runaway staging pod can't take down the cluster.
3. `NetworkPolicy default-deny-all` selecting all pods, denying all ingress and egress.
4. `NetworkPolicy allow-dns-egress` selecting all pods, allowing UDP+TCP/53 to `kube-system/kube-dns`.

That is the entire bootstrap surface. After this point, no pod in the namespace can do anything but resolve DNS; every per-pod policy added below is an allow-rule on top of this floor.

## 6. Per-Pod NetworkPolicies

One policy per pod, named `allow-<pod>`. Each policy is small (∼20 lines of YAML) and only lists the connections the audit table above says the pod needs.

Patterns:
- **Public ingress** (3 pods): allow `from: namespaceSelector{kubernetes.io/metadata.name: ingress}` on the listener port.
- **In-namespace caller** (most): `from: podSelector{app: <caller>}` in the same namespace.
- **External egress** (conduit, conduit-llm): allow `to: ipBlock{cidr: 0.0.0.0/0, except: [10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16]}` on TCP/443. This forces in-cluster traffic through the explicit pod-selector rules and lets only public-internet traffic out.

Critical refusal: `dapple-prompt-api` policy does NOT list `namespaceSelector{ingress}` as an allowed source. Combined with the default-deny floor, that closes off any path from the public ingress controller into the internal API. A misconfigured Ingress object somewhere else can't reach it.

All policies live in `assets/k8s/dapple-staging/network-policies/`. Apply as a directory: `kubectl apply -f assets/k8s/dapple-staging/network-policies/`. They are deletable as a directory too, which is the rollback.

## 7. Ingress Hardening

Three Ingress resources, each with the same hardening annotations:

```yaml
# TLS
nginx.ingress.kubernetes.io/ssl-redirect: "true"
nginx.ingress.kubernetes.io/force-ssl-redirect: "true"
nginx.ingress.kubernetes.io/ssl-protocols: "TLSv1.2 TLSv1.3"
nginx.ingress.kubernetes.io/ssl-ciphers: "<modern cipher list, see implementation>"
# HSTS
nginx.ingress.kubernetes.io/hsts: "true"
nginx.ingress.kubernetes.io/hsts-max-age: "31536000"
nginx.ingress.kubernetes.io/hsts-include-subdomains: "true"
nginx.ingress.kubernetes.io/hsts-preload: "true"
# Rate limit (per source IP)
nginx.ingress.kubernetes.io/limit-rps: "<D3/D4 thresholds>"
nginx.ingress.kubernetes.io/limit-connections: "20"
# Body size
nginx.ingress.kubernetes.io/proxy-body-size: "50m"
# Server header strip
nginx.ingress.kubernetes.io/server-snippet: |
  more_clear_headers "Server";
  more_clear_headers "X-Powered-By";
```

UI / landing-page additionally get `Content-Security-Policy`, `X-Frame-Options: DENY`, `Referrer-Policy: strict-origin-when-cross-origin`, `Permissions-Policy` via `configuration-snippet`.

`external-api` does NOT enable CORS (it's server-to-server; browsers don't call it). UI and landing-page do, with the UI hostname as the only allowed origin.

**Host validation** is already correct in ingress-nginx: a request whose `Host:` header doesn't match a configured Ingress returns 404 default-backend. Verified with `curl -sk -H 'Host: random.example.com' https://<edge-ip>/`.

Cert issuance: at staging, `cert-manager.io/cluster-issuer: local-dev-ca`, hostnames under `*.staging.kube.dappleai.dev` matched by the existing wildcard cert. Promotion swaps to a Let's Encrypt `ClusterIssuer` with per-hostname certs.

## 8. Database Posture

Own `prompt-db` Deployment in `dapple-staging`, on its own PVC. Not shared with `dapple-sandbox` at the Service or PVC level.

Two roles inside that Postgres instance, created by the entrypoint init script (NOT by the application migrations — order-of-operations matters):

| Role | Capabilities | Used by |
|---|---|---|
| `dapple_admin` | superuser-equivalent; owns all schemas | the migration Job, and only the migration Job |
| `dapple_app` | DML only on the app schemas (`web`, `rev`, `public`, `auth`); no DDL, no superuser | every application pod in `dapple-staging` |

Two secrets:
- `prompt-db-staging-admin` — mounted only by the `prompt-db-migrate-job` ServiceAccount.
- `prompt-db-staging-app` — mounted by every application pod via `DAPPLE_PROMPT_DATABASE_URL`.

The migration flow becomes:

1. Each deploy renders `assets/k8s/dapple-staging/registry/prompt-db-migrate-job.yaml` with a `metadata.generateName` so it never collides.
2. Job runs `sqlx migrate run` with admin creds, `backoffLimit: 0`, then exits.
3. Application pods restart (`kubectl rollout restart`) and pick up code that the migration just made valid.

No socat gateway. Local engineering reaches `prompt-db` in dapple-staging via `kubectl port-forward -n dapple-staging svc/prompt-db 15432:5432` and `psql -h localhost -p 15432` — when they need to, which should be rare.

Verification: `kubectl exec -n dapple-staging deploy/dapple-prompt-api -- psql ... -c 'CREATE TABLE _check (id INT)'` returns `permission denied`. The app role cannot DDL even if compromised.

## 9. Conduit Egress

Conduit needs two external hostnames: `api.anthropic.com` for Anthropic models and `huggingface.co` for first-time model downloads. NetworkPolicy can't allow by FQDN directly — only by IP CIDR — so the chosen approach is:

- conduit egress NetworkPolicy: allow TCP/443 to `0.0.0.0/0` minus RFC1918, plus the in-cluster pod-selector allows for conduit-onnx, conduit-llm, prompt-db.
- conduit-llm: pre-warm the HuggingFace cache as part of the deploy (the existing model-download flow already supports this). With the cache warm, runtime egress to `huggingface.co` is unnecessary. Once that's verified for a few release cycles, drop the public-internet egress from conduit-llm entirely.

This is weaker than a true FQDN allow-list but materially better than the current "any pod can call anywhere." A proper egress proxy (Squid, OpenPolicyAgent-with-Cilium, or NetworkPolicy with FQDN support via Calico's enterprise extensions) is a possible upgrade later.

## 10. Image Hygiene

For each of the three public pods, the deploy script computes the image digest after `docker push` and writes it into the manifest before `kubectl apply`:

```yaml
image: registry.kube.dappleai.dev/dapple-prompt-external-api@sha256:<computed>
imagePullPolicy: IfNotPresent
```

Internal pods (api, conduit, flow, conduit-onnx, conduit-llm) stay on `:dapple-latest` with `imagePullPolicy: Always`. A malicious tag swap on those requires already being inside the cluster, where the NetworkPolicies in section 6 make lateral movement structurally harder.

Public base images (`node:24.8-bookworm`, `alpine:3.20`, `redis:8.2.1-bookworm`, `coredns/coredns`, `curlimages/curl`, etc.) are mirrored into `registry.kube.dappleai.dev` and referenced from the mirror in dapple-staging manifests. Docker Hub outage / supply-chain compromise of an upstream tag does not affect the staging deploy.

Deploy-script change lives in `assets/nu/k8s/deploy_dapple_staging_pods.nu` (new file). The dapple-sandbox deploy script keeps its current `:dapple-latest` behavior unchanged.

## 11. Secret Handling

Mount mode: JWT signing keys (`DAPPLE_PROMPT_API_SIGNING_KEY`, `DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY`) are projected onto tmpfs paths and read on startup, not exposed via `env: { valueFrom: secretKeyRef }`. This is a defense-in-depth change: a panic stack trace can't leak `/proc/<pid>/environ`. Pattern:

```yaml
volumes:
  - name: signing-keys
    projected:
      sources:
        - secret:
            name: dapple-prompt-auth-staging
            items: [{ key: DAPPLE_PROMPT_API_SIGNING_KEY, path: api-signing-key }]
volumeMounts:
  - name: signing-keys
    mountPath: /var/run/secrets/dapple-keys
    readOnly: true
```

Code reads `/var/run/secrets/dapple-keys/api-signing-key` at startup, holds it in memory, and never touches `std::env::var` for it.

Per-pod ServiceAccount: every pod runs as `dapple-staging-<pod>-sa` with `automountServiceAccountToken: false` and no RBAC bindings. The ServiceAccount exists purely as a handle for audit-log filtering and possible future RBAC.

Key divergence check: `dapple-prompt-external-api` on startup verifies it can NOT read `dapple-prompt-auth-staging` (refuses to start if it can). Already enforced at secret-mount level in the manifest; the runtime check is paranoia + auditable proof.

Rotation runbook (separate doc, not blocking): comma-split signing keys, 30-day grace window, rolling redeploy. The code already supports comma-split signers (see `AUTH_TOKEN_ALLOWED_SIGNERS`).

## 12. Customer Onboarding (staging)

Once `dapple-staging` is stable, customer re-onboarding is:

1. Customer generates a fresh Ed25519 keypair locally (different from their dapple-sandbox keypair).
2. Run `dapple external company create <slug> --namespace dapple-staging` (CLI grows a `--namespace` flag).
3. `INSERT INTO rev.companies` in the dapple-staging Postgres.
4. `INSERT INTO rev.external_service_tokens` with the customer's public key.
5. Hand the `service_id` back to the customer.
6. Customer points their integration at `external-api.staging.kube.dappleai.dev` (or whatever the final hostname is once DNS is decided).

The customer's existing sandbox tokens keep working in dapple-sandbox for testing. The two are entirely independent.

## 13. Deploy-Script Changes

`assets/nu/k8s/deploy_dapple_staging_pods.nu` is a new file, modeled on `deploy_dapple_prompt_pods.nu` but with structural refusals:

- Refuses to deploy `dapple-mock-openai` (already gone, but a belt-and-suspenders check).
- Refuses to deploy `dev-dns`, `prompt-db-gateway`, `dapple-redis-gateway`.
- Refuses to deploy `rust-docs`.
- Refuses to deploy without `network-policy` addon enabled.
- Defaults to digest-pinned images for the three public pods; refuses to apply if a build came from a dirty git tree.

The dapple-sandbox deploy script is unchanged.

## 14. Verification Checklist

Run against `dapple-staging` after deploy. Every line should produce the indicated result. CI runs this against a fresh `dapple-staging` build.

```bash
# (1) network-policy enforcement active
microk8s status | grep "network-policy.*enabled"

# (2) default-deny floor
kubectl get networkpolicy -n dapple-staging default-deny-all
# expect: object exists, PodSelector <none>, PolicyTypes Ingress,Egress

# (3) public pods reachable via the public ingress
curl -sk -o /dev/null -w "%{http_code}\n" https://external-api.staging.kube.dappleai.dev/api/health_check
# expect: 200 (or 401, with WWW-Authenticate)

# (4) internal pods NOT reachable from ingress namespace
kubectl run --rm -it nptest --image=alpine -n ingress --restart=Never -- \
  sh -c "nc -zv dapple-prompt-api.dapple-staging 8001 2>&1"
# expect: connection refused / timed out

# (5) cross-namespace block: dapple-sandbox cannot reach dapple-staging pods
kubectl run --rm -it nptest --image=alpine -n dapple-sandbox --restart=Never -- \
  sh -c "nc -zv prompt-db.dapple-staging 5432 2>&1"
# expect: connection refused / timed out

# (6) public ingress count = 3
kubectl get ingress -n dapple-staging -o name | wc -l
# expect: 3

# (7) public pods digest-pinned
for d in dapple-prompt-external-api dapple-ui dapple-web-landing-page; do
  kubectl get deploy -n dapple-staging $d -o jsonpath="{.spec.template.spec.containers[0].image}" | grep "@sha256:" >/dev/null || echo "MISSING DIGEST: $d"
done
# expect: no output

# (8) signing keys diverge between internal and external secrets
kubectl get secret -n dapple-staging dapple-prompt-auth-staging -o jsonpath="{.data.DAPPLE_PROMPT_API_SIGNING_KEY}" | base64 -d | sha256sum
kubectl get secret -n dapple-staging dapple-prompt-external-auth-staging -o jsonpath="{.data.DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY}" | base64 -d | sha256sum
# expect: two different digests

# (9) app pod cannot DDL
kubectl exec -n dapple-staging deploy/dapple-prompt-api -- \
  psql $DAPPLE_PROMPT_DATABASE_URL -c "CREATE TABLE _check (id INT)" 2>&1 | grep "permission denied"
# expect: permission denied

# (10) no DaemonSet with hostNetwork
kubectl get daemonset -n dapple-staging -o json | jq -r '.items[].spec.template.spec.hostNetwork' | grep -v null | grep true
# expect: no output

# (11) dev-dns, rust-docs, mock-openai, db/redis gateways absent
for d in dev-dns rust-docs dapple-mock-openai prompt-db-gateway dapple-redis-gateway; do
  kubectl get deploy -n dapple-staging $d 2>&1 | grep "not found" >/dev/null || echo "PRESENT IN STAGING: $d"
done
# expect: no output

# (12) conduit can reach Anthropic
kubectl exec -n dapple-staging deploy/dapple-prompt-conduit -- \
  curl -sko /dev/null -w "%{http_code}\n" https://api.anthropic.com/
# expect: 401 or 404 (request fails auth; TLS handshake succeeded)

# (13) conduit cannot reach an arbitrary public host
kubectl exec -n dapple-staging deploy/dapple-prompt-conduit -- \
  timeout 5 curl -sk https://example.com/ 2>&1 | grep -iE "could not|timeout|denied"
# expect: a connection failure
```

## 15. Rollout Phasing

Four phases. Each is independently revertable by `kubectl delete namespace dapple-staging` (which removes everything in one shot).

**Phase A — Cluster prerequisite + namespace bootstrap.**
1. `microk8s enable network-policy` (cluster-wide; dapple-sandbox unaffected).
2. Create `dapple-staging` namespace + ResourceQuota + default-deny floor + DNS rescue.
Verify: section 14 (1) and (2).

**Phase B — Database + secrets.**
1. Deploy `prompt-db` Deployment with PVC, two roles (init script), two Secrets.
2. Deploy `dapple-redis` with its own secret.
3. Apply `prompt-db-migrate-job.yaml` (creates schema with admin role).
Verify: section 14 (9).

**Phase C — Internal pods.**
1. Deploy api, conduit, conduit-onnx, conduit-llm, flow with NetworkPolicies.
2. Pre-warm conduit-llm's HuggingFace cache.
Verify: section 14 (4), (12), (13).

**Phase D — Public pods + ingresses.**
1. Deploy external-api, ui, landing-page with NetworkPolicies + Ingresses + hardening annotations.
2. Validate TLS / HSTS / rate-limit posture.
3. Run section 14 in full.

**Phase E — Customer onboarding.**
1. Onboard a single test customer (or migrate acme) against dapple-staging.
2. Customer hits `external-api.staging.kube.dappleai.dev/api/...` end-to-end.
3. Validate audit-trail rows appear in dapple-staging Postgres.

Promotion to production (separate plan when ready):
- Repoint DNS from `*.kube.dappleai.dev` to `dapple.x` hostnames.
- Swap `ClusterIssuer` from `local-dev-ca` to Let's Encrypt.
- Replicate the namespace shape into a new `dapple-prod` namespace (or rename, depending on how cleanly the manifests are templated).

## 16. Documentation Touchpoints

When this plan starts shipping:

- **`assets/k8s/AGENTS.md`** — Add a "Multi-Namespace Topology" section describing `dapple-sandbox` (dev) vs `dapple-staging` (production-shaped) vs eventual `dapple-prod`. Reference this plan.
- **`crates/dapple-prompt-external-api/AGENTS.md`** — Surface posture section gets a note about NetworkPolicy isolation in dapple-staging.
- **`crates/dapple-prompt-api/AGENTS.md`** — Single-Tenant Posture section: note that dapple-staging additionally refuses public ingress at the NetworkPolicy layer.
- **`crates/dapple-data/AGENTS.md`** — Migrations section: in dapple-staging, migrations run as a Job with the admin role; app pods carry app-role credentials only.
- **`assets/nu/k8s/setup_microk8s.nu`** — Add `microk8s enable network-policy` line.
- **`CLAUDE.md`** — Add a bullet under Multi-Surface Architecture noting that dapple-staging adds NetworkPolicy enforcement on top of the cryptographic isolation.

## 17. Out of Scope / Followups

- **dapple-ui prod runtime (D8).** dapple-staging starts with the current `pnpm dev` against hostPath, same as dapple-sandbox. A hardened Node image (`pnpm build` → `next start`, no source mount, readOnlyRootFilesystem) is a separate plan blocking promotion to dapple-prod.
- **Real DNS + Let's Encrypt.** Staging uses `*.kube.dappleai.dev`. Real `dapple.x` certs need a DNS-01 ACME challenge (we don't control HTTP-01 reachability until DNS points at the cluster). Tracked separately.
- **Egress proxy.** Section 9's CIDR-based allow rules are coarse. A proper FQDN-aware egress proxy (Cilium with FQDN policy, or Squid + iptables) is a possible upgrade.
- **Key rotation runbook.** Documented but not automated.
- **Audit-log scraping.** dapple-staging needs structured logs forwarded somewhere (Loki / external sink). Out of scope until prod is real.
- **Monitoring / alerting.** Same.
