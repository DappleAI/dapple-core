# Environment variable export for local web development
#
# Generates .env.local files for dapple-ui and dapple-landing-page
# so Mac developers can run locally with the same env vars as k8s pods
#
# Examples:
#   dapple env ui                    # Print env vars for dapple-ui
#   dapple env ui --write            # Write to ~/Source/Dapple/dapple-ui/packages/ui/.env.local
#   dapple env landing --write       # Write for landing page

use std/log

# The generated-secrets file moved under ~/.dapple/; env_files.nu owns the path.
use ../dapple_env_profiles/env_files.nu [generated_env_path read_pairs]

# dapple-ui appends its own /api, so the .env.local value must not carry one;
# the table module owns that conversion.
use ../dapple_env_profiles/environments.nu [api_base]

# Get the home directory
def get-home []: nothing -> string {
    if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
}

# Load the machine-written secrets file as a record.
#
# Parsing is delegated to env_files.nu's read_pairs, which mirrors load-dotenv
# exactly — so the .env.local this writes matches what the shell would export
# rather than a near-miss of it. The local copy this replaced kept its own
# slightly different parser [no quote stripping] and read the pre-move path.
def load-env-dapple []: nothing -> record {
    let generated = (generated_env_path (get-home))

    if not ($generated | path exists) {
        log warning $"($generated) not found. Run: nu assets/nu/init/setup.nu --environment sandbox"
        return {}
    }

    read_pairs $generated
}

# Generate environment variables for dapple-ui local development
export def ui [
    --write (-w)     # Write to .env.local in dapple-ui directory
    --path: string = ""  # Custom path for dapple-ui (default: ~/Source/Dapple/dapple-ui/packages/ui)
] {
    let secrets = (load-env-dapple)
    let home = (get-home)

    # Determine target directory
    # dapple-ui is a monorepo - Next.js app is in packages/ui/
    let ui_dir = if ($path | is-not-empty) {
        $path
    } else {
        match $nu.os-info.name {
            "macos" => ($home | path join "Source" | path join "Dapple" | path join "dapple-ui" | path join "packages" | path join "ui")
            _ => ($home | path join "src" | path join "dapple" | path join "dapple-ui" | path join "packages" | path join "ui")
        }
    }

    # Build env vars - use external URLs for local dev (not internal k8s URLs)
    let env_vars = {
        # Node environment
        NODE_ENV: "development"
        PORT: "4321"

        # Database (broken-out form consumed by packages/ui/lib/db/pool.ts).
        # For local dev we point at the external hostname; the cluster pod
        # version (see deploy_dapple_web_pods.nu) targets prompt-db directly.
        DATABASE_HOST: "prompt-db.kube.dappleai.dev"
        DATABASE_PORT: "5432"
        DATABASE_USER: "dapple"
        DATABASE_PASSWORD: "ai"
        DATABASE_NAME: "dapple"
        DATABASE_SEARCH_PATH: "auth,web,rev,public"

        # NextAuth
        NEXTAUTH_SECRET: ($secrets | get -o NEXTAUTH_SECRET | default "dev-secret-change-me")
        NEXTAUTH_URL: "http://localhost:4321"

        # API connection — the INGRESS url, because this runs on the developer's
        # machine, not in a pod. Trimmed of /api: internalApiClient.ts appends
        # its own path [`${baseUrl}/api/get-service-token`], so leaving the
        # suffix on produced .../api/api/... and a 404 on every server-side call.
        DAPPLE_PROMPT_API_INTERNAL_URL: (api_base ($env | get -o DAPPLE_PROMPT_API_URL | default "https://dapple-prompt-api.kube.dappleai.dev/api"))

        # Service auth
        DAPPLE_UI_SAT_SIGNING_KEY: ($secrets | get -o DAPPLE_UI_SAT_SIGNING_KEY | default "")
        DAPPLE_UI_SERVICE_ID: ($secrets | get -o DAPPLE_UI_SERVICE_ID | default "dapple_ui")

        # PostHog (optional, empty is fine)
        NEXT_PUBLIC_POSTHOG_KEY: ($secrets | get -o NEXT_PUBLIC_POSTHOG_KEY | default "")
        NEXT_PUBLIC_POSTHOG_HOST: ($secrets | get -o NEXT_PUBLIC_POSTHOG_HOST | default "")

        # OAuth (optional for local dev)
        OAUTH_GOOGLE_CLIENT_ID: ($secrets | get -o OAUTH_GOOGLE_CLIENT_ID | default "")
        OAUTH_GOOGLE_CLIENT_SECRET: ($secrets | get -o OAUTH_GOOGLE_CLIENT_SECRET | default "")
    }

    # Format as .env file
    let env_content = (format-env-file $env_vars "dapple-ui")

    if $write {
        let target_file = ($ui_dir | path join ".env.local")
        if not ($ui_dir | path exists) {
            log error $"Directory not found: ($ui_dir)"
            log error "Specify path with --path or clone dapple-ui to the expected location"
            return
        }
        $env_content | save -f $target_file
        log info $"Wrote ($target_file)"
        print $"
Environment file written to: ($target_file)

Next steps:
  cd ($ui_dir)
  pnpm install
  pnpm dev

The app will connect to:
  - API: ($env_vars.DAPPLE_PROMPT_API_INTERNAL_URL)
  - Database: ($env_vars.DATABASE_USER)@($env_vars.DATABASE_HOST):($env_vars.DATABASE_PORT)/($env_vars.DATABASE_NAME)
"
    } else {
        print $env_content
    }
}

