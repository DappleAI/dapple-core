# Log viewing commands for dapple services
#
# Examples:
#   dapple logs api           # Tail logs for api service
#   dapple logs api --follow  # Stream logs (default)
#   dapple logs api -p        # Previous container logs

use std/log

use ./aliases.nu [resolve-service]

# View container logs for a service
export def main [
    service: string        # Service alias (api, conduit, flow, etc.)
    --follow (-f)          # Stream logs continuously (default: true)
    --previous (-p)        # Show logs from previous container instance
    --tail: int = 100      # Number of lines to show
    --container (-c): string = ""  # Specific container name (for multi-container pods)
    --since: string = ""   # Show logs since duration (e.g., "1h", "30m")
] {
    let deployment = (resolve-service $service)
    let ns = $env.KUBE_DAPPLE_NAMESPACE

    # Build kubectl logs command arguments
    mut args = ["logs" "-n" $ns $"deployment/($deployment)" "--tail" ($tail | into string)]

    # Follow is default behavior unless --previous is specified
    if not $previous {
        $args = ($args | append "-f")
    }

    if $previous {
        $args = ($args | append "--previous")
    }

    if ($container | is-not-empty) {
        $args = ($args | append ["-c" $container])
    }

    if ($since | is-not-empty) {
        $args = ($args | append ["--since" $since])
    }

    kubectl ...$args
}
