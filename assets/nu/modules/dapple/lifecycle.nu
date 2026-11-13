# Lifecycle commands for dapple services (restart, cleanup)
#
# Examples:
#   dapple restart api        # Rolling restart of api service
#   dapple restart all        # Restart all dapple services
#   dapple cleanup            # Remove stuck/failed pods
#   dapple cleanup --force    # Force delete stuck pods

use std/log

use ./aliases.nu [resolve-service list-aliases]

# Perform a rolling restart of a service
export def restart [
    service: string     # Service alias or "all" for all services
    --timeout: string = "120s"  # Rollout timeout
] {
    let ns = $env.KUBE_DAPPLE_NAMESPACE

    if $service == "all" {
        # Restart all dapple services
        let deployments = (kubectl get deployments -n $ns -o name | lines)

        for deploy in $deployments {
            let name = ($deploy | str replace "deployment.apps/" "")
            log info $"Restarting ($name)..."
            kubectl rollout restart $deploy -n $ns
        }

        log info "Waiting for all rollouts to complete..."
        for deploy in $deployments {
            let name = ($deploy | str replace "deployment.apps/" "")
            let result = (kubectl rollout status $deploy -n $ns --timeout $timeout | complete)
            if $result.exit_code == 0 {
                log info $"($name) restarted successfully"
            } else {
                log warning $"($name) rollout may not have completed: ($result.stderr)"
            }
        }
    } else {
        let deployment = (resolve-service $service)
        log info $"Restarting ($deployment)..."

        kubectl rollout restart $"deployment/($deployment)" -n $ns

        log info "Waiting for rollout to complete..."
        let result = (kubectl rollout status $"deployment/($deployment)" -n $ns --timeout $timeout | complete)

        if $result.exit_code == 0 {
            log info $"($deployment) restarted successfully"
        } else {
            log error $"Rollout failed: ($result.stderr)"
            error make { msg: $"Restart failed for ($deployment)" }
        }
    }
}

# Clean up stuck, failed, or terminated pods
export def cleanup [
    --force (-f)      # Force delete stuck pods (grace-period=0)
    --dry-run (-d)    # Show what would be deleted without deleting
] {
    let ns = $env.KUBE_DAPPLE_NAMESPACE

    # Get pods in bad states
    let all_pods = (kubectl get pods -n $ns -o json | from json | get items)

    let bad_pods = ($all_pods | where {|p|
        let phase = ($p.status.phase)
        # Include: Failed, Succeeded (completed jobs), Unknown, or pods being deleted
        ($phase in ["Failed" "Succeeded" "Unknown"]
            or ($p.metadata? | get deletionTimestamp? | is-not-empty))
    })

    if ($bad_pods | is-empty) {
        log info "No pods to clean up"
        return
    }

    let count = ($bad_pods | length)
    print $"Found ($count) pods to clean up:"
    for pod in $bad_pods {
        let name = $pod.metadata.name
        let phase = $pod.status.phase
        let deleting = if ($pod.metadata? | get deletionTimestamp? | is-not-empty) { " (Terminating)" } else { "" }
        print $"  - ($name) [($phase)($deleting)]"
    }

    if $dry_run {
        print "\nDry run - no pods deleted"
        return
    }

    print ""

    for pod in $bad_pods {
        let name = $pod.metadata.name
        if $force {
            log info $"Force deleting ($name)..."
            kubectl delete pod $name -n $ns --grace-period=0 --force
        } else {
            log info $"Deleting ($name)..."
            kubectl delete pod $name -n $ns
        }
    }

    log info "Cleanup complete"
}

# Scale a deployment (useful for debugging)
export def scale [
    service: string     # Service alias
    replicas: int       # Number of replicas
] {
    let deployment = (resolve-service $service)
    let ns = $env.KUBE_DAPPLE_NAMESPACE

    log info $"Scaling ($deployment) to ($replicas) replicas..."
    kubectl scale $"deployment/($deployment)" --replicas $replicas -n $ns

    if $replicas > 0 {
        log info "Waiting for rollout..."
        kubectl rollout status $"deployment/($deployment)" -n $ns --timeout "120s"
    }

    log info $"($deployment) scaled to ($replicas) replicas"
}
