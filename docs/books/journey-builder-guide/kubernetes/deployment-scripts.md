# Deployment Scripts

> **Audience:** Platform Engineers · Advanced Users

This document covers the nushell scripts that power the `dapple` CLI and how to use them directly when needed.

## Script Locations

```
assets/nu/
├── init/
│   └── setup.nu                 # One-time environment setup
├── k8s/
│   ├── deploy_dapple_prompt_pods.nu  # Full deployment script
│   ├── build_image.nu           # Docker image building
│   └── patch_image.nu           # Quick image updates
└── modules/
    ├── dapple/                  # dapple CLI module
    ├── dapple_env_profiles/     # Environment overlays
    ├── k8s/                     # Kubernetes utilities
    └── tools/                   # Helper scripts
```

## setup.nu

One-time setup for the development environment.

**What it does:**
1. Copies nushell modules to your nushell config dir's `modules/` (`~/.config/nushell/modules/` on Linux, `~/Library/Application Support/nushell/modules/` on macOS)
2. Generates `~/.env.dapple` with secrets
3. Configures `config.nu` to load the dapple overlay
4. Verifies kubectl connectivity

**Usage:**
```bash
nu assets/nu/init/setup.nu --environment sandbox
exec nu  # Restart nushell
```

**When to re-run:**
- After updating modules in the repo
- If `~/.env.dapple` needs regeneration
- After a fresh nushell install

## build_image.nu

Builds and pushes Docker images for Dapple services.

**Features:**
- Skips build if image already exists (unless forced)
- Detects uncommitted changes and forces rebuild
- Tags images with commit hash and `dapple-latest`

**Usage:**
```bash
nu assets/nu/k8s/build_image.nu <service> [flags]
```

**Arguments:**
| Arg | Description |
|-----|-------------|
| `service` | Service alias (api, conduit, flow, etc.) |

**Flags:**
| Flag | Description |
|------|-------------|
| `--force`, `-f` | Force rebuild |
| `--profile` | Build profile (default: "dev") |

**Examples:**
```bash
nu assets/nu/k8s/build_image.nu api
nu assets/nu/k8s/build_image.nu api --force
nu assets/nu/k8s/build_image.nu conduit --profile release
```

**Output:**
Prints the image tag on success (useful for piping):
```
registry.kube.dappleai.dev/dapple-prompt-api:abc123
```

## patch_image.nu

Updates a deployment with a new image without applying manifests.

**Why use this:**
- Faster than full deploy
- No manifest changes applied
- No migrations run
- Simple rolling update

**Usage:**
```bash
nu assets/nu/k8s/patch_image.nu <service> [flags]
```

**Flags:**
| Flag | Description |
|------|-------------|
| `--tag` | Specific image tag (default: dapple-latest) |
| `--timeout` | Rollout timeout (default: "120s") |

**Examples:**
```bash
nu assets/nu/k8s/patch_image.nu api
nu assets/nu/k8s/patch_image.nu api --tag abc123
```

## deploy_dapple_prompt_pods.nu

Full deployment script including infrastructure setup.

**What it does (full deploy):**
1. Ensures sqlx-cli is installed
2. Runs database migrations
3. Ensures service tokens
4. Prepares SQLx query cache
5. Sets up Docker registry certificate
6. Creates auth and database secrets
7. Builds and pushes Docker images
8. Applies Kubernetes manifests
9. Waits for rollout completion

**What it does (targeted deploy):**
1. Sets up Docker registry certificate
2. Builds and pushes Docker images
3. Applies Kubernetes manifests
4. Waits for rollout completion

**Usage:**
```bash
nu assets/nu/k8s/deploy_dapple_prompt_pods.nu [flags]
```

**Deployment modes (most current):**

| Mode | Flag | Behavior |
|------|------|----------|
| **hostPath** (default) | none | Compiles each service with cargo, then mounts `target/debug/` into the pod and runs your local binary directly. No Docker build. Best for iterating on uncommitted code. |
| **Docker** | `--docker dev` | Builds a Docker image, pushes to the registry, deploys from the image. Refuses to run on a dirty git tree (the dirty-tree guard prevents content-hash cache poisoning, where a dirty build is tagged with the clean hash). Use for CI / staging. |

**Flags:**
| Flag | Description |
|------|-------------|
| `--svc` | Deploy specific services (comma-separated) |
| `--docker <profile>` | Switch to Docker mode with the given build profile (typically `dev`) |
| `--force` | Force rebuild |
| `--no-build` | hostPath only: skip the cargo build and mount the binary already in `target/` |

**The hostPath build is part of the deploy.** A hostPath pod executes the binary
sitting in `target/debug/`, so if that binary is stale the rollout still succeeds
and the code you just wrote never runs — a silent-wrong-version failure that
looks exactly like a successful deploy. The script therefore builds each hostPath
service itself before patching the deployment (`cargo build` on Linux, `cargo
zigbuild` on macOS). Cargo's freshness check makes an already-current build a
fast no-op, so re-running the deploy after an edit is the whole loop:

