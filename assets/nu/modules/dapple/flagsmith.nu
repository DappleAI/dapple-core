# Flagsmith Feature Flag Platform CLI Commands
#
# Manage the Flagsmith feature flag platform deployed in the 'flagsmith' namespace.
# This module operates independently from the main dapple namespace.
#
# Examples:
#   dapple flagsmith setup              # Deploy Flagsmith to cluster
#   dapple flagsmith status             # Show all Flagsmith pods
#   dapple flagsmith logs api           # View API service logs
#   dapple flagsmith restart api        # Rolling restart of API
#
# Service Aliases:
#   api            -> flagsmith-api (unified API + frontend)
#   task-processor -> flagsmith-task-processor
#   postgres       -> flagsmith-postgres
#   redis          -> flagsmith-redis

use std/log

# The generated-secrets file moved under ~/.dapple/; env_files.nu owns the path.
use ../dapple_env_profiles/env_files.nu [generated_env_path read_pairs]

# Flagsmith namespace (separate from dapple-sandbox and lago)
const FLAGSMITH_NAMESPACE = "flagsmith"

# Resolve service alias to full deployment name
def resolve-flagsmith-service [alias: string]: nothing -> string {
    match $alias {
        "api" | "frontend" | "front" | "ui" => "flagsmith-api"
        "task-processor" | "processor" | "worker" => "flagsmith-task-processor"
        "postgres" | "pg" | "db" => "flagsmith-postgres"
        "redis" => "flagsmith-redis"
        _ => $alias  # Pass through full names
    }
}

# List all Flagsmith service aliases
def flagsmith-service-aliases []: nothing -> list<record<alias: string, deployment: string>> {
    [
        { alias: "api", deployment: "flagsmith-api" }
        { alias: "task-processor", deployment: "flagsmith-task-processor" }
        { alias: "postgres", deployment: "flagsmith-postgres" }
        { alias: "redis", deployment: "flagsmith-redis" }
    ]
}

# Deploy Flagsmith feature flag platform to the cluster
export def setup [
    --force (-f)  # Regenerate secrets and redeploy
] {
    let flagsmith_dir = $env.DAPPLE_PROMPT_WORKSPACE_DIR | path join "assets" "k8s" "registry" "flagsmith"

    log info "Starting Flagsmith feature flag platform deployment"

    # Phase 1: Namespace
    log info "Phase 1: Creating namespace"
    kubectl apply -f ($flagsmith_dir | path join "namespace.yaml")

    # Phase 2: Secrets
    log info "Phase 2: Setting up secrets"
    setup-flagsmith-secrets $force

    # Phase 3: ConfigMap
    log info "Phase 3: Applying ConfigMap"
    kubectl apply -f ($flagsmith_dir | path join "configmap.yaml")

    # Phase 4: Data stores (postgres, redis)
    log info "Phase 4: Deploying data stores"
    kubectl apply -f ($flagsmith_dir | path join "postgres.yaml")
    kubectl apply -f ($flagsmith_dir | path join "redis.yaml")

    log info "Waiting for postgres to be ready..."
    wait-for-flagsmith-deployment "flagsmith-postgres" "180s"

    log info "Waiting for redis to be ready..."
    wait-for-flagsmith-deployment "flagsmith-redis" "60s"

    # Phase 5: API + Frontend (unified container)
    log info "Phase 5: Deploying API service"
    kubectl apply -f ($flagsmith_dir | path join "api.yaml")

    log info "Waiting for API to be ready..."
    wait-for-flagsmith-deployment "flagsmith-api" "120s"

    # Phase 6: Task Processor
    log info "Phase 6: Deploying task processor"
    kubectl apply -f ($flagsmith_dir | path join "task-processor.yaml")

    log info "Waiting for task processor to be ready..."
    wait-for-flagsmith-deployment "flagsmith-task-processor" "120s"

    # Phase 7: Ingress
    log info "Phase 7: Configuring ingress"
    kubectl apply -f ($flagsmith_dir | path join "ingress.yaml")

    log info "Flagsmith deployment complete!"
    print ""
    print "Access Flagsmith at:"
    print "  Frontend: https://flagsmith.kube.dappleai.dev"
    print "  API: https://flagsmith-api.kube.dappleai.dev"
    print ""
    print "Check status with: dapple flagsmith status"
}

# Show pod status for Flagsmith services
export def status [
    service?: string  # Service alias (optional - shows all if omitted)
    --wide (-w)       # Show additional columns (node, IP)
] {
    if ($service | is-empty) {
        if $wide {
            kubectl get pods -n $FLAGSMITH_NAMESPACE -o wide
        } else {
            kubectl get pods -n $FLAGSMITH_NAMESPACE
        }
    } else {
        let deployment = (resolve-flagsmith-service $service)
        if $wide {
            kubectl get pods -n $FLAGSMITH_NAMESPACE -l $"app=($deployment)" -o wide
        } else {
            kubectl get pods -n $FLAGSMITH_NAMESPACE -l $"app=($deployment)"
        }
    }
}

