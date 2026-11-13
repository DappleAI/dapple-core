# Kubernetes Development at Dapple

> **Audience:** Backend Developers · Web Developers · Platform Engineers

This chapter covers using Kubernetes for local development with the Dapple stack. We use MicroK8s on Linux hosts for our development cluster, with macOS developers connecting over VPN.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                    Development Cluster                          │
│                    (MicroK8s on Linux)                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │dapple-      │  │dapple-      │  │dapple-      │             │
│  │prompt-api   │  │prompt-      │  │prompt-flow  │             │
│  │             │  │conduit      │  │             │             │
│  └─────────────┘  └─────────────┘  └─────────────┘             │
│         │               │               │                       │
│         └───────────────┼───────────────┘                       │
│                         │                                       │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │prompt-db    │  │dapple-      │  │ Private     │             │
│  │             │  │redis        │  │ Registry    │             │
│  └─────────────┘  └─────────────┘  └─────────────┘             │
└─────────────────────────────────────────────────────────────────┘
         ▲
         │ kubectl / VPN
         │
┌────────┴────────┐    ┌─────────────────┐
│ Linux Devs      │    │ macOS Devs      │
│ (direct access) │    │ (VPN + kubectl) │
└─────────────────┘    └─────────────────┘
```

## Key Concepts

### Namespace: `dapple-sandbox`

All development resources live in the `dapple-sandbox` namespace. This isolates dev work from any production or staging environments.

```bash
kubectl get pods -n dapple-sandbox
```

### Private Docker Registry

We run a private Docker registry at `registry.kube.dappleai.dev`. Images are pushed there after builds and pulled by Kubernetes for deployments.

### Service Discovery

Services communicate using Kubernetes DNS:
- `dapple-prompt-api:8001` - API service
- `dapple-prompt-conduit:8081` - Conduit service
- `prompt-db:5432` - PostgreSQL (prompt services)
- `dapple-redis:6379` - Redis

External access uses hostnames like `dapple-prompt-api.kube.dappleai.dev`.

## Getting Started

### Prerequisites

- Nushell installed (`brew install nushell` on macOS)
- kubectl configured for the cluster
- VPN connection (for macOS)
- Docker Desktop (macOS) or docker-ce (Linux) for building images

### One-Time Setup

```bash
# Clone the repo
cd ~/Source/Dapple  # macOS
cd ~/src/dapple     # Linux
git clone git@github.com:dapple/dapple-prompt.git
cd dapple-prompt

# Run setup
nu assets/nu/init/setup.nu --environment sandbox

# Restart nushell
exec nu

# Verify setup
dapple verify
```

The setup script:
1. Installs nushell modules to your nushell config dir's `modules/` (`~/.config/nushell/modules/` on Linux, `~/Library/Application Support/nushell/modules/` on macOS)
2. Generates `~/.env.dapple` with required secrets
3. Configures your `config.nu` to load the dapple environment
4. Verifies cluster connectivity

### Daily Workflow

```nu
# Start of day: check cluster health
dapple status

# Make code changes, then build and deploy
dapple quick api      # Build + deploy in one command

# Watch logs while testing
dapple logs api

# If something crashes
dapple describe api   # Check events
dapple logs api -p    # Previous container logs
dapple restart api    # Rolling restart
```

## The dapple CLI

The `dapple` CLI provides developer-friendly wrappers around kubectl. Instead of remembering complex commands, use simple aliases.

### Service Aliases

| Alias | Deployment |
|-------|------------|
| `api` | dapple-prompt-api |
| `conduit` | dapple-prompt-conduit |
| `flow` | dapple-prompt-flow |
| `ui` | dapple-ui |
| `postgres` | prompt-db |
| `redis` | dapple-redis |

### Essential Commands

```nu
# Status & Logs
dapple status                 # All pods
dapple status api             # Specific service
dapple logs api               # Stream logs
dapple logs api -p            # Previous container
dapple describe api           # Detailed info
dapple events                 # Cluster events

# Lifecycle
dapple restart api            # Rolling restart
dapple restart all            # Restart everything
dapple cleanup                # Remove stuck pods
dapple cleanup --force        # Force delete

# Build & Deploy
dapple build api              # Build docker image
dapple patch api              # Update image only
dapple quick api              # Build + patch
dapple deploy api             # Full deploy

# Access
dapple shell api              # Shell into container
dapple port-forward postgres  # Forward database port
```

For complete documentation: `dapple --help` or see the [dapple CLI README](../../../nu/modules/dapple/README.md).

## Build & Deploy Workflows

### Quick Iteration (Code Changes Only)

When you only changed code (no dependencies, no manifest changes):

```nu
dapple quick api
```

This builds the Docker image and patches the deployment. It's the fastest way to test changes.

### Full Deployment

When you need to apply manifest changes or run migrations:

```nu
dapple deploy api
```

This runs the full deployment script including:
- SQLx migrations
- Service token setup
- Manifest application
- Image build and push
- Rollout wait

### Specific Image Tag

To deploy a specific version:

```nu
# Build returns the image tag
dapple build api
# → registry.kube.dappleai.dev/dapple-prompt-api:abc123

