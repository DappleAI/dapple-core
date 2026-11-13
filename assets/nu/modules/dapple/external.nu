# External (B2B-partner) administration.
#
# Manages `rev.companies` and `rev.external_service_tokens` via the
# admin REST endpoints on `dapple-prompt-api`. Tokens are Ed25519
# keypairs; the keypair is generated locally on `token issue` and the
# private key prints to stdout exactly once.
#
# Requires `$env.DAPPLE_AUTH_TOKEN` set to an admin bearer (PAT or JWT).
#
# This is a sibling to `dapple token` (which handles dapple-user PATs
# and internal service tokens). It lives at the top level under `dapple`
# because B2B-partner management spans more than tokens — it includes
# company CRUD as well — and because deeper nesting hits a nushell
# subcommand-dispatch depth limit.

use ../tools/external_service_token.nu

# ----------------------------------------------------------------------------
# Company commands
# ----------------------------------------------------------------------------

export def "company create" [
    slug: string                        # Partner slug (immutable; ^[a-z][a-z0-9_-]{1,31}$)
    --name: string = ""                 # Display name (defaults to slug)
    --jwt: string = ""                  # Admin JWT/PAT (or DAPPLE_AUTH_TOKEN)
    --api-url: string = ""              # Override API URL
] {
    if ($name | is-empty) {
        external_service_token company create $slug --jwt $jwt --api-url $api_url
    } else {
        external_service_token company create $slug --name $name --jwt $jwt --api-url $api_url
    }
}

export def "company list" [
    --revoked                           # Include revoked rows
    --jwt: string = ""
    --api-url: string = ""
] {
    if $revoked {
        external_service_token company list --revoked --jwt $jwt --api-url $api_url
    } else {
        external_service_token company list --jwt $jwt --api-url $api_url
    }
}

export def "company show" [
    company: string                     # Slug or UUID
    --jwt: string = ""
    --api-url: string = ""
] {
    external_service_token company show $company --jwt $jwt --api-url $api_url
}

export def "company revoke" [
    company: string                     # Slug or UUID
    --reason: string = "Manual revocation"
    --jwt: string = ""
    --api-url: string = ""
] {
    external_service_token company revoke $company --reason $reason --jwt $jwt --api-url $api_url
}

# ----------------------------------------------------------------------------
# Token commands
# ----------------------------------------------------------------------------

export def "token issue" [
    service_id: string                  # Globally-unique service id (^[a-z][a-z0-9_]{2,31}$)
    --company: string                   # Owning company slug or UUID (required)
    --name: string = ""                 # Display name (defaults to service_id)
    --description: string = ""          # Optional description
    --jwt: string = ""
    --api-url: string = ""
] {
    external_service_token token issue $service_id --company $company --name $name --description $description --jwt $jwt --api-url $api_url
}

export def "token list" [
    --company: string                   # Slug or UUID (required)
    --jwt: string = ""
    --api-url: string = ""
] {
    external_service_token token list --company $company --jwt $jwt --api-url $api_url
}

export def "token revoke" [
    token_id: string                    # Token UUID
    --reason: string = "Manual revocation"
    --jwt: string = ""
    --api-url: string = ""
] {
    external_service_token token revoke $token_id --reason $reason --jwt $jwt --api-url $api_url
}

# ----------------------------------------------------------------------------
# Help
# ----------------------------------------------------------------------------

export def main [] {
    print "
dapple external - B2B-partner administration

Usage: dapple external <subcommand>

Company subcommands:
  dapple external company create <slug> [--name <text>]
      Create a new B2B-partner company (slug is immutable).

  dapple external company list [--revoked]
      List companies. Default excludes revoked rows.

  dapple external company show <slug-or-uuid>
      Show one company including its issued tokens.

  dapple external company revoke <slug-or-uuid> [--reason <text>]
      Revoke a company (soft-delete via revoked_at).

Token subcommands:
  dapple external token issue <service_id> --company <slug-or-uuid>
                              [--name <text>] [--description <text>]
      Generate an Ed25519 keypair LOCALLY, register the public PEM, and
      print the private key once. Server never sees the private key.

  dapple external token list --company <slug-or-uuid>
      List external service tokens registered for a company.

  dapple external token revoke <token-uuid> [--reason <text>]
      Revoke a single token without affecting siblings on the same company.

Common flags:
  --jwt <token>     Admin JWT/PAT (or set DAPPLE_AUTH_TOKEN env var).
  --api-url <url>   Skip kubectl port-forward and hit this URL directly.

Environment Variables:
  DAPPLE_AUTH_TOKEN      Admin Bearer token (required).
  KUBE_DAPPLE_NAMESPACE  K8s namespace (default: dapple-sandbox).

Examples:
  dapple external company create acme --name \"Acme\"
  dapple external token issue acme_prod --company acme
  dapple external token list --company acme
"
}
