#!/usr/bin/nu

use std/log
use std/assert

$env.NU_LOG_LEVEL = "debug"

use ../modules/dapple_env_profiles dapple_sandbox_tools
overlay use dapple_sandbox_tools as dapple_env

use ../modules/k8s
use k8s deploy
use k8s ensure_vm_mounts
use k8s rollout_restart

use ../modules/tools/service_token.nu [register ensure]

# Environment variables are loaded via dapple_sandbox_tools overlay
# Loading order: project .env -> ~/.env.dapple -> ~/.env (highest priority)

# Validate that web app source directories exist locally
# Returns true if all paths exist, false otherwise (with helpful error messages)
def validate_web_source_paths []: nothing -> bool {
    let paths = [
        { name: "dapple-landing-page", path: $env.DAPPLE_WEB_LANDING_PAGE_SOURCE_DIR }
        { name: "dapple-ui", path: ($env | get -o DAPPLE_UI_SOURCE_DIR | default "") }
        { name: "dapple-ui-demo", path: ($env | get -o DAPPLE_UI_DEMO_SOURCE_DIR | default "") }
    ]

    mut all_valid = true
    mut missing = []

    for p in $paths {
        if ($p.path | is-empty) {
            continue  # Optional path not configured
        }
        if not ($p.path | path exists) {
            $missing = ($missing | append $p.name)
            log error $"Source directory not found: ($p.path)"
            $all_valid = false
        }
    }

    if not $all_valid {
        log error ""
        log error "To fix this, clone the missing repos:"
        let base = if ($nu.os-info.name | str contains "mac") {
            "~/Source/Dapple"
        } else {
            "~/src/dapple"
        }
        for name in $missing {
            log error $"  git clone <repo-url> ($base)/($name)"
        }
        log error ""
        log error "Or override the path in ~/.env:"
        log error "  DAPPLE_WEB_LANDING_PAGE_SOURCE_DIR=/your/path"
        log error "  DAPPLE_UI_SOURCE_DIR=/your/path"
        log error "  DAPPLE_UI_DEMO_SOURCE_DIR=/your/path"
    }

    $all_valid
}

