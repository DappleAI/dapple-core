# Token Management CLI Commands
#
# Unified interface for managing PAT (Personal Access Token) and
# Service Token authentication credentials.
#
# Examples:
#   dapple token pat create <user-uuid>       # Create PAT for user
#   dapple token pat create <email> --admin   # Create admin PAT by email
#   dapple token pat list <user-uuid>         # List user's PATs
#   dapple token pat revoke <token-uuid>      # Revoke a PAT
#   dapple token pat create-via-api --jwt ..  # Create PAT via API
#   dapple token service list                 # List service tokens
#   dapple token service ensure <id>          # Ensure service has keys
#   dapple token service generate [id]        # Generate new keypair
#   dapple token service revoke <id>          # Revoke service token

use ../tools/pat_token.nu [create, list, revoke]
use ../tools/service_token.nu [ensure, generate]

# --- PAT Token Commands ---

# Create a PAT token for a user via direct database insert
export def "pat create" [
    user: string                     # User's object_id (UUID) or email address
    --name: string = "CLI Test Token"  # Human-readable name
    --expires-days: int = 30         # Days until expiration (0 = never)
    --admin                          # Create admin-capable token
] {
    if $admin {
        create $user --name $name --expires-days $expires_days --admin
    } else {
        create $user --name $name --expires-days $expires_days
    }
}

# Create a PAT token via the dapple-prompt-api service (requires JWT)
export def "pat create-via-api" [
    --name: string = "CLI Test Token"  # Human-readable name
    --expires-days: int = 30           # Days until expiration (0 = never)
    --jwt: string = ""                 # JWT for authentication (or set DAPPLE_AUTH_TOKEN)
    --api-url: string = ""             # API URL (skip port-forward if provided)
] {
    # Delegate to the pat_token module's main create-via-api
    use ../tools/pat_token.nu
    if ($jwt | is-empty) and ($api_url | is-empty) {
        pat_token main create-via-api --name $name --expires-days $expires_days
    } else if ($api_url | is-empty) {
        pat_token main create-via-api --name $name --expires-days $expires_days --jwt $jwt
    } else if ($jwt | is-empty) {
        pat_token main create-via-api --name $name --expires-days $expires_days --api-url $api_url
    } else {
        pat_token main create-via-api --name $name --expires-days $expires_days --jwt $jwt --api-url $api_url
    }
}

# List PAT tokens for a user
export def "pat list" [
    user: string  # User's object_id (UUID) or email address
] {
    list $user
}

# Revoke a PAT token
export def "pat revoke" [
    token_id: string                 # Token UUID
    --reason: string = "Manual revocation"  # Reason for revocation
] {
    revoke $token_id --reason $reason
}

# --- Service Token Commands ---

# List registered service tokens
export def "service list" [] {
    use ../tools/service_token.nu
    service_token main list
}

# Ensure a service has keys configured, generating and registering if missing
export def "service ensure" [
    service_id: string              # Service identifier
    --env-var: string = ""          # Env var name (defaults to {SERVICE_ID}_SAT_SIGNING_KEY)
    --name: string = ""             # Human-readable name
] {
    ensure $service_id --env-var $env_var --name $name
}

# Generate a new Ed25519 keypair for service token authentication
export def "service generate" [
    service_id: string = "dapple_ui"  # Service identifier
    --name: string = ""               # Human-readable name
] {
    generate $service_id --name $name
}

# Revoke a service token
export def "service revoke" [
    service_id: string  # Service identifier to revoke
    --reason: string = "Manual revocation"  # Reason for revocation
] {
    use ../tools/service_token.nu
    service_token main revoke $service_id --reason $reason
}

# Show token management help
export def main [] {
    print "
dapple token - Token management for dapple services

Usage: dapple token <subcommand>

PAT (Personal Access Token) Commands:
  dapple token pat create <user-uuid|email>        Create PAT via direct DB insert
  dapple token pat create <email> --admin          Create admin-capable PAT
  dapple token pat create <uuid> --expires-days 7  Short-lived token
  dapple token pat create-via-api --jwt <token>    Create PAT via API (for your user)
  dapple token pat list <user-uuid|email>          List tokens for a user
  dapple token pat revoke <token-uuid>             Revoke a token

Service Token Commands:
  dapple token service list                        List registered service tokens
  dapple token service ensure <id>                 Ensure keys exist (auto-generates)
  dapple token service generate [id]               Generate new Ed25519 keypair
  dapple token service revoke <id>                 Revoke a service token

For B2B-partner (external) credentials, see `dapple external` instead.

Environment Variables:
  DATABASE_URL           PostgreSQL connection string (set by dapple env overlay)
  DAPPLE_AUTH_TOKEN      JWT for create-via-api command

Examples:
  dapple token pat create 550e8400-e29b-41d4-a716-446655440000 --name \"Test\"
  dapple token pat create user@example.com --admin
  dapple token pat list user@example.com
  dapple token service list
  dapple token service ensure dapple_ui --name \"Dapple UI\"
"
}
