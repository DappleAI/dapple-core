#!/usr/bin/env nu

# Service token management tool for dapple-prompt
#
# Generates Ed25519 keypairs and SQL for registering services
# in the web.service_tokens table.

# Generated keys are written to the machine-written env file, and WHERE that
# lives is env_files.nu's to say — not this file's. It used to hardcode
# ~/.env.dapple, which the shell profile stopped loading when configuration
# moved under ~/.dapple/: `ensure` then wrote each new key somewhere nothing
# read, found the variable unset on the next run, and minted another one. A
# fresh signing key on every deploy, silently.
use ../dapple_env_profiles/env_files.nu [generated_env_path]

# `verify` needs the API address without the /api it appends itself; the table
# module owns that conversion so the rule lives in one place.
use ../dapple_env_profiles/environments.nu [api_base]

# Generate a new Ed25519 keypair for service token authentication
#
# This generates a private key (for DAPPLE_UI_SAT_SIGNING_KEY) and public key PEM
# (for registering in the database). The output includes:
# - Environment variable to add to dapple-ui's .env
# - SQL INSERT statement to register the service in dapple-prompt-api
export def "main generate" [
    service_id: string = "dapple_ui"  # Service identifier (3-32 chars, lowercase, starts with letter)
    --name: string = ""               # Human-readable name for the service
] {
    # Validate service_id format (must match: ^[a-z][a-z0-9_]{2,31}$)
    if ($service_id | str length) < 3 or ($service_id | str length) > 32 {
        print $"Error: service_id must be 3-32 characters, got ($service_id | str length)"
        return
    }

    if not ($service_id =~ '^[a-z][a-z0-9_]+$') {
        print "Error: service_id must start with lowercase letter, contain only [a-z0-9_]"
        return
    }

    let display_name = if ($name | is-empty) { $service_id } else { $name }

    # Generate Ed25519 keypair using openssl
    let temp_key = (mktemp)

    # Generate the private key in PEM format
    openssl genpkey -algorithm Ed25519 -out $temp_key e> /dev/null

    # Extract the raw 32-byte private key seed as hex
    let private_key_hex = (openssl pkey -in $temp_key -outform DER e> /dev/null
        | bytes at 16..48
        | encode hex)

    # Extract public key in PEM format
    let public_key_pem = (openssl pkey -in $temp_key -pubout -outform PEM e> /dev/null)

    # Clean up temp file
    rm -f $temp_key

    print $"
# Service Token Generated for: ($service_id)
# ==========================================

# 1. Add this to dapple-ui's .env file:
# --------------------------------------
DAPPLE_UI_SAT_SIGNING_KEY=($private_key_hex)
DAPPLE_UI_SERVICE_ID=($service_id)

# 2. Run this SQL against your database to register the service:
# --------------------------------------------------------------
INSERT INTO web.service_tokens \(service_id, name, public_key_pem, key_algorithm\)
VALUES \(
    '($service_id)',
    '($display_name)',
    '($public_key_pem | str trim)',
    'Ed25519'
\);

# 3. Verify registration:
# -----------------------
SELECT service_id, name, created_at
FROM web.service_tokens
WHERE service_id = '($service_id)';
"
}

# Derive Ed25519 public key PEM from private key hex
def derive_public_key [private_key_hex: string] {
    let temp_key = (mktemp)

    # Convert hex to binary and create a proper PKCS#8 DER structure
    # Ed25519 PKCS#8 header (16 bytes) + 32 byte private key
    let header = "302e020100300506032b657004220420"
    let der_hex = $"($header)($private_key_hex)"

    # Write DER to temp file
    $der_hex | decode hex | save -f $temp_key

    # Convert to PEM and extract public key
    let public_pem = (openssl pkey -inform DER -in $temp_key -pubout -outform PEM e> /dev/null | str trim)

    rm -f $temp_key

    $public_pem
}