def main [] {
    log info "starting dapple-web deployments"

    # Validate source directories exist before attempting hostPath deployment
    if not (validate_web_source_paths) {
        return
    }

    # Ensure source directories are visible inside the MicroK8s VM (macOS only)
    ensure_vm_mounts [
        $env.DAPPLE_WEB_LANDING_PAGE_SOURCE_DIR
        ($env | get -o DAPPLE_UI_SOURCE_DIR | default "")
        ($env | get -o DAPPLE_UI_DEMO_SOURCE_DIR | default "")
    ]

    # Create/update k8s secret for dapple-ui auth config
    log info "setting up dapple-ui auth secret"
    setup_dapple_ui_secret

    log info "deploying dapple-web-landing-page"

    # Apply the registry manifest (has secret refs built-in)
    let registry_file = ($env.DAPPLE_KUBE_ASSETS_DIR | path join "registry" | path join "dapple-web-landing-page.yaml")
    if ($registry_file | path exists) {
        log info "applying k8s registry manifest for dapple-web-landing-page"

        # Replace SOURCE_PATH placeholder with actual path
        let manifest_content = (open $registry_file --raw | str replace "{{SOURCE_PATH}}" $env.DAPPLE_WEB_LANDING_PAGE_SOURCE_DIR)
        $manifest_content | kubectl apply -n $env.KUBE_DAPPLE_NAMESPACE -f -

        # Force pod recreation (kubectl apply doesn't trigger rollout if manifest unchanged)
        log info "restarting deployment to pick up changes..."
        rollout_restart { name: "dapple-web-landing-page", namespace: $env.KUBE_DAPPLE_NAMESPACE }

        # Wait for deployment to be ready
        log info "waiting for deployment to be ready..."
        kubectl rollout status deployment/dapple-web-landing-page -n $env.KUBE_DAPPLE_NAMESPACE --timeout=300s
    } else {
        log warning $"registry manifest not found at ($registry_file), falling back to template"
        deploy {
            name: "dapple-web-landing-page"
            namespace: $env.KUBE_DAPPLE_NAMESPACE
            source_path: $env.DAPPLE_WEB_LANDING_PAGE_SOURCE_DIR
            template: dapple-node-sandbox.yaml
            port: "4444"
            node_port: $env.DAPPLE_WEB_LANDING_PAGE_PORT
        } --wait
    }

    # Deploy dapple-ui if source directory is configured
    if ($env | get -o DAPPLE_UI_SOURCE_DIR | is-not-empty) {
        log info "deploying dapple-ui"
        let dapple_ui_registry = ($env.DAPPLE_KUBE_ASSETS_DIR | path join "registry" | path join "dapple-ui.yaml")
        if ($dapple_ui_registry | path exists) {
            log info "applying k8s registry manifest for dapple-ui"

            # Replace SOURCE_PATH placeholder with actual path
            let manifest_content = (open $dapple_ui_registry --raw | str replace "{{SOURCE_PATH}}" $env.DAPPLE_UI_SOURCE_DIR)
            $manifest_content | kubectl apply -n $env.KUBE_DAPPLE_NAMESPACE -f -

            # Force pod recreation (kubectl apply doesn't trigger rollout if manifest unchanged)
            log info "restarting dapple-ui to pick up changes..."
            rollout_restart { name: "dapple-ui", namespace: $env.KUBE_DAPPLE_NAMESPACE }

            # Wait for deployment to be ready
            log info "waiting for dapple-ui deployment to be ready..."
            kubectl rollout status deployment/dapple-ui -n $env.KUBE_DAPPLE_NAMESPACE --timeout=300s
        } else {
            log warning $"dapple-ui registry manifest not found at ($dapple_ui_registry)"
        }
    } else {
        log info "DAPPLE_UI_SOURCE_DIR not set, skipping dapple-ui deployment"
    }

    # Deploy dapple-ui-demo if source directory is configured.
    # Same monorepo as dapple-ui; opted in independently so operators can
    # leave the demo pod off when they don't need it.
    if ($env | get -o DAPPLE_UI_DEMO_SOURCE_DIR | is-not-empty) {
        log info "deploying dapple-ui-demo"
        let dapple_ui_demo_registry = ($env.DAPPLE_KUBE_ASSETS_DIR | path join "registry" | path join "dapple-ui-demo.yaml")
        if ($dapple_ui_demo_registry | path exists) {
            log info "applying k8s registry manifest for dapple-ui-demo"

            # Replace SOURCE_PATH placeholder with actual path
            let manifest_content = (open $dapple_ui_demo_registry --raw | str replace "{{SOURCE_PATH}}" $env.DAPPLE_UI_DEMO_SOURCE_DIR)
            $manifest_content | kubectl apply -n $env.KUBE_DAPPLE_NAMESPACE -f -

            # Force pod recreation (kubectl apply doesn't trigger rollout if manifest unchanged)
            log info "restarting dapple-ui-demo to pick up changes..."
            rollout_restart { name: "dapple-ui-demo", namespace: $env.KUBE_DAPPLE_NAMESPACE }

            # Wait for deployment to be ready
            log info "waiting for dapple-ui-demo deployment to be ready..."
            kubectl rollout status deployment/dapple-ui-demo -n $env.KUBE_DAPPLE_NAMESPACE --timeout=300s
        } else {
            log warning $"dapple-ui-demo registry manifest not found at ($dapple_ui_demo_registry)"
        }
    } else {
        log info "DAPPLE_UI_DEMO_SOURCE_DIR not set, skipping dapple-ui-demo deployment"
    }
}

# Ask Google's authorization endpoint whether a redirect URI is registered on an
# OAuth client. Returns the `location:` header line of the 302, or "" when the
# probe could not be completed [offline, proxy, timeout, no Location header].
#
# Uses ^curl rather than nu's `http get` for the same reason deploy_lago_billing.nu
# does: `http get` raises on non-2xx and follows redirects, and what we need is
# precisely the Location header of a 302. A 302 carries no body, so any
# body-based check reads as "accepted" and is wrong.
#
# --get + --data-urlencode lets curl do the percent-encoding rather than
# hand-rolling it. Nothing secret is sent: client_id and redirect_uri are both
# public values that appear in every sign-in URL.
def probe_google_redirect [client_id: string, redirect_uri: string]: nothing -> string {
    let args = [
        "-s" "-D" "-" "-o" "/dev/null" "--max-time" "10" "--get"
        "--data-urlencode" $"client_id=($client_id)"
        "--data-urlencode" $"redirect_uri=($redirect_uri)"
        "--data-urlencode" "response_type=code"
        "--data-urlencode" "scope=openid email profile"
        "https://accounts.google.com/o/oauth2/v2/auth"
    ]
    let result = (^curl ...$args | complete)
    if $result.exit_code != 0 { return "" }

    let header = (
        $result.stdout
        | lines
        | where ($it | str lowercase | str starts-with "location:")
    )
    if ($header | is-empty) { "" } else { $header | first }
}

