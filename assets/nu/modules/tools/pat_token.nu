#!/usr/bin/env nu

# PAT (Personal Access Token) management tool for dapple-prompt
#
# Creates API tokens for testing purposes by directly inserting into the database.
# This bypasses the normal API flow which requires JWT authentication as the target user.
#
# Token format: dpl_<43 char base64url> (256 bits entropy, 47 chars total)
# Storage: SHA-256 hash in database, plaintext shown once
#
# Requires DATABASE_URL environment variable (set by conf.d/dapple.nu).

use std/log

# Generate a random API token in the dpl_ format
#
# Returns a record with:
# - token: The plaintext token (dpl_<43 chars>)
# - token_hash: SHA-256 hash as hex string
# - token_prefix: First 8 characters for identification
def generate_token [] {
    # Generate 32 bytes of random data using openssl, base64url encoded
    let encoded = (^openssl rand -base64 32
        | str trim
        | str replace -a '+' '-'
        | str replace -a '/' '_'
        | str replace -a '=' ''
        | str substring 0..<43)

    let token = $"dpl_($encoded)"

    # Compute SHA-256 hash using openssl
    let token_hash = (^printf '%s' $token | ^openssl dgst -sha256 -hex | str trim | split row '= ' | last)

    let token_prefix = ($token | str substring 0..<8)

    {
        token: $token
        token_hash: $token_hash
        token_prefix: $token_prefix
    }
}

# Get DATABASE_URL from environment or error
def require_db_url [] {
    let db_url = ($env | get -o DATABASE_URL | default "")
    if ($db_url | is-empty) {
        error make { msg: "DATABASE_URL environment variable not set. Load the dapple environment overlay first." }
    }
    $db_url
}