# View container logs for a Flagsmith service
export def logs [
    service: string        # Service alias (api, task-processor, etc.)
    --follow (-f)          # Stream logs continuously (default: true)
    --previous (-p)        # Show logs from previous container instance
    --tail: int = 100      # Number of lines to show
    --since: string = ""   # Show logs since duration (e.g., "1h", "30m")
] {
    let deployment = (resolve-flagsmith-service $service)

    mut args = ["logs" "-n" $FLAGSMITH_NAMESPACE $"deployment/($deployment)" "--tail" ($tail | into string)]

    # Follow is default behavior unless --previous is specified
    if not $previous {
        $args = ($args | append "-f")
    }

    if $previous {
        $args = ($args | append "--previous")
    }

    if ($since | is-not-empty) {
        $args = ($args | append ["--since" $since])
    }

    kubectl ...$args
}

# Show detailed pod information (kubectl describe)
export def pod-describe [
    service: string  # Service alias (api, task-processor, etc.)
] {
    let deployment = (resolve-flagsmith-service $service)

    let pod = (kubectl get pods -n $FLAGSMITH_NAMESPACE -l $"app=($deployment)" -o name
        | lines | first | default "" | str replace "pod/" "")

    if ($pod == "") {
        log error $"No pod found for service ($service) \(deployment: ($deployment))"
        return
    }

    kubectl describe pod $pod -n $FLAGSMITH_NAMESPACE
}

# Show recent cluster events for Flagsmith namespace
export def events [
    --all (-a)        # Show all events (default: only warnings/errors)
    --watch (-w)      # Watch for new events
] {
    mut args = ["get" "events" "-n" $FLAGSMITH_NAMESPACE "--sort-by=.lastTimestamp"]

    if not $all {
        $args = ($args | append ["--field-selector" "type!=Normal"])
    }

    if $watch {
        $args = ($args | append "-w")
    }

    kubectl ...$args
}

# Perform a rolling restart of a Flagsmith service
export def restart [
    service: string        # Service alias or "all" for all services
    --timeout: string = "120s"  # Rollout timeout
] {
    if $service == "all" {
        let deployments = (kubectl get deployments -n $FLAGSMITH_NAMESPACE -o name | lines)

        for deploy in $deployments {
            let name = ($deploy | str replace "deployment.apps/" "")
            log info $"Restarting ($name)..."
            kubectl rollout restart $deploy -n $FLAGSMITH_NAMESPACE
        }

        log info "Waiting for all rollouts to complete..."
        for deploy in $deployments {
            let name = ($deploy | str replace "deployment.apps/" "")
            let result = (kubectl rollout status $deploy -n $FLAGSMITH_NAMESPACE --timeout $timeout | complete)
            if $result.exit_code == 0 {
                log info $"($name) restarted successfully"
            } else {
                log warning $"($name) rollout may not have completed: ($result.stderr)"
            }
        }
    } else {
        let deployment = (resolve-flagsmith-service $service)
        log info $"Restarting ($deployment)..."

        kubectl rollout restart $"deployment/($deployment)" -n $FLAGSMITH_NAMESPACE

        log info "Waiting for rollout to complete..."
        let result = (kubectl rollout status $"deployment/($deployment)" -n $FLAGSMITH_NAMESPACE --timeout $timeout | complete)

        if $result.exit_code == 0 {
            log info $"($deployment) restarted successfully"
        } else {
            log error $"Rollout failed: ($result.stderr)"
            error make { msg: $"Restart failed for ($deployment)" }
        }
    }
}

# Clean up stuck, failed, or terminated pods in Flagsmith namespace
export def cleanup [
    --force (-f)      # Force delete stuck pods (grace-period=0)
    --dry-run (-d)    # Show what would be deleted without deleting
] {
    let all_pods = (kubectl get pods -n $FLAGSMITH_NAMESPACE -o json | from json | get items)

    let bad_pods = ($all_pods | where {|p|
        let phase = ($p.status.phase)
        ($phase in ["Failed" "Succeeded" "Unknown"]
            or ($p.metadata? | get deletionTimestamp? | is-not-empty))
    })

    if ($bad_pods | is-empty) {
        log info "No pods to clean up"
        return
    }

    print $"Found ($bad_pods | length) pod\(s\) to clean up:"
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
            kubectl delete pod $name -n $FLAGSMITH_NAMESPACE --grace-period=0 --force
        } else {
            log info $"Deleting ($name)..."
            kubectl delete pod $name -n $FLAGSMITH_NAMESPACE
        }
    }

    log info "Cleanup complete"
}

# List Kubernetes services in Flagsmith namespace
export def services [
    service?: string  # Service alias (optional - shows all if omitted)
] {
    if ($service | is-empty) {
        kubectl get services -n $FLAGSMITH_NAMESPACE
    } else {
        let deployment = (resolve-flagsmith-service $service)
        kubectl get service $deployment -n $FLAGSMITH_NAMESPACE -o yaml
    }
}

# List deployments in Flagsmith namespace
export def deployments [
    service?: string  # Service alias (optional - shows all if omitted)
] {
    if ($service | is-empty) {
        kubectl get deployments -n $FLAGSMITH_NAMESPACE
    } else {
        let deployment = (resolve-flagsmith-service $service)
        kubectl get deployment $deployment -n $FLAGSMITH_NAMESPACE -o yaml
    }
}

