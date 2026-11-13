# dapple CLI Reference

> **Audience:** All Developers

Complete reference for the `dapple` CLI module. This nushell module provides developer-friendly wrappers around kubectl and deployment scripts.

## Installation

The CLI is installed automatically by `setup.nu`:

```bash
cd dapple-prompt
nu assets/nu/init/setup.nu --environment sandbox
exec nu  # Restart nushell
```

## Service Aliases

Use short names instead of full deployment names:

| Alias | Deployment | Port |
|-------|------------|------|
| `api` | dapple-prompt-api | 8001 |
| `conduit` | dapple-prompt-conduit | 8081 |
| `flow` | dapple-prompt-flow | 8082 |
| `rust-docs` / `docs` | rust-docs | 8080 |
| `ui` | dapple-ui | 3000 |
| `landing` | dapple-web-landing-page | 3000 |
| `postgres` | prompt-db | 5432 |
| `redis` | dapple-redis | 6379 |

## Commands

### Logging Commands

#### `dapple logs`

View container logs for a service.

```nu
dapple logs <service> [flags]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--follow` | `-f` | Stream logs continuously (default: true) |
| `--previous` | `-p` | Show logs from previous container |
| `--tail` | | Number of lines (default: 100) |
| `--container` | `-c` | Specific container name |
| `--since` | | Time duration (e.g., "1h", "30m") |

**Examples:**

```nu
dapple logs api                    # Stream api logs
dapple logs api --previous         # Crashed container logs
dapple logs api --tail 500         # Last 500 lines
dapple logs api --since "2h"       # Last 2 hours
dapple logs api -c sidecar         # Specific container
```

### Status Commands

#### `dapple status`

Show pod status overview.

```nu
dapple status [service] [flags]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--wide` | `-w` | Show node and IP columns |

**Examples:**

```nu
dapple status                # All pods
dapple status api            # Specific service
dapple status -w             # Wide output
```

#### `dapple describe`

Show detailed pod information (kubectl describe).

```nu
dapple describe <service>
```

**Examples:**

```nu
dapple describe api          # Detailed api pod info
```

#### `dapple events`

Show recent cluster events.

```nu
dapple events [flags]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--all` | `-a` | Show all events (default: warnings only) |
| `--watch` | `-w` | Watch for new events |

**Examples:**

```nu
dapple events                # Warnings and errors
dapple events -a             # All events
dapple events -w             # Watch mode
```

#### `dapple deployments`

List all deployments in the namespace.

```nu
dapple deployments [service]
```

#### `dapple services`

List Kubernetes Service resources.

```nu
dapple services [service]
```

### Lifecycle Commands

#### `dapple restart`

Perform a rolling restart of a service.

```nu
dapple restart <service> [flags]
```

| Flag | Description |
|------|-------------|
| `--timeout` | Rollout timeout (default: "120s") |

**Examples:**

```nu
dapple restart api           # Restart api
dapple restart all           # Restart all services
dapple restart ui --timeout "180s"
```

#### `dapple cleanup`

Remove stuck, failed, or terminating pods.

```nu
dapple cleanup [flags]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--force` | `-f` | Force delete (grace-period=0) |
| `--dry-run` | `-d` | Preview without deleting |

**Examples:**

```nu
dapple cleanup               # Delete failed pods
dapple cleanup --force       # Force delete stuck pods
dapple cleanup --dry-run     # Preview only
```

#### `dapple scale`

Scale a deployment to a specific number of replicas.

```nu
dapple scale <service> <replicas>
```

**Examples:**

```nu
dapple scale api 2           # Scale to 2 replicas
dapple scale api 0           # Scale down (stop)
```

### Build & Deploy Commands

#### `dapple build`

Build and push a Docker image.

```nu
dapple build <service> [flags]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--force` | `-f` | Force rebuild |
| `--profile` | | Build profile (default: "dev") |

**Examples:**

```nu
dapple build api             # Build if changed
dapple build api --force     # Force rebuild
dapple build api --profile release
```

#### `dapple deploy`

Full deployment (manifests + rollout).

```nu
dapple deploy <service> [flags]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--force` | `-f` | Force rebuild |
| `--hostpath` | | Enable hostPath mode (mount local binaries) |

