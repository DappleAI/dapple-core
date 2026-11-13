#!/usr/bin/env nu

# B2B partner administration via the internal `dapple-prompt-api` REST surface.
#
# Backs `dapple external …` in `assets/nu/modules/dapple/external.nu`.
#
# Hits the admin endpoints introduced for managing `rev.companies` and
# `rev.external_service_tokens`. Talks to the cluster's already-exposed
# Ingress (`https://dapple-prompt-api.kube.dappleai.dev`) — no kubectl
# port-forward, no in-cluster routing dance. The `kubectl` wrapper from
# `dapple_env_profiles/dapple_microk8s.nu` is an overlay-scope nu function,
# so it isn't reachable from inside this tools module anyway; the env-URL
# path is both simpler and works regardless of K8s tooling state.
#
# Token issuance is asymmetric on purpose: the Ed25519 keypair is generated
# LOCALLY in this script. The server only ever sees the public PEM. The
# response includes the SHA-256 of the registered PEM so the operator can
# confirm the bytes that landed in the DB match what was just generated.
#
# Required:
#   $env.DAPPLE_AUTH_TOKEN    Admin-role JWT or PAT (Bearer).
#   $env.DAPPLE_PROMPT_API_URL   Internal-API base URL, INCLUDING `/api`.
#                                Set by the dapple_sandbox env overlay
#                                (`assets/nu/modules/dapple_env_profiles/dapple_sandbox.nu:72`)
#                                to `https://(host)/api`. This module appends
#                                resource paths to it directly.
#                                Override per-invocation with --api-url <url>.

use std/log

# ----------------------------------------------------------------------------
# Internal helpers
# ----------------------------------------------------------------------------

# Resolve the API base URL. `--api-url` wins; otherwise read
# `$env.DAPPLE_PROMPT_API_URL`. No compiled-in default — symmetric with the
# `DAPPLE_EXTERNAL_API_HOST` treatment in `crates/dapple-sdk/src/lib.rs`.
def resolve_api_base [api_url: string]: nothing -> string {
    if not ($api_url | is-empty) {
        return ($api_url | str trim --right --char '/')
    }
    let from_env = ($env | get -o DAPPLE_PROMPT_API_URL | default "")
    if ($from_env | is-empty) {
        error make {
            msg: "DAPPLE_PROMPT_API_URL is not set. Either export it (e.g. https://dapple-prompt-api.kube.dappleai.dev) or pass --api-url <url> per command."
        }
    }
    ($from_env | str trim --right --char '/')
}

# Look up the admin JWT/PAT from --jwt or $env.DAPPLE_AUTH_TOKEN.
def require_jwt [jwt: string]: nothing -> string {
    let token = if ($jwt | is-empty) {
        $env | get -o DAPPLE_AUTH_TOKEN | default ""
    } else {
        $jwt
    }
    if ($token | is-empty) {
        error make { msg: "Admin JWT/PAT required. Use --jwt or set DAPPLE_AUTH_TOKEN env var." }
    }
    $token
}

