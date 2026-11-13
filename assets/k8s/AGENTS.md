# AGENTS.md — Kubernetes Manifests

This directory contains Kubernetes manifest templates and service-specific configurations for the dapple-prompt infrastructure.

## Deployment Topology (Multi-Surface)

dapple-prompt runs as **three logical API surfaces**, each fronted by its own K8s ingress and its own Deployment. V1 ships internal and external as separate binaries (`dapple-prompt-api` and `dapple-prompt-external-api`); the long-term plan (Pattern B) consolidates them under a single `dapple-prompt-server --surface={internal,external,user}` binary, but the structural-isolation property is achieved either way.

| Surface | Hostname | Listener port | Public exposure | Token table | Signing key |
|---|---|---|---|---|---|
| **Internal** | `api.dapple.x` | 8080 (default) | **Cluster-internal only** — NetworkPolicy refuses traffic from public ingress | `web.service_tokens` | internal-only |
| **External** | `external-api.dapple.x` | 8081 | Public | `rev.external_service_tokens` | external-only |
| **User** (future) | `user-api.dapple.x` | 8082 | Public | TBD | user-only |

The same `dapple-prompt-server` binary serves any of the three surfaces; the `--surface` flag selects which router to mount, which port to bind, and which JWT signing key to load. K8s runs each surface as its own Deployment so they scale independently and have independent NetworkPolicies.

**Cross-surface JWT replay is impossible by signing-key separation.** A JWT minted by the external surface cannot be verified by the internal surface (or vice versa) — verification fails at signature check before any claim is read. This is *cryptographic* isolation, not just an `aud`-claim convention.

**Crate layout:**

```
crates/
  dapple-prompt-api/             ← internal handlers, mounted by --surface=internal
  dapple-prompt-external-api/    ← external handlers, mounted by --surface=external
  dapple-prompt-user-api/        ← reserved stub, not built in V1
  dapple-prompt-server/          ← main binary; --surface flag selects router
```

**Per-surface manifests follow this naming pattern**:

- `assets/k8s/registry/dapple-prompt-api.yaml` — internal Deployment, port 8001, ingress `api.dapple.x`
- `assets/k8s/registry/dapple-prompt-external-api.yaml` — external Deployment, port 8081, ingress `external-api.dapple.x`
- (future) `assets/k8s/registry/dapple-prompt-user-api.yaml` — user Deployment, ingress `user-api.dapple.x`

Each manifest includes:
- A NetworkPolicy that restricts ingress (internal: cluster-only; external/user: from public ingress only).
- A Secret mount for the surface-specific JWT signing key (independent rotation per surface).
- Independent resource limits and replica counts.

**Anti-patterns:**

- ❌ Mounting the external router at `/external/api/` on the internal Deployment. Surface separation is at the listener/port and Deployment level, not URL prefix.
- ❌ Sharing a JWT signing key across surfaces. Defeats the cryptographic isolation guarantee.
- ❌ Skipping NetworkPolicies on the internal Deployment. The internal surface must refuse traffic from public ingress.

## Directory Structure

```
assets/k8s/
├── config/           # Cluster configuration files
├── manifests/        # Reusable manifest templates (with {{TOKENS}})
│   ├── dapple-prompt-service.yaml   # HTTP services with health checks
│   ├── dapple-prompt-engine.yaml    # Background workers
│   └── dapple-node-sandbox.yaml     # Node.js sandbox template
└── registry/         # Service-specific manifests (applied directly)
    ├── dapple-prompt-api.yaml
    ├── dapple-prompt-conduit.yaml
    ├── dapple-prompt-flow.yaml
    ├── postgres.yaml
    ├── redis.yaml
    └── ...
```

## Templates vs Registry

### Templates (`manifests/`)

Templates use `{{TOKEN}}` syntax for variable substitution. Rendered by the `k8s render` function.

**Available Tokens:**

| Token | Description | Example |
|-------|-------------|---------|
| `{{NAME}}` | Service/deployment name | `dapple-prompt-api` |
| `{{PORT}}` | Container port | `8001` |
| `{{IMAGE_NAME}}` | Docker image name (if different from NAME) | `dapple-prompt-api` |
| `{{NAMESPACE}}` | K8s namespace | `dapple-sandbox` |
| `{{NODE_PORT}}` | NodePort for external access | `31082` |

### Registry (`registry/`)

Fully-specified manifests that are applied directly via `kubectl apply -f`. Use these when:

- Service needs custom environment variables
- Non-standard resource limits
- Custom health check paths or timing
- Additional volumes or secrets

## Template Types

### `dapple-prompt-service.yaml`

For HTTP services that expose endpoints and require health checks.

**Includes:**
- Deployment with 1 replica
- Container port configuration
- `DAPPLE_PROMPT_DATABASE_URL` environment variable (pointing to `prompt-db`)
- Readiness probe: `GET /api/health_check` (5s initial, 5s period)
- Liveness probe: `GET /api/health_check` (10s initial, 10s period)
- ClusterIP Service
- Ingress with TLS (`*.kube.dappleai.dev`)

