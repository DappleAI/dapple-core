# Status and describe commands for dapple services
#
# Examples:
#   dapple status             # Overview of all pods
#   dapple status api         # Detailed status of api service
#   dapple describe api       # kubectl describe for the pod
#   dapple events             # Recent cluster events

use std/log

use ./aliases.nu [resolve-service]

# Show pod status for dapple services
export def main [
    service?: string  # Service alias (optional - shows all if omitted)
    --wide (-w)       # Show additional columns (node, IP)
] {
    let ns = $env.KUBE_DAPPLE_NAMESPACE

    if ($service | is-empty) {
        # Show all pods in the namespace
        if $wide {
            kubectl get pods -n $ns -o wide
        } else {
            kubectl get pods -n $ns
        }
    } else {
        let deployment = (resolve-service $service)
        # Show pods for specific service
        if $wide {
            kubectl get pods -n $ns -l $"app=($deployment)" -o wide
        } else {
            kubectl get pods -n $ns -l $"app=($deployment)"
        }
    }
}

# Show detailed pod information (kubectl describe)
export def pod-describe [
    service: string  # Service alias (api, conduit, flow, etc.)
] {
    let deployment = (resolve-service $service)
    let ns = $env.KUBE_DAPPLE_NAMESPACE

    # Get the pod name for this deployment
    let pod = (kubectl get pods -n $ns -l $"app=($deployment)" -o name
        | lines | first | default "" | str replace "pod/" "")

    if ($pod == "") {
        log error $"No pod found for service ($service) (deployment: ($deployment))"
        return
    }

    kubectl describe pod $pod -n $ns
}

# Show recent cluster events (useful for debugging)
export def events [
    --all (-a)        # Show all events (default: only warnings/errors)
    --watch (-w)      # Watch for new events
] {
    let ns = $env.KUBE_DAPPLE_NAMESPACE

    mut args = ["get" "events" "-n" $ns "--sort-by=.lastTimestamp"]

    if not $all {
        # Filter to Warning type events by default
        $args = ($args | append ["--field-selector" "type!=Normal"])
    }

    if $watch {
        $args = ($args | append "-w")
    }

    kubectl ...$args
}

# Show deployment information
export def deployments [
    service?: string  # Service alias (optional - shows all if omitted)
] {
    let ns = $env.KUBE_DAPPLE_NAMESPACE

    if ($service | is-empty) {
        kubectl get deployments -n $ns
    } else {
        let deployment = (resolve-service $service)
        kubectl get deployment $deployment -n $ns -o yaml
    }
}

# Show services (k8s Service resources)
export def services [
    service?: string  # Service alias (optional - shows all if omitted)
] {
    let ns = $env.KUBE_DAPPLE_NAMESPACE

    if ($service | is-empty) {
        kubectl get services -n $ns
    } else {
        let deployment = (resolve-service $service)
        kubectl get service $deployment -n $ns -o yaml
    }
}
