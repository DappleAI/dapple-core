# Access commands for dapple services (shell, port-forward)
#
# Examples:
#   dapple shell api           # Open shell in api pod
#   dapple port-forward api    # Forward local port to api service

use std/log

use ./aliases.nu [resolve-service]

# Open a shell in a running container
export def shell [
    service: string     # Service alias (api, conduit, flow, etc.)
    --container (-c): string = ""  # Specific container name
    --shell: string = "/bin/sh"    # Shell to use (default: /bin/sh)
] {
    let deployment = (resolve-service $service)
    let ns = $env.KUBE_DAPPLE_NAMESPACE

    # Get the pod name for this deployment
    let pod = (kubectl get pods -n $ns -l $"app=($deployment)" -o name --field-selector=status.phase=Running
        | lines | first | default "" | str replace "pod/" "")

    if ($pod == "") {
        log error $"No running pod found for service ($service) (deployment: ($deployment))"
        return
    }

    log info $"Opening shell in pod ($pod)..."

    mut args = ["exec" "-it" "-n" $ns $pod]

    if ($container | is-not-empty) {
        $args = ($args | append ["-c" $container])
    }

    $args = ($args | append ["--" $shell])

    kubectl ...$args
}

# Forward local port to a service
export def port-forward [
    service: string        # Service alias (api, conduit, flow, etc.)
    local_port?: int       # Local port to use (default: same as service port)
    --remote-port (-r): int = 0  # Remote port on the pod (default: inferred from service)
] {
    let deployment = (resolve-service $service)
    let ns = $env.KUBE_DAPPLE_NAMESPACE

    # Default ports for known services
    let default_port = match $deployment {
        "dapple-prompt-api" => 8001
        "dapple-prompt-conduit" => 8081
        "dapple-prompt-flow" => 8082
        "dapple-ui" => 3000
        "dapple-web-landing-page" => 3000
        "prompt-db" => 5432
        "dapple-redis" => 6379
        _ => 8080
    }

    let remote = if $remote_port > 0 { $remote_port } else { $default_port }
    let local = if ($local_port | is-empty) { $remote } else { $local_port }

    log info $"Forwarding localhost:($local) -> ($deployment):($remote)"
    log info "Press Ctrl+C to stop forwarding"

    kubectl port-forward -n $ns $"deployment/($deployment)" $"($local):($remote)"
}

# Get the external URL for a service
export def url [
    service: string     # Service alias (api, conduit, flow, etc.)
] {
    let deployment = (resolve-service $service)

    # Return known URLs
    match $deployment {
        "dapple-prompt-api" => { print $env.DAPPLE_PROMPT_API_URL }
        "dapple-ui" => { print $"https://($env.DAPPLE_UI_HOST)" }
        "dapple-web-landing-page" => { print $env.DAPPLE_WEB_LANDING_PAGE_URL }
        _ => { log warning $"No known external URL for ($deployment)" }
    }
}
