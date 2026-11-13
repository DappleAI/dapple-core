#!/usr/bin/nu
# Flagsmith Feature Flag Platform Deployment Script
#
# Deploys Flagsmith to a dedicated 'flagsmith' namespace with all dependencies.
# Uses manifests from assets/k8s/registry/flagsmith/
#
# Usage:
#   nu assets/nu/k8s/setup_flagsmith.nu           # Full deployment
#   nu assets/nu/k8s/setup_flagsmith.nu --force   # Regenerate secrets and redeploy

use std/log

# Load dapple environment profile (provides kubectl, environment vars, etc.)
use ../modules/dapple_env_profiles dapple_sandbox_tools
overlay use dapple_sandbox_tools as dapple_env

# The generated-secrets file moved under ~/.dapple/; env_files.nu owns the path.
use ../modules/dapple_env_profiles/env_files.nu [generated_env_path read_pairs]

# Main entry point for Flagsmith deployment
def main [
    --force  # Regenerate secrets and redeploy everything
] {
    let flagsmith_dir = $env.DAPPLE_PROMPT_WORKSPACE_DIR | path join "assets" "k8s" "registry" "flagsmith"

    log info "Starting Flagsmith feature flag platform deployment"

    # Phase 1: Namespace
    log info "Phase 1: Creating namespace"
    kubectl apply -f ($flagsmith_dir | path join "namespace.yaml")

    # Phase 2: Secrets
    log info "Phase 2: Setting up secrets"
    setup_flagsmith_secrets $force

    # Phase 3: ConfigMap
    log info "Phase 3: Applying ConfigMap"
    kubectl apply -f ($flagsmith_dir | path join "configmap.yaml")

    # Phase 4: Data stores (postgres, redis)
    log info "Phase 4: Deploying data stores"
    kubectl apply -f ($flagsmith_dir | path join "postgres.yaml")
    kubectl apply -f ($flagsmith_dir | path join "redis.yaml")

    log info "Waiting for postgres to be ready..."
    wait_for_deployment "flagsmith-postgres" "flagsmith" "180s"

    log info "Waiting for redis to be ready..."
    wait_for_deployment "flagsmith-redis" "flagsmith" "60s"

    # Phase 5: API + Frontend (unified container)
    log info "Phase 5: Deploying API service"
    kubectl apply -f ($flagsmith_dir | path join "api.yaml")

    log info "Waiting for API to be ready..."
    wait_for_deployment "flagsmith-api" "flagsmith" "120s"

    # Phase 6: Task Processor
    log info "Phase 6: Deploying task processor"
    kubectl apply -f ($flagsmith_dir | path join "task-processor.yaml")

    log info "Waiting for task processor to be ready..."
    wait_for_deployment "flagsmith-task-processor" "flagsmith" "120s"

    # Phase 7: Ingress
    log info "Phase 7: Configuring ingress"
    kubectl apply -f ($flagsmith_dir | path join "ingress.yaml")

    log info "Flagsmith deployment complete!"
    log info ""
    log info "Access Flagsmith at:"
    log info "  Frontend: https://flagsmith.kube.dappleai.dev"
    log info "  API: https://flagsmith-api.kube.dappleai.dev"
    log info ""
    log info "Check status with: dapple flagsmith status"
}

# Wait for a deployment to be ready
def wait_for_deployment [name: string, namespace: string, timeout: string] {
    let result = (kubectl rollout status $"deployment/($name)" -n $namespace --timeout $timeout | complete)

    if $result.exit_code != 0 {
        log error $"Deployment ($name) failed to become ready: ($result.stderr)"
        error make { msg: $"Deployment ($name) not ready after ($timeout)" }
    }

    log info $"($name) is ready"
}

# Setup Flagsmith secrets (generate if missing, apply to cluster)
def setup_flagsmith_secrets [force: bool] {
    let namespace = "flagsmith"
    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    let dapple_env_file = (generated_env_path $home)

    # Regenerate the secrets file to ensure Flagsmith secrets exist
    use ../modules/tools/env_dapple.nu
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
        do { kubectl delete secret flagsmith-secrets -n $namespace } | ignore
        do { kubectl delete secret flagsmith-postgres-secret -n $namespace } | ignore
    }

    # Create flagsmith-secrets using kubectl
    log info "Creating flagsmith-secrets..."
    let result = (kubectl create secret generic flagsmith-secrets -n $namespace
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
    let pg_result = (kubectl create secret generic flagsmith-postgres-secret -n $namespace
        "--from-literal=POSTGRES_PASSWORD=flagsmith"
        --dry-run=client -o yaml | kubectl apply -f - | complete)

    if $pg_result.exit_code != 0 {
        log error $"Failed to create flagsmith-postgres-secret: ($pg_result.stderr)"
        error make { msg: "Failed to create flagsmith-postgres-secret" }
    }

    log info "Secrets configured successfully"
}