**Examples:**

```nu
dapple deploy api            # Full deploy
dapple deploy api,conduit    # Multiple services
dapple deploy api --force    # Force rebuild
```

#### `dapple patch`

Quick image update (no manifest changes).

```nu
dapple patch <service> [flags]
```

| Flag | Description |
|------|-------------|
| `--tag` | Specific image tag |
| `--timeout` | Rollout timeout (default: "120s") |

**Examples:**

```nu
dapple patch api             # Update to latest
dapple patch api --tag abc123  # Specific version
```

#### `dapple quick`

Build and patch in one command.

```nu
dapple quick <service> [flags]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--force` | `-f` | Force rebuild |

**Examples:**

```nu
dapple quick api             # Build + patch
dapple quick api --force     # Force rebuild + patch
```

### Access Commands

#### `dapple shell`

Open a shell in a running container.

```nu
dapple shell <service> [flags]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--container` | `-c` | Specific container |
| `--shell` | | Shell to use (default: "/bin/sh") |

**Examples:**

```nu
dapple shell api             # Default shell
dapple shell api --shell /bin/bash
```

#### `dapple port-forward`

Forward local port to a service.

```nu
dapple port-forward <service> [local_port] [flags]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--remote-port` | `-r` | Remote port (default: service default) |

**Examples:**

```nu
dapple port-forward api          # Use default port (8001)
dapple port-forward api 9001     # Custom local port
dapple port-forward postgres     # Forward database
```

#### `dapple url`

Show the external URL for a service.

```nu
dapple url <service>
```

**Examples:**

```nu
dapple url api              # https://dapple-prompt-api.kube.dappleai.dev/api
dapple url ui               # https://dapple-ui.kube.dappleai.dev
```

### Environment Commands (Local Web Dev)

These commands help Mac developers run dapple-ui or landing-page locally with the same environment variables the k8s pods would have.

#### `dapple env ui`

Generate environment variables for local dapple-ui development.

```nu
dapple env ui [flags]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--write` | `-w` | Write to .env.local |
| `--path` | | Custom directory path |

**Examples:**

```nu
dapple env ui                           # Show what would be written
dapple env ui --write                   # Write to ~/Source/Dapple/dapple-ui/.env.local
dapple env ui --write --path /my/path   # Custom directory
```

#### `dapple env landing`

Generate environment variables for local landing-page development.

```nu
dapple env landing [flags]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--write` | `-w` | Write to .env.local |
| `--path` | | Custom directory path |

**Examples:**

```nu
dapple env landing                      # Show env vars
dapple env landing --write              # Write .env.local
```

#### `dapple env show`

Show available secrets from ~/.env.dapple (values masked).

```nu
dapple env show
```

#### `dapple env regenerate`

Regenerate ~/.env.dapple from template.

```nu
dapple env regenerate
```

### Information Commands

#### `dapple verify`

Verify setup and cluster connectivity.

```nu
dapple verify
```

#### `dapple aliases`

List all service aliases.

```nu
dapple aliases
```

## Workflow Recipes

### Daily Development

```nu
# Morning check
dapple status
dapple events

# Code iteration
dapple quick api

# Watch logs
dapple logs api
```

### Debugging Crashes

```nu
# Investigate
dapple status api
dapple describe api
dapple logs api --previous
dapple events

# Fix
dapple cleanup
dapple restart api
```

### Database Work

```nu
# Terminal 1: Forward port
dapple port-forward postgres

# Terminal 2: Connect
psql -h localhost -p 5432 -U dapple dapple
```

### Deploy Specific Version

```nu
# Build and note the tag
dapple build api
# → registry.kube.dappleai.dev/dapple-prompt-api:abc123

# Later, deploy that version
dapple patch api --tag abc123
```

### Local Web Development (Skip K8s)

When k8s pods are giving you trouble, run the web app locally:

```nu
# Generate .env.local with k8s-equivalent env vars
dapple env ui --write

# Run locally
cd ~/Source/Dapple/dapple-ui
pnpm install
pnpm dev

# App runs at http://localhost:4321
# Connects to k8s API at https://dapple-prompt-api.kube.dappleai.dev
```

This lets you verify your code works before debugging k8s issues.