# Best-effort decode of the base64 error blob Google puts in the `authError`
# query param of its error redirect. Distinguishes a genuine
# redirect_uri_mismatch from e.g. invalid_client, so we never tell someone to
# register a URI when the client id is the actual problem. Any failure yields ""
# and the caller falls back to a generic message.
def google_auth_error_reason [location: string]: nothing -> string {
    let matches = ($location | parse --regex 'authError=(?<v>[^&]+)')
    if ($matches | is-empty) {
        ""
    } else {
        try { $matches | first | get v | decode base64 | decode utf-8 } catch { "" }
    }
}

# Warn when an ingress host's OAuth callback is not registered on the Google
# client. Registering it is a manual Google Cloud Console step no script can
# perform, and nothing else checks it — dapple-ui-demo shipped without it and the
# gap stayed invisible for 69 days, because both pods share NEXTAUTH_COOKIE_DOMAIN
# so one sign-in on dapple-ui carries to the demo host and the demo host's OAuth
# flow never runs. It only surfaced when a session cookie got deleted.
#
# EVERY ingress hostname needs its own entry: packages/ui/auth.config.ts sets
# `trustHost: true`, so Auth.js v5 builds redirect_uri from the incoming
# forwarded host — NEXTAUTH_URL is NOT the lever, even though all hosts share one
# OAuth client.
#
# Warns, never fails: a deploy must not break because Google is unreachable.
def check_google_redirect_uris [client_id: string, base_urls: list<string>]: nothing -> nothing {
    for base_url in $base_urls {
        let redirect_uri = $"($base_url)/api/auth/callback/google"
        let location = (probe_google_redirect $client_id $redirect_uri)

        if ($location | str contains "/signin/oauth/error") {
            let reason = (google_auth_error_reason $location)
            if ($reason | str contains "redirect_uri_mismatch") {
                log warning $"OAuth redirect URI is NOT registered: ($redirect_uri)"
                log warning $"  add it to client ($client_id) in the Google Cloud Console"
                log warning "  until then, sign in at the primary host - the shared cookie carries over"
            } else {
                log warning $"Google rejected the OAuth probe for ($redirect_uri)"
                log warning $"  client ($client_id) - check the client id/secret and the Console config"
            }
        } else if ($location | str contains "/signin/identifier") {
            log debug $"OAuth redirect URI registered: ($redirect_uri)"
        } else {
            log debug $"could not verify OAuth redirect URI ($redirect_uri) [probe inconclusive]"
        }
    }
}

