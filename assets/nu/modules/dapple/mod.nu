# Dapple CLI - Developer-friendly Kubernetes operations for dapple services
#
# This module provides simple commands for common development tasks.
# Use service aliases (api, conduit, flow, etc.) instead of full deployment names.
#
# Examples:
#   dapple logs api              # View container logs
#   dapple status                # Show all pod status
#   dapple restart api           # Rolling restart
#   dapple cleanup               # Remove stuck pods
#   dapple build api             # Build and push docker image
#   dapple patch api             # Quick image update
#   dapple quick api             # Build + patch in one command
#
# Service Aliases:
#   api          -> dapple-prompt-api
#   external-api -> dapple-prompt-external-api
#   conduit      -> dapple-prompt-conduit
#   flow         -> dapple-prompt-flow
#   rust-docs    -> rust-docs
#   docs         -> rust-docs
#   ui           -> dapple-ui
#   landing      -> dapple-web-landing-page
#   postgres     -> prompt-db
#   redis        -> dapple-redis

use std/log

# Re-export submodules
export use ./aliases.nu [resolve-service list-aliases buildable-services]
export use ./logs.nu
export use ./status.nu
export use ./lifecycle.nu [restart, cleanup, scale]
export use ./build.nu
export use ./access.nu [shell, port-forward, url]
export use ./lago.nu
export use ./flagsmith.nu
export use ./token.nu
export use ./external.nu
export use ./sqlc.nu
export use ./sqlx.nu
export use ./guide.nu

# Convenience re-exports from status module (available at top level)
use ./status.nu

# Show detailed pod information (kubectl describe)
export def pod-describe [
    service: string  # Service alias (api, conduit, flow, etc.)
] {
    status pod-describe $service
}

# Show recent cluster events (useful for debugging)
export def events [
    --all (-a)        # Show all events (default: only warnings/errors)
    --watch (-w)      # Watch for new events
] {
    if $all and $watch {
        status events --all --watch
    } else if $all {
        status events --all
    } else if $watch {
        status events --watch
    } else {
        status events
    }
}

# Show deployment information
export def deployments [
    service?: string  # Service alias (optional - shows all if omitted)
] {
    if ($service | is-empty) {
        status deployments
    } else {
        status deployments $service
    }
}

# Show services (k8s Service resources)
export def services [
    service?: string  # Service alias (optional - shows all if omitted)
] {
    if ($service | is-empty) {
        status services
    } else {
        status services $service
    }
}

# Convenience re-exports from build module (available at top level)
use ./build.nu

# Build and push Docker image for a service
export def build [
    service: string     # Service alias (api, conduit, flow, rust-docs)
    --force (-f)        # Force rebuild even if image exists
    --profile: string = "dev"  # Build profile (dev, release)
] {
    if $force {
        build image $service --force --profile $profile
    } else {
        build image $service --profile $profile
    }
}

# Full deploy (manifests + rollout)
export def deploy [
    service: string     # Service alias or comma-separated list
    --force (-f)        # Force rebuild
    --hostpath: string = ""  # Enable hostPath mode (Linux only)
] {
    if $force {
        if ($hostpath | is-not-empty) {
            build deploy $service --force --hostpath $hostpath
        } else {
            build deploy $service --force
        }
    } else {
        if ($hostpath | is-not-empty) {
            build deploy $service --hostpath $hostpath
        } else {
            build deploy $service
        }
    }
}

# Quick image update (no manifest changes)
#
# Services deployed in hostPath mode run the binary from a mounted host cargo
# target dir rather than from the Docker image, so the image paths below do
# nothing for them — use --build (compile + restart) or --restart instead.
export def patch [
    service: string        # Service alias (api, conduit, flow, etc.)
    --tag: string = ""     # Specific image tag (default: dapple-latest)
    --timeout: string = "120s"  # Rollout timeout
    --restart (-r)         # hostPath mode: just restart (binary already built)
    --build (-b)           # hostPath mode: build the binary (cross-compiles on macOS) then restart
    --profile: string = "debug"  # Cargo profile for --build (debug or release)
] {
    if $build {
        build patch $service --build --profile $profile --timeout $timeout
    } else if $restart {
        build patch $service --restart --timeout $timeout
    } else if ($tag | is-not-empty) {
        build patch $service --tag $tag --timeout $timeout
    } else {
        build patch $service --timeout $timeout
    }
}

# Build and patch in one command (common workflow)
export def quick [
    service: string     # Service alias
    --force (-f)        # Force rebuild
] {
    if $force {
        build quick $service --force
    } else {
        build quick $service
    }
}

# Prune old Docker images, build cache, and registry tags
export def "build prune" [
    --dry-run (-d)    # Show what would be removed without removing
    --cache           # Also prune Docker build cache
    --registry        # Also prune old tags from registry
] {
    if $dry_run and $cache and $registry {
        build prune --dry-run --cache --registry
    } else if $dry_run and $cache {
        build prune --dry-run --cache
    } else if $dry_run and $registry {
        build prune --dry-run --registry
    } else if $cache and $registry {
        build prune --cache --registry
    } else if $dry_run {
        build prune --dry-run
    } else if $cache {
        build prune --cache
    } else if $registry {
        build prune --registry
    } else {
        build prune
    }
}

# Environment export module (for local web dev)
export use ./env.nu