# Register a service token in the database from a private key
#
# Derives the public key from the private key and upserts into web.service_tokens.
# Returns the service_id on success.
export def "main register" [
    service_id: string              # Service identifier (3-32 chars, lowercase, starts with letter)
    --private-key: string = ""      # Ed25519 private key (64 hex chars). Defaults to env var based on service_id
    --name: string = ""             # Human-readable name for the service
] {
    let db_url = ($env | get -o DATABASE_URL | default "")
    if ($db_url | is-empty) {
        error make { msg: "DATABASE_URL environment variable not set" }
    }

    # Validate service_id format
    if ($service_id | str length) < 3 or ($service_id | str length) > 32 {
        error make { msg: $"service_id must be 3-32 characters, got ($service_id | str length)" }
    }
    if not ($service_id =~ '^[a-z][a-z0-9_]+$') {
        error make { msg: "service_id must start with lowercase letter, contain only [a-z0-9_]" }
    }

    # Get private key from arg or derive env var name from service_id
    let key = if ($private_key | is-empty) {
        let env_var = $service_id | str uppercase | $"($in)_SAT_SIGNING_KEY"
        $env | get -o $env_var | default ""
    } else {
        $private_key
    }

    if ($key | is-empty) {
        error make { msg: "No private key provided and env var not set" }
    }

    # Normalize key (remove 0x prefix if present)
    let key_normalized = $key | str replace "^0x" ""
    if ($key_normalized | str length) != 64 {
        error make { msg: $"Private key must be 64 hex characters, got ($key_normalized | str length)" }
    }

    let display_name = if ($name | is-empty) { $service_id } else { $name }

    # Derive public key from private key
    let public_key_pem = (derive_public_key $key_normalized)

    # Upsert into database
    let sql = $"
        INSERT INTO web.service_tokens \(service_id, name, public_key_pem, key_algorithm\)
        VALUES \('($service_id)', '($display_name)', '($public_key_pem)', 'Ed25519'\)
        ON CONFLICT \(service_id\) DO UPDATE SET
            public_key_pem = EXCLUDED.public_key_pem,
            name = COALESCE\(NULLIF\(EXCLUDED.name, ''\), web.service_tokens.name\)
        RETURNING service_id, name;
    "

    let result = (psql $db_url -t -c $sql | str trim)
    print $"Registered service token: ($result)"

    $service_id
}

# Ensure a service has keys configured, generating and registering if missing
#
# If the env var is not set:
# 1. Generates a new Ed25519 keypair
# 2. Appends the private key to ~/.dapple/generated.env (machine-written file)
# 3. Registers the public key in the database
#
# Returns the private key (from env or newly generated).
export def "main ensure" [
    service_id: string              # Service identifier
    --env-var: string = ""          # Env var name (defaults to {SERVICE_ID}_API_SECRET)
    --env-file: string = ""         # Env file path (defaults to ~/.dapple/generated.env)
    --name: string = ""             # Human-readable name for the service
] {
    use std/log

    let db_url = ($env | get -o DATABASE_URL | default "")
    if ($db_url | is-empty) {
        error make { msg: "DATABASE_URL environment variable not set" }
    }

    # Derive env var name from service_id if not provided
    let var_name = if ($env_var | is-empty) {
        $service_id | str uppercase | $"($in)_SAT_SIGNING_KEY"
    } else {
        $env_var
    }

    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    let file_path = if ($env_file | is-empty) {
        # The machine-written file, never the hand-authored one — a generated
        # key is not an operator override. env_files.nu owns the path.
        generated_env_path $home
    } else {
        $env_file
    }

    let display_name = if ($name | is-empty) { $service_id } else { $name }

    # Check if env var is already set
    let existing_key = ($env | get -o $var_name | default "")

    let private_key = if ($existing_key | is-empty) {
        log info $"($var_name) not set, generating new keypair for ($service_id)"

        # Generate new keypair
        let temp_key = (mktemp)
        openssl genpkey -algorithm Ed25519 -out $temp_key e> /dev/null

        let new_key = (openssl pkey -in $temp_key -outform DER e> /dev/null
            | bytes at 16..48
            | encode hex)

        rm -f $temp_key

        # Append to env file
        log info $"appending ($var_name) to ($file_path)"
        # Note: appending to ~/.dapple/generated.env [machine-written]
        # User overrides belong in ~/.dapple/env
        let line = $"\n# Auto-generated service token for ($service_id)\n($var_name)=($new_key)\n"

        if ($file_path | path exists) {
            $line | save -a $file_path
        } else {
            $line | save $file_path
        }

        # Also load into current environment
        load-env { $var_name: $new_key }

        $new_key
    } else {
        log info $"($var_name) already set, using existing key"
        $existing_key
    }

    # Register in database (upsert handles existing records)
    log info $"registering service token for ($service_id) in database"
    main register $service_id --private-key $private_key --name $display_name

    $private_key
}

# List registered service tokens from the database
export def "main list" [] {
    let db_url = ($env | get -o DATABASE_URL | default "")

    if ($db_url | is-empty) {
        print "Error: DATABASE_URL environment variable not set"
        return
    }

    psql $db_url -c "
        SELECT
            st.service_id,
            st.name,
            st.key_algorithm,
            st.created_at,
            CASE WHEN str.revoked_at IS NOT NULL THEN 'REVOKED' ELSE 'ACTIVE' END as status
        FROM web.service_tokens st
        LEFT JOIN web.service_token_revocations str ON str.service_token_id = st.id
        ORDER BY st.created_at DESC;
    "
}

