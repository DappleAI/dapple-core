# Lago Billing Platform CLI Commands
#
# Manage the Lago billing platform deployed in the 'lago' namespace.
# This module operates independently from the main dapple namespace.
#
# Examples:
#   dapple lago setup              # Deploy Lago to cluster
#   dapple lago status             # Show all Lago pods
#   dapple lago logs api           # View API service logs
#   dapple lago restart worker     # Rolling restart of worker
#
# Service Aliases:
#   api       -> lago-api
#   front     -> lago-front
#   worker    -> lago-worker
#   clock     -> lago-clock
#   pdf       -> lago-pdf
#   postgres  -> lago-postgres
#   redis     -> lago-redis

use std/log

# The generated-secrets file moved under ~/.dapple/; env_files.nu owns the path.
# Both call sites below called `env_dapple gen` [which writes the NEW path] and
# then read the OLD one, so setup-lago-secrets hard-failed with "not found after
# generation" on any host that had migrated.
use ../dapple_env_profiles/env_files.nu [generated_env_path read_pairs]

# Lago namespace (separate from dapple-sandbox)
const LAGO_NAMESPACE = "lago"

# Resolve service alias to full deployment name
def resolve-lago-service [alias: string]: nothing -> string {
    match $alias {
        "api" => "lago-api"
        "front" | "frontend" | "ui" => "lago-front"
        "worker" => "lago-worker"
        "clock" => "lago-clock"
        "pdf" => "lago-pdf"
        "postgres" | "pg" | "db" => "lago-postgres"
        "redis" => "lago-redis"
        _ => $alias  # Pass through full names
    }
}

# List all Lago service aliases
def lago-service-aliases []: nothing -> list<record<alias: string, deployment: string>> {
    [
        { alias: "api", deployment: "lago-api" }
        { alias: "front", deployment: "lago-front" }
        { alias: "worker", deployment: "lago-worker" }
        { alias: "clock", deployment: "lago-clock" }
        { alias: "pdf", deployment: "lago-pdf" }
        { alias: "postgres", deployment: "lago-postgres" }
        { alias: "redis", deployment: "lago-redis" }
    ]
}

# Deploy Lago billing platform to the cluster
export def setup [
    --force (-f)  # Regenerate secrets and redeploy
] {
    let lago_dir = $env.DAPPLE_PROMPT_WORKSPACE_DIR | path join "assets" "k8s" "registry" "lago"

    log info "Starting Lago billing platform deployment"

    # Phase 1: Namespace
    log info "Phase 1: Creating namespace"
    kubectl apply -f ($lago_dir | path join "namespace.yaml")

    # Phase 2: Secrets
    log info "Phase 2: Setting up secrets"
    setup-lago-secrets $force

    # Phase 3: ConfigMap
    log info "Phase 3: Applying ConfigMap"
    kubectl apply -f ($lago_dir | path join "configmap.yaml")

    # Phase 4: Data stores (postgres, redis)
    log info "Phase 4: Deploying data stores"
    kubectl apply -f ($lago_dir | path join "postgres.yaml")
    kubectl apply -f ($lago_dir | path join "redis.yaml")

    log info "Waiting for postgres to be ready..."
    wait-for-lago-deployment "lago-postgres" "180s"

    log info "Waiting for redis to be ready..."
    wait-for-lago-deployment "lago-redis" "60s"

    # Phase 5: Migrations
    log info "Phase 5: Running database migrations"
    run-lago-migrations $lago_dir

    # Phase 6: Application services
    log info "Phase 6: Deploying application services"
    kubectl apply -f ($lago_dir | path join "api.yaml")
    kubectl apply -f ($lago_dir | path join "worker.yaml")
    kubectl apply -f ($lago_dir | path join "clock.yaml")
    kubectl apply -f ($lago_dir | path join "pdf.yaml")
    kubectl apply -f ($lago_dir | path join "front.yaml")

    log info "Waiting for api to be ready..."
    wait-for-lago-deployment "lago-api" "120s"

    log info "Waiting for worker to be ready..."
    wait-for-lago-deployment "lago-worker" "120s"

    log info "Waiting for clock to be ready..."
    wait-for-lago-deployment "lago-clock" "60s"

    log info "Waiting for pdf service to be ready..."
    wait-for-lago-deployment "lago-pdf" "60s"

    log info "Waiting for frontend to be ready..."
    wait-for-lago-deployment "lago-front" "60s"

    # Phase 7: Ingress
    log info "Phase 7: Configuring ingress"
    kubectl apply -f ($lago_dir | path join "ingress.yaml")

    log info "Lago deployment complete!"
    print ""
    print "Access Lago at:"
    print "  Frontend: https://lago.kube.dappleai.dev"
    print "  API: https://lago-api.kube.dappleai.dev"
    print ""
    print "Check status with: dapple lago status"
}