# Main help command
export def main [] {
    print "
dapple - Dapple development cluster management

Usage: dapple <command> [options]

Log & Debug Commands:
  dapple logs <service>       View container logs (streams by default)
  dapple logs <svc> -p        Show logs from previous crashed container
  dapple status [service]     Show pod status (all pods if no service specified)
  dapple status -w            Show status with extra columns (node, IP)
  dapple pod-describe <service>   Detailed pod information (kubectl describe)
  dapple events               Recent cluster events (warnings by default)
  dapple events -a            Show all events including normal ones

Lifecycle Commands:
  dapple restart <service>    Rolling restart of a service
  dapple restart all          Restart all dapple services
  dapple cleanup              Remove stuck/failed pods
  dapple cleanup --force      Force delete stuck pods (grace-period=0)
  dapple scale <svc> <n>      Scale deployment to n replicas

Build & Deploy Commands:
  dapple build <service>      Build and push docker image
  dapple build <svc> -f       Force rebuild even if image exists
  dapple build prune           Prune old local images
  dapple build prune -d        Dry run (show what would be removed)
  dapple build prune --cache   Also prune Docker build cache
  dapple build prune --registry Also prune old registry tags
  dapple deploy <service>     Full deploy (manifests + rollout)
  dapple patch <service>      Quick image update (just image, no manifest)
  dapple patch <svc> --tag x  Patch with specific image tag
  dapple patch <svc> --build  hostPath mode: compile + restart (no Docker)
  dapple patch <svc> -r       hostPath mode: restart only (binary already built)
  dapple quick <service>      Build + patch in one command

  NOTE: services deployed with --hostpath run their binary from a mounted host
  cargo target dir, not the Docker image. For those, use `patch --build`;
  `patch`/`quick` (image paths) will refuse rather than silently no-op.

Access Commands:
  dapple shell <service>      Open shell in running container
  dapple port-forward <svc>   Forward local port to service
  dapple url <service>        Show external URL for service

Local Development (Web Team):
  dapple env ui               Show env vars for dapple-ui local dev
  dapple env ui --write       Write .env.local to dapple-ui directory
  dapple env landing          Show env vars for landing page local dev
  dapple env landing --write  Write .env.local to landing page directory
  dapple env show             Show available secrets from ~/.env.dapple
  dapple env regenerate       Regenerate ~/.env.dapple from template

Information:
  dapple deployments          List all deployments
  dapple services             List all k8s services

Lago Billing Platform:
  dapple lago setup           Deploy Lago billing platform
  dapple lago status          Show Lago pod status
  dapple lago logs <service>  View Lago service logs (api, front, worker, etc.)
  dapple lago restart <svc>   Rolling restart of Lago service
  dapple lago                 Show full Lago help

Flagsmith Feature Flags:
  dapple flagsmith setup           Deploy Flagsmith platform
  dapple flagsmith status          Show Flagsmith pod status
  dapple flagsmith logs <service>  View Flagsmith logs (api, task-processor, etc.)
  dapple flagsmith restart <svc>   Rolling restart of Flagsmith service
  dapple flagsmith                 Show full Flagsmith help

Token Management:
  dapple token pat create <uuid>   Create PAT for a user (direct DB insert)
  dapple token pat list <uuid>     List PAT tokens for a user
  dapple token pat revoke <uuid>   Revoke a PAT token
  dapple token service list        List registered service tokens
  dapple token service ensure <id> Ensure service has keys configured
  dapple token                     Show full token management help

SQL Composition (dapple-data):
  dapple sqlc compose              Regenerate .sql from .sqlc + sqlx prepare --workspace --all-targets
  dapple sqlc compose -n           Regenerate .sql only (skip prepare)
  dapple sqlc verify               CI drift check: .sql matches .sqlc sources?
  dapple sqlc                      Show full sqlc help

Database (sqlx):
  dapple sqlx migrate              Apply pending migrations (cargo sqlx migrate run)
  dapple sqlx info                 Show migration status
  dapple sqlx prepare              Refresh offline query cache (--workspace --all-targets)
  dapple sqlx prepare --check      Verify offline cache is up to date (CI)
  dapple sqlx                      Show full sqlx help

Guide Authoring:
  dapple guide validate [dir]      Validate a guide dir locally, exactly as
                                   ingest would (structure + task scripts via
                                   the embedded nu parser). Pre-sync check.
  dapple guide                     Show full guide help

Service Aliases:
  api, conduit, flow, rust-docs (or docs),
  ui, landing, postgres, redis

Examples:
  dapple logs api                    # Stream api logs
  dapple status                      # Overview of all pods
  dapple restart ui                  # Rolling restart
  dapple build api && dapple patch api  # Build then update
  dapple quick api                   # Build + patch in one command
  dapple cleanup --force             # Clean up stuck pods
  dapple env ui --write              # Generate .env.local for local UI dev
"
}

# Show all service aliases
export def aliases [] {
    list-aliases | table
}

# Verify cluster connectivity
export def verify [] {
    let ns = $env.KUBE_DAPPLE_NAMESPACE

    print $"Checking connectivity to namespace: ($ns)"
    print ""

    let result = (do { kubectl get pods -n $ns } | complete)

    if $result.exit_code != 0 {
        log error "Failed to connect to cluster"
        log error $result.stderr
        print ""
        print "Troubleshooting:"
        print "  1. Check VPN connection"
        print "  2. Run: kubectl config use-context microk8s"
        print "  3. Run: kubectl cluster-info"
        return false
    }

    print "Cluster connection: OK"
    print ""
    print "Environment:"
    print $"  KUBE_HOST: ($env.KUBE_HOST)"
    print $"  KUBE_DAPPLE_NAMESPACE: ($ns)"
    print $"  KUBE_REGISTRY_HOST: ($env.KUBE_REGISTRY_HOST)"
    print ""
    print "Pod Status:"
    print $result.stdout

    true
}
