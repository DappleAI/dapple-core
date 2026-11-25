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

Update `assets/nu/k8s/deploy_dapple_core.nu`:

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

1. `deploy_dapple_core.nu` builds Docker images and pushes to registry
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
