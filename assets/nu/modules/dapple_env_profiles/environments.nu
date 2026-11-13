# The environment table.
#
# One row per environment. Adding stage/perf later is a row here and nothing
# else — that is the whole point of this file.
#
# It exists because sandbox and prod diverged by accident rather than by
# decision. The prod host was set up with the DEV workstation script, so every
# shell on it exported the dev cluster's API host and
# `postgres://dapple:ai@prompt-db.kube.dappleai.dev:5432/dapple` — the dev
# database, with the dev password. Nothing was corrupted only because that
# hostname does not resolve from prod. When "which environment am I" is implied
# by which script last touched a host, that is the failure you get.
#
# Rules for this table:
#
#   - Every field must be meaningful for EVERY environment. A field only one
#     environment uses belongs in that environment's provisioning script, not
#     here.
#   - Values are addresses and identities, never secrets. Secrets come from
#     ~/.dapple/env [hand-authored] and ~/.dapple/generated.env [machine-
#     written]; see env_files.nu.
#   - In-cluster service names, never ClusterIPs. A ClusterIP dies with its
#     Service; a name does not.

# Environment names this table knows. Order is display order.
export const ENVIRONMENT_NAMES = ["sandbox" "prod"]

# Static per-environment facts.
#
# `workspace_rel` is relative to $HOME and joined at load time, so the same row
# works for every operator account.
#
# `source_rel` is where sibling repos [dapple-ui, dapple-guides, ...] live, or
# null for environments that deploy published images and never build from a
# checkout.
#
# `onnx_dir` null means "use the per-user cache"; prod pins a hostPath because
# the conduit-onnx pod mounts that exact directory.
export const ENVIRONMENTS = {
    sandbox: {
        namespace: "dapple-sandbox"
        kube_host: "kube.dappleai.dev"
        registry_host: "registry.kube.dappleai.dev"
        # Ingress hostnames: sandbox reaches services through the dev-dns
        # wildcard, which resolves on workstations via /etc/resolver.
        api_base: "https://dapple-prompt-api.kube.dappleai.dev"
        # Where a POD reaches the API. Never the ingress: cluster CoreDNS has no
        # kube.dappleai.dev zone and no CA is mounted into the web pods, so the
        # ingress URL fails twice over from inside. Short Service name is enough
        # here because nothing on the sandbox workstation reads this field.
        internal_api_url: "http://dapple-prompt-api:8001"
        ui_host: "dapple-ui.kube.dappleai.dev"
        workspace_rel: null          # OS-dependent; resolved in the profile
        source_rel: null             # ditto
        onnx_dir: null
        builds_from_source: true
        # Sandbox's database credentials are fixed, non-secret, and already in
        # this repo's manifests. Prod's password is generated at install time
        # and lives in the prompt-db secret, so prod leaves this null and takes
        # the value from generated.env instead.
        database_url: "postgres://dapple:ai@prompt-db.kube.dappleai.dev:5432/dapple"
        internal_postgres_url: "postgres://dapple:ai@prompt-db:5432/dapple"
    }
    prod: {
        namespace: "dapple-sandbox"  # same NAME, different cluster
        kube_host: "demo.example.com"   # placeholder: set to your production hostname
        registry_host: ""            # pulls from ghcr.io; no cluster registry
        # Prod exposes exactly one ingress [the demo UI]. The API has no public
        # address at all, so it is reached by in-cluster DNS, which the node can
        # resolve because install points systemd-resolved at CoreDNS.
        api_base: "http://dapple-prompt-api.dapple-sandbox.svc.cluster.local:8001"
        # FQDN, not the short name, for the same reason internal_postgres_url is:
        # the node's drop-in is `Domains=~cluster.local`, a ROUTING domain with
        # no search list, so a bare `dapple-prompt-api` resolves in a pod and
        # fails in a prod shell. Equal to api_base today only because prod
        # publishes no API ingress — if that ever changes, api_base moves and
        # this field must NOT follow it.
        internal_api_url: "http://dapple-prompt-api.dapple-sandbox.svc.cluster.local:8001"
        ui_host: "demo.example.com"     # placeholder: set to your production hostname
        workspace_rel: "dapple-prompt"
        source_rel: null
        onnx_dir: "/var/dapple-prompt/onnx-models"
        builds_from_source: false
        # null: the password is generated at install time and lives only in the
        # prompt-db secret. The prod deploy tooling resolves it into generated.env.
        database_url: null
        internal_postgres_url: "postgres://prompt-db.dapple-sandbox.svc.cluster.local:5432/dapple"
    }
}

# Look up one environment, failing with the valid names rather than a nu
# cell-path error.
export def environment [name: string]: nothing -> record {
    let known = ($ENVIRONMENTS | columns)
    if not ($name in $known) {
        error make {
            msg: $"unknown environment '($name)' — known: ($known | str join ', '). Set DAPPLE_ENV in ~/.dapple/env."
        }
    }
    $ENVIRONMENTS | get $name
}

# The API address WITHOUT the /api suffix.
#
# DAPPLE_PROMPT_API_URL carries /api because dapple-suite's resolve_api_url and
# external_service_token.nu expect it there. Consumers that append their own
# /api path — `service_token verify`, `dapple ui env` writing dapple-ui's
# .env.local — need the bare base, and this is the one place that knows how to
# get from one to the other. Trailing slash tolerated; only a whole final path
# segment matches, so `/apiary` is left alone.
export def api_base [url: string]: nothing -> string {
    $url | str replace -r '/api/?$' ''
}

# Hostname fragments that mean "this is the sandbox cluster".
#
# Used by the profile's guard to catch a host addressing the wrong environment,
# which is the specific accident this whole table exists to prevent.
export const SANDBOX_MARKERS = ["kube.dappleai.dev"]