**Examples:**
```bash
# Default (hostPath): builds dapple-prompt-api, then mounts and rolls the pod
nu assets/nu/k8s/deploy_dapple_prompt_pods.nu --svc api

# Edit code, run the exact same command — the rebuild is part of the deploy
nu assets/nu/k8s/deploy_dapple_prompt_pods.nu --svc api

# Mount a binary you already built, skipping the build entirely
nu assets/nu/k8s/deploy_dapple_prompt_pods.nu --svc api --no-build

# Docker mode (requires clean git tree)
nu assets/nu/k8s/deploy_dapple_prompt_pods.nu --docker dev --svc api

# Multiple services in default (hostPath) mode
nu assets/nu/k8s/deploy_dapple_prompt_pods.nu --svc api,conduit
```

## Platform Behavior

The deployment scripts detect the platform and adjust behavior accordingly.

### Linux

MicroK8s runs natively. hostPath volumes work directly with the host filesystem. No additional setup required.

### macOS

MicroK8s runs inside an [OrbStack](https://orbstack.dev)-hosted Ubuntu VM.
OrbStack auto-mounts `/Users/$USER` two-way at the same path inside every
Linux VM, so the deploy scripts do **not** need to register host paths
individually. The scripts automatically:

1. **Detect GPU availability** (typically none on Mac VMs)
2. **Patch ONNX model paths** in manifests to use host paths (visible in the VM via auto-mount)
3. **Log certificate paths** for Docker Desktop troubleshooting

**Before deploying conduit with ONNX models:**

```bash
# Convert/download ONNX models to host cache
nu assets/nu/k8s/convert_onnx_models.nu

# Verify models are ready
nu assets/nu/k8s/convert_onnx_models.nu verify
```

**The `ensure_vm_mounts` function:**

```nushell
use k8s
k8s ensure_vm_mounts [
    $env.DAPPLE_ONNX_CACHE_DIR
    $env.DAPPLE_PROMPT_WORKSPACE_DIR
]
```

This function is now a logging stub — OrbStack's auto-mount makes the
prior per-path mount registration unnecessary. The signature is preserved so
existing call sites continue to compile. It:

- No-ops on Linux (paths accessible directly)
- On macOS: validates that each path is under `/Users/$USER` (which is auto-mounted) and warns if not

## Available Services

| Service | Description |
|---------|-------------|
| `api` | dapple-prompt-api REST service |
| `conduit` | dapple-prompt-conduit LLM routing |
| `flow` | dapple-prompt-flow workflow engine |
| `rust-docs` | Rust documentation server |

## Environment Dependencies

The scripts expect these environment variables (set by dapple_sandbox overlay):

| Variable | Description |
|----------|-------------|
| `KUBE_DAPPLE_NAMESPACE` | Target namespace |
| `KUBE_REGISTRY_HOST` | Docker registry |
| `DOCKER_DAPPLE_TAG_LATEST` | Default image tag |
| `DAPPLE_PROMPT_WORKSPACE_DIR` | Repo root |
| `DAPPLE_PROMPT_DATABASE_URL` | Prompt services PostgreSQL URL |
| `DAPPLE_ONNX_CACHE_DIR` | ONNX model cache (`~/.cache/dapple-onnx`) |

## Script Development

### Adding a New Service

1. Update `build_image.nu`:
   - Add to `resolve-service` if needed
   - Add special build logic if different from standard crates

2. Update `deploy_dapple_prompt_pods.nu`:
   - Add new `setup_dapple_svc` call in main
   - Configure manifest template and ports

3. Update `modules/dapple/aliases.nu`:
   - Add service alias mapping

### Customizing Build

The Docker build uses these build args:
- `DAPPLE_PROMPT_DATABASE_URL` - Database URL for sqlx
- `BIN` - Binary name to build
- `PROFILE` - Cargo profile (dev/release)

To modify:
```nu
# In build_image.nu
let build_args = [
    --network=host
    --build-arg $"DAPPLE_PROMPT_DATABASE_URL=($env.DAPPLE_INTERNAL_POSTGRES_URL)"
    --build-arg $"BIN=($deployment)"
    --build-arg $"PROFILE=($profile)"
    --build-arg "CUSTOM_ARG=value"  # Add custom args here
    --tag $commit_tag
    --tag $latest_tag
    .
]
```

### Testing Changes

```nu
# Syntax check
nu -c "source assets/nu/k8s/build_image.nu"

# Dry run (won't push)
# Comment out docker push lines temporarily

# Full test
dapple build api --force
dapple patch api
dapple logs api
```