# Generate environment variables for dapple-landing-page local development
export def landing [
    --write (-w)     # Write to .env.local in landing page directory
    --path: string = ""  # Custom path (default: ~/Source/Dapple/dapple-landing-page)
] {
    let secrets = (load-env-dapple)
    let home = (get-home)

    # Determine target directory
    let landing_dir = if ($path | is-not-empty) {
        $path
    } else {
        match $nu.os-info.name {
            "macos" => ($home | path join "Source" | path join "Dapple" | path join "dapple-landing-page")
            _ => ($home | path join "src" | path join "dapple" | path join "dapple-landing-page")
        }
    }

    # Build env vars - landing page has fewer requirements
    let env_vars = {
        # Node environment
        NODE_ENV: "development"
        PORT: "4444"

        # NextAuth (shared with UI)
        NEXTAUTH_SECRET: ($secrets | get -o NEXTAUTH_SECRET | default "dev-secret-change-me")
        NEXTAUTH_URL: "http://localhost:4444"

        # API connection — the INGRESS url, because this runs on the developer's
        # machine, not in a pod. Trimmed of /api: internalApiClient.ts appends
        # its own path [`${baseUrl}/api/get-service-token`], so leaving the
        # suffix on produced .../api/api/... and a 404 on every server-side call.
        DAPPLE_PROMPT_API_INTERNAL_URL: (api_base ($env | get -o DAPPLE_PROMPT_API_URL | default "https://dapple-prompt-api.kube.dappleai.dev/api"))

        # Service auth (uses same as UI for simplicity)
        DAPPLE_UI_SAT_SIGNING_KEY: ($secrets | get -o DAPPLE_UI_SAT_SIGNING_KEY | default "")
        DAPPLE_UI_SERVICE_ID: ($secrets | get -o DAPPLE_LANDING_SERVICE_ID | default ($secrets | get -o DAPPLE_UI_SERVICE_ID | default "dapple_landing"))

        # PostHog (optional)
        NEXT_PUBLIC_POSTHOG_KEY: ($secrets | get -o NEXT_PUBLIC_POSTHOG_KEY | default "")
        NEXT_PUBLIC_POSTHOG_HOST: ($secrets | get -o NEXT_PUBLIC_POSTHOG_HOST | default "")
    }

    # Format as .env file
    let env_content = (format-env-file $env_vars "dapple-landing-page")

    if $write {
        let target_file = ($landing_dir | path join ".env.local")
        if not ($landing_dir | path exists) {
            log error $"Directory not found: ($landing_dir)"
            log error "Specify path with --path or clone dapple-landing-page to the expected location"
            return
        }
        $env_content | save -f $target_file
        log info $"Wrote ($target_file)"
        print $"
Environment file written to: ($target_file)

Next steps:
  cd ($landing_dir)
  pnpm install
  pnpm dev

The app will connect to:
  - API: ($env_vars.DAPPLE_PROMPT_API_INTERNAL_URL)
"
    } else {
        print $env_content
    }
}

# Show what env vars are available from the generated secrets file
export def show [] {
    let generated = (generated_env_path (get-home))
    let secrets = (load-env-dapple)

    if ($secrets | is-empty) {
        print $"No secrets found in ($generated)"
        print "Run: nu assets/nu/init/setup.nu --environment sandbox"
        return
    }

    print $"Environment variables from ($generated):"
    print ""

    $secrets | transpose key value | each {|row|
        let masked = if ($row.value | str length) > 8 {
            let len = ($row.value | str length)
            let prefix = ($row.value | str substring 0..4)
            let suffix = ($row.value | str substring ($len - 4)..)
            $"($prefix)...($suffix)"
        } else {
            "****"
        }
        print $"  ($row.key)=($masked)"
    }

    print ""
    print $"To see full values: cat ($generated)"
}

# Regenerate the generated secrets file from template
export def regenerate [] {
    log info $"Regenerating (generated_env_path (get-home))..."

    let script = $env.DAPPLE_ASSETS_DIR | path join "nu" | path join "modules" | path join "tools" | path join "env_dapple.nu"
    let template = $env.DAPPLE_PROMPT_WORKSPACE_DIR | path join "assets" | path join "env.template"

    nu $script gen --example-file $template

    log info "Done. Restart nushell to pick up new values."
}

# Format a record as .env file content
def format-env-file [vars: record, app_name: string]: nothing -> string {
    let header = $"# Generated by: dapple env ($app_name)
# Date: (date now | format date '%Y-%m-%d %H:%M:%S')
#
# This file is for LOCAL development only.
# It mirrors the environment variables that ($app_name) receives in k8s.
#
# DO NOT commit this file to git.
# Regenerate with: dapple env ($app_name) --write
"

    let body = ($vars | transpose key value | each {|row|
        $"($row.key)=($row.value)"
    } | str join "\n")

    $"($header)\n($body)\n"
}