# Show pod status for Lago services
export def status [
    service?: string  # Service alias (optional - shows all if omitted)
    --wide (-w)       # Show additional columns (node, IP)
] {
    if ($service | is-empty) {
        if $wide {
            kubectl get pods -n $LAGO_NAMESPACE -o wide
        } else {
            kubectl get pods -n $LAGO_NAMESPACE
        }
    } else {
        let deployment = (resolve-lago-service $service)
        if $wide {
            kubectl get pods -n $LAGO_NAMESPACE -l $"app=($deployment)" -o wide
        } else {
            kubectl get pods -n $LAGO_NAMESPACE -l $"app=($deployment)"
        }
    }
}

# View container logs for a Lago service
export def logs [
    service: string        # Service alias (api, front, worker, etc.)
    --follow (-f)          # Stream logs continuously (default: true)
    --previous (-p)        # Show logs from previous container instance
    --tail: int = 100      # Number of lines to show
    --since: string = ""   # Show logs since duration (e.g., "1h", "30m")
] {
    let deployment = (resolve-lago-service $service)

    mut args = ["logs" "-n" $LAGO_NAMESPACE $"deployment/($deployment)" "--tail" ($tail | into string)]

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
    service: string  # Service alias (api, front, worker, etc.)
] {
    let deployment = (resolve-lago-service $service)

    let pod = (kubectl get pods -n $LAGO_NAMESPACE -l $"app=($deployment)" -o name
        | lines | first | default "" | str replace "pod/" "")

    if ($pod == "") {
        log error $"No pod found for service ($service) \(deployment: ($deployment))"
        return
    }

    kubectl describe pod $pod -n $LAGO_NAMESPACE
}

# Show recent cluster events for Lago namespace
export def events [
    --all (-a)        # Show all events (default: only warnings/errors)
    --watch (-w)      # Watch for new events
] {
    mut args = ["get" "events" "-n" $LAGO_NAMESPACE "--sort-by=.lastTimestamp"]

    if not $all {
        $args = ($args | append ["--field-selector" "type!=Normal"])
    }

    if $watch {
        $args = ($args | append "-w")
    }

    kubectl ...$args
}

# Perform a rolling restart of a Lago service
export def restart [
    service: string        # Service alias or "all" for all services
    --timeout: string = "120s"  # Rollout timeout
] {
    if $service == "all" {
        let deployments = (kubectl get deployments -n $LAGO_NAMESPACE -o name | lines)

        for deploy in $deployments {
            let name = ($deploy | str replace "deployment.apps/" "")
            log info $"Restarting ($name)..."
            kubectl rollout restart $deploy -n $LAGO_NAMESPACE
        }

        log info "Waiting for all rollouts to complete..."
        for deploy in $deployments {
            let name = ($deploy | str replace "deployment.apps/" "")
            let result = (kubectl rollout status $deploy -n $LAGO_NAMESPACE --timeout $timeout | complete)
            if $result.exit_code == 0 {
                log info $"($name) restarted successfully"
            } else {
                log warning $"($name) rollout may not have completed: ($result.stderr)"
            }
        }
    } else {
        let deployment = (resolve-lago-service $service)
        log info $"Restarting ($deployment)..."

        kubectl rollout restart $"deployment/($deployment)" -n $LAGO_NAMESPACE

        log info "Waiting for rollout to complete..."
        let result = (kubectl rollout status $"deployment/($deployment)" -n $LAGO_NAMESPACE --timeout $timeout | complete)

        if $result.exit_code == 0 {
            log info $"($deployment) restarted successfully"
        } else {
            log error $"Rollout failed: ($result.stderr)"
            error make { msg: $"Restart failed for ($deployment)" }
        }
    }
}

# Clean up stuck, failed, or terminated pods in Lago namespace
export def cleanup [
    --force (-f)      # Force delete stuck pods (grace-period=0)
    --dry-run (-d)    # Show what would be deleted without deleting
] {
    let all_pods = (kubectl get pods -n $LAGO_NAMESPACE -o json | from json | get items)

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
            kubectl delete pod $name -n $LAGO_NAMESPACE --grace-period=0 --force
        } else {
            log info $"Deleting ($name)..."
            kubectl delete pod $name -n $LAGO_NAMESPACE
        }
    }

    log info "Cleanup complete"
}