**Requirements:**
- Service must implement `GET /api/health_check` endpoint
- Port must be specified in deployment config

### `dapple-prompt-engine.yaml`

For background workers and processing engines that don't expose HTTP endpoints.

**Includes:**
- Deployment with 1 replica
- `DAPPLE_PROMPT_DATABASE_URL` environment variable (pointing to `prompt-db`)
- Resource limits

**Does NOT include:**
- Health probes (no HTTP endpoints)
- Service or Ingress

## Creating a New Service Manifest

### Step 1: Add Health Check Endpoint

If using `dapple-prompt-service.yaml` template, implement:

```rust
// Axum example
async fn health_check() -> &'static str {
    "OK"
}

let app = Router::new()
    .route("/api/health_check", get(health_check))
    // ... other routes
```

### Step 2: Create Registry Manifest (if needed)

Copy an existing manifest and modify:

```yaml
# assets/k8s/registry/my-new-service.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-new-service
  namespace: dapple-sandbox
  labels:
    app: my-new-service
spec:
  # ... (see existing manifests for pattern)
```

### Step 3: Add to Deployment Script

Update `assets/nu/k8s/deploy_dapple_prompt_pods.nu`:

```nushell
setup_dapple_svc {
  name: "my-new-service"
  profile: "release"
  namespace: $env.KUBE_DAPPLE_NAMESPACE
  source_path: $env.DAPPLE_PROMPT_WORKSPACE_DIR
  template: dapple-prompt-service.yaml  # or dapple-prompt-engine.yaml
  port: "8082"
  node_port: $env.DAPPLE_WEB_LANDING_PAGE_PORT
}
```

## Common Patterns

### Environment Variables

For **prompt services** (dapple-prompt-api, dapple-prompt-conduit, dapple-prompt-flow):
```yaml
env:
  - name: DAPPLE_PROMPT_DATABASE_URL
    value: postgres://dapple:ai@prompt-db:5432/dapple
  - name: MY_SERVICE_PORT
    value: "8081"
  # For secrets, use secretKeyRef:
  - name: API_KEY
    valueFrom:
      secretKeyRef:
        name: my-secret
        key: api-key
```

**Note:** `dapple-ui` does not consume `DAPPLE_PROMPT_DATABASE_URL`. Its NextAuth adapter reads broken-out `DATABASE_HOST` / `DATABASE_PORT` / `DATABASE_USER` / `DATABASE_PASSWORD` / `DATABASE_NAME` / `DATABASE_SEARCH_PATH` env vars (see the dapple-ui Secrets section below).

### Secrets and Pod Restarts (CRITICAL)

**Kubernetes does NOT automatically reload secrets into running pods.** When you update a secret, existing pods continue using the old cached values.

After updating a secret, you must restart the deployment:

```bash
kubectl rollout restart deployment/<deployment-name> -n <namespace>
```

Example after updating `dapple-ui-auth` secret:
```bash
kubectl rollout restart deployment/dapple-ui -n dapple-sandbox
```

This triggers a rolling restart that loads the new secret values into fresh pods.

### Resource Limits

Standard development limits:

```yaml
resources:
  requests: { cpu: "100m", memory: "128Mi" }
  limits:   { cpu: "500m", memory: "512Mi" }
```

### Health Probes

```yaml
readinessProbe:
  httpGet: { path: /api/health_check, port: 8081 }
  initialDelaySeconds: 5
  periodSeconds: 5
livenessProbe:
  httpGet: { path: /api/health_check, port: 8081 }
  initialDelaySeconds: 10
  periodSeconds: 10
```

## Validation

Test manifest syntax without applying:

```bash
kubectl apply --dry-run=client -f assets/k8s/registry/my-service.yaml
```

## Relationship to Nushell Scripts

1. `deploy_dapple_prompt_pods.nu` builds Docker images and pushes to registry
2. If `assets/k8s/registry/{name}.yaml` exists, it's applied via `kubectl apply`
3. Template from `manifests/` is rendered with config values
4. Rendered manifest is applied via `kubectl apply`
5. Script waits for rollout completion

The registry manifest provides override/customization, while the template provides base configuration. Both are applied, with the registry manifest taking precedence for matching resources.

## Critical: Docker Image ID Parsing

When working with Docker images in Nushell scripts, **NEVER** parse `docker images` output using column-based string splitting. See `assets/nu/AGENTS.md` for the full explanation.

**Always use:**
```nushell
let image_id = (^docker images -f $"reference=($tag)" --format "{{.ID}}" | str trim)
```

**Never use:**
```nushell
# This breaks with Docker Desktop's variable-width columns
^docker images | split column --regex "\\s+"
```

## dapple-ui Secrets

### `dapple-ui-auth` Secret

Contains all authentication-related configuration for dapple-ui's NextAuth integration.

