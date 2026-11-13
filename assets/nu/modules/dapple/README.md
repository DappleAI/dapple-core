# Dapple CLI Module

Developer-friendly Kubernetes operations for the Dapple development cluster. This nushell module wraps `kubectl` and deployment scripts to provide simple, memorable commands for common development tasks.

## Quick Start

```nu
# After running setup.nu and restarting nushell:
dapple status           # Check all pods
dapple logs api         # Stream API logs
dapple quick api        # Build and deploy in one command
```

## Installation

The module is installed automatically by `setup.nu`:

```bash
cd dapple-prompt
nu assets/nu/init/setup.nu --environment sandbox
# Restart nushell
exec nu
```

This will:
1. Copy modules to your nushell config dir's `modules/` (`~/.config/nushell/modules/` on Linux, `~/Library/Application Support/nushell/modules/` on macOS)
2. Generate `~/.env.dapple` with required secrets
3. Add the dapple overlay to your `config.nu`
4. Verify cluster connectivity

## Service Aliases

Instead of typing full deployment names, use short aliases:

| Alias | Deployment Name |
|-------|-----------------|
| `api` | dapple-prompt-api |
| `conduit` | dapple-prompt-conduit |
| `flow` | dapple-prompt-flow |
| `rust-docs` or `docs` | rust-docs |
| `ui` | dapple-ui |
| `landing` | dapple-web-landing-page |
| `postgres` | prompt-db |
| `redis` | dapple-redis |

## Commands Reference

### Logs & Debugging

```nu
# View logs (streams by default)
dapple logs api

# View logs from crashed container
dapple logs api --previous
dapple logs api -p

# View last 50 lines only (no streaming)
dapple logs api --tail 50

# Logs from specific time range
dapple logs api --since "30m"

# Pod status overview
dapple status                    # All pods
dapple status api                # Specific service
dapple status -w                 # Wide output with node/IP

# Detailed information
dapple describe api              # kubectl describe
dapple events                    # Recent warnings/errors
dapple events -a                 # All events including normal
dapple events -w                 # Watch for new events
```

### Lifecycle Management

```nu
# Rolling restart (zero downtime)
dapple restart api
dapple restart all              # Restart everything

# Clean up problematic pods
dapple cleanup                  # Delete failed/stuck pods
dapple cleanup --force          # Force delete (grace-period=0)
dapple cleanup --dry-run        # Preview what would be deleted

# Scale deployments
dapple scale api 2              # Scale to 2 replicas
dapple scale api 0              # Scale down (stop)
```

### Build & Deploy

```nu
# Build Docker image
dapple build api                 # Build if changed
dapple build api --force         # Force rebuild
dapple build api --profile release  # Release build

# Full deployment (applies manifests, waits for rollout)
dapple deploy api
dapple deploy api,conduit       # Multiple services
dapple deploy api --force       # Force rebuild

# Quick image update (no manifest changes) -- Docker-mode services only
dapple patch api                # Update with latest image
dapple patch api --tag abc123   # Specific commit/tag

# Build + patch in one command (most common workflow)
dapple quick api
dapple quick api --force

# hostPath-mode services (the DEFAULT for local dev -- see ../../AGENTS.md)
dapple patch api --build        # compile (cross-compiles on macOS) + restart
dapple patch api --restart      # restart only (binary already built)
dapple patch api --build --profile release
```