# Re-run database migrations
export def migrate [
    --force (-f)  # Delete existing job and re-run
] {
    let lago_dir = $env.DAPPLE_PROMPT_WORKSPACE_DIR | path join "assets" "k8s" "registry" "lago"

    if $force {
        log info "Deleting existing migration job..."
        do { kubectl delete job lago-migrations -n $LAGO_NAMESPACE } | ignore
    }

    run-lago-migrations $lago_dir
}

# List Kubernetes services in Lago namespace
export def services [
    service?: string  # Service alias (optional - shows all if omitted)
] {
    if ($service | is-empty) {
        kubectl get services -n $LAGO_NAMESPACE
    } else {
        let deployment = (resolve-lago-service $service)
        kubectl get service $deployment -n $LAGO_NAMESPACE -o yaml
    }
}

# List deployments in Lago namespace
export def deployments [
    service?: string  # Service alias (optional - shows all if omitted)
] {
    if ($service | is-empty) {
        kubectl get deployments -n $LAGO_NAMESPACE
    } else {
        let deployment = (resolve-lago-service $service)
        kubectl get deployment $deployment -n $LAGO_NAMESPACE -o yaml
    }
}

# Show all Lago service aliases
export def aliases [] {
    lago-service-aliases | table
}

# Create (or show) the isolated test organization in Lago.
#
# The test org keeps integration-test data (customers, events, subscriptions)
# separate from your personal/manual testing org.
#
# On first run it signs up a dedicated user+org in Lago and writes
# LAGO_TEST_API_KEY to ~/.dapple/generated.env. Subsequent runs print the key.
export def test-org [
    --force (-f)  # Re-create even if LAGO_TEST_API_KEY already exists
] {
    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    let dapple_env_file = (generated_env_path $home)
    let lago_api_url = ($env | get -o LAGO_API_URL | default "https://lago-api.kube.dappleai.dev")

    # Check if test key already exists
    if not $force {
        let existing = ($env | get -o LAGO_TEST_API_KEY | default "")
        if ($existing | is-not-empty) {
            print $"Test org already configured. LAGO_TEST_API_KEY=($existing | str substring 0..8)..."
            print "Use --force to recreate."
            return
        }

        # Also check the env file
        if ($dapple_env_file | path exists) {
            let file_key = (open $dapple_env_file --raw | lines
                | where {|l| $l starts-with "LAGO_TEST_API_KEY="}
                | first | default ""
                | str replace "LAGO_TEST_API_KEY=" "")
            if ($file_key | is-not-empty) {
                print $"Test org already configured in ($dapple_env_file). LAGO_TEST_API_KEY=($file_key | str substring 0..8)..."
                print "Use --force to recreate."
                return
            }
        }
    }

    log info "Creating Lago test organization..."

    # Sign up a new user+org via Lago's auth API
    let signup_body = {
        user: {
            email: "test@dappleai.dev"
            password: "dapple-test-org-2026!"
            organization_name: "dapple-test"
        }
    }

    let signup_result = (http post $"($lago_api_url)/auth/sign_up"
        --content-type application/json
        $signup_body
        | complete)

    if $signup_result.exit_code != 0 {
        # Lago returns 422 if user already exists — try logging in instead
        log info "Signup returned error (user may already exist), trying login..."

        let login_body = {
            user: {
                email: "test@dappleai.dev"
                password: "dapple-test-org-2026!"
            }
        }

        let login_result = (http post $"($lago_api_url)/auth/sign_in"
            --content-type application/json
            $login_body)

        let token = ($login_result | get token)
        log info "Logged in to existing test user"

        # Fetch org details to get the API key
        let org = (http get $"($lago_api_url)/api/v1/organizations"
            --headers [Authorization $"Bearer ($token)"])

        let api_key = ($org | get organization.api_key)
        save-test-api-key $dapple_env_file $api_key
        return
    }

    # Parse signup response
    let response = ($signup_result.stdout | from json)
    let token = ($response | get token)

    log info $"Created test org: ($response | get user.organizations | first | get name)"

    # Fetch org API key using the bearer token
    let org = (http get $"($lago_api_url)/api/v1/organizations"
        --headers [Authorization $"Bearer ($token)"])

    let api_key = ($org | get organization.api_key)
    save-test-api-key $dapple_env_file $api_key
}