# Later, patch with that specific tag
dapple patch api --tag abc123
```

### Multiple Services

Deploy multiple services at once:

```nu
dapple deploy api,conduit,flow
```

## Debugging

### Check Pod Status

```nu
# Quick overview
dapple status

# Detailed status with node info
dapple status -w

# Specific service
dapple status api
```

### View Logs

```nu
# Live log stream
dapple logs api

# Logs from crashed container
dapple logs api --previous

# Last 500 lines without streaming
dapple logs api --tail 500

# Logs from last hour
dapple logs api --since "1h"
```

### Investigate Crashes

```nu
# Check events (warnings by default)
dapple events

# Full describe output
dapple describe api

# Common issues:
# - ImagePullBackOff: Registry cert issue — restart Docker Desktop (macOS) or check /etc/docker/certs.d/ (Linux)
# - CrashLoopBackOff: Check logs --previous
# - Pending: Resource constraints, check events
```

### Shell into Container

```nu
# Default shell
dapple shell api

# Specific shell
dapple shell api --shell /bin/bash
```

### Database Access

```nu
# Forward postgres to localhost
dapple port-forward postgres

# In another terminal
psql -h localhost -p 5432 -U dapple dapple
```

## Common Issues

### Certificate Errors on Push

The Docker daemon needs to trust the registry certificate:

```bash
# macOS (Docker Desktop): Restart Docker Desktop to pick up the cert, then retry
dapple build api --force

# Linux (native dockerd): Ensure cert is installed — no restart needed
nu -c 'use k8s; k8s setup_docker_registry_cert $env.KUBE_REGISTRY_HOST $env.KUBE_ROOT_CA_CERT_NAME'
dapple build api --force
```

### Pods Stuck Terminating

Force cleanup:

```nu
dapple cleanup --force
```

### kubectl Connection Fails

1. Check VPN connection
2. Verify kubectl context:
   ```bash
   kubectl config use-context microk8s
   kubectl cluster-info
   ```
3. Run verification:
   ```nu
   dapple verify
   ```

### Old Code Running After Deploy

The image might be cached. Force rebuild:

```nu
dapple build api --force
dapple patch api
```

## Environment Variables

The dapple environment sets up these key variables:

| Variable | Description |
|----------|-------------|
| `KUBE_HOST` | Cluster hostname |
| `KUBE_DAPPLE_NAMESPACE` | Target namespace |
| `KUBE_REGISTRY_HOST` | Docker registry |
| `DAPPLE_PROMPT_DATABASE_URL` | Prompt services postgres URL |
| `DAPPLE_PROMPT_API_URL` | API endpoint URL |

View current values:

```nu
echo $env.KUBE_HOST
echo $env.KUBE_DAPPLE_NAMESPACE
echo $env.DAPPLE_PROMPT_DATABASE_URL
```

## Local Web Development (Without K8s)

If you're having trouble getting pods running, you can run dapple-ui or landing-page locally while still connecting to the k8s backend services.

### Generate Environment File

```nu
# Generate .env.local with the same env vars the k8s pod would have
dapple env ui --write

# This creates ~/Source/Dapple/dapple-ui/.env.local with:
# - Database connection (external URL)
# - NextAuth configuration
# - API endpoint URLs
# - Service auth tokens
# - Analytics and OAuth (optional)
```

### Run Locally

```bash
cd ~/Source/Dapple/dapple-ui
pnpm install
pnpm dev
```

The app will:
- Connect to the k8s API at `https://dapple-prompt-api.kube.dappleai.dev`
- Connect to the k8s database at `dapple-postgres.kube.dappleai.dev` (dapple-ui uses legacy postgres; prompt services use `prompt-db`)
- Run on `http://localhost:4321`

### Benefits

1. **Faster iteration** - No Docker build or k8s deployment needed
2. **Easier debugging** - Use local dev tools and debuggers
3. **Prove it works** - Verify your code works before k8s deployment
4. **Isolate issues** - Determine if problems are code or infrastructure

### For Landing Page

```nu
dapple env landing --write
cd ~/Source/Dapple/dapple-landing-page
pnpm dev
# Runs on http://localhost:4444
```

### Troubleshooting Local Dev

```nu
# Check what secrets are available
dapple env show

# Regenerate secrets if missing
dapple env regenerate

# View what would be generated (without writing)
dapple env ui
```

## Next Steps

- [dapple CLI Reference](./dapple-cli.md) - Complete command documentation
- [Deployment Scripts](./deployment-scripts.md) - Under-the-hood details
- [Debugging Guide](../development/debugging.md) - Troubleshooting patterns
