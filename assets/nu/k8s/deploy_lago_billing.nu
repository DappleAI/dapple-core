#!/usr/bin/nu

# Deploy Lago billing integration for dapple-prompt.
#
# Sets up everything needed for billing: Lago test org, K8s secrets,
# lago-worker binary, and the Lago billable metric.
#
# Prerequisites:
#   - Lago platform deployed: `dapple lago setup`
#   - dapple-prompt pods deployed: `nu assets/nu/k8s/deploy_dapple_prompt_pods.nu`
#
# Usage:
#   nu assets/nu/k8s/deploy_lago_billing.nu             # Full setup
#   nu assets/nu/k8s/deploy_lago_billing.nu --svc worker # Deploy only the worker
#   nu assets/nu/k8s/deploy_lago_billing.nu --test-org   # Create test org only
#   nu assets/nu/k8s/deploy_lago_billing.nu --force      # Recreate secrets + redeploy

use std/log
use std/assert

$env.NU_LOG_LEVEL = "debug"

use ../modules/dapple_env_profiles dapple_sandbox_tools
overlay use dapple_sandbox_tools as dapple_env

use ../modules/k8s
use k8s deploy
use k8s rollout_status
use k8s rollout_restart
use k8s setup_docker_registry_cert
use k8s ensure_vm_mounts

use ../modules/tools/env_dapple.nu

# env_dapple writes ~/.dapple/generated.env now; env_files.nu owns that path.
use ../modules/dapple_env_profiles/env_files.nu [generated_env_path read_pairs]

const LAGO_NAMESPACE = "lago"

# Lago API URL for internal K8s access (within dapple-sandbox pods → lago namespace)
const LAGO_INTERNAL_URL = "http://lago-api.lago:3000"

# Lago API URL for external access (from host machine / scripts)
const LAGO_EXTERNAL_URL = "https://lago-api.kube.dappleai.dev"

def main [
    --force (-f)             # Recreate secrets and redeploy
    --docker: string = ""    # Build Docker image instead of hostPath. Value is cargo profile (e.g., --docker dev)
    --svc: string = ""       # Deploy only these steps, comma-separated (e.g., --svc worker)
    --test-org               # Create test org only, skip deployment
    --allow-dirty            # Allow Docker builds from a dirty working tree (e.g. when deploying from a worktree). Image will be tagged with the committed content_hash but contain uncommitted code, so subsequent clean deploys with the same HEAD will reuse it — commit and re-tag once validated.
] {
    let svc_list = if ($svc | is-empty) { [] } else { $svc | split row "," | each { str trim } }
    let deploy_all = ($svc_list | is-empty) and (not $test_org)

    # macOS auto-promote to Docker mode
    mut use_docker = ($docker | is-not-empty)
    mut docker_profile = $docker
    if (not $use_docker) and ($nu.os-info.name | str contains "mac") {
        log warning "macOS detected — switching to Docker mode"
        $use_docker = true
        $docker_profile = "dev"
    }

    let hostpath_profile = if $use_docker { "" } else { "debug" }

    let should_deploy = {|name: string|
        $deploy_all or ($name in $svc_list)
    }

    log info "Starting Lago billing integration setup"

    # =========================================================================
    # Phase 1: Verify Lago platform is running
    # =========================================================================
    if $deploy_all or $test_org {
        log info "Phase 1: Verifying Lago platform is running"
        verify_lago_running
    }

    # =========================================================================
    # Phase 2: Load/refresh environment secrets
    # =========================================================================
    if $deploy_all or $test_org {
        log info "Phase 2: Loading environment secrets"
        load_env_secrets
    }

    # =========================================================================
    # Phase 3: Ensure primary Lago API key exists
    # =========================================================================
    if $deploy_all {
        log info "Phase 3: Ensuring Lago API key"
        ensure_lago_api_key $force
    }

    # =========================================================================
    # Phase 4: Create test organization (isolated from manual testing)
    # =========================================================================
    if $deploy_all or $test_org or ("test-org" in $svc_list) {
        log info "Phase 4: Setting up test organization"
        setup_test_org $force
    }

    if $test_org {
        log info "Test org setup complete. Skipping deployment."
        return
    }

    # =========================================================================
    # Phase 5: Create K8s secrets for billing services
    # =========================================================================
    if $deploy_all or (do $should_deploy "worker") {
        log info "Phase 5: Setting up billing secrets in dapple-sandbox"
        setup_lago_billing_secret $force
    }

    # =========================================================================
    # Phase 6: Create billable metric in Lago
    # =========================================================================
    if $deploy_all {
        log info "Phase 6: Ensuring Lago billable metrics"
        ensure_billable_metrics
    }

    # =========================================================================
    # Phase 7: Deploy lago-worker
    # =========================================================================
    if (do $should_deploy "worker") {
        if not $use_docker {
            ensure_vm_mounts [$env.DAPPLE_PROMPT_WORKSPACE_DIR]
        }

        log info "Phase 7: Deploying lago-worker"
        setup_lago_worker {
            name: "dapple-prompt-lago-worker"
            profile: (if $use_docker { $docker_profile } else { "dev" })
            namespace: $env.KUBE_DAPPLE_NAMESPACE
            source_path: $env.DAPPLE_PROMPT_WORKSPACE_DIR
            template: dapple-prompt-engine.yaml
        } --force=$force --hostpath=$hostpath_profile --docker=$use_docker --allow-dirty=$allow_dirty
    }

    log info "Lago billing integration setup complete!"
    print ""
    print "Services:"
    print $"  Lago UI:     https://lago.kube.dappleai.dev"
    print $"  Lago API:    ($LAGO_EXTERNAL_URL)"
    print $"  Worker:      dapple-prompt-lago-worker in ($env.KUBE_DAPPLE_NAMESPACE)"
    print ""
    print "Verify with:"
    print "  kubectl get pods -n dapple-sandbox -l app=dapple-prompt-lago-worker"
    print "  dapple lago status"
}