# Internal: persist the test API key to the generated secrets file
def save-test-api-key [env_file: path, api_key: string] {
    # Append or replace in env file
    if ($env_file | path exists) {
        let content = (open $env_file --raw)
        if ($content | str contains "LAGO_TEST_API_KEY=") {
            # Replace existing line
            let updated = ($content | lines
                | each {|l| if ($l starts-with "LAGO_TEST_API_KEY=") { $"LAGO_TEST_API_KEY=($api_key)" } else { $l } }
                | str join "\n")
            $updated | save -f $env_file
        } else {
            # Append new line
            $"\nLAGO_TEST_API_KEY=($api_key)\n" | save --append $env_file
        }
    } else {
        $"LAGO_TEST_API_KEY=($api_key)\n" | save $env_file
    }

    log info $"LAGO_TEST_API_KEY saved to ($env_file)"
    print $"Test org API key: ($api_key | str substring 0..8)..."
    print ""
    print "Integration tests will use this key automatically."
    print "Restart your shell to pick it up."
}

# Main help command
export def main [] {
    print "
dapple lago - Lago billing platform management

Usage: dapple lago <command> [options]

Deployment:
  dapple lago setup            Deploy Lago to cluster
  dapple lago setup --force    Regenerate secrets and redeploy
  dapple lago migrate          Re-run database migrations
  dapple lago migrate --force  Delete job and re-run migrations

Test Isolation:
  dapple lago test-org         Create isolated test org (writes LAGO_TEST_API_KEY)
  dapple lago test-org --force Re-create even if key exists

Log & Debug Commands:
  dapple lago status [svc]     Show pod status (all if no service)
  dapple lago status -w        Show status with extra columns
  dapple lago logs <service>   View container logs (streams by default)
  dapple lago logs <svc> -p    Show logs from previous container
  dapple lago pod-describe <svc>   Detailed pod information
  dapple lago events           Recent cluster events (warnings only)
  dapple lago events -a        Show all events including normal

Lifecycle Commands:
  dapple lago restart <svc>    Rolling restart of a service
  dapple lago restart all      Restart all Lago services
  dapple lago cleanup          Remove stuck/failed pods
  dapple lago cleanup --force  Force delete stuck pods

Information:
  dapple lago deployments      List all deployments
  dapple lago services         List all k8s services
  dapple lago aliases          Show service alias mappings

Service Aliases:
  api, front, worker, clock, pdf, postgres, redis

Examples:
  dapple lago setup                    # Full deployment
  dapple lago logs api                 # Stream API logs
  dapple lago status                   # Overview of all pods
  dapple lago restart worker           # Rolling restart
  dapple lago cleanup --force          # Clean up stuck pods
"
}

# Internal: Wait for a deployment to be ready
def wait-for-lago-deployment [name: string, timeout: string] {
    let result = (kubectl rollout status $"deployment/($name)" -n $LAGO_NAMESPACE --timeout $timeout | complete)

    if $result.exit_code != 0 {
        log error $"Deployment ($name) failed to become ready: ($result.stderr)"
        error make { msg: $"Deployment ($name) not ready after ($timeout)" }
    }

    log info $"($name) is ready"
}

# Internal: Run database migrations
def run-lago-migrations [lago_dir: path] {
    let job_name = "lago-migrations"

    # Check if job already exists
    let existing = (kubectl get job $job_name -n $LAGO_NAMESPACE -o name | complete)

    if $existing.exit_code == 0 {
        # Job exists - check if completed
        let status = (kubectl get job $job_name -n $LAGO_NAMESPACE -o 'jsonpath={.status.conditions[?(@.type=="Complete")].status}' | str trim)

        if $status == "True" {
            log info "Migrations already completed successfully"
            return
        }

        # Job exists but not complete - delete and retry
        log info "Deleting previous migration job..."
        kubectl delete job $job_name -n $LAGO_NAMESPACE --wait=true
    }

    # Apply fresh migration job
    log info "Applying migration job..."
    kubectl apply -f ($lago_dir | path join "migrations-job.yaml")

    # Wait for job completion
    log info "Waiting for migrations to complete..."

    mut attempts = 0
    let max_attempts = 60  # 5 minutes max

    loop {
        $attempts = $attempts + 1

        let status = (kubectl get job $job_name -n $LAGO_NAMESPACE -o 'jsonpath={.status.conditions[?(@.type=="Complete")].status}' | str trim)
        let failed = (kubectl get job $job_name -n $LAGO_NAMESPACE -o 'jsonpath={.status.conditions[?(@.type=="Failed")].status}' | str trim)

        if $status == "True" {
            log info "Migrations completed successfully"
            return
        }

        if $failed == "True" {
            let logs = (kubectl logs -n $LAGO_NAMESPACE $"job/($job_name)" --tail=50 | str trim)
            log error $"Migration failed. Last 50 lines of logs:\n($logs)"
            error make { msg: "Migration job failed" }
        }

        if $attempts >= $max_attempts {
            log error "Migrations timed out"
            error make { msg: "Migration job timed out after 5 minutes" }
        }

        sleep 5sec
    }
}