| Key | Description | Source |
|-----|-------------|--------|
| `NEXTAUTH_SECRET` | JWT signing secret | `~/.env.dapple` (generated) |
| `NEXTAUTH_URL` | Auth callback URL | Defaults to `https://dapple-ui.kube.dappleai.dev` |
| `DAPPLE_PROMPT_API_INTERNAL_URL` | dapple-prompt-api internal URL | Defaults to `http://dapple-prompt-api` |
| `DAPPLE_UI_SAT_SIGNING_KEY` | Service Auth Token key | `~/.env.dapple` (generated) |
| `DAPPLE_UI_SERVICE_ID` | Service identifier | Hardcoded: `dapple_ui` |
| `NEXT_PUBLIC_POSTHOG_KEY` | PostHog analytics key | `~/.env` |
| `NEXT_PUBLIC_POSTHOG_HOST` | PostHog host | Defaults to `https://us.i.posthog.com` |
| `OAUTH_GOOGLE_CLIENT_ID` | Google OAuth client ID | `~/.env` |
| `OAUTH_GOOGLE_CLIENT_SECRET` | Google OAuth secret | `~/.env` |
| `DATABASE_HOST` | NextAuth DB host | Hardcoded: `prompt-db` |
| `DATABASE_PORT` | NextAuth DB port | Hardcoded: `5432` |
| `DATABASE_USER` | NextAuth DB user | Hardcoded: `dapple` |
| `DATABASE_PASSWORD` | NextAuth DB password | Hardcoded: `ai` |
| `DATABASE_NAME` | NextAuth DB name | Hardcoded: `dapple` |
| `DATABASE_SEARCH_PATH` | Schema search path | Hardcoded: `auth,public` |
| `DAPPLE_UI_DEMO_SAT_SIGNING_KEY` | dapple-ui-demo SAT key | `~/.env.dapple` (auto-generated by `ensure dapple_ui_demo` on first run) |
| `DAPPLE_UI_DEMO_SERVICE_ID` | dapple-ui-demo service identifier | Hardcoded: `dapple_ui_demo` |
| `NEXTAUTH_URL_DAPPLE_UI_DEMO` | dapple-ui-demo NextAuth callback URL | Hardcoded: `https://dapple-ui-demo.kube.dappleai.dev` |
| `NEXTAUTH_COOKIE_DOMAIN` | Shared session-cookie scope across hosts | Hardcoded: `.kube.dappleai.dev` (read by both pods) |

**Note:** The `dapple-ui-auth` Secret is shared between two pods. `dapple-ui` reads `NEXTAUTH_URL` plus its own `DAPPLE_UI_*` keys; `dapple-ui-demo` reads `NEXTAUTH_URL_DAPPLE_UI_DEMO` plus its own `DAPPLE_UI_DEMO_*` keys. Both share `NEXTAUTH_SECRET` and `NEXTAUTH_COOKIE_DOMAIN` so a cookie minted on either host validates on the other.

**Note — `NEXTAUTH_URL*` does NOT control the Google OAuth redirect.** `packages/ui/auth.config.ts` sets `trustHost: true`, so Auth.js v5 builds `redirect_uri` from the **incoming request's forwarded host** and ignores `NEXTAUTH_URL` for URL construction. Consequence: **every ingress hostname needs its own Authorized redirect URI** (`https://<host>/api/auth/callback/google`) registered in the Google Cloud Console, even though all hosts share one OAuth client. Missing one produces `Error 400: redirect_uri_mismatch` on that host only.

This is easy to miss because the shared `NEXTAUTH_COOKIE_DOMAIN` masks it — signing in once on `dapple-ui` mints a cookie valid on every `*.kube.dappleai.dev` host, so a secondary host's OAuth flow never runs until something deletes the cookie. `dapple-ui-demo` shipped without its URI registered (prerequisite #3 of [docs/plans/dapple-ui-demo-pod.md](../../docs/plans/dapple-ui-demo-pod.md)) and nothing surfaced it for 69 days.

`check_google_redirect_uris` ([assets/nu/k8s/deploy_dapple_web_pods.nu:226](../nu/k8s/deploy_dapple_web_pods.nu)) now probes each host at deploy time and warns with the exact URI to register. It warns and never fails — an unreachable Google logs at debug rather than raising a false alarm.

**Note:** dapple-ui and the prompt services share the same PG18 database (`prompt-db`). dapple-ui assembles its connection from the six `DATABASE_*` keys above (consumed by `packages/ui/lib/db/pool.ts`); prompt services read `DAPPLE_PROMPT_DATABASE_URL` (e.g. `postgres://dapple:ai@prompt-db:5432/dapple`).

**Note:** `DATABASE_SEARCH_PATH=auth,public` is required because NextAuth tables (`users`, `accounts`, `sessions`) live in the `auth` schema. The dapple-ui pool sets this search path on each connection.

**Note:** All six `DATABASE_*` values are hardcoded in `setup_dapple_ui_secret` ([assets/nu/k8s/deploy_dapple_web_pods.nu:250](assets/nu/k8s/deploy_dapple_web_pods.nu:250)) — they are not read from `~/.dapple/env`, `~/.dapple/generated.env`, or any environment profile. Changing them requires editing the script.

Created by: `deploy_dapple_web_pods.nu`
