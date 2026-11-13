#!/usr/bin/env nu
# Update deployment with new image OR restart for hostPath mode
#
# Usage:
#   nu patch_image.nu api                    # Patch with dapple-latest tag
#   nu patch_image.nu conduit --tag abc123   # Patch with specific commit tag
#   nu patch_image.nu api --restart          # Just restart (hostPath mode - no Docker)
#   nu patch_image.nu api --build            # Build cargo + restart (fast iteration)

use std/log

$env.NU_LOG_LEVEL = "info"

use ../modules/dapple_env_profiles dapple_sandbox_tools
overlay use dapple_sandbox_tools as dapple_env

use ../modules/k8s
use k8s ensure_vm_mounts

def is_macos []: nothing -> bool {
    $nu.os-info.name | str contains "mac"
}

# Service alias to deployment name mapping
def resolve-service [alias: string]: nothing -> string {
    match $alias {
        "api" => "dapple-prompt-api"
        "conduit" => "dapple-prompt-conduit"
        "flow" => "dapple-prompt-flow"
        "rust-docs" | "docs" => "rust-docs"
        "ui" => "dapple-ui"
        "landing" => "dapple-web-landing-page"
        "postgres" => "prompt-db"
        "redis" => "dapple-redis"
        _ => $alias  # Pass through full names
    }
}

def main [
    service: string      # Service alias (api, conduit, flow, etc.)
    --tag: string = ""   # Specific image tag (default: dapple-latest)
    --timeout: string = "120s"  # Rollout timeout
    --restart (-r)       # Just restart deployment (for hostPath mode, no Docker)
    --build (-b)         # Build cargo first, then restart (fast iteration for hostPath)
    --profile: string = "debug"  # Cargo build profile (debug or release)
] {
    let deployment = (resolve-service $service)
    let ns = $env.KUBE_DAPPLE_NAMESPACE

    # Fast path: build + restart for hostPath mode
    if $build {
        # On macOS, ensure workspace is mounted into MicroK8s VM
        if (is_macos) {
            log info "Ensuring workspace is mounted in MicroK8s VM..."
            ensure_vm_mounts [$env.DAPPLE_PROMPT_WORKSPACE_DIR]
        }

        log info $"Building ($deployment) with cargo..."
        cd $env.DAPPLE_PROMPT_WORKSPACE_DIR
        # Cargo reserves the profile name "debug": the dev profile is spelled
        # "dev" but still outputs to target/debug (where the hostPath mounts
        # point), so map the alias before invoking cargo.
        let cargo_profile = if $profile == "debug" { "dev" } else { $profile }
        let build_result = (cargo build -p $deployment --profile $cargo_profile | complete)
        if $build_result.exit_code != 0 {
            log error $"Cargo build failed"
            print $build_result.stderr
            error make { msg: $"Build failed for ($deployment)" }
        }
        log info $"Build complete, restarting deployment..."
        restart_deployment $deployment $ns $timeout
        return
    }

    # Restart-only path for hostPath mode (binary already built)
    if $restart {
        # On macOS, ensure workspace is mounted into MicroK8s VM
        if (is_macos) {
            ensure_vm_mounts [$env.DAPPLE_PROMPT_WORKSPACE_DIR]
        }

        restart_deployment $deployment $ns $timeout
        return
    }

    # Docker image path (original behavior).
    #
    # Guard first: a hostPath-mode deployment runs code from a mounted host
    # directory, so the image's copy is never executed. Patching the image there
    # looks like a successful deploy while the pod keeps running whatever was
    # last built on the host — fail loudly with the commands that actually work.
    let host_mount = (hostpath_mount $deployment $ns)
    if ($host_mount | is-not-empty) {
        log error $"($deployment) is deployed in hostPath mode: it runs from the mounted host path"
        log error $"  ($host_mount)"
        log error "NOT from the Docker image, so patching the image would silently do nothing."
        log error ""
        log error "Use one of:"
        log error $"  dapple patch ($service) --build     # Rust svc: compile \(cross-compiles on macOS) + restart"
        log error $"  dapple patch ($service) --restart   # restart to pick up an already-built binary or mounted source"
        error make { msg: $"($deployment) runs in hostPath mode; an image patch would be a no-op" }
    }

    let image_tag = if ($tag | is-not-empty) {
        $"($env.KUBE_REGISTRY_HOST)/($deployment):($tag)"
    } else {
        $"($env.KUBE_REGISTRY_HOST)/($deployment):($env.DOCKER_DAPPLE_TAG_LATEST)"
    }

    log info $"Patching ($deployment) with image ($image_tag)"

    # Point the deployment at the intended image tag. With the default
    # `dapple-latest` tag this is usually a NO-OP: the tag string in the spec
    # is unchanged, so `set image` alone does not roll the pod — a rebuild that
    # pushes new bytes under the same tag would never be pulled.
    kubectl set image $"deployment/($deployment)" $"($deployment)=($image_tag)" -n $ns

    # Force a rollout so a fresh pod actually pulls the new image. `rollout
    # restart` stamps a `restartedAt` annotation that always triggers a roll,
    # and imagePullPolicy: Always makes the new pod re-pull the tag. This is
    # what makes `dapple patch/quick` effective when the tag is unchanged; when
    # a distinct --tag is given, `set image` above already changed the spec and
    # this simply collapses into the same single rollout.
    log info $"Forcing rollout of ($deployment) to pull the new image..."
    kubectl rollout restart $"deployment/($deployment)" -n $ns

    # Wait for rollout
    log info $"Waiting for rollout of ($deployment)..."
    let rollout_result = (kubectl rollout status $"deployment/($deployment)" -n $ns --timeout $timeout | complete)

    if $rollout_result.exit_code != 0 {
        log error $"Rollout failed: ($rollout_result.stderr)"
        log error "Check pod status with: kubectl get pods -n ($ns) -l app=($deployment)"
        error make { msg: $"Rollout failed for ($deployment)" }
    }

    log info $"($deployment) updated successfully"
}

# The host directory a deployment mounts, or "" when it runs purely from its
# image. A non-empty result means the pod runs code from the host (a Rust
# service's cargo target dir, or a JS service's mounted source), so the image's
# copy is never executed and patching the image is a silent no-op.
#
# Fails open ("") so a kubectl hiccup can't block an otherwise valid deploy.
def hostpath_mount [deployment: string, ns: string]: nothing -> string {
    let result = (
        do {
            kubectl get $"deployment/($deployment)" -n $ns -o 'jsonpath={.spec.template.spec.volumes[*].hostPath.path}'
        } | complete
    )
    if $result.exit_code != 0 {
        return ""
    }
    ($result.stdout | str trim)
}

# Restart deployment and wait for rollout (for hostPath mode)
def restart_deployment [deployment: string, ns: string, timeout: string] {
    log info $"Restarting ($deployment) to pick up new binary..."
    kubectl rollout restart $"deployment/($deployment)" -n $ns

    log info $"Waiting for rollout of ($deployment)..."
    let rollout_result = (kubectl rollout status $"deployment/($deployment)" -n $ns --timeout $timeout | complete)

    if $rollout_result.exit_code != 0 {
        log error $"Rollout failed: ($rollout_result.stderr)"
        log error "Check pod status with: kubectl get pods -n ($ns) -l app=($deployment)"
        error make { msg: $"Rollout failed for ($deployment)" }
    }

    log info $"($deployment) restarted successfully"
}