# Internal: Setup Lago secrets
def setup-lago-secrets [force: bool] {
    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    let dapple_env_file = (generated_env_path $home)

    # Regenerate the secrets file to ensure Lago secrets exist
    use ../tools/env_dapple.nu
    env_dapple gen

    # Load them back. Parsing via read_pairs so this agrees with load-dotenv;
    # the local parser this replaced did not strip quotes, so a quoted value
    # reached Lago with its quotes attached.
    if not ($dapple_env_file | path exists) {
        error make { msg: $"($dapple_env_file) not found after generation" }
    }
    let env_vars = (read_pairs $dapple_env_file)

    # Extract Lago-specific secrets.
    #
    # LAGO_RSA_PRIVATE_KEY is stored in generated.env as a base64-encoded PEM
    # (single line, no escaping needed) — that's what Lago's
    # config/initializers/rsa_keys.rb expects, since it
    # Base64.decode64s the env var to get the raw PEM. We pass the value
    # through unchanged. See tools/env_dapple.nu where the encoding happens.
    let rsa_key = ($env_vars | get -o LAGO_RSA_PRIVATE_KEY | default "")
    let secret_key_base = ($env_vars | get -o LAGO_SECRET_KEY_BASE | default "")
    let enc_primary = ($env_vars | get -o LAGO_ENCRYPTION_PRIMARY_KEY | default "")
    let enc_det = ($env_vars | get -o LAGO_ENCRYPTION_DETERMINISTIC_KEY | default "")
    let enc_salt = ($env_vars | get -o LAGO_ENCRYPTION_KEY_DERIVATION_SALT | default "")

    # Validate all secrets are present
    if ($rsa_key | is-empty) or ($secret_key_base | is-empty) or ($enc_primary | is-empty) or ($enc_det | is-empty) or ($enc_salt | is-empty) {
        log error $"Missing Lago secrets in ($dapple_env_file)"
        log error "Required: LAGO_RSA_PRIVATE_KEY, LAGO_SECRET_KEY_BASE, LAGO_ENCRYPTION_PRIMARY_KEY, LAGO_ENCRYPTION_DETERMINISTIC_KEY, LAGO_ENCRYPTION_KEY_DERIVATION_SALT"
        error make { msg: "Missing Lago secrets" }
    }

    # Delete existing secrets if force flag is set
    if $force {
        log info "Force flag set - recreating secrets"
        do { kubectl delete secret lago-secrets -n $LAGO_NAMESPACE } | ignore
        do { kubectl delete secret lago-postgres-secret -n $LAGO_NAMESPACE } | ignore
    }

    # Create lago-secrets using kubectl
    log info "Creating lago-secrets..."
    let result = (kubectl create secret generic lago-secrets -n $LAGO_NAMESPACE
        $"--from-literal=SECRET_KEY_BASE=($secret_key_base)"
        $"--from-literal=LAGO_RSA_PRIVATE_KEY=($rsa_key)"
        $"--from-literal=ENCRYPTION_PRIMARY_KEY=($enc_primary)"
        $"--from-literal=ENCRYPTION_DETERMINISTIC_KEY=($enc_det)"
        $"--from-literal=ENCRYPTION_KEY_DERIVATION_SALT=($enc_salt)"
        "--from-literal=POSTGRES_PASSWORD=lago"
        "--from-literal=DATABASE_URL=postgresql://lago:lago@lago-postgres:5432/lago"
        "--from-literal=REDIS_URL=redis://lago-redis:6379"
        --dry-run=client -o yaml | kubectl apply -f - | complete)

    if $result.exit_code != 0 {
        log error $"Failed to create lago-secrets: ($result.stderr)"
        error make { msg: "Failed to create lago-secrets" }
    }

    # Create postgres-specific secret
    log info "Creating lago-postgres-secret..."
    let pg_result = (kubectl create secret generic lago-postgres-secret -n $LAGO_NAMESPACE
        "--from-literal=POSTGRES_PASSWORD=lago"
        --dry-run=client -o yaml | kubectl apply -f - | complete)

    if $pg_result.exit_code != 0 {
        log error $"Failed to create lago-postgres-secret: ($pg_result.stderr)"
        error make { msg: "Failed to create lago-postgres-secret" }
    }

    log info "Secrets configured successfully"
}