**hostPath vs Docker mode.** `deploy` defaults to **hostPath mode**, where the pod
runs code straight from a mounted host directory (a Rust service's
`target/<triple>/debug/`, or a web service's source tree) — the image's copy is
never executed. So `patch`/`quick`, which only update the image, do nothing for
those services: they'd push a new image while the pod keeps running the last
binary you built locally. `patch` now detects this and refuses rather than
reporting a successful no-op. For a hostPath service, use `patch --build`
(compile + restart) or `patch --restart`.

### Access & Connectivity

```nu
# Shell into running container
dapple shell api
dapple shell api --shell /bin/bash

# Port forwarding
dapple port-forward api          # Uses default port (8001)
dapple port-forward api 9001     # Custom local port
dapple port-forward postgres     # Forward database (5432)

# Get service URLs
dapple url api                   # https://dapple-prompt-api.kube.dappleai.dev/api
dapple url ui                    # https://dapple-ui.kube.dappleai.dev
```

### Local Web Development

```nu
# Generate .env.local for local dapple-ui development
dapple env ui                   # Show what would be written
dapple env ui --write           # Write to ~/Source/Dapple/dapple-ui/.env.local
dapple env ui --write --path /custom/path  # Custom directory

# Same for landing page
dapple env landing              # Show env vars
dapple env landing --write      # Write .env.local

# Manage secrets
dapple env show                 # See available secrets (masked)
dapple env regenerate           # Regenerate ~/.env.dapple
```

### Information & Verification

```nu
# Verify setup and connectivity
dapple verify

# List all service aliases
dapple aliases

# Kubernetes resources
dapple deployments              # List deployments
dapple services                 # List k8s services
```

## Common Workflows

### Daily Development

```nu
# Start of day: check cluster status
dapple status
dapple events

# Make changes to API code, then:
dapple quick api               # Build + deploy

# Watch logs while testing
dapple logs api

# If something goes wrong
dapple describe api            # Check pod events
dapple logs api -p             # Previous container logs
dapple restart api             # Try restart
```

### Debugging a Crash Loop

```nu
# Check what's happening
dapple status api
dapple describe api            # Look at Events section

# Get logs from the crashed container
dapple logs api --previous

# Check recent cluster events
dapple events

# Clean up and restart fresh
dapple cleanup
dapple restart api
```

### Working with Database

```nu
# Forward postgres to localhost
dapple port-forward postgres

# In another terminal, connect with psql
psql -h localhost -p 5432 -U dapple dapple
```

### Testing Against Local Build

```nu
# Build with release profile
dapple build api --profile release

# Deploy with specific image tag
dapple patch api --tag abc123

# Verify it's running
dapple status api
dapple logs api
```

### Local Web Development (Mac Team)

For Mac developers who want to run dapple-ui or landing-page locally (without k8s):

```nu
# Generate .env.local for dapple-ui with the same env vars as k8s
dapple env ui --write

# Then in the dapple-ui directory:
cd ~/Source/Dapple/dapple-ui
pnpm install
pnpm dev

# For landing page:
dapple env landing --write
cd ~/Source/Dapple/dapple-landing-page
pnpm dev
```

The generated `.env.local` includes:
- Database connection (external URL)
- NextAuth configuration
- API endpoint URLs
- Service auth tokens
- PostHog analytics (optional)
- OAuth credentials (optional)

This lets you verify your web app works locally before attempting k8s deployment.

```nu
# View what env vars would be generated (without writing)
dapple env ui

# See what secrets are available
dapple env show

# Regenerate secrets if needed
dapple env regenerate
```

## Module Structure

```
nu/modules/dapple/
├── mod.nu           # Main module, exports all commands, help text
├── aliases.nu       # Service name -> deployment mapping
├── logs.nu          # Log viewing commands
├── status.nu        # Status, describe, events commands
├── lifecycle.nu     # restart, cleanup, scale commands
├── build.nu         # build, deploy, patch, quick commands
├── access.nu        # shell, port-forward, url commands
├── env.nu           # Environment export for local web dev
├── README.md        # This file
└── AGENTS.md        # AI agent guidance
```

## Environment Variables

The module expects these environment variables (set by the dapple_sandbox overlay):

| Variable | Description | Example |
|----------|-------------|---------|
| `KUBE_DAPPLE_NAMESPACE` | Kubernetes namespace | `dapple-sandbox` |
| `KUBE_REGISTRY_HOST` | Docker registry host | `registry.kube.dappleai.dev` |
| `DOCKER_DAPPLE_TAG_LATEST` | Default image tag | `dapple-latest` |
| `DAPPLE_ASSETS_DIR` | Path to assets directory | `/path/to/dapple-prompt/assets` |
| `DAPPLE_ONNX_CACHE_DIR` | ONNX model cache | `~/.cache/dapple-onnx` |

## Troubleshooting

### "Command not found: dapple"

Restart nushell after running `setup.nu`:
```bash
exec nu
```

### "No pod found for service..."

The deployment may not be running:
```nu
dapple status                  # Check overall status
dapple deploy api              # Deploy if missing
```

### kubectl connection errors

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

### Image push fails with certificate error

Docker Desktop may need to be restarted to pick up registry certificates:
```bash
# Restart Docker Desktop, then retry
dapple build api --force
```

### Pods stuck in Terminating state

Use force cleanup:
```nu
dapple cleanup --force
```

### macOS: Conduit fails with "file not found" for ONNX models

On macOS, OrbStack auto-mounts `/Users/$USER` two-way into the MicroK8s VM
at the same path, so paths under your home directory (including
`~/.cache/dapple-onnx`) are visible from inside the VM with no
per-path registration. If the conduit pod still can't find the models:

```bash
# Confirm the models actually exist on the host
nu assets/nu/k8s/convert_onnx_models.nu verify

# Confirm the VM can see them (sanity check the auto-mount)
nu -c 'orb vm_run ls ~/.cache/dapple-onnx'
```

If `orb vm_run` returns "not found" but the host directory exists, restart
the OrbStack VM (`orb stop microk8s-vm; orb start microk8s-vm`) to refresh
the virtiofs mount.

### macOS: Web apps can't see source files (hostPath mode)

Source directories under `/Users/$USER` are auto-mounted by OrbStack — no
explicit mount step is needed. To verify:

```bash
# Confirm the VM sees the workspace at the same path as the host
nu -c 'orb vm_run ls $env.DAPPLE_PROMPT_WORKSPACE_DIR'
```

If that command fails with "no such directory", the path is either outside
`/Users/$USER` (OrbStack only auto-mounts the user home) or the VM's virtiofs
mount has degraded — restart the VM with `orb stop microk8s-vm; orb start microk8s-vm`.

## Contributing

When adding new commands:

1. Add the command to the appropriate submodule (`logs.nu`, `status.nu`, etc.)
2. Export the command in `mod.nu`
3. Update the help text in `mod.nu main`
4. Add examples to this README

Follow existing patterns for consistency:
- Use `resolve-service` for service name handling
- Get namespace from `$env.KUBE_DAPPLE_NAMESPACE`
- Use `std/log` for status messages
- Return meaningful exit codes
