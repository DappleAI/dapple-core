# The shell profile, for every environment.
#
# One module, driven by environments.nu, selected by the DAPPLE_ENV declaration
# in ~/.dapple/env. Replaces the dapple_sandbox / dapple_prod split, where the
# environment a host belonged to was implied by which setup script last ran on
# it — and where the prod box consequently ran for weeks with every shell
# exporting the dev cluster's database URL and password.
#
# What differs between environments is data [environments.nu]. What differs in
# BEHAVIOUR is limited to two things, both real:
#
#   - source-building environments resolve sibling repo paths; prod, which
#     deploys published images, does not.
#   - sandbox turns on the infrastructure-backed test gates; prod is not a test
#     host.
#
# Everything else — file locations, precedence, the guard — is identical
# everywhere on purpose. Divergence here is what cost us an evening.

use (path self dapple_base.nu) *
use (path self environments.nu) *
use (path self env_files.nu) *
export use (path self dapple_microk8s.nu) *

export-env {
    if not (overlay list | any {|o| $o == "dapple_microk8s"}) {
        overlay use dapple_microk8s
    }

    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }

    # --- Which environment is this host? ---------------------------------
    let declared = (declared_environment $home $env)
    if ($declared | is-empty) {
        print $"(ansi red_bold)⚠  no ($ENV_KEY) declared(ansi reset)"
        print $"   add `($ENV_KEY)=sandbox` or `($ENV_KEY)=prod` to (user_env_path $home),"
        print $"   or re-run the installer: nu assets/nu/init/setup.nu --environment <name>"
    }
    let env_name = if ($declared | is-empty) { "sandbox" } else { $declared }
    let cfg = (environment $env_name)

    $env.DAPPLE_ENV = $env_name

    # --- Cluster identity -------------------------------------------------
    $env.KUBE_DAPPLE_NAMESPACE = $cfg.namespace
    $env.KUBE_HOST = $cfg.kube_host
    $env.KUBE_REGISTRY_HOST = $cfg.registry_host

    # --- Workspace + sibling repos ---------------------------------------
    # Respect a pre-set value so a deploy can run from a git worktree.
    let default_workspace = if ($cfg.workspace_rel | is-not-empty) {
        $home | path join $cfg.workspace_rel
    } else {
        # Source-building environments keep repos under an OS-conventional dir.
        let source_dir = match $nu.os-info.name {
            "macos" => ($home | path join "Source" "Dapple")
            _ => ($home | path join "src" "dapple")
        }
        $source_dir | path join "dapple-prompt"
    }
    $env.DAPPLE_PROMPT_WORKSPACE_DIR = ($env.DAPPLE_PROMPT_WORKSPACE_DIR? | default $default_workspace)

    if $env.DAPPLE_PROMPT_WORKSPACE_DIR != $default_workspace {
        let r = (do { ^git -C $env.DAPPLE_PROMPT_WORKSPACE_DIR rev-parse --abbrev-ref HEAD } | complete)
        let branch = if $r.exit_code == 0 { $r.stdout | str trim } else { "?" }
        print $"(ansi red_bold)⚠  DAPPLE_PROMPT_WORKSPACE_DIR override active(ansi reset)"
        print $"   path:   ($env.DAPPLE_PROMPT_WORKSPACE_DIR)"
        print $"   branch: (ansi cyan)($branch)(ansi reset)"
    }

    $env.DAPPLE_PROMPT_CRATES_DIR = ($env.DAPPLE_PROMPT_WORKSPACE_DIR | path join "crates")
    $env.DAPPLE_PROMPT_DATA_DIR = ($env.DAPPLE_PROMPT_CRATES_DIR | path join "dapple-data")
    $env.DAPPLE_PROMPT_API_DIR = ($env.DAPPLE_PROMPT_CRATES_DIR | path join "dapple-prompt-api")
    $env.DAPPLE_ASSETS_DIR = ($env.DAPPLE_PROMPT_WORKSPACE_DIR | path join "assets")
    $env.DAPPLE_KUBE_ASSETS_DIR = ($env.DAPPLE_ASSETS_DIR | path join "k8s")
    $env.DAPPLE_KUBE_MANIFESTS_DIR = ($env.DAPPLE_KUBE_ASSETS_DIR | path join "manifests")
    $env.DAPPLE_DOCKER_ASSETS_DIR = ($env.DAPPLE_ASSETS_DIR | path join "docker")

    if $cfg.builds_from_source {
        let siblings = ($env.DAPPLE_PROMPT_WORKSPACE_DIR | path dirname)
        $env.DAPPLE_SOURCE_DIR = $siblings
        $env.DAPPLE_GUIDES_DIR = ($siblings | path join "dapple-guides")
        $env.DAPPLE_UI_SOURCE_DIR = ($siblings | path join "dapple-ui")
        # dapple-ui-demo shares the dapple-ui monorepo [different pnpm filter].
        $env.DAPPLE_UI_DEMO_SOURCE_DIR = $env.DAPPLE_UI_SOURCE_DIR
        $env.DAPPLE_WEB_LANDING_PAGE_SOURCE_DIR = ($siblings | path join "dapple-landing-page")

        # The landing page is served on a dev-cluster NodePort; prod does not
        # run it at all. deploy_dapple_core.nu reads the port directly
        # [twice], and `dapple url dapple-web-landing-page` prints the URL.
        $env.DAPPLE_WEB_LANDING_PAGE_HOST = $cfg.kube_host
        $env.DAPPLE_WEB_LANDING_PAGE_PORT = "31082"
        $env.DAPPLE_WEB_LANDING_PAGE_URL = $"http://($cfg.kube_host):31082/api"
    }

    # --- Service addresses ------------------------------------------------
    # INTERNAL means in-cluster, and its only consumer is the dapple-ui-auth
    # secret (deployed outside this repo), read by pods. It briefly held
    # $cfg.api_base, which is an INGRESS url in sandbox — pods can resolve
    # neither the zone nor the private CA, so that silently broke every
    # server-side call dapple-ui makes. Tools that need a workstation-reachable
    # address use DAPPLE_PROMPT_API_URL below, not this.
    $env.DAPPLE_PROMPT_API_INTERNAL_URL = $cfg.internal_api_url
    # Consumers expect this one to INCLUDE /api [tools/external_service_token.nu];
    # dapple-suite's resolve_api_url trims the suffix back off, as does this
    # module's own `api_base` helper for consumers that append their own path.
    $env.DAPPLE_PROMPT_API_URL = $"($cfg.api_base)/api"
    # `dapple url dapple-ui` prints https://$DAPPLE_UI_HOST, so this is consumed
    # by tooling, not only by NEXTAUTH_URL.
    $env.DAPPLE_UI_HOST = $cfg.ui_host
    $env.NEXTAUTH_URL = $"https://($cfg.ui_host)"

    $env.DAPPLE_ONNX_CACHE_DIR = if ($cfg.onnx_dir | is-not-empty) {
        $cfg.onnx_dir
    } else {
        $home | path join ".cache" "dapple-onnx"
    }

    # --- Test gates -------------------------------------------------------
    # Sandbox RUNS the infrastructure-backed tests and FAILS loudly when the
    # infra is missing, so a regression can't hide behind a skipped test. Prod
    # is not a test host and nobody should be running cargo test against it.
    if $cfg.builds_from_source {
        $env.DAPPLE_REDIS_STRICT = ($env.DAPPLE_REDIS_STRICT? | default "1")
        $env.SUMMARIZATION_ENABLED = ($env.SUMMARIZATION_ENABLED? | default "true")
        $env.DAPPLE_BACKEND_TESTS_STRICT = ($env.DAPPLE_BACKEND_TESTS_STRICT? | default "1")
    }

    # --- Env files --------------------------------------------------------
    # load-dotenv never overwrites an already-set variable, so the FIRST file to
    # define a key wins. ONE rule, every environment: the hand-authored file
    # outranks the machine-written one.
    #
    # Uniform on purpose. An earlier version made this per-environment, which
    # contradicted this module's own premise — file locations, precedence and
    # the guard are identical everywhere so that no host's behaviour depends on
    # which script last touched it. A precedence rule you have to look up per
    # environment is the same trap in miniature.
    #
    # Hand-authored wins because it is the only file a human edits: ~/.dapple/env
    # is where an operator puts a deliberate override, and the old profile
    # documented its predecessor as "user overrides - highest priority". The
    # machine-written file is regenerated at will [env_dapple gen on a
    # workstation, the prod deploy tooling on a deployed host], so it is never the place a
    # decision is recorded.
    #
    # The one case where this could hide fresh truth is prod, where the deploy tooling
    # resolves DAPPLE_PROMPT_DATABASE_URL from the LIVE cluster into
    # generated.env. That is handled where it happens rather than by bending the
    # rule: materialize_operator_env warns when ~/.dapple/env shadows a key it
    # just resolved. (The prod deploy tooling lives outside this repository.)
    load-dotenv (user_env_path $home)
    load-dotenv (generated_env_path $home)

    # Table default LAST, so a deploy-resolved value [generated.env] and then a
    # hand-authored one both outrank it. Environments whose database URL is a
    # constant get it here; environments whose password is generated at install
    # time leave the table field null and rely on generated.env.
    if ($env.DAPPLE_PROMPT_DATABASE_URL? | is-empty) and ($cfg.database_url | is-not-empty) {
        $env.DAPPLE_PROMPT_DATABASE_URL = $cfg.database_url
    }
    $env.DAPPLE_PROMPT_INTERNAL_POSTGRES_URL = $cfg.internal_postgres_url

    $env.DATABASE_URL = ($env.DAPPLE_PROMPT_DATABASE_URL? | default "")

    # --- Guard: is this host addressing the environment it claims? --------
    #
    # The declaration above is copied along with the file it lives in — that is
    # precisely how the prod host acquired dev configuration — so verify it
    # against the addresses that actually got resolved.
    #
    # Fails SAFE, not STOP: offenders are BLANKED and named in red. The shell
    # stays usable to fix the cause, while any tool that would have written to
    # the wrong cluster now stops with "DATABASE_URL is not set" instead of
    # quietly succeeding. A warning alone would not do: this entire failure
    # class produces no error at all.
    if $env_name != "sandbox" {
        let checked = [
            DAPPLE_PROMPT_API_URL
            DAPPLE_PROMPT_API_INTERNAL_URL
            DAPPLE_PROMPT_DATABASE_URL
            DATABASE_URL
            NEXTAUTH_URL
        ]
        let offenders = ($checked | where {|name|
            let value = ($env | get -o $name | default "")
            $SANDBOX_MARKERS | any {|marker| $value | str contains $marker }
        })

        if ($offenders | is-not-empty) {
            print $"(ansi red_bold)⚠  '($env_name)' HOST ADDRESSING THE SANDBOX CLUSTER(ansi reset)"
            print "   these have been BLANKED:"
            for name in $offenders { print $"     ($name)" }
            print $"   fix (user_env_path $home) or re-run the deploy, then restart the shell."
            load-env ($offenders | reduce -f {} {|name, acc| $acc | merge {($name): ""} })
        }
    }
}