# Verify Lago API is reachable.
#
# Uses curl rather than nu's `http get` because the latter auto-parses
# the response body by content-type. lago-api returns 401-with-body to
# unauthenticated requests, and the body shape can trigger nu's "Invalid
# data" I/O error that escapes the do{} block before our exit-code check
# ever runs — which then aborts the whole script before reaching Phase 5
# (where the lago-api secret in dapple-sandbox gets created).
#
# curl with `-o /dev/null -w '%{http_code}'` just hands us the status
# code as plain text. We expect 401 (auth required = API alive) and only
# fail on 000 (network unreachable). Side benefit: clearer failure logs
# that include the actual code instead of nu's generic "invalid data".
def verify_lago_running [] {
    let result = (do {
        ^curl -sk -o /dev/null -w "%{http_code}" --max-time 10 $"($LAGO_EXTERNAL_URL)/api/v1/organizations"
    } | complete)

    let code = ($result.stdout | str trim)

    if $result.exit_code != 0 or $code == "000" or ($code | is-empty) {
        log error $"Lago API is not reachable at ($LAGO_EXTERNAL_URL) \(curl exit ($result.exit_code), code '($code)')"
        log error "Deploy Lago first: dapple lago setup"
        error make { msg: "Lago platform not running" }
    }

    log info $"Lago API is reachable \(HTTP ($code))"
}

# Load the generated secrets file into the current environment
def load_env_secrets [] {
    env_dapple gen

    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    let dapple_env_file = (generated_env_path $home)

    if ($dapple_env_file | path exists) {
        # read_pairs mirrors load-dotenv, so what lands in $env here matches
        # what a fresh shell would have exported.
        load-env (read_pairs $dapple_env_file)
    }
}

# Ensure LAGO_API_KEY is set (prompt user to get it from Lago UI if not)
def ensure_lago_api_key [force: bool] {
    let api_key = ($env | get -o LAGO_API_KEY | default "")

    if ($api_key | is-not-empty) and (not $force) {
        # Hoist the prefix to a let-binding because nu's parser treats
        # `(...)...` inside string interpolation as cell-path access on
        # the result. With `str substring 0..8` (or `0..<8`) inline,
        # the trailing `...` ellipsis triggers either "can't convert
        # string to int" or "string doesn't support cell paths"
        # depending on nu version. Keeping the substring out of the
        # interpolation sidesteps both.
        let prefix = ($api_key | str substring 0..<8)
        log info $"LAGO_API_KEY is set \(($prefix)...)"
        return
    }

    log warning "LAGO_API_KEY is not set in your environment"
    print ""
    print "To get your API key:"
    print "  1. Open https://lago.kube.dappleai.dev"
    print "  2. Log in (or sign up if first time)"
    print "  3. Go to Developers → API Keys"
    print "  4. Copy your API key"
    print "  5. Add to ~/.env.dapple:  LAGO_API_KEY=<your-key>"
    print "  6. Re-run this script"
    print ""
    error make { msg: "LAGO_API_KEY required — see instructions above" }
}