# Create/update k8s secret for dapple-ui authentication configuration
def setup_dapple_ui_secret [] {
    let secret_name = "dapple-ui-auth"
    let namespace = $env.KUBE_DAPPLE_NAMESPACE

    # ~/.env.dapple should already be generated by deploy_dapple_prompt_pods.nu
    # Read NEXTAUTH_SECRET from environment (loaded via profile or ~/.env)
    let nextauth_secret = ($env | get -o NEXTAUTH_SECRET | default "")
    if ($nextauth_secret | is-empty) {
        log warning "NEXTAUTH_SECRET not set - run deploy_dapple_prompt_pods.nu first to generate ~/.env.dapple"
    }

    let nextauth_url = ($env | get -o NEXTAUTH_URL | default "https://dapple-ui.kube.dappleai.dev")
    let internal_api = ($env | get -o DAPPLE_PROMPT_API_INTERNAL_URL | default "http://dapple-prompt-api:8001")

    # Use service-specific env var (SAT = Service Authorization Token)
    let sat_signing_key = ($env | get -o DAPPLE_UI_SAT_SIGNING_KEY | default "")
    let service_id = "dapple_ui"  # Hardcoded - no longer configurable

    let posthog_key = ($env | get -o NEXT_PUBLIC_POSTHOG_KEY | default "")
    let posthog_host = ($env | get -o NEXT_PUBLIC_POSTHOG_HOST | default "https://us.i.posthog.com")

    # OAuth credentials for NextAuth Google provider
    let oauth_google_client_id = ($env | get -o OAUTH_GOOGLE_CLIENT_ID | default "")
    let oauth_google_client_secret = ($env | get -o OAUTH_GOOGLE_CLIENT_SECRET | default "")

    # Database connection for NextAuth adapter.
    # dapple-ui's packages/ui/lib/db/pool.ts reads broken-out DATABASE_* vars
    # (not a single URL), so we write them in that shape.
    let database_host = "prompt-db"
    let database_port = "5432"
    let database_user = "dapple"
    let database_password = "ai"
    let database_name = "dapple"
    # auth schema is searched first because NextAuth tables live there
    let database_search_path = "auth,public"

    # Check required secrets
    if ($sat_signing_key | is-empty) {
        log warning "DAPPLE_UI_SAT_SIGNING_KEY not set - run deploy_dapple_prompt_pods.nu first to generate keys"
    } else {
        # Idempotent upsert so a DB rebuild without a fresh ensure_service_tokens
        # run does not leave the UI minting SATs that dapple-prompt-api rejects.
        log info $"registering ($service_id) service token in database [idempotent]"
        register $service_id --private-key $sat_signing_key --name "Dapple UI"
    }

    # dapple-ui-demo: auto-generate + register the SAT key on first run.
    # `ensure` writes the new key to ~/.env.dapple, upserts the public key into
    # web.service_tokens, and returns the private key (existing or fresh).
    log info "ensuring dapple_ui_demo service token [auto-generate + register if missing]"
    let demo_signing_key = (ensure "dapple_ui_demo" --env-var "DAPPLE_UI_DEMO_SAT_SIGNING_KEY" --name "Dapple UI Demo")
    let demo_service_id = "dapple_ui_demo"
    let demo_nextauth_url = "https://dapple-ui-demo.kube.dappleai.dev"
    # Shared cookie domain so a session minted on dapple-ui validates on
    # dapple-ui-demo (both pods read this same key via secretKeyRef).
    let nextauth_cookie_domain = ".kube.dappleai.dev"
    if ($oauth_google_client_id | is-empty) or ($oauth_google_client_secret | is-empty) {
        log warning "OAUTH_GOOGLE_CLIENT_ID or OAUTH_GOOGLE_CLIENT_SECRET not set - add to ~/.dapple/env"
    } else {
        # Every ingress host that serves a sign-in page needs its own registered
        # redirect URI - see check_google_redirect_uris for why NEXTAUTH_URL does
        # not control this.
        check_google_redirect_uris $oauth_google_client_id [$nextauth_url $demo_nextauth_url]
    }

    # Delete existing secret if present (ignore errors if it doesn't exist)
    log debug $"removing existing secret ($secret_name) if present"
    do { kubectl delete secret $secret_name -n $namespace } | ignore

    # Create the secret with all env vars
    log debug $"creating secret ($secret_name) in namespace ($namespace)"
    (kubectl create secret generic $secret_name -n $namespace
        $"--from-literal=NEXTAUTH_SECRET=($nextauth_secret)"
        $"--from-literal=NEXTAUTH_URL=($nextauth_url)"
        $"--from-literal=DAPPLE_PROMPT_API_INTERNAL_URL=($internal_api)"
        $"--from-literal=DAPPLE_UI_SAT_SIGNING_KEY=($sat_signing_key)"
        $"--from-literal=DAPPLE_UI_SERVICE_ID=($service_id)"
        $"--from-literal=NEXT_PUBLIC_POSTHOG_KEY=($posthog_key)"
        $"--from-literal=NEXT_PUBLIC_POSTHOG_HOST=($posthog_host)"
        $"--from-literal=OAUTH_GOOGLE_CLIENT_ID=($oauth_google_client_id)"
        $"--from-literal=OAUTH_GOOGLE_CLIENT_SECRET=($oauth_google_client_secret)"
        $"--from-literal=DATABASE_HOST=($database_host)"
        $"--from-literal=DATABASE_PORT=($database_port)"
        $"--from-literal=DATABASE_USER=($database_user)"
        $"--from-literal=DATABASE_PASSWORD=($database_password)"
        $"--from-literal=DATABASE_NAME=($database_name)"
        $"--from-literal=DATABASE_SEARCH_PATH=($database_search_path)"
        $"--from-literal=DAPPLE_UI_DEMO_SAT_SIGNING_KEY=($demo_signing_key)"
        $"--from-literal=DAPPLE_UI_DEMO_SERVICE_ID=($demo_service_id)"
        $"--from-literal=NEXTAUTH_URL_DAPPLE_UI_DEMO=($demo_nextauth_url)"
        $"--from-literal=NEXTAUTH_COOKIE_DOMAIN=($nextauth_cookie_domain)")

    log info $"dapple-ui secret ($secret_name) created successfully"
}
