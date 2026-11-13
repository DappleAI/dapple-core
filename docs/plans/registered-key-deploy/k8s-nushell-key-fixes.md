# Plan: Automated Service Token Registration During K8s Deployment

## Problem Statement

When dapple-ui/dapple-landing connect to dapple-prompt-api, they use Ed25519 service tokens for authentication. The flow requires:

1. **Web services** have private keys to sign tokens
2. **dapple-prompt-api** has corresponding public keys registered in `web.service_tokens`

Currently, the k8s deploy scripts:
- ✅ Set up secrets in k8s
- ❌ Do NOT auto-generate missing keys
- ❌ Do NOT register public keys in the database

This causes: `[InternalApiClient] Failed to get JWT: 401 Unauthorized`

## Proposed Solution

### Key Management Strategy

Each service gets its own Ed25519 keypair:

| Service | Env Var | Service ID |
|---------|---------|------------|
| dapple-ui | `DAPPLE_UI_API_SECRET` | `dapple_ui` |
| dapple-landing | `DAPPLE_LANDING_API_SECRET` | `dapple_landing` |

### Auto-Generation Flow

When deploying, if a service's secret is not in `~/.env`:
1. Generate new Ed25519 keypair
2. Append the private key to `~/.env`
3. Register the public key in database

## Files to Modify

| File | Change |
|------|--------|
| `assets/nu/tools/service_token.nu` | Add `register` and `ensure` subcommands |
| `assets/nu/k8s/deploy_dapple_prompt_pods.nu` | Call `ensure_service_tokens` after migrations |
| `assets/nu/k8s/deploy_dapple_web_pods.nu` | Update to use service-specific env vars |

## Implementation Details

### 1. Extend `service_token.nu`

**New `register` subcommand** - register a service given its private key:

```nu
export def "main register" [
    service_id: string
    --private-key: string       # Required: 64-hex-char Ed25519 private key
    --name: string = ""         # Human-readable name
] {
    # 1. Derive public key PEM from private key using openssl
    # 2. Upsert into web.service_tokens via psql
}
```

**New `ensure` subcommand** - ensure a service has keys, generating if missing:

```nu
export def "main ensure" [
    service_id: string
    --env-var: string           # e.g., "DAPPLE_UI_API_SECRET"
    --env-file: string = ""     # Defaults to ~/.env
    --name: string = ""
] {
    # 1. Check if env var is set
    # 2. If not: generate keypair, append to env file
    # 3. Register public key in database
    # 4. Return the private key (for passing to k8s secret setup)
}
```

### 2. Update `deploy_dapple_prompt_pods.nu`

After migrations (line 28), add:

```nu
# Ensure service tokens exist for all web services
log info "ensuring service tokens for web services"
ensure_service_tokens

def ensure_service_tokens [] {
    use ../tools/service_token.nu

    # Ensure dapple-ui token
    let ui_secret = (service_token ensure dapple_ui
        --env-var DAPPLE_UI_API_SECRET
        --name "Dapple UI")

    # Ensure dapple-landing token
    let landing_secret = (service_token ensure dapple_landing
        --env-var DAPPLE_LANDING_API_SECRET
        --name "Dapple Landing Page")

    # Store in env for later use by web pod deployment
    $env.DAPPLE_UI_API_SECRET = $ui_secret
    $env.DAPPLE_LANDING_API_SECRET = $landing_secret
}
```

### 3. Update `deploy_dapple_web_pods.nu`

Update `setup_dapple_ui_secret` (line 82+) to use service-specific env var:

```nu
def setup_dapple_ui_secret [] {
    # Change from INTERNAL_API_SECRET to DAPPLE_UI_API_SECRET
    let internal_api_secret = ($env | get -i DAPPLE_UI_API_SECRET | default "")
    let service_id = "dapple_ui"  # Hardcode, no longer configurable per-deploy

    # ... rest unchanged, but use new var names
}
```

Add similar function for dapple-landing if it needs its own secret.

### 4. SQL for Upsert

```sql
INSERT INTO web.service_tokens (service_id, name, public_key_pem, key_algorithm)
VALUES ($1, $2, $3, 'Ed25519')
ON CONFLICT (service_id) DO UPDATE SET
    public_key_pem = EXCLUDED.public_key_pem,
    name = COALESCE(NULLIF(EXCLUDED.name, ''), web.service_tokens.name);
```

## Environment Variable Changes

### Before (current)
```bash
INTERNAL_API_SECRET=<shared-key>
DAPPLE_SERVICE_ID=dapple_ui
```

### After (proposed)
```bash
# Auto-generated if missing:
DAPPLE_UI_API_SECRET=<64-hex-ed25519-key>
DAPPLE_LANDING_API_SECRET=<64-hex-ed25519-key>

# No longer needed:
# INTERNAL_API_SECRET (replaced by service-specific vars)
# DAPPLE_SERVICE_ID (hardcoded per service)
```