# Create the isolated test organization
def setup_test_org [force: bool] {
    let existing_key = ($env | get -o LAGO_TEST_API_KEY | default "")

    if ($existing_key | is-not-empty) and (not $force) {
        # See ensure_lago_api_key for why this is hoisted.
        let prefix = ($existing_key | str substring 0..<8)
        log info $"Test org already configured. LAGO_TEST_API_KEY=($prefix)..."
        return
    }

    log info "Creating Lago test organization..."

    let signup_body = {
        user: {
            email: "test@dappleai.dev"
            password: "dapple-test-org-2026!"
            organization_name: "dapple-test"
        }
    }

    # Try signup first.
    #
    # Use curl rather than nu's `http post`/`get` for the same reason
    # verify_lago_running does: nu auto-parses the response body by
    # content-type and lago-api's auth/org responses (especially on
    # 422-validation-error or non-JSON error pages from ingress) can
    # surface as "Invalid data" — an I/O error that escapes the do{}
    # block and aborts the script. curl just hands us the raw body as
    # text and we explicitly `from json` it.
    let signup_result = (do {
        ^curl -sk --max-time 10 -X POST -H "Content-Type: application/json" -d ($signup_body | to json) $"($LAGO_EXTERNAL_URL)/auth/sign_up"
    } | complete)

    mut token = ""

    if $signup_result.exit_code == 0 and ($signup_result.stdout | str trim | str starts-with "{") {
        let response = ($signup_result.stdout | from json)
        $token = ($response | get token)
        log info $"Created test org: dapple-test"
    } else {
        # User may already exist — try login
        log info "Signup returned error (user may exist), trying login..."

        let login_body = {
            user: {
                email: "test@dappleai.dev"
                password: "dapple-test-org-2026!"
            }
        }

        let login_result = (do {
            ^curl -sk --max-time 10 -X POST -H "Content-Type: application/json" -d ($login_body | to json) $"($LAGO_EXTERNAL_URL)/auth/sign_in"
        } | complete)

        if $login_result.exit_code != 0 or not ($login_result.stdout | str trim | str starts-with "{") {
            log error $"Lago login failed: ($login_result.stdout | str trim)"
            error make { msg: "Lago test-org auth failed (could not signup or login)" }
        }

        let response = ($login_result.stdout | from json)
        $token = ($response | get token)
        log info "Logged in to existing test user"
    }

    # Fetch org API key using bearer token.
    # Snapshot $token (mut) to a let binding because nu closures —
    # including the do{} block below — can't capture mutable vars.
    let bearer_token = $token
    let org_result = (do {
        ^curl -sk --max-time 10 -H $"Authorization: Bearer ($bearer_token)" $"($LAGO_EXTERNAL_URL)/api/v1/organizations"
    } | complete)
    let org = ($org_result.stdout | from json)
    let api_key = ($org | get organization.api_key)

    # Save to the generated secrets file
    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    let env_file = (generated_env_path $home)

    if ($env_file | path exists) {
        let content = (open $env_file --raw)
        if ($content | str contains "LAGO_TEST_API_KEY=") {
            let updated = ($content | lines
                | each {|l| if ($l starts-with "LAGO_TEST_API_KEY=") { $"LAGO_TEST_API_KEY=($api_key)" } else { $l } }
                | str join "\n")
            $updated | save -f $env_file
        } else {
            $"\nLAGO_TEST_API_KEY=($api_key)\n" | save --append $env_file
        }
    } else {
        $"LAGO_TEST_API_KEY=($api_key)\n" | save $env_file
    }

    # Also load into current env
    load-env { LAGO_TEST_API_KEY: $api_key }

    # See ensure_lago_api_key for why this is hoisted.
    let saved_prefix = ($api_key | str substring 0..<8)
    log info $"Test org API key saved: ($saved_prefix)..."
    print "Integration tests will use this key to isolate test data from your primary org."
}