# Resolve a user identifier to { user_id, object_id }
#
# Accepts either a rev.user_details object_id (UUID) or an email address
# (matched case-insensitively against auth.users.email, joined to
# rev.user_details).
def resolve_user [db_url: string, identifier: string] {
    if ($identifier | str contains "@") {
        let safe_email = ($identifier | str replace -a "'" "''")
        let result = (^psql $db_url -t -A -c $"
            SELECT u.user_id, u.object_id
            FROM rev.user_details u
            JOIN auth.users a ON a.id = u.user_id
            WHERE lower\(a.email\) = lower\('($safe_email)'\);
        " | str trim)

        if ($result | is-empty) {
            error make { msg: $"No user found with email: ($identifier)" }
        }

        let rows = ($result | lines)
        if ($rows | length) > 1 {
            let object_ids = ($rows | each {|r| $r | split row "|" | get 1 } | str join ", ")
            error make { msg: $"Multiple users match email ($identifier) — use an object_id instead: ($object_ids)" }
        }

        let parts = ($rows | first | split row "|")
        { user_id: ($parts | first | into int), object_id: ($parts | get 1) }
    } else {
        if not ($identifier =~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$') {
            error make { msg: $"Expected a UUID or email address, got: ($identifier)" }
        }

        let user_result = (^psql $db_url -t -A -c $"SELECT user_id FROM rev.user_details WHERE object_id = '($identifier)';" | str trim)

        if ($user_result | is-empty) {
            error make { msg: $"User not found with object_id: ($identifier)" }
        }

        { user_id: ($user_result | into int), object_id: $identifier }
    }
}

# Create a PAT token for a user via direct database insert
#
# This creates an API token by directly inserting into web.api_tokens.
# Use this for testing when you need tokens for arbitrary users.
#
# The token is shown ONCE - store it securely!
export def "main create" [
    user: string                     # User's object_id (UUID) or email address
    --name: string = "CLI Test Token"  # Human-readable name for the token
    --expires-days: int = 30         # Days until expiration (0 = never expires)
    --admin                          # Create admin-capable token (also grants admin role to user)
] {
    let db_url = (require_db_url)

    log info $"Looking up user: ($user)"

    let resolved = (resolve_user $db_url $user)
    let user_id = $resolved.user_id
    let user_object_id = $resolved.object_id

    log info $"Found user_id: ($user_id) for object_id: ($user_object_id)"

    # Generate token
    let token_data = (generate_token)

    # Calculate expiration
    let expires_at = if $expires_days == 0 {
        "NULL"
    } else {
        let exp_date = ((date now) + ($expires_days * 1day) | format date "%Y-%m-%dT%H:%M:%S%z")
        $"'($exp_date)'"
    }

    # Escape single quotes in name for SQL
    let safe_name = ($name | str replace -a "'" "''")

    # Insert token and optionally set up admin privileges
    let insert_sql = if $admin {
        $"
            WITH inserted_token AS \(
                INSERT INTO web.api_tokens \(user_id, name, token_hash, token_prefix, expires_at\)
                VALUES \(
                    ($user_id),
                    '($safe_name)',
                    '\\x($token_data.token_hash)',
                    '($token_data.token_prefix)',
                    ($expires_at)
                \)
                RETURNING id, name, token_prefix, expires_at, created_at
            \),
            admin_token AS \(
                INSERT INTO web.admin_api_tokens \(api_token_id\)
                SELECT id FROM inserted_token
            \),
            user_role AS \(
                INSERT INTO web.user_roles \(user_id, role\)
                VALUES \(($user_id), 'user'\), \(($user_id), 'admin'\)
                ON CONFLICT \(user_id, role\) DO NOTHING
            \)
            SELECT id, name, token_prefix, expires_at, created_at FROM inserted_token;
        "
    } else {
        $"
            INSERT INTO web.api_tokens \(user_id, name, token_hash, token_prefix, expires_at\)
            VALUES \(
                ($user_id),
                '($safe_name)',
                '\\x($token_data.token_hash)',
                '($token_data.token_prefix)',
                ($expires_at)
            \)
            RETURNING id, name, token_prefix, expires_at, created_at;
        "
    }

    let result = (^psql $db_url -t -c $insert_sql)

    print ""
    print "=== API Token Created ==="
    if $admin {
        print "WARNING: ADMIN TOKEN - use with care"
    }
    print ""
    print $"Token: ($token_data.token)"
    print ""
    print "IMPORTANT: This token is shown ONLY ONCE. Store it securely!"
    print ""
    print "Database result:"
    print $result
    print ""
    print "Usage:"
    print $"  curl -H 'Authorization: Bearer ($token_data.token)' ..."
    print ""

    {
        token: $token_data.token
        token_prefix: $token_data.token_prefix
        user_object_id: $user_object_id
        user_id: $user_id
        name: $name
        is_admin: $admin
    }
}

# Create a PAT token via the dapple-prompt-api service
#
# Port-forwards to the API pod and creates the token via HTTP.
# Requires an existing JWT or service token for authentication.
#
# Note: The API creates tokens for the AUTHENTICATED user, not arbitrary users.
# For arbitrary users, use `create` which does direct DB insert.
export def "main create-via-api" [
    --name: string = "CLI Test Token"  # Human-readable name for the token
    --expires-days: int = 30           # Days until expiration (0 = never expires)
    --jwt: string = ""                 # JWT token for authentication
    --api-url: string = ""             # API URL (skip port-forward if provided)
] {
    let token = if ($jwt | is-empty) {
        $env | get -o DAPPLE_JWT | default ""
    } else {
        $jwt
    }

    if ($token | is-empty) {
        error make { msg: "JWT token required. Use --jwt or set DAPPLE_JWT env var." }
    }

    let ns = ($env | get -o KUBE_DAPPLE_NAMESPACE | default "dapple-sandbox")

    # Determine API URL
    let api_base = if not ($api_url | is-empty) {
        $api_url
    } else {
        let port = 18080
        log info $"Starting port-forward to dapple-prompt-api on port ($port)..."

        ^kubectl port-forward -n $ns svc/dapple-prompt-api $"($port):80" out+err> /dev/null &
        sleep 2sec

        $"http://localhost:($port)"
    }

    # Build request
    let expires_in_secs = if $expires_days == 0 { null } else { $expires_days * 86400 }

    let body = if ($expires_in_secs == null) {
        { name: $name }
    } else {
        { name: $name, expires_in_secs: $expires_in_secs }
    }

    log info $"Creating token via ($api_base)/api/api-tokens/"

    let result = try {
        http post $"($api_base)/api/api-tokens/" $body --headers [
            "Authorization" $"Bearer ($token)"
            "Content-Type" "application/json"
        ]
    } catch { |err|
        log error $"Error creating token: ($err.msg)"
        null
    }

    if ($result != null) {
        print ""
        print "=== API Token Created ==="
        print ($result | to json -i 2)
    }

    $result
}

# List PAT tokens for a user
export def "main list" [
    user: string                     # User's object_id (UUID) or email address
] {
    let db_url = (require_db_url)

    let resolved = (resolve_user $db_url $user)

    let sql = $"
        SELECT
            t.id,
            t.name,
            t.token_prefix,
            t.expires_at,
            t.created_at,
            r.revoked_at
        FROM web.api_tokens t
        LEFT JOIN web.api_token_revocations r ON r.api_token_id = t.id
        WHERE t.user_id = ($resolved.user_id)
        ORDER BY t.created_at DESC;
    "

    ^psql $db_url -t -c $sql
}

# Revoke a PAT token
export def "main revoke" [
    token_id: string                 # Token UUID
    --reason: string = "Manual revocation"  # Reason for revocation
] {
    let db_url = (require_db_url)

    let safe_reason = ($reason | str replace -a "'" "''")

    let sql = $"
        INSERT INTO web.api_token_revocations \(api_token_id, reason\)
        SELECT id, '($safe_reason)'
        FROM web.api_tokens
        WHERE id = '($token_id)'
        RETURNING id, api_token_id, reason, revoked_at;
    "

    let result = (^psql $db_url -t -c $sql)
    log info $"Revoked token: ($token_id)"
    print $result
}

export def "main help" [] {
    print-help
}

export def main [] {
    print-help
}

# Module-friendly exports (for `use pat_token` pattern)
export def create [
    user: string                     # User's object_id (UUID) or email address
    --name: string = "CLI Test Token"
    --expires-days: int = 30
    --admin
] {
    if $admin {
        main create $user --name $name --expires-days $expires_days --admin
    } else {
        main create $user --name $name --expires-days $expires_days
    }
}

export def list [
    user: string                     # User's object_id (UUID) or email address
] {
    main list $user
}

export def revoke [
    token_id: string
    --reason: string = "Manual revocation"
] {
    main revoke $token_id --reason $reason
}

def print-help [] {
    print "PAT (Personal Access Token) Management Tool

Usage:
  pat_token.nu <subcommand> [args]

Subcommands:
  create <user_object_id|email> [--name] [--expires-days] [--admin]
      Create a PAT for a user via direct database insert.
      Accepts the user's object_id (UUID) or email (matched against auth.users.email).
      This bypasses the API (which requires JWT auth as the target user).
      Best for testing when you need tokens for arbitrary users.
      Use --admin to create an admin-capable token (also grants admin role to user).

  create-via-api [--name] [--expires-days] [--jwt] [--api-url]
      Create a PAT via the Kubernetes API (requires JWT for YOUR user).
      Note: Creates token for the authenticated user, not arbitrary users.

  list <user_object_id|email>
      List all PAT tokens for a user.

  revoke <token_id> [--reason]
      Revoke a PAT token.

  help
      Show this help.

Examples:
  # Create a token for a specific user (for testing):
  pat_token.nu create 550e8400-e29b-41d4-a716-446655440000 --name \"UI Test\" --expires-days 7

  # Create a token by email (looked up via auth.users):
  pat_token.nu create user@example.com --name \"UI Test\"

  # Create an admin token (also grants admin role to user):
  pat_token.nu create 550e8400-e29b-41d4-a716-446655440000 --name \"Admin Token\" --admin

  # List tokens for a user:
  pat_token.nu list 550e8400-e29b-41d4-a716-446655440000

  # Revoke a token:
  pat_token.nu revoke 660e8400-e29b-41d4-a716-446655440001 --reason \"No longer needed\"

  # Use as a module:
  use pat_token
  pat_token create <user-uuid> --name \"My Token\"
  pat_token create <user-uuid> --name \"Admin Token\" --admin

Environment Variables:
  DATABASE_URL           PostgreSQL connection string (set by dapple env overlay)

Token Format:
  dpl_<43 char base64url>  (256 bits entropy, 47 chars total)
  Example: dpl_7Kj2mNpQrStUvWxYz3AbCdEfGhIjKlMn_abcdefg

Security Notes:
  - Tokens are shown ONLY ONCE at creation - store them securely
  - The database stores only the SHA-256 hash of the token
  - Use short expiration times for testing tokens
  - Revoke tokens when no longer needed
  - Admin tokens should be used sparingly and with extra caution"
}