## Verification

1. **Clean slate test** - Remove keys from `~/.env`:
   ```bash
   # Comment out or remove:
   # DAPPLE_UI_API_SECRET=...
   # DAPPLE_LANDING_API_SECRET=...
   ```

2. **Run prompt deployment**:
   ```nu
   nu assets/nu/k8s/deploy_dapple_prompt_pods.nu
   ```
   Should see:
   - `generating keypair for dapple_ui`
   - `appending DAPPLE_UI_API_SECRET to ~/.env`
   - `registering service token for dapple_ui`
   - (same for dapple_landing)

3. **Verify keys written to ~/.env**:
   ```bash
   grep API_SECRET ~/.env
   ```

4. **Verify DB registration**:
   ```nu
   nu assets/nu/tools/service_token.nu list
   ```

5. **Run web deployment**:
   ```nu
   nu assets/nu/k8s/deploy_dapple_web_pods.nu
   ```

6. **Test connectivity** - dapple-ui should connect without 401 errors

## Migration Path

For existing deployments with `INTERNAL_API_SECRET`:
1. First deploy will generate new service-specific keys
2. Old `INTERNAL_API_SECRET` can be removed from `~/.env` after successful deploy
3. Backward compatible - if old var exists but new ones don't, could copy it (optional)

## Rollback

- Service tokens can be revoked: `nu service_token.nu revoke <service_id>`
- Keys in ~/.env can be manually edited
- Re-running deployment will re-register with current keys

---

# Full Implementation Plan

## Step 1: Add `register` subcommand to `service_token.nu`

Insert after the `generate` function (after line 73):

```nu
# Register a service token in the database from a private key
#
# Derives the public key from the private key and upserts into web.service_tokens.
# Returns the service_id on success.
export def "main register" [
    service_id: string              # Service identifier (3-32 chars, lowercase, starts with letter)
    --private-key: string = ""      # Ed25519 private key (64 hex chars). Defaults to env var based on service_id
    --name: string = ""             # Human-readable name for the service
] {
    let db_url = ($env | get -i DATABASE_URL | default "")
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
        let env_var = $service_id | str upcase | str replace "_" "_" | $"($in)_API_SECRET"
        $env | get -i $env_var | default ""
    } else {
        $private_key
    }

    if ($key | is-empty) {
        error make { msg: $"No private key provided and env var not set" }
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

# Derive Ed25519 public key PEM from private key hex
def derive_public_key [private_key_hex: string]: string {
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
```

## Step 2: Add `ensure` subcommand to `service_token.nu`

Insert after the `register` function:

```nu
# Ensure a service has keys configured, generating and registering if missing
#
# If the env var is not set:
# 1. Generates a new Ed25519 keypair
# 2. Appends the private key to the env file
# 3. Registers the public key in the database
#
# Returns the private key (from env or newly generated).
export def "main ensure" [
    service_id: string              # Service identifier
    --env-var: string = ""          # Env var name (defaults to {SERVICE_ID}_API_SECRET)
    --env-file: string = ""         # Env file path (defaults to ~/.env)
    --name: string = ""             # Human-readable name for the service
]: string {
    use std/log

    let db_url = ($env | get -i DATABASE_URL | default "")
    if ($db_url | is-empty) {
        error make { msg: "DATABASE_URL environment variable not set" }
    }

    # Derive env var name from service_id if not provided
    let var_name = if ($env_var | is-empty) {
        $service_id | str upcase | $"($in)_API_SECRET"
    } else {
        $env_var
    }

    let file_path = if ($env_file | is-empty) {
        $nu.home-dir | path join ".env"
    } else {
        $env_file
    }

    let display_name = if ($name | is-empty) { $service_id } else { $name }

    # Check if env var is already set
    let existing_key = ($env | get -i $var_name | default "")

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
```

## Step 3: Update `deploy_dapple_prompt_pods.nu`

After line 28 (after `sqlx migrate run`), add:

```nu
    # Ensure service tokens are registered for web services
    log info "ensuring service tokens for web services"
    ensure_service_tokens
```

Add new function at the end of the file (before or after `setup_dapple_svc`):

```nu
# Ensure service tokens exist for all web services that need API access
def ensure_service_tokens [] {
    # Import the service_token tool
    use ($env.DAPPLE_PROMPT_WORKSPACE_DIR | path join "assets" | path join "nu" | path join "tools" | path join "service_token.nu")

    # Ensure dapple-ui token
    log info "ensuring service token for dapple_ui"
    let ui_secret = (service_token ensure dapple_ui
        --env-var DAPPLE_UI_API_SECRET
        --name "Dapple UI")

    # Ensure dapple-landing token
    log info "ensuring service token for dapple_landing"
    let landing_secret = (service_token ensure dapple_landing
        --env-var DAPPLE_LANDING_API_SECRET
        --name "Dapple Landing Page")

    # Store in env for later use by web pod deployment
    load-env {
        DAPPLE_UI_API_SECRET: $ui_secret
        DAPPLE_LANDING_API_SECRET: $landing_secret
    }

    log info "service tokens ensured successfully"
}
```