# Revoke a service token
export def "main revoke" [
    service_id: string  # Service identifier to revoke
    --reason: string = "Manual revocation"  # Reason for revocation
] {
    let db_url = ($env | get -o DATABASE_URL | default "")

    if ($db_url | is-empty) {
        print "Error: DATABASE_URL environment variable not set"
        return
    }

    print $"Revoking service token for: ($service_id)"

    psql $db_url -c $"
        INSERT INTO web.service_token_revocations \(service_token_id, reason\)
        SELECT id, '($reason)'
        FROM web.service_tokens
        WHERE service_id = '($service_id)'
        RETURNING id, service_token_id, reason, revoked_at;
    "
}

# Verify a service token configuration by testing the authentication flow
#
# Runs from a WORKSTATION, so it needs an address reachable from the host:
# the ingress in sandbox, the in-cluster FQDN in prod [prod's node resolves
# *.cluster.local]. DAPPLE_PROMPT_API_URL is that address in both, and
# `api_base` drops the /api this function appends itself. It used to read
# DAPPLE_PROMPT_API_INTERNAL_URL, which is a Service name pods use and no
# workstation can resolve.
export def "main verify" [
    --api-url: string = ""  # API URL (defaults to DAPPLE_PROMPT_API_URL env var)
] {
    let api = if ($api_url | is-empty) {
        api_base ($env | get -o DAPPLE_PROMPT_API_URL | default "")
    } else {
        $api_url
    }
    let secret = ($env | get -o DAPPLE_UI_SAT_SIGNING_KEY | default "")
    let service_id = ($env | get -o DAPPLE_UI_SERVICE_ID | default "dapple_ui")

    if ($api | is-empty) {
        print "Error: DAPPLE_PROMPT_API_URL not set (use --api-url or set env var)"
        return
    }

    if ($secret | is-empty) {
        print "Error: DAPPLE_UI_SAT_SIGNING_KEY not set"
        return
    }

    print $"Verifying service token for ($service_id) against ($api)..."
    print ""

    # Test health endpoint first
    print "1. Testing health endpoint..."
    let health_result = (http get $"($api)/api/health_check" | to json)
    print $"   Health check: ($health_result)"
    print ""

    print $"2. Testing service token exchange at ($api)/api/get-service-token..."
    print "   (Note: This requires a signed token - manual signing not implemented in nu)"
    print "   Use the dapple-ui to test the full flow, or check API logs."
}

export def "main help" [] {
    print-help
}

export def main [] {
    print-help
}

# Module-friendly exports (for `use service_token` pattern)
# These wrap the subcommands for use when imported as a module

export def ensure [
    service_id: string              # Service identifier
    --env-var: string = ""          # Env var name (defaults to {SERVICE_ID}_API_SECRET)
    --env-file: string = ""         # Env file path (defaults to ~/.dapple/generated.env)
    --name: string = ""             # Human-readable name for the service
] {
    main ensure $service_id --env-var $env_var --env-file $env_file --name $name
}

export def register [
    service_id: string              # Service identifier
    --private-key: string = ""      # Ed25519 private key (64 hex chars)
    --name: string = ""             # Human-readable name for the service
] {
    main register $service_id --private-key $private_key --name $name
}

export def generate [
    service_id: string = "dapple_ui"  # Service identifier
    --name: string = ""               # Human-readable name for the service
] {
    main generate $service_id --name $name
}

def print-help [] {
    print $'Service Token Management Tool

Usage:
  service_token.nu <subcommand> [args]

Subcommands:
  generate [service_id] [--name <name>]     Generate new Ed25519 keypair and print SQL
  register <service_id> [--private-key]     Register a service token in the database
  ensure <service_id> [--env-var] [--name]  Ensure keys exist, generating if missing
  list                                       List registered service tokens
  revoke <service_id> [--reason <reason>]   Revoke a service token
  verify [--api-url <url>]                  Verify configuration
  help                                       Show this help

Examples:
  # Generate keys - print only, no DB changes:
  service_token.nu generate

  # Ensure dapple-ui has keys - auto-generates and registers if missing:
  service_token.nu ensure dapple_ui --name "Dapple UI"

  # Register existing key in database:
  service_token.nu register dapple_ui --private-key $env.DAPPLE_UI_SAT_SIGNING_KEY

  # List all registered services:
  service_token.nu list

  # Revoke a compromised token:
  service_token.nu revoke old_service --reason "Key rotation"

Environment Variables:
  DATABASE_URL                     PostgreSQL connection string [register/ensure/list/revoke]
  DAPPLE_UI_SAT_SIGNING_KEY        Ed25519 private key for dapple-ui
  DAPPLE_LANDING_SAT_SIGNING_KEY   Ed25519 private key for dapple-landing
  DAPPLE_PROMPT_API_URL            API URL, /api suffix trimmed [verify command]'
}