# Create K8s secret with Lago API keys in the dapple-sandbox namespace
# The lago-worker and flow service read from this secret.
def setup_lago_billing_secret [force: bool] {
    let secret_name = "lago-api"
    let namespace = $env.KUBE_DAPPLE_NAMESPACE

    let api_key = ($env | get -o LAGO_API_KEY | default "")
    let test_api_key = ($env | get -o LAGO_TEST_API_KEY | default "")

    if ($api_key | is-empty) {
        log warning "LAGO_API_KEY not set — lago-worker will not be able to sync events"
        log warning "Get your API key from the Lago UI and add to ~/.env.dapple"
        return
    }

    mut literal_args = [
        $"--from-literal=LAGO_API_KEY=($api_key)"
        $"--from-literal=LAGO_API_URL=($LAGO_INTERNAL_URL)"
    ]

    if ($test_api_key | is-not-empty) {
        $literal_args = ($literal_args | append $"--from-literal=LAGO_TEST_API_KEY=($test_api_key)")
        log info "  LAGO_TEST_API_KEY: set"
    }

    log info $"Applying secret ($secret_name) in ($namespace)"
    kubectl create secret generic $secret_name -n $namespace ...$literal_args --dry-run=client -o yaml | kubectl apply -f -

    log info $"Billing secret ($secret_name) applied"
}

# Ensure the llm_tokens billable metric exists in Lago
def ensure_billable_metrics [] {
    let api_key = ($env | get -o LAGO_API_KEY | default "")
    if ($api_key | is-empty) {
        log warning "Skipping metric creation — no LAGO_API_KEY"
        return
    }

    let metrics = [
        {
            code: "llm_tokens"
            name: "LLM Token Usage"
            description: "Total LLM tokens consumed (input + output)"
            aggregation_type: "sum_agg"
            field_name: "tokens"
            recurring: false
        }
        {
            code: "asset_storage"
            name: "Asset Storage"
            description: "Asset storage bytes"
            aggregation_type: "sum_agg"
            field_name: "size_bytes"
            recurring: false
        }
    ]

    for metric in $metrics {
        let body = { billable_metric: $metric }

        # Use curl for the same reason the auth POSTs do — nu's
        # http auto-parsing chokes on lago-api error responses.
        # We capture the HTTP code via -w and the body to stdout to
        # distinguish "metric created" (201) from "already exists" (422)
        # without depending on stderr substring matching.
        let result = (do {
            ^curl -sk --max-time 10 -w "\n%{http_code}" -X POST -H "Content-Type: application/json" -H $"Authorization: Bearer ($api_key)" -d ($body | to json) $"($LAGO_EXTERNAL_URL)/api/v1/billable_metrics"
        } | complete)

        # curl writes "<body>\n<status>" to stdout via -w. Last line is the status.
        let lines = ($result.stdout | lines)
        let http_code = ($lines | last | default "")

        if $http_code == "201" or $http_code == "200" {
            log info $"Billable metric '($metric.code)' created"
        } else if $http_code == "422" {
            log info $"Billable metric '($metric.code)' already exists"
        } else {
            log warning $"Failed to create metric '($metric.code)': HTTP ($http_code) — ($result.stdout | str trim)"
        }
    }
}

# Deploy the lago-worker service
def setup_lago_worker [cfg: record, --force, --hostpath: string = "", --docker, --allow-dirty] {
    if $docker {
        build_and_push_worker_image $cfg $force --allow-dirty=$allow_dirty
    } else {
        log info $"hostPath mode for ($cfg.name) — using local binary from target/"
    }

    let registry_file = $"assets/k8s/registry/($cfg.name).yaml"
    if ($registry_file | path exists) {
        log info $"Applying registry manifest for ($cfg.name)"
        kubectl apply -n $cfg.namespace -f $registry_file

        # Apply hostPath patch in dev mode
        if ($hostpath | is-not-empty) {
            apply_worker_hostpath_patch $cfg $hostpath
        }

        # Restart to pick up changes
        log info $"Restarting ($cfg.name)"
        rollout_restart { name: $cfg.name, namespace: $cfg.namespace }

        # Wait for rollout (engine template has no health probes, so rollout is availability-based)
        let rollout_result = (
            kubectl rollout status $"deployment/($cfg.name)" -n $cfg.namespace --timeout "90s"
            | complete
        )
        if $rollout_result.exit_code != 0 {
            log error $"Rollout failed for ($cfg.name): ($rollout_result.stderr)"
            error make { msg: $"Rollout timed out for ($cfg.name)" }
        }

        log info $"($cfg.name) deployed successfully"
    } else {
        log info $"No registry manifest for ($cfg.name), using template"
        deploy $cfg --wait
    }
}