## Step 4: Update `deploy_dapple_web_pods.nu`

Replace the `setup_dapple_ui_secret` function (lines 82-120) with:

```nu
# Create/update k8s secret for dapple-ui authentication configuration
def setup_dapple_ui_secret [] {
    let secret_name = "dapple-ui-auth"
    let namespace = $env.KUBE_DAPPLE_NAMESPACE

    # Get env vars (these should be set via .env or calling environment)
    let nextauth_secret = ($env | get -i NEXTAUTH_SECRET | default "")
    let nextauth_url = ($env | get -i NEXTAUTH_URL | default "https://landing-page.kube.dappleai.dev")
    let internal_api = ($env | get -i INTERNAL_API | default "https://dapple-prompt-api.kube.dappleai.dev")

    # Use service-specific env var (new naming convention)
    let internal_api_secret = ($env | get -i DAPPLE_UI_API_SECRET | default "")
    let service_id = "dapple_ui"  # Hardcoded - no longer configurable

    let posthog_key = ($env | get -i NEXT_PUBLIC_POSTHOG_KEY | default "")
    let posthog_host = ($env | get -i NEXT_PUBLIC_POSTHOG_HOST | default "https://us.i.posthog.com")

    # Check required secrets
    if ($nextauth_secret | is-empty) {
        log warning "NEXTAUTH_SECRET not set - auth features will not work. See .env.example for details."
    }

    if ($internal_api_secret | is-empty) {
        log warning "DAPPLE_UI_API_SECRET not set - run deploy_dapple_prompt_pods.nu first to generate keys"
    }

    # Delete existing secret if present (ignore errors if it doesn't exist)
    log debug $"removing existing secret ($secret_name) if present"
    do { kubectl delete secret $secret_name -n $namespace } | ignore

    # Create the secret with all env vars
    log debug $"creating secret ($secret_name) in namespace ($namespace)"
    (kubectl create secret generic $secret_name -n $namespace
        --from-literal=NEXTAUTH_SECRET=$nextauth_secret
        --from-literal=NEXTAUTH_URL=$nextauth_url
        --from-literal=INTERNAL_API=$internal_api
        --from-literal=INTERNAL_API_SECRET=$internal_api_secret
        --from-literal=DAPPLE_SERVICE_ID=$service_id
        --from-literal=NEXT_PUBLIC_POSTHOG_KEY=$posthog_key
        --from-literal=NEXT_PUBLIC_POSTHOG_HOST=$posthog_host)

    log info $"dapple-ui secret ($secret_name) created successfully"
}
```

Also add a similar function for dapple-landing if it needs API access (can be added later when needed).

## Step 5: Update help text in `service_token.nu`

Replace `print-help` function:

```nu
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
  # Generate keys for dapple-ui \(print only, no DB changes\):
  service_token.nu generate

  # Ensure dapple-ui has keys \(auto-generates and registers if missing\):
  service_token.nu ensure dapple_ui --name "Dapple UI"

  # Register existing key in database:
  service_token.nu register dapple_ui --private-key $env.DAPPLE_UI_API_SECRET

  # List all registered services:
  service_token.nu list

  # Revoke a compromised token:
  service_token.nu revoke old_service --reason "Key rotation"

Environment Variables:
  DATABASE_URL              PostgreSQL connection string \(required for register/ensure/list/revoke\)
  DAPPLE_UI_API_SECRET      Ed25519 private key for dapple-ui
  DAPPLE_LANDING_API_SECRET Ed25519 private key for dapple-landing
  INTERNAL_API              API URL \(for verify\)'
}
```

## Implementation Order

1. **service_token.nu** - Add `derive_public_key`, `register`, `ensure` functions
2. **service_token.nu** - Update `print-help` with new subcommands
3. **deploy_dapple_prompt_pods.nu** - Add `ensure_service_tokens` function and call it after migrations
4. **deploy_dapple_web_pods.nu** - Update `setup_dapple_ui_secret` to use new env var names
5. **Test** - Run the verification steps

## Test Commands

```nu
# 1. Clear any existing keys (optional, for clean test)
# Edit ~/.env and remove DAPPLE_UI_API_SECRET, DAPPLE_LANDING_API_SECRET

# 2. Test ensure command directly
nu assets/nu/tools/service_token.nu ensure dapple_ui --name "Dapple UI"

# 3. Verify key was written to ~/.env
grep API_SECRET ~/.env

# 4. Verify registration in database
nu assets/nu/tools/service_token.nu list

# 5. Full deployment test
nu assets/nu/k8s/deploy_dapple_prompt_pods.nu
nu assets/nu/k8s/deploy_dapple_web_pods.nu
```