# Show all Flagsmith service aliases
export def aliases [] {
    flagsmith-service-aliases | table
}

# Main help command
export def main [] {
    print "
dapple flagsmith - Flagsmith feature flag platform management

Usage: dapple flagsmith <command> [options]

Deployment:
  dapple flagsmith setup            Deploy Flagsmith to cluster
  dapple flagsmith setup --force    Regenerate secrets and redeploy

Log & Debug Commands:
  dapple flagsmith status [svc]     Show pod status (all if no service)
  dapple flagsmith status -w        Show status with extra columns
  dapple flagsmith logs <service>   View container logs (streams by default)
  dapple flagsmith logs <svc> -p    Show logs from previous container
  dapple flagsmith pod-describe <svc>   Detailed pod information
  dapple flagsmith events           Recent cluster events (warnings only)
  dapple flagsmith events -a        Show all events including normal

Lifecycle Commands:
  dapple flagsmith restart <svc>    Rolling restart of a service
  dapple flagsmith restart all      Restart all Flagsmith services
  dapple flagsmith cleanup          Remove stuck/failed pods
  dapple flagsmith cleanup --force  Force delete stuck pods

Information:
  dapple flagsmith deployments      List all deployments
  dapple flagsmith services         List all k8s services
  dapple flagsmith aliases          Show service alias mappings

Service Aliases:
  api (or front, ui), task-processor (or processor, worker), postgres, redis

Examples:
  dapple flagsmith setup                    # Full deployment
  dapple flagsmith logs api                 # Stream API logs
  dapple flagsmith status                   # Overview of all pods
  dapple flagsmith restart task-processor   # Rolling restart
  dapple flagsmith cleanup --force          # Clean up stuck pods
"
}

# Internal: Wait for a deployment to be ready
def wait-for-flagsmith-deployment [name: string, timeout: string] {
    let result = (kubectl rollout status $"deployment/($name)" -n $FLAGSMITH_NAMESPACE --timeout $timeout | complete)

    if $result.exit_code != 0 {
        log error $"Deployment ($name) failed to become ready: ($result.stderr)"
        error make { msg: $"Deployment ($name) not ready after ($timeout)" }
    }

    log info $"($name) is ready"
}

# Internal: Setup Flagsmith secrets
def setup-flagsmith-secrets [force: bool] {
    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    let dapple_env_file = (generated_env_path $home)

    # Regenerate the secrets file to ensure Flagsmith secrets exist
    use ../tools/env_dapple.nu
    env_dapple gen

    # Load them back via read_pairs, so this agrees with load-dotenv.
    if not ($dapple_env_file | path exists) {
        error make { msg: $"($dapple_env_file) not found after generation" }
    }
    let env_vars = (read_pairs $dapple_env_file)

    # Extract Flagsmith-specific secrets
    let django_secret_key = ($env_vars | get -o FLAGSMITH_DJANGO_SECRET_KEY | default "")

    # Validate all secrets are present
    if ($django_secret_key | is-empty) {
        log error $"Missing Flagsmith secrets in ($dapple_env_file)"
        log error "Required: FLAGSMITH_DJANGO_SECRET_KEY"
        error make { msg: "Missing Flagsmith secrets" }
    }

    # Delete existing secrets if force flag is set
    if $force {
        log info "Force flag set - recreating secrets"
        do { kubectl delete secret flagsmith-secrets -n $FLAGSMITH_NAMESPACE } | ignore
        do { kubectl delete secret flagsmith-postgres-secret -n $FLAGSMITH_NAMESPACE } | ignore
    }

    # Create flagsmith-secrets using kubectl
    log info "Creating flagsmith-secrets..."
    let result = (kubectl create secret generic flagsmith-secrets -n $FLAGSMITH_NAMESPACE
        $"--from-literal=DJANGO_SECRET_KEY=($django_secret_key)"
        "--from-literal=POSTGRES_PASSWORD=flagsmith"
        "--from-literal=DATABASE_URL=postgresql://flagsmith:flagsmith@flagsmith-postgres:5432/flagsmith"
        "--from-literal=REDIS_URL=redis://flagsmith-redis:6379"
        --dry-run=client -o yaml | kubectl apply -f - | complete)

    if $result.exit_code != 0 {
        log error $"Failed to create flagsmith-secrets: ($result.stderr)"
        error make { msg: "Failed to create flagsmith-secrets" }
    }

    # Create postgres-specific secret
    log info "Creating flagsmith-postgres-secret..."
    let pg_result = (kubectl create secret generic flagsmith-postgres-secret -n $FLAGSMITH_NAMESPACE
        "--from-literal=POSTGRES_PASSWORD=flagsmith"
        --dry-run=client -o yaml | kubectl apply -f - | complete)

    if $pg_result.exit_code != 0 {
        log error $"Failed to create flagsmith-postgres-secret: ($pg_result.stderr)"
        error make { msg: "Failed to create flagsmith-postgres-secret" }
    }

    log info "Secrets configured successfully"
}