# Apply hostPath patch for lago-worker (same pattern as prompt pods).
#
# Note: this code path only runs on Linux. The script auto-promotes Mac to
# Docker mode at the top of `main`, so cross-compile is not attempted here —
# `target_dir` is just the cargo profile directory. If lago-worker ever needs
# Mac hostPath support, mirror the cross-compile flow from
# deploy_dapple_prompt_pods.nu's apply_hostpath_patch.
def apply_worker_hostpath_patch [cfg: record, hostpath_profile: string] {
    let profile = if ($hostpath_profile | is-empty) or ($hostpath_profile == "true") {
        "debug"
    } else {
        $hostpath_profile
    }

    let run_as_user = (id -u | str trim)
    let run_as_group = (id -g | str trim)

    log info $"Applying hostPath patch for ($cfg.name) with profile ($profile)"

    let patch = (k8s render "dapple-prompt-hostpath.yaml" {
        name: $cfg.name,
        namespace: $cfg.namespace,
        source_path: $cfg.source_path,
        target_dir: $profile,
        run_as_user: $run_as_user,
        run_as_group: $run_as_group
    })

    kubectl patch deployment $cfg.name -n $cfg.namespace --type=strategic --patch $patch
}

# Build and push Docker image for lago-worker
def build_and_push_worker_image [cfg: record, force: bool, --allow-dirty] {
    # Hash inputs mirror the Dockerfile build context so cross-crate edits
    # (e.g. dapple-data changes affecting dapple-prompt-lago-worker) actually
    # change the hash. See deploy_dapple_prompt_pods.nu's build_and_push_image
    # and assets/nu/k8s/build_image.nu for the canonical comment block.
    let hash_inputs = ["crates/" "Cargo.toml" "Cargo.lock" "Dockerfile" ".dockerignore" ".sqlx/"]
    let content_hash = (
        git ls-tree -r HEAD -- ...$hash_inputs
        | ^sha256sum
        | split row " "
        | first
        | str substring 0..12
    )

    let commit_tag = $"($env.KUBE_REGISTRY_HOST)/($cfg.name):($content_hash)"
    let latest_tag = $"($env.KUBE_REGISTRY_HOST)/($cfg.name):($env.DOCKER_DAPPLE_TAG_LATEST)"

    let dirty_paths = (git status --porcelain ...$hash_inputs | str trim)
    let is_dirty = ($dirty_paths | is-not-empty)
    if $is_dirty and (not $allow_dirty) {
        log error $"Workspace has uncommitted changes affecting ($cfg.name) — refusing Docker build"
        log error "Dirty paths:"
        for line in ($dirty_paths | lines) {
            log error $"  ($line)"
        }
        log error "Commit, stash, or pass --allow-dirty to override."
        error make { msg: $"Dirty tree: cannot Docker-build ($cfg.name). Commit first or pass --allow-dirty." }
    }
    if $is_dirty and $allow_dirty {
        log warning $"Workspace has uncommitted changes — building anyway because --allow-dirty was passed"
    }

    let should_skip = if $force { false } else {
        (do { ^docker manifest inspect $commit_tag } | complete).exit_code == 0
    }

    if $should_skip {
        log info $"Image ($commit_tag) already exists, skipping build"
        ^docker pull $commit_tag
        ^docker tag $commit_tag $latest_tag
        ^docker push $latest_tag
    } else {
        setup_docker_registry_cert $env.KUBE_REGISTRY_HOST $env.KUBE_ROOT_CA_CERT_NAME

        let build_args = [
            --network=host
            --build-arg $"DAPPLE_PROMPT_DATABASE_URL=($env.DAPPLE_PROMPT_INTERNAL_POSTGRES_URL)"
            --build-arg $"BIN=($cfg.name)"
            --build-arg $"PROFILE=($cfg.profile)"
            --tag $commit_tag
            --tag $latest_tag
            .
        ]

        do { ^docker rmi $commit_tag } | ignore
        do { ^docker rmi $latest_tag } | ignore

        log debug $"Building Docker image ($commit_tag)"
        ^docker build ...$build_args

        ^docker push $commit_tag
        ^docker push $latest_tag
    }
}