# Check whether a string looks like a UUID.
def is_uuid [s: string]: nothing -> bool {
    ($s =~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$')
}

# Resolve a `--company` argument: UUID passes through, slug looks up via the
# list endpoint and filters client-side (there's no /by-slug route today).
def resolve_company_id [
    company: string
    api_base: string
    jwt: string
]: nothing -> string {
    if (is_uuid $company) {
        return $company
    }
    let resp = (http get $"($api_base)/companies/" --headers [
        Authorization $"Bearer ($jwt)"
    ])
    let matches = ($resp.objs | where slug == $company)
    if ($matches | is-empty) {
        error make { msg: $"No company with slug '($company)' (and not a UUID). Run `dapple token external company list` to see what's registered." }
    }
    ($matches | first | get id)
}

# Generate an Ed25519 keypair locally. Returns a record:
#   { priv_pem, pub_pem, raw_hex, pub_pem_sha256 }
# Uses a tempfile because openssl's stdin/stdout binary handling in nu is
# fragile. The tempfile lives in $env.TMPDIR (or /tmp) and is unlinked
# before this function returns even on error.
def generate_ed25519_keypair []: nothing -> record {
    let tmp = (^mktemp -t dapple-ed25519-XXXXXX.pem | str trim)
    let result = try {
        ^openssl genpkey -algorithm ed25519 -out $tmp out+err> /dev/null
        let priv_pem = (open $tmp --raw | decode utf-8)
        let pub_pem = (^openssl pkey -in $tmp -pubout | decode utf-8)
        # PKCS8 DER for Ed25519 is 48 bytes total; last 32 are the raw key.
        let raw_hex = (
            ^openssl pkey -in $tmp -outform DER
            | ^tail -c 32
            | ^od -An -tx1
            | str trim
            | str replace --regex --all '\s+' ''
        )
        # SHA-256 of the registered PEM, hex. Matches what the server reports
        # back in `public_key_pem_sha256` so the operator can verify.
        let pub_pem_sha256 = (
            $pub_pem
            | ^openssl dgst -sha256 -hex
            | str trim
            | split row ' '
            | last
        )
        {
            priv_pem: $priv_pem
            pub_pem: $pub_pem
            raw_hex: $raw_hex
            pub_pem_sha256: $pub_pem_sha256
        }
    } catch {|e|
        ^rm -f $tmp
        error make { msg: $"keypair generation failed: ($e.msg)" }
    }
    ^rm -f $tmp
    $result
}

# ----------------------------------------------------------------------------
# Company commands
# ----------------------------------------------------------------------------

# Create a B2B-partner company.
#
# Slug must match `^[a-z][a-z0-9_-]{1,31}$` and is immutable after insert.
export def "company create" [
    slug: string                    # Partner slug (immutable)
    --name: string = ""             # Display name (defaults to slug)
    --jwt: string = ""              # Admin JWT/PAT (or $env.DAPPLE_AUTH_TOKEN)
    --api-url: string = ""          # Override DAPPLE_PROMPT_API_URL for this call
] {
    let token = (require_jwt $jwt)
    let api_base = (resolve_api_base $api_url)
    let display_name = if ($name | is-empty) { $slug } else { $name }
    let body = { slug: $slug, name: $display_name }

    log info $"POST ($api_base)/companies/"
    let resp = (http post $"($api_base)/companies/" ($body | to json) --content-type "application/json" --headers [
        Authorization $"Bearer ($token)"
    ])
    print ""
    print "=== Company Created ==="
    print ($resp | to json -i 2)
    print ""
    $resp
}

# List B2B-partner companies. By default excludes revoked rows.
export def "company list" [
    --revoked                       # Include revoked rows
    --jwt: string = ""
    --api-url: string = ""
] {
    let token = (require_jwt $jwt)
    let api_base = (resolve_api_base $api_url)
    let url = if $revoked {
        $"($api_base)/companies/?revoked=true"
    } else {
        $"($api_base)/companies/"
    }
    let resp = (http get $url --headers [Authorization $"Bearer ($token)"])
    $resp.objs
}

# Show one company including its external service tokens.
export def "company show" [
    company: string                 # Slug or UUID
    --jwt: string = ""
    --api-url: string = ""
] {
    let token = (require_jwt $jwt)
    let api_base = (resolve_api_base $api_url)
    let company_id = (resolve_company_id $company $api_base $token)
    let resp = (http get $"($api_base)/companies/($company_id)" --headers [
        Authorization $"Bearer ($token)"
    ])
    $resp.obj
}

# Revoke a company (soft-delete via `revoked_at`).
export def "company revoke" [
    company: string                 # Slug or UUID
    --reason: string = "Manual revocation"
    --jwt: string = ""
    --api-url: string = ""
] {
    let token = (require_jwt $jwt)
    let api_base = (resolve_api_base $api_url)
    let company_id = (resolve_company_id $company $api_base $token)
    let resp = (http delete $"($api_base)/companies/($company_id)" --data ({ reason: $reason } | to json) --content-type "application/json" --headers [
        Authorization $"Bearer ($token)"
    ])
    print ($resp | to json -i 2)
    $resp
}

# ----------------------------------------------------------------------------
# Token commands
# ----------------------------------------------------------------------------

# Issue a new external service token for a company.
#
# Generates the Ed25519 keypair LOCALLY. The server only sees the public
# PEM. The private key (PEM + raw 32-byte hex) prints to stdout exactly
# once — there is no recovery if you lose it.
#
# service_id must match `^[a-z][a-z0-9_]{2,31}$` (no hyphens, global-unique).
export def "token issue" [
    service_id: string              # Globally-unique service id
    --company: string               # Owning company slug or UUID (required)
    --name: string = ""             # Token display name (defaults to service_id)
    --description: string = ""      # Optional description
    --jwt: string = ""
    --api-url: string = ""
] {
    if ($company | is-empty) {
        error make { msg: "--company <slug-or-uuid> is required" }
    }
    let token = (require_jwt $jwt)
    let api_base = (resolve_api_base $api_url)
    let company_id = (resolve_company_id $company $api_base $token)

    let display_name = if ($name | is-empty) { $service_id } else { $name }

    log info "Generating Ed25519 keypair locally..."
    let kp = (generate_ed25519_keypair)

    let body = if ($description | is-empty) {
        {
            service_id: $service_id
            name: $display_name
            public_key_pem: $kp.pub_pem
        }
    } else {
        {
            service_id: $service_id
            name: $display_name
            description: $description
            public_key_pem: $kp.pub_pem
        }
    }

    log info $"POST ($api_base)/companies/($company_id)/external-service-tokens/"
    let resp = (http post $"($api_base)/companies/($company_id)/external-service-tokens/" ($body | to json) --content-type "application/json" --headers [
        Authorization $"Bearer ($token)"
    ])

    # Hash check: the server reports the PEM's SHA-256 it stored. Compare
    # to what we generated; any mismatch means proxy/encoding shenanigans.
    let server_sha = ($resp.public_key_pem_sha256 | str lowercase)
    let local_sha = ($kp.pub_pem_sha256 | str lowercase)
    let sha_match = ($server_sha == $local_sha)

    print ""
    print "==========================================================="
    print "=== External Service Token Created — STORE PRIVATE KEY  ==="
    print "==========================================================="
    print ""
    print $"  company_id:            ($company_id)"
    print $"  service_id:            ($resp.service_id)"
    print $"  token_id:              ($resp.id)"
    print $"  principal_id:          ($resp.principal_id)"
    print $"  created_at:            ($resp.created_at)"
    print ""
    print $"  public_key_pem_sha256: ($server_sha)"
    if $sha_match {
        print "    (matches locally-generated keypair)"
    } else {
        print $"  WARNING: server SHA does not match local generation \(local=($local_sha)\)"
    }
    print ""
    print "-------- PRIVATE KEY PEM \(shown ONCE; not persisted by this CLI\) --------"
    print $kp.priv_pem
    print "------------------------------------------------------------------------"
    print ""
    print "-------- PRIVATE KEY RAW \(32 bytes, hex; for SDK consumption\) --------"
    print $kp.raw_hex
    print "------------------------------------------------------------------------"
    print ""
    print "Hand off PEM + service_id to the partner over a secure channel."
    print "The partner's SDK call shape:"
    print $"  DAPPLE_EXTERNAL_API_HOST=<external-api-url> \\"
    print $"  ExternalDappleClient::external\(\"($resp.service_id)\", &priv_key_32_bytes\)"
    print ""

    {
        token_id: $resp.id
        service_id: $resp.service_id
        principal_id: $resp.principal_id
        company_id: $company_id
        priv_pem: $kp.priv_pem
        raw_hex: $kp.raw_hex
        public_key_pem_sha256: $server_sha
        sha_match: $sha_match
    }
}

# List external service tokens for a company.
export def "token list" [
    --company: string               # Slug or UUID (required)
    --jwt: string = ""
    --api-url: string = ""
] {
    if ($company | is-empty) {
        error make { msg: "--company <slug-or-uuid> is required" }
    }
    let token = (require_jwt $jwt)
    let api_base = (resolve_api_base $api_url)
    let company_id = (resolve_company_id $company $api_base $token)
    let resp = (http get $"($api_base)/companies/($company_id)/external-service-tokens/" --headers [
        Authorization $"Bearer ($token)"
    ])
    $resp.objs
}

# Revoke a single external service token (does not affect siblings on the
# same company).
export def "token revoke" [
    token_id: string                # Token UUID
    --reason: string = "Manual revocation"
    --jwt: string = ""
    --api-url: string = ""
] {
    let admin_token = (require_jwt $jwt)
    let api_base = (resolve_api_base $api_url)
    let resp = (http delete $"($api_base)/external-service-tokens/($token_id)" --data ({ reason: $reason } | to json) --content-type "application/json" --headers [
        Authorization $"Bearer ($admin_token)"
    ])
    print ($resp | to json -i 2)
    $resp
}

# ----------------------------------------------------------------------------
# Default help
# ----------------------------------------------------------------------------

export def main [] {
    print "External Service Token (B2B Partner) Management

Usage:
  external_service_token <subcommand> [args]

Company subcommands:
  company create <slug> [--name <display>]
      Create a new B2B-partner company.

  company list [--revoked]
      List companies. Default excludes revoked rows.

  company show <slug-or-uuid>
      Show one company including its issued tokens.

  company revoke <slug-or-uuid> [--reason <text>]
      Revoke a company (soft-delete).

Token subcommands:
  token issue <service_id> --company <slug-or-uuid> [--name <text>] [--description <text>]
      Generate an Ed25519 keypair LOCALLY, register the public PEM, and
      print the private key once. Server never sees the private key.

  token list --company <slug-or-uuid>
      List external service tokens registered for a company.

  token revoke <token-uuid> [--reason <text>]
      Revoke a single token without affecting siblings on the same company.

Common flags:
  --jwt <token>     Admin JWT/PAT (or set DAPPLE_AUTH_TOKEN env var).
  --api-url <url>   Override DAPPLE_PROMPT_API_URL for this invocation.

Environment Variables:
  DAPPLE_AUTH_TOKEN      Admin Bearer token (required).
  DAPPLE_PROMPT_API_URL  Internal-API base URL (required unless --api-url given).
                         Includes /api suffix (set by dapple_sandbox overlay).
                         Dev cluster: https://dapple-prompt-api.kube.dappleai.dev/api

Security notes:
  - Private keys print to stdout exactly once during `token issue`.
  - Nothing is persisted to disk by this CLI.
  - Hand off PEM + service_id to the partner over a secure channel.
"
}
