#!/usr/bin/nu

use std/log
use std/assert

$env.NU_LOG_LEVEL = "debug"

use ../modules/dapple_env_profiles dapple_sandbox_tools
overlay use dapple_sandbox_tools as dapple_env

use ../modules/k8s
use k8s deploy
use k8s rollout_status
use k8s rollout_restart
use k8s setup_docker_registry_cert
use k8s ensure_vm_mounts

use ../modules/tools/service_token.nu [ensure]
use ../modules/tools/env_dapple.nu
use ../modules/tools/gpu.nu

# Where env_dapple writes, and how to parse it back. Both are env_files.nu's to
# say: the two secret-loading defs below called `env_dapple gen` [which writes
# ~/.dapple/generated.env] and then read a hardcoded ~/.env.dapple, so on a host
# with no legacy file the reload was a no-op and the signing keys came back
# empty — "not set after generation", `return`, no secret created, deploy
# carries on.
use ../modules/dapple_env_profiles/env_files.nu [generated_env_path read_pairs]

# Shared reclaim helpers (containerd byte size, state file r/w) — also used
# by reclaim_disk.nu so both files agree on "how big is containerd?" and
# the deploys-since-reset counter.
use ../modules/dapple/reclaim.nu

# OrbStack VM abstraction — used by the auto-reclaim plan to invoke
# `microk8s ctr` content prune inside the VM on macOS. On Linux the
# `_ctr_content_prune` helper returns early without invoking `orb` at all
# (microk8s runs natively as a snap; no VM layer to enter), so this `use`
# is only consequential on macOS.
use ../modules/orb

# Captured at parse time so `_check_installed_modules_fresh` can locate the
# repo source modules dir from the script's own filesystem location, without
# relying on runtime env vars (which the check needs to fire before any other
# setup). `path self` is only available at top-level — it cannot be called
# from inside a `def` body.
const _SCRIPT_PATH = path self

# Compare repo-source nushell modules under assets/nu/modules/ against the
# installed copies under $nu.default-config-dir/modules/. If any source file
# is newer than its installed counterpart (or missing entirely from the
# install), abort with a clear pointer at `setup.nu` instead of letting
# a stale `use … reclaim.nu` (etc.) surface as "Command `reclaim` not found"
# later in the run.
#
# The chicken-and-egg: this script's own `use` statements at the top of the
# file load the stale module copies at parse time, so we cannot DETECT
# staleness from inside a module that was just loaded. We have to do the
# freshness check from this script, against the filesystem, before invoking
# any imported function — hence the call site at the very first line of
# `main`.
def _check_installed_modules_fresh [] {
    # Walk up from <repo>/assets/nu/k8s/deploy_dapple_core.nu to the
    # repo root via four `path dirname` operations, then join the standard
    # modules subpath. Best-effort: if the script is being run from a copy
    # outside the repo and the source dir doesn't exist, silently return.
    let script_dir = ($_SCRIPT_PATH | path dirname)
    let repo_root = ($script_dir | path dirname | path dirname | path dirname)
    let src_dir = ($repo_root | path join 'assets' | path join 'nu' | path join 'modules')
    if not ($src_dir | path exists) { return }

    let dst_dir = ($nu.default-config-dir | path join 'modules')
    if not ($dst_dir | path exists) {
        # Fresh checkout that never ran the installer. The existing `use`
        # statements above will already have failed loudly with "module not
        # found", so there's nothing for this check to add.
        return
    }

    let src_files = (glob $"($src_dir)/**/*.nu")
    mut stale = []
    for src_file in $src_files {
        let rel = ($src_file | str replace --regex $"^($src_dir)/" "")
        let dst_file = ($dst_dir | path join $rel)
        if not ($dst_file | path exists) {
            $stale = ($stale | append { rel: $rel, reason: "missing" })
            continue
        }
        let src_mtime = (ls $src_file | get 0.modified)
        let dst_mtime = (ls $dst_file | get 0.modified)
        if $src_mtime > $dst_mtime {
            $stale = ($stale | append { rel: $rel, reason: "outdated" })
        }
    }

    if ($stale | is-empty) { return }

    # A WARNING, not an error. This script imports its modules from the repo
    # checkout now, so a stale install cannot affect this run — it only affects
    # `dapple ...` commands in an interactive shell, which still load the
    # installed copy via the conf.d fragment. Blocking a deploy on a condition
    # the deploy is immune to was pure friction.
    log warning "installed nushell modules are out of date with repo source"
    for entry in $stale {
        log warning $"  - ($entry.rel) [($entry.reason)]"
    }
    log warning "this run is unaffected [modules load from the checkout]; interactive `dapple` commands are stale until:"
    log warning "  nu assets/nu/init/setup.nu --environment sandbox"
}

# Available services: api, external-api, conduit, conduit-onnx, conduit-llm, conduit-oximedia (opt-in via DAPPLE_CONDUIT_OXIMEDIA_ENABLED), conduit-media-gateway (opt-in via DAPPLE_MEDIA_GATEWAY_ENABLED), flow, rust-docs
#
# Default mode: hostPath (mounts local target/ into pod — no Docker build needed).
# In hostPath mode the deploy compiles each service itself (cargo build on Linux,
# cargo zigbuild on macOS) so the mounted binary always matches the source tree —
# pass --no-build to mount whatever is already in target/.
# Use --docker to build and push Docker images instead (requires clean git tree)
def main [
    --force,
    --docker: string = "",   # Build Docker image instead of hostPath. Value is cargo profile (e.g., --docker dev)
    --svc: string = "",      # Deploy only these services, comma-separated (e.g., --svc rust-docs,api)
    --allow-dirty            # Allow Docker builds from a dirty working tree (e.g. when deploying from a worktree). Image will be tagged with the committed content_hash but contain uncommitted code, so subsequent clean deploys with the same HEAD will reuse it — commit and re-tag once validated.
    --no-build               # hostPath mode only: skip the cargo build and mount the binary already in target/. Use when you just built by hand, or to redeploy a known-good artifact. Ignored by --docker (Docker builds are never skippable).
] {
    # Guard against the recurring papercut where a PR adds a new exported
    # function to a repo-source module (e.g. `reclaim orbstack_img_bytes`)
    # but the installed copy under $nu.default-config-dir/modules/ is stale,
    # making the new function present itself as "Command not found" at the
    # call site instead of "module out of date". Fires before any imported
    # function is invoked, so the operator sees an actionable pointer at
    # setup.nu instead of an opaque resolver error.
    _check_installed_modules_fresh

    let svc_list = if ($svc | is-empty) { [] } else { $svc | split row "," | each { str trim } }
    let deploy_all = ($svc_list | is-empty)

    # Whether the user explicitly asked for Docker mode. When false, we default
    # to hostPath; Mac-only services that can't cross-compile (conduit family)
    # are auto-promoted to Docker per-service in resolve_use_docker.
    let user_wants_docker = ($docker | is-not-empty)
    let docker_profile = if $user_wants_docker { $docker } else { "dev" }
    let is_mac = ($nu.os-info.name | str contains "mac")

    # Services that pull third-party native deps (mistralrs, hf-hub, rig-core,
    # ort) and therefore can't cross-compile to aarch64-linux from a Mac host.
    # These auto-promote to Docker mode on Mac.
    let docker_only_on_mac = ["conduit" "conduit-onnx" "conduit-llm"]

    # Services that DO cross-compile cleanly on Mac and run via hostPath. The
    # complement of docker_only_on_mac among the deployable Rust services.
    let cross_compile_on_mac = ["api" "external-api" "flow"]

    # True if at least one service in this run will need the macOS cross-compile
    # toolchain. Without this gate, `--svc conduit` on Mac would fail the
    # prereq check even though conduit auto-promotes to Docker.
    let any_cross_compile_service = (
        $is_mac
        and (not $user_wants_docker)
        and (
            $deploy_all
            or ($svc_list | any {|s| $s in $cross_compile_on_mac })
        )
    )

    # On macOS, hostPath binaries are produced by cargo zigbuild targeting
    # aarch64-unknown-linux-gnu. Verify the toolchain is available up front so
    # we fail loudly with install instructions instead of dying mid-deploy on
    # a missing zig binary. Skip the check when no service in this run will
    # actually invoke zigbuild (e.g. --docker dev, or --svc conduit on Mac
    # which auto-promotes to Docker).
    if $any_cross_compile_service {
        ensure_cross_compile_prereqs
    }

    # True if at least one service in this run will push a Docker image. Used
    # to gate registry-cert setup so first-time Mac deploys don't fail to push
    # the conduit family due to missing registry trust.
    let any_docker_service = (
        $user_wants_docker
        or ($is_mac and (
            $deploy_all
            or ($svc_list | any {|s| $s in $docker_only_on_mac })
        ))
    )

    # Helper to check if a service should be deployed
    let should_deploy = {|name: string|
        $deploy_all or ($name in $svc_list)
    }

    log info "starting dapple-prompt deployments"

    # Skip setup steps when deploying specific services only
    if $deploy_all {
        #TODO: check for cargo and do rustup if you can't

        # Before ANY registry call. ensure_prompt_db_image below does a
        # manifest HEAD against registry.kube.dappleai.dev, so a rotated CA
        # fails HERE — earlier than the setup_docker_registry_cert call further
        # down, which is why that one never got the chance to fix it. The call
        # is idempotent, so running it in both places is free.
        setup_docker_registry_cert $env.KUBE_REGISTRY_HOST $env.KUBE_ROOT_CA_CERT_NAME

        # Ensure prompt-db image is built and available in microk8s
        log info "ensuring prompt-db image (PG18 + PostGIS + pgvector)"
        ensure_prompt_db_image

        log info "applying prompt-db manifest"
        kubectl apply -n $env.KUBE_DAPPLE_NAMESPACE -f $"($env.DAPPLE_PROMPT_WORKSPACE_DIR)/assets/k8s/registry/prompt-db.yaml"

        log debug "ensuring sqlx-cli ^0.9 exists"
        # Pin to ^0.9 so the CLI tracks the workspace's sqlx 0.9 library / .sqlx
        # cache format and can't float to a future 0.10. `cargo install` upgrades
        # a mismatched install in place. Do NOT add --locked (broken for sqlx-cli
        # as of 0.9).
        cargo install sqlx-cli --version '^0.9' --features native-tls,postgres,sqlite

        log info "running postgres migrations"
        cd $env.DAPPLE_PROMPT_DATA_DIR
        log debug $"running database creation against ($env.DAPPLE_PROMPT_DATABASE_URL)"
        sqlx database create --database-url $env.DAPPLE_PROMPT_DATABASE_URL
        log debug $"running database migrations against ($env.DAPPLE_PROMPT_DATABASE_URL)"
        sqlx migrate run --database-url $env.DAPPLE_PROMPT_DATABASE_URL
        cd $env.DAPPLE_PROMPT_WORKSPACE_DIR

        # Ensure service tokens are registered for web services
        log info "ensuring service tokens for web services"
        ensure_service_tokens

        # NOTE: the deploy intentionally does NOT regenerate the sqlx offline
        # cache. Writing it here dirtied the working tree on every deploy — a
        # `prepare` against the dev's local Postgres re-flips `nullable` on a
        # few CTE queries vs the committed cache (PG-version drift). The
        # committed `.sqlx/` is the source of truth for offline builds
        # (Docker / Mac cross-compile set SQLX_OFFLINE=true); Linux hostPath
        # builds compile against the live DB and don't use the cache at all.
        # If you changed a query/migration, regenerate + commit the cache
        # yourself with `dapple sqlx prepare` (it runs `--workspace --
        # --all-targets`) — the step every query checklist already calls for.
        # A stale cache surfaces as a clear sqlx compile error pointing at that
        # command, never a silent deploy-time rewrite.

        if $any_docker_service {
            # Ensure Docker registry certificate is configured before pushing images
            log info "ensuring Docker registry certificate is configured"
            setup_docker_registry_cert $env.KUBE_REGISTRY_HOST $env.KUBE_ROOT_CA_CERT_NAME
        }

        # Create/update k8s secret for auth config
        log info "setting up auth secret"
        setup_auth_secret

        # Create/update k8s secret for the external-api signing key
        # (separate from the internal auth secret — see setup_external_auth_secret).
        log info "setting up external-api auth secret"
        setup_external_auth_secret

        # Create/update k8s secret for LLM API keys
        log info "setting up LLM secret"
        setup_llm_secret

        # Create/update k8s secret for internal sub-service auth
        log info "setting up conduit internal auth secret"
        setup_internal_auth_secret

        # Create/update k8s secret for database connection
        log info "setting up database secret"
        kubectl apply -n $env.KUBE_DAPPLE_NAMESPACE -f $"($env.DAPPLE_PROMPT_WORKSPACE_DIR)/assets/k8s/registry/prompt-db-secret.yaml"
    } else {
        log info $"deploying specific services: ($svc | str join ', ')"
        # Minimal setup for targeted deploys
        cd $env.DAPPLE_PROMPT_WORKSPACE_DIR
        if $any_docker_service {
            setup_docker_registry_cert $env.KUBE_REGISTRY_HOST $env.KUBE_ROOT_CA_CERT_NAME
        }
        # Every service manifest mounts the prompt-db secret (DAPPLE_PROMPT_DATABASE_URL).
        # Ensure it exists even when --svc narrows the deploy; kubectl apply is idempotent.
        log info "ensuring database secret"
        kubectl apply -n $env.KUBE_DAPPLE_NAMESPACE -f $"($env.DAPPLE_PROMPT_WORKSPACE_DIR)/assets/k8s/registry/prompt-db-secret.yaml"
    }

    # ensure_vm_mounts is a no-op on Linux and (post-OrbStack) on macOS, so it's
    # cheap to call unconditionally — and the previous gating on $use_docker was
    # incorrect now that hostPath / Docker is decided per-service.
    ensure_vm_mounts [$env.DAPPLE_PROMPT_WORKSPACE_DIR]

    if (do $should_deploy "api") {
        # Ensure auth secret exists (API needs signing key)
        setup_auth_secret

        let svc_docker = (resolve_use_docker $user_wants_docker $is_mac true "api")
        let svc_hostpath = if $svc_docker { "" } else { "debug" }

        setup_dapple_svc {
          name: "dapple-prompt-api",
          profile: (if $svc_docker { $docker_profile } else { "dev" })
          namespace: $env.KUBE_DAPPLE_NAMESPACE
          source_path: $env.DAPPLE_PROMPT_WORKSPACE_DIR
          template: dapple-prompt-service.yaml
          port: "8001"
          node_port: $env.DAPPLE_WEB_LANDING_PAGE_PORT
        } --force=$force --hostpath=$svc_hostpath --docker=$svc_docker --allow-dirty=$allow_dirty --no-build=$no_build
    }

    if (do $should_deploy "external-api") {
        # Customer-facing surface. Separate signing-key secret from the
        # internal API — see setup_external_auth_secret.
        setup_external_auth_secret

        let svc_docker = (resolve_use_docker $user_wants_docker $is_mac true "external-api")
        let svc_hostpath = if $svc_docker { "" } else { "debug" }

        setup_dapple_svc {
          name: "dapple-prompt-external-api",
          profile: (if $svc_docker { $docker_profile } else { "dev" })
          namespace: $env.KUBE_DAPPLE_NAMESPACE
          source_path: $env.DAPPLE_PROMPT_WORKSPACE_DIR
          template: dapple-prompt-service.yaml
          port: "8081"
          node_port: $env.DAPPLE_WEB_LANDING_PAGE_PORT
        } --force=$force --hostpath=$svc_hostpath --docker=$svc_docker --allow-dirty=$allow_dirty --no-build=$no_build
    }

    if (do $should_deploy "conduit") {
        # Conduit is now a lightweight orchestrator — ONNX inference and local LLM
        # are handled by conduit-onnx and conduit-llm sub-services.
        # Conduit only needs the LLM secret for the Anthropic API key.
        setup_llm_secret
        setup_internal_auth_secret

        let svc_docker = (resolve_use_docker $user_wants_docker $is_mac false "conduit")
        let svc_hostpath = if $svc_docker { "" } else { "debug" }

        setup_dapple_svc {
          name: "dapple-prompt-conduit"
          profile: (if $svc_docker { $docker_profile } else { "dev" })
          namespace: $env.KUBE_DAPPLE_NAMESPACE
          source_path: $env.DAPPLE_PROMPT_WORKSPACE_DIR
          template: dapple-prompt-service.yaml
          port: "8081"
          node_port: $env.DAPPLE_WEB_LANDING_PAGE_PORT
        } --force=$force --hostpath=$svc_hostpath --docker=$svc_docker --allow-dirty=$allow_dirty --no-build=$no_build
    }

    if (do $should_deploy "conduit-onnx") {
        # Ensure internal auth secret exists
        setup_internal_auth_secret

        # Resolve ONNX cache directory
        let onnx_cache_dir = $env.DAPPLE_ONNX_CACHE_DIR

        # Ensure cache directories are mounted into MicroK8s VM (macOS only)
        ensure_vm_mounts [$onnx_cache_dir]

        # Ensure ONNX models are downloaded
        ensure_onnx_models $onnx_cache_dir

        # Detect GPU for ONNX Runtime acceleration
        let gpu = (gpu detect)
        log info $"Detected GPU for conduit-onnx: ($gpu.type) arch=($gpu.arch) EP=($gpu.onnx_ep)"

        if ($gpu.type in ["amd", "nvidia"]) {
            log info "Ensuring GPU device plugin is installed"
            gpu ensure-plugin $gpu
        }

        let svc_docker = (resolve_use_docker $user_wants_docker $is_mac false "conduit-onnx")
        let svc_hostpath = if $svc_docker { "" } else { "debug" }

        setup_dapple_svc {
          name: "conduit-onnx"
          profile: (if $svc_docker { $docker_profile } else { "dev" })
          namespace: $env.KUBE_DAPPLE_NAMESPACE
          source_path: $env.DAPPLE_PROMPT_WORKSPACE_DIR
          template: dapple-prompt-service.yaml
          port: "8083"
          node_port: $env.DAPPLE_WEB_LANDING_PAGE_PORT
          gpu: $gpu
          onnx_cache_dir: $onnx_cache_dir
        } --force=$force --hostpath=$svc_hostpath --docker=$svc_docker --allow-dirty=$allow_dirty --no-build=$no_build
    }

    # conduit-oximedia — OxiMedia media sub-service (audio song-map analysis +
    # OTIO timeline compose / WAV mix). Kept out of the default deploy_all so
    # contributors who don't need it never build the young OxiMedia deps or run
    # the pod. Two ways to get it:
    #   * set DAPPLE_CONDUIT_OXIMEDIA_ENABLED=true (e.g. in ~/.env) → included in
    #     a full deploy_all; OR
    #   * request it explicitly with `--svc conduit-oximedia` — an explicit
    #     request overrides the opt-in flag, so it is never a silent no-op.
    # Its endpoints exist only with the `oximedia` cargo feature; setup_dapple_svc
    # builds it with that feature in BOTH hostPath (cargo build --features) and
    # Docker (the FEATURES build-arg) mode.
    let oximedia_enabled = (
        (($env | get -o DAPPLE_CONDUIT_OXIMEDIA_ENABLED | default "false") | str lowercase) == "true"
    )
    # deploy_all is gated on the flag; an explicit `--svc conduit-oximedia`
    # (not deploy_all) always deploys.
    if (do $should_deploy "conduit-oximedia") and ($oximedia_enabled or (not $deploy_all)) {
        setup_internal_auth_secret

        let svc_docker = (resolve_use_docker $user_wants_docker $is_mac false "conduit-oximedia")
        let svc_hostpath = if $svc_docker { "" } else { "debug" }

        setup_dapple_svc {
          name: "conduit-oximedia"
          profile: (if $svc_docker { $docker_profile } else { "dev" })
          namespace: $env.KUBE_DAPPLE_NAMESPACE
          source_path: $env.DAPPLE_PROMPT_WORKSPACE_DIR
          template: dapple-prompt-service.yaml
          port: "8085"
          node_port: $env.DAPPLE_WEB_LANDING_PAGE_PORT
          features: "oximedia"
        } --force=$force --hostpath=$svc_hostpath --docker=$svc_docker --allow-dirty=$allow_dirty --no-build=$no_build
    }

    # conduit-media-gateway — opt-in Twitch VOD → learning-highlight service.
    # Docker-ONLY: native whisper.cpp + bundled ffmpeg/yt-dlp binaries + a baked
    # whisper model can neither cross-compile nor run in hostPath mode. Two ways to
    # get it (mirrors conduit-oximedia):
    #   * set DAPPLE_MEDIA_GATEWAY_ENABLED=true (e.g. in ~/.env) → included in a
    #     full deploy_all; OR
    #   * request it explicitly with `--svc conduit-media-gateway`.
    let media_gateway_enabled = (
        (($env | get -o DAPPLE_MEDIA_GATEWAY_ENABLED | default "false") | str lowercase) == "true"
    )
    if (do $should_deploy "conduit-media-gateway") and ($media_gateway_enabled or (not $deploy_all)) {
        setup_internal_auth_secret
        setup_media_gateway_secret

        setup_dapple_svc {
          name: "conduit-media-gateway"
          profile: $docker_profile
          namespace: $env.KUBE_DAPPLE_NAMESPACE
          source_path: $env.DAPPLE_PROMPT_WORKSPACE_DIR
          template: dapple-prompt-service.yaml
          port: "8086"
          node_port: $env.DAPPLE_WEB_LANDING_PAGE_PORT
          features: "media-gateway"
        } --force=$force --hostpath="" --docker=true --allow-dirty=$allow_dirty --no-build=$no_build
    }

    if (do $should_deploy "conduit-llm") {
        # Ensure internal auth secret and LLM keys exist
        setup_internal_auth_secret
        setup_llm_secret

        let hf_cache_dir = ($env | get -o HF_HUB_CACHE | default ($env.HOME | path join ".cache" | path join "huggingface"))
        ensure_vm_mounts [$hf_cache_dir]

        # Pre-download local LLM model (Linux only — macOS VMs have limited resources)
        let is_linux_host = ($nu.os-info.name == "linux")
        if $is_linux_host {
            let mistralrs_model_id = ($env | get -o MISTRALRS_MODEL_ID | default "")
            let hf_token = ($env | get -o HF_TOKEN | default "")
            if ($mistralrs_model_id | is-not-empty) or ($hf_token | is-not-empty) {
                prefetch_mistral_model
            } else {
                log warning "Neither MISTRALRS_MODEL_ID nor HF_TOKEN set — skipping local LLM model prefetch"
            }
        } else {
            log info "macOS host detected — skipping Mistral model prefetch"
        }

        let svc_docker = (resolve_use_docker $user_wants_docker $is_mac false "conduit-llm")
        let svc_hostpath = if $svc_docker { "" } else { "debug" }

        setup_dapple_svc {
          name: "conduit-llm"
          profile: (if $svc_docker { $docker_profile } else { "dev" })
          namespace: $env.KUBE_DAPPLE_NAMESPACE
          source_path: $env.DAPPLE_PROMPT_WORKSPACE_DIR
          template: dapple-prompt-service.yaml
          port: "8084"
          node_port: $env.DAPPLE_WEB_LANDING_PAGE_PORT
          hf_cache_dir: $hf_cache_dir
          mistral_enabled: $is_linux_host
        } --force=$force --hostpath=$svc_hostpath --docker=$svc_docker --allow-dirty=$allow_dirty --no-build=$no_build
    }

    if (do $should_deploy "flow") {
        let svc_docker = (resolve_use_docker $user_wants_docker $is_mac true "flow")
        let svc_hostpath = if $svc_docker { "" } else { "debug" }

        setup_dapple_svc {
          name: "dapple-prompt-flow"
          profile: (if $svc_docker { $docker_profile } else { "dev" })
          namespace: $env.KUBE_DAPPLE_NAMESPACE
          source_path: $env.DAPPLE_PROMPT_WORKSPACE_DIR
          template: dapple-prompt-service.yaml
          port: "8082"
        } --force=$force --hostpath=$svc_hostpath --docker=$svc_docker --allow-dirty=$allow_dirty --no-build=$no_build
    }

    if (do $should_deploy "rust-docs") {
        setup_rust_docs {
            name: "rust-docs"
            namespace: $env.KUBE_DAPPLE_NAMESPACE
            source_path: $env.DAPPLE_PROMPT_WORKSPACE_DIR
        } --force=$force
    }

    # The in-cluster microk8s registry never garbage-collects orphaned blobs:
    # every --docker deploy pushes :dapple-latest, stranding the previous
    # layers until the node crosses kubelet's image-GC watermark and cascades.
    # Run the SAFE reclaim plan (registry GC + ctr content prune) after a
    # full deploy_all, and WARN — never auto-execute — for the destructive
    # containerd reset if a heuristic (real bytes via `du -sb`, or
    # deploys-since-reset counter) trips. Targeted --svc runs skip the cost.
    if $deploy_all {
        _run_reclaim_plan
    }
}

# Create/update k8s secret for auth configuration
def setup_auth_secret [] {
    let secret_name = "dapple-prompt-auth"
    let namespace = $env.KUBE_DAPPLE_NAMESPACE

    # Generate/refresh the managed secrets file
    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    let dapple_env_file = (generated_env_path $home)
    log info $"generating ($dapple_env_file) from env.template"
    env_dapple gen

    # Reload generated secrets into current environment.
    # Note: load-env inside each/pipeline doesn't persist, so collect first then load
    if ($dapple_env_file | path exists) {
        load-env (read_pairs $dapple_env_file)
    }

    # Read secrets from environment (now includes the generated values)
    let signing_key = ($env | get -o DAPPLE_PROMPT_API_SIGNING_KEY | default "")
    if ($signing_key | is-empty) {
        log error "DAPPLE_PROMPT_API_SIGNING_KEY not set after generation"
        return
    }

    let allowed_signers_env = ($env | get -o AUTH_TOKEN_ALLOWED_SIGNERS | default "")

    # Auto-derive dapple-ui's Ethereum address from private key if set
    # This ensures the web app can exchange signed tokens for JWTs
    let ui_eth_key = ($env | get -o DAPPLE_UI_ETH_PRIVATE_KEY | default "")
    let allowed_signers = if ($ui_eth_key | is-not-empty) {
        # Derive address using cast (from Foundry)
        let ui_address = (do { cast wallet address --private-key $ui_eth_key } | complete)
        if $ui_address.exit_code == 0 {
            let addr = ($ui_address.stdout | str trim)
            log info $"Auto-derived dapple-ui Ethereum address: ($addr)"
            # Merge with any manually specified signers
            if ($allowed_signers_env | is-empty) {
                $addr
            } else {
                $"($allowed_signers_env),($addr)"
            }
        } else {
            log warning "Failed to derive address from DAPPLE_UI_ETH_PRIVATE_KEY - is 'cast' installed?"
            $allowed_signers_env
        }
    } else {
        $allowed_signers_env
    }

    if ($allowed_signers | is-empty) {
        log warning "AUTH_TOKEN_ALLOWED_SIGNERS is empty - no services can exchange signed tokens for JWTs"
    }

    # Delete existing secret if present (ignore errors if it doesn't exist)
    log debug $"removing existing secret ($secret_name) if present"
    do { kubectl delete secret $secret_name -n $namespace } | ignore

    # Create the secret
    log debug $"creating secret ($secret_name) in namespace ($namespace)"
    kubectl create secret generic $secret_name -n $namespace $"--from-literal=DAPPLE_PROMPT_API_SIGNING_KEY=($signing_key)" $"--from-literal=AUTH_TOKEN_ALLOWED_SIGNERS=($allowed_signers)"

    log info $"auth secret ($secret_name) created successfully"
}

# Create/update k8s secret holding the external-API JWT signing key.
#
# Intentionally a SEPARATE secret from `dapple-prompt-auth` so the two
# surfaces can never accidentally share a signing key. A JWT minted under
# DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY must fail signature verification
# on the internal surface — that's the cryptographic-isolation guarantee
# the external surface relies on. Same auto-generation flow as
# setup_auth_secret: env_dapple gen creates the key on first run and
# persists it to ~/.dapple/generated.env.
def setup_external_auth_secret [] {
    let secret_name = "dapple-prompt-external-auth"
    let namespace = $env.KUBE_DAPPLE_NAMESPACE

    # env_dapple gen + reload was already done by setup_auth_secret in
    # deploy_all, but for targeted `--svc external-api` deploys this is the
    # first time we touch env_dapple. Calling it again is idempotent.
    let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
    let dapple_env_file = (generated_env_path $home)
    log debug $"ensuring ($dapple_env_file) exists for external-api signing key"
    env_dapple gen

    if ($dapple_env_file | path exists) {
        load-env (read_pairs $dapple_env_file)
    }

    let signing_key = ($env | get -o DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY | default "")
    if ($signing_key | is-empty) {
        log error "DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY not set after generation"
        return
    }

    let api_signing_key = ($env | get -o DAPPLE_PROMPT_API_SIGNING_KEY | default "")
    if ($signing_key == $api_signing_key) {
        log error "DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY must NOT equal DAPPLE_PROMPT_API_SIGNING_KEY — refusing to create the external secret"
        log error $"Edit ($dapple_env_file) and assign distinct values [or delete the line and re-run to auto-regenerate]"
        return
    }

    log debug $"removing existing secret ($secret_name) if present"
    do { kubectl delete secret $secret_name -n $namespace } | ignore

    log debug $"creating secret ($secret_name) in namespace ($namespace)"
    kubectl create secret generic $secret_name -n $namespace $"--from-literal=DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY=($signing_key)"

    log info $"external-api auth secret ($secret_name) created successfully"
}

# Create/update k8s secret for LLM API keys (Anthropic, HuggingFace)
# Create/update k8s secret for conduit internal sub-service auth.
# Generates a random key if CONDUIT_INTERNAL_KEY is not already set in the environment.
def setup_internal_auth_secret [] {
    let secret_name = "conduit-internal-auth"
    let namespace = $env.KUBE_DAPPLE_NAMESPACE

    # 1. Check env override — explicit key takes precedence
    let env_key = ($env | get -o CONDUIT_INTERNAL_KEY | default "")
    if ($env_key | is-not-empty) {
        log info "Using CONDUIT_INTERNAL_KEY from environment"
        kubectl create secret generic $secret_name -n $namespace $"--from-literal=internal-key=($env_key)" --dry-run=client -o yaml | kubectl apply -f -
        return
    }

    # 2. Check if secret already exists in K8s — reuse it
    let existing = (do { kubectl get secret $secret_name -n $namespace -o "jsonpath={.data.internal-key}" } | complete)
    if $existing.exit_code == 0 and ($existing.stdout | str trim | is-not-empty) {
        log info $"Internal auth secret ($secret_name) already exists, reusing"
        return
    }

    # 3. First time — generate and create
    let generated = (random chars -l 64)
    log info "Generated new CONDUIT_INTERNAL_KEY"
    kubectl create secret generic $secret_name -n $namespace $"--from-literal=internal-key=($generated)" --dry-run=client -o yaml | kubectl apply -f -
    log info $"Internal auth secret ($secret_name) created"
}

# Twitch Helix app credentials for conduit-media-gateway. Best-effort: created
# from TWITCH_CLIENT_ID / TWITCH_CLIENT_SECRET (e.g. from ~/.env) when set; when
# both are unset an existing secret is left as-is and a warning is logged (Helix
# is required at job time, so the pipeline fails loud until they are provided).
def setup_media_gateway_secret [] {
    let secret_name = "media-gateway-twitch"
    let namespace = $env.KUBE_DAPPLE_NAMESPACE
    let client_id = ($env | get -o TWITCH_CLIENT_ID | default "")
    let client_secret = ($env | get -o TWITCH_CLIENT_SECRET | default "")

    if ($client_id | is-empty) and ($client_secret | is-empty) {
        let existing = (do { kubectl get secret $secret_name -n $namespace } | complete)
        if $existing.exit_code != 0 {
            log warning "TWITCH_CLIENT_ID / TWITCH_CLIENT_SECRET unset — conduit-media-gateway will fail at job time until you set them (register a Twitch app at dev.twitch.tv)."
        }
        return
    }

    log info $"Creating ($secret_name) from TWITCH_CLIENT_ID/SECRET"
    kubectl create secret generic $secret_name -n $namespace $"--from-literal=client-id=($client_id)" $"--from-literal=client-secret=($client_secret)" --dry-run=client -o yaml | kubectl apply -f -
}

def setup_llm_secret [] {
    let secret_name = "dapple-llm-keys"
    let namespace = $env.KUBE_DAPPLE_NAMESPACE

    # All keys come from ~/.env -> ~/.env.dapple (loaded by setup_auth_secret)
    let anthropic_key = ($env | get -o ANTHROPIC_API_KEY | default "")
    let hf_token = ($env | get -o HF_TOKEN | default "")
    let openai_key = ($env | get -o OPENAI_API_KEY | default "")
    let fal_key = ($env | get -o FAL_AUTH_KEY | default "")

    if ($anthropic_key | is-empty) {
        log info "ANTHROPIC_API_KEY not set — Claude backend will not be available"
    }
    if ($hf_token | is-empty) {
        log info "HF_TOKEN not set — Mistral local backend will not be available"
    }
    if ($openai_key | is-empty) {
        log info "OPENAI_API_KEY not set — image generation [gpt-image] will not be available"
    }
    if ($fal_key | is-empty) {
        log info "FAL_AUTH_KEY not set — Fal image/video backends will not be available [provider-less animate/imagine fall back to the stub]"
    }

    if ($anthropic_key | is-empty) and ($hf_token | is-empty) and ($openai_key | is-empty) and ($fal_key | is-empty) {
        log warning "No LLM keys set — no backends will be available"
        log warning "Set HF_TOKEN for Mistral, ANTHROPIC_API_KEY for Claude, OPENAI_API_KEY for images, or FAL_AUTH_KEY for Fal media in ~/.env"
        return
    }

    # Build --from-literal args for whichever keys are set
    mut literal_args = []
    if ($anthropic_key | is-not-empty) {
        log info "  ANTHROPIC_API_KEY: set"
        $literal_args = ($literal_args | append $"--from-literal=ANTHROPIC_API_KEY=($anthropic_key)")
    }
    if ($hf_token | is-not-empty) {
        log info "  HF_TOKEN: set"
        $literal_args = ($literal_args | append $"--from-literal=HF_TOKEN=($hf_token)")
    }
    if ($openai_key | is-not-empty) {
        log info "  OPENAI_API_KEY: set"
        $literal_args = ($literal_args | append $"--from-literal=OPENAI_API_KEY=($openai_key)")
    }
    if ($fal_key | is-not-empty) {
        log info "  FAL_AUTH_KEY: set"
        $literal_args = ($literal_args | append $"--from-literal=FAL_AUTH_KEY=($fal_key)")
    }

    # Idempotent create: dry-run + apply avoids delete/create race conditions
    log debug $"applying secret ($secret_name) in namespace ($namespace)"
    kubectl create secret generic $secret_name -n $namespace ...$literal_args --dry-run=client -o yaml | kubectl apply -f -

    log info $"LLM secret ($secret_name) applied successfully"
}

# Ensure ONNX models (embedding, mlm) are downloaded from HuggingFace.
# Downloads pre-converted ONNX files directly — no Python/torch needed.
# Also downloads the ONNX Runtime shared library (libonnxruntime.so) so pods
# don't depend on the Docker image having the right version.
# Non-fatal: warns on failure so deploy continues (model load fails lazily at runtime).
def ensure_onnx_models [onnx_cache_dir: string] {
    const HF_BASE = "https://huggingface.co"
    const ORT_VERSION = "1.23.2"

    let models = [
        {
            name: "embedding"
            hf_repo: "lightonai/modernbert-embed-large"
            dir: "modernbert-embed-large"
            files: ["onnx/model.onnx", "tokenizer.json"]
        }
        {
            name: "mlm"
            hf_repo: "answerdotai/ModernBERT-base"
            dir: "ModernBERT-base"
            files: ["onnx/model.onnx", "tokenizer.json"]
        }
    ]

    try { mkdir $onnx_cache_dir } catch {|e|
        log warning $"Cannot create ONNX cache dir ($onnx_cache_dir): ($e.msg)"
        return
    }

    # Download ONNX Runtime shared library.
    # Always download the Linux variant — pods run Linux even on macOS (MicroK8s VM).
    let ort_dir = ($onnx_cache_dir | path join "onnxruntime")
    let ort_lib = ($ort_dir | path join "libonnxruntime.so")

    if not ($ort_lib | path exists) {
        # microsoft/onnxruntime was removed from GitHub [2026-11], so its
        # release tarballs 404. Microsoft still publishes the same native
        # libraries in the Microsoft.ML.OnnxRuntime NuGet package — the
        # linux-x64 libonnxruntime.so is byte-identical to the old tarball's.
        # One .nupkg carries every RID, so one pinned checksum covers both.
        const ORT_NUPKG_SHA256 = "25fe172f7fcdf34f2b5b02c8b997f6b88abc282956849d4af4335af23d0c4e4a"
        let rid = if ($nu.os-info.arch == "aarch64") or ($nu.os-info.arch == "arm64") {
            "linux-arm64"
        } else {
            "linux-x64"
        }
        let url = $"https://www.nuget.org/api/v2/package/Microsoft.ML.OnnxRuntime/($ORT_VERSION)"

        log info $"ONNX Runtime: downloading v($ORT_VERSION) for ($rid) from NuGet..."
        try { mkdir $ort_dir } catch {|e|
            log warning $"Cannot create ONNX Runtime dir ($ort_dir): ($e.msg)"
        }

        let tmp_pkg = ($ort_dir | path join "ort.nupkg")
        try {
            if (which unzip | is-empty) {
                error make { msg: "unzip is not installed [needed to unpack the ONNX Runtime .nupkg] — install it, e.g. `sudo apt install unzip`" }
            }
            http get --raw $url | save -f $tmp_pkg
            let actual = (open --raw $tmp_pkg | hash sha256)
            if $actual != $ORT_NUPKG_SHA256 {
                error make { msg: $"checksum mismatch for ($url): expected ($ORT_NUPKG_SHA256), got ($actual)" }
            }
            # .nupkg is a zip; -j drops the runtimes/<rid>/native/ prefix.
            ^unzip -q -o -j $tmp_pkg $"runtimes/($rid)/native/*" -d $ort_dir
            rm -f $tmp_pkg
            log info $"ONNX Runtime: v($ORT_VERSION) installed at ($ort_dir)"
        } catch {|e|
            log warning $"ONNX Runtime: download failed: ($e.msg)"
            rm -f $tmp_pkg
        }
    } else {
        log info $"ONNX Runtime: already present at ($ort_dir)"
    }

    for m in $models {
        let out_path = ($onnx_cache_dir | path join $m.dir)
        let model_file = ($out_path | path join "model.onnx")

        if ($model_file | path exists) {
            log info $"ONNX ($m.name): already present at ($out_path)"
            continue
        }

        log info $"ONNX ($m.name): downloading from ($m.hf_repo)..."
        try { mkdir $out_path } catch {|e|
            log warning $"ONNX ($m.name): cannot create directory ($out_path): ($e.msg)"
            continue
        }

        for file in $m.files {
            let url = $"($HF_BASE)/($m.hf_repo)/resolve/main/($file)"
            let filename = ($file | path basename)
            let dest = ($out_path | path join $filename)

            try {
                http get --raw $url | save -f $dest
            } catch {|e|
                log warning $"ONNX ($m.name): failed to download ($file): ($e.msg)"
            }
        }

        if ($model_file | path exists) {
            log info $"ONNX ($m.name): downloaded to ($out_path)"
        } else {
            log warning $"ONNX ($m.name): download incomplete — conduit embedding will fail at runtime"
        }
    }
}

# Pre-download Mistral model weights using the conduit binary's built-in prefetch.
# This uses the same hf_hub Rust crate the runtime uses, so the cache is shared.
# Requires: cargo build of dapple-prompt-conduit (already done by this point).
def prefetch_mistral_model [] {
    let bin = ($env.DAPPLE_PROMPT_WORKSPACE_DIR | path join "target" | path join "debug" | path join "dapple-prompt-conduit")

    if not ($bin | path exists) {
        log warning "conduit binary not found — building before prefetch"
        cargo build -p dapple-prompt-conduit
    }

    log info "Prefetching Mistral model via conduit binary"
    ^$bin prefetch
    log info "Mistral model prefetched successfully"
}

# Ensure service tokens exist for all web services that need API access
def ensure_service_tokens [] {
    # Ensure dapple-ui token (SAT = Service Authorization Token)
    log info "ensuring service token for dapple_ui"
    let ui_secret = (ensure dapple_ui
        --env-var DAPPLE_UI_SAT_SIGNING_KEY
        --name "Dapple UI")

    # Ensure dapple-landing token
    log info "ensuring service token for dapple_landing"
    let landing_secret = (ensure dapple_landing
        --env-var DAPPLE_LANDING_SAT_SIGNING_KEY
        --name "Dapple Landing Page")

    # Store in env for later use by web pod deployment
    load-env {
        DAPPLE_UI_SAT_SIGNING_KEY: $ui_secret
        DAPPLE_LANDING_SAT_SIGNING_KEY: $landing_secret
    }

    log info "service tokens ensured successfully"
}

# Remove stale hostPath patch artifacts from a deployment.
# When switching from hostPath to Docker mode, the strategic merge patch
# (cargo-target volume, volumeMount, command override, securityContext)
# persists and shadows the Docker image's binary. This function cleans it up.
def remove_hostpath_patch [cfg: record] {
    log info $"checking for stale hostPath artifacts on ($cfg.name)"

    let result = (do { kubectl get deployment $cfg.name -n $cfg.namespace -o json } | complete)
    if $result.exit_code != 0 {
        log debug $"deployment ($cfg.name) not found, skipping hostPath cleanup"
        return
    }

    let spec = ($result.stdout | from json | get spec.template.spec)
    let container = ($spec.containers | first)

    # Remove pod-level securityContext (runAsUser/runAsGroup/fsGroup from hostPath)
    if ($spec | get -o securityContext | is-not-empty) {
        log info $"removing stale securityContext from ($cfg.name)"
        do { kubectl patch deployment $cfg.name -n $cfg.namespace --type=json --patch '[{"op": "remove", "path": "/spec/template/spec/securityContext"}]' } | ignore
    }

    # Remove command override from first container
    if ($container | get -o command | is-not-empty) {
        log info $"removing stale command override from ($cfg.name)"
        do { kubectl patch deployment $cfg.name -n $cfg.namespace --type=json --patch '[{"op": "remove", "path": "/spec/template/spec/containers/0/command"}]' } | ignore
    }

    # Remove cargo-target volumeMount (before volume to avoid dangling mount reference)
    let vol_mounts = ($container | get -o volumeMounts | default [])
    let cargo_mounts = ($vol_mounts | enumerate | where {|it| $it.item.name == "cargo-target" })
    if ($cargo_mounts | is-not-empty) {
        let vm_idx = ($cargo_mounts | first | get index)
        log info $"removing stale cargo-target volumeMount at index ($vm_idx) from ($cfg.name)"
        do { kubectl patch deployment $cfg.name -n $cfg.namespace --type=json --patch $'[{"op": "remove", "path": "/spec/template/spec/containers/0/volumeMounts/($vm_idx)"}]' } | ignore
    }

    # Remove cargo-target volume
    let volumes = ($spec | get -o volumes | default [])
    let cargo_vols = ($volumes | enumerate | where {|it| $it.item.name == "cargo-target" })
    if ($cargo_vols | is-not-empty) {
        let vol_idx = ($cargo_vols | first | get index)
        log info $"removing stale cargo-target volume at index ($vol_idx) from ($cfg.name)"
        do { kubectl patch deployment $cfg.name -n $cfg.namespace --type=json --patch $'[{"op": "remove", "path": "/spec/template/spec/volumes/($vol_idx)"}]' } | ignore
    }
}

# Run the Docker registry's blob garbage-collector inside the in-cluster
# microk8s registry pod (registry addon, namespace `container-registry`).
#
# Probe-first design (added in the auto-reclaim follow-up to PR #488):
#  1. Find the registry pod.
#  2. Try a `--dry-run` first — if the registry image supports the flag, it
#     prints lines like "blob eligible for deletion: sha256:..." without
#     mutating anything. We count those lines to decide whether to bother
#     with the real GC at all. Zero eligible blobs → log and skip; >0 → run
#     the real GC.
#  3. If `--dry-run` is unrecognized (older registry images that predate
#     the flag), fall back to running GC unconditionally (PR #488 behavior).
#  4. Any other dry-run failure (pod down, exec failure): log and skip.
#
# Best-effort throughout: every failure path logs a warning and returns —
# GC must never abort a deploy. Called once at the end of a deploy_all run
# because that's the only time a Docker push could have stranded blobs.
def registry_garbage_collect [] {
    let pod = (do { kubectl get pods -n container-registry -l app=registry -o "jsonpath={.items[0].metadata.name}" } | complete)
    if $pod.exit_code != 0 {
        log warning "registry GC skipped — could not query the container-registry registry pod (microk8s registry addon not enabled?)"
        return
    }

    let pod_name = ($pod.stdout | str trim)
    if ($pod_name | is-empty) {
        log warning "registry GC skipped — no registry pod found in the container-registry namespace"
        return
    }

    let dry = (do { kubectl exec -n container-registry $pod_name -- bin/registry garbage-collect --dry-run /etc/docker/registry/config.yml } | complete)

    if $dry.exit_code == 0 {
        # Registry version supports --dry-run. Parse stdout for eligible
        # blobs. Format varies by registry version — be defensive and just
        # count the distinctive phrase.
        let eligible = ($dry.stdout | lines | where { |l| ($l | str contains "eligible for deletion") } | length)
        if $eligible == 0 {
            log info $"registry GC: dry-run on pod ($pod_name) reports no orphaned blobs — skipping real GC"
            return
        }
        log info $"registry GC: dry-run on pod ($pod_name) found ($eligible) orphaned blob marker\(s\) — running real GC"
        let gc = (do { kubectl exec -n container-registry $pod_name -- bin/registry garbage-collect /etc/docker/registry/config.yml } | complete)
        if $gc.exit_code == 0 {
            log info $"registry GC complete on pod ($pod_name) — orphaned blobs reclaimed"
        } else {
            log warning $"registry GC on pod ($pod_name) exited non-zero \(best-effort, continuing\): ($gc.stderr | str trim)"
        }
        return
    }

    # Dry-run failed. Decide whether it failed because the flag is
    # unrecognized (older registry image — fall back to unconditional GC,
    # matching PR #488 behavior) or for some other reason (skip entirely).
    let err = ($dry.stderr | str lowercase)
    let flag_unknown = (($err | str contains "unknown flag") or ($err | str contains "unknown command") or ($err | str contains "flag provided but not defined"))
    if not $flag_unknown {
        log warning $"registry GC dry-run on pod ($pod_name) failed \(best-effort, skipping\): ($dry.stderr | str trim)"
        return
    }

    log debug "registry image does not support --dry-run — falling back to unconditional GC"
    let gc = (do { kubectl exec -n container-registry $pod_name -- bin/registry garbage-collect /etc/docker/registry/config.yml } | complete)
    if $gc.exit_code == 0 {
        log info $"registry GC complete on pod ($pod_name) — orphaned blobs reclaimed"
    } else {
        log warning $"registry GC on pod ($pod_name) exited non-zero \(best-effort, continuing\): ($gc.stderr | str trim)"
    }
}

# Containerd content-store prune. Removes content blobs that no image or
# snapshot still references. Non-destructive (only references no live
# object), idempotent — safe to auto-run every deploy_all. Uses the
# microk8s-wrapped `ctr` in the `k8s.io` namespace (the namespace kubelet
# uses).
#
# On Linux microk8s runs natively as a snap and there is no VM abstraction
# — skip the prune (the equivalent Linux command path is left to operator
# discretion; the leak this addresses is dominantly a macOS+OrbStack issue
# because of how `--docker` retags happen against the in-VM registry).
def _ctr_content_prune [] {
    if not (orb is_macos) {
        log debug "containerd content prune skipped — Linux microk8s runs natively, no VM to enter"
        return
    }
    let result = (do { orb vm_run [sudo microk8s ctr content prune references] } | complete)
    if $result.exit_code == 0 {
        log info "containerd content prune complete — unreferenced blobs reclaimed"
    } else {
        log warning $"containerd content prune exited non-zero \(best-effort, continuing\): ($result.stderr | str trim)"
    }
}

# Prune OrbStack's host-side Docker engine cache. Unlike microk8s
# containerd INSIDE the VM (which is backed by the sparse data.img.raw
# and where guest TRIM does not pass through to host APFS hole-punching),
# the host Docker engine stores its content as APFS clones — `docker
# system prune` reclaims host disk IMMEDIATELY.
#
# This is typically the largest single recoverable bucket after a series
# of --docker deploys: every replaced :dapple-latest tag and every
# intermediate build layer accumulates as APFS clones in the OrbStack
# data dir, INDEPENDENT of the sparse VM file. Empirically verified
# May 2026 against the dapple-prompt dev cluster — one invocation
# reclaimed a multi-tens-of-GiB delta the OrbStack UI did not show as
# "live content".
#
# `-a` prunes ALL unused images (not just dangling). This is
# intentional: the deploy script's --docker mode rebuilds and retags on
# every run, so any unused image really is unused — it will not be
# referenced again until rebuilt. Running this every deploy_all is the
# right semantic; do not reflexively narrow this prune.
#
# `--volumes` includes orphaned volumes. `-f` skips the interactive
# confirmation prompt.
#
# Skipped when `docker` is not on PATH (Linux contributors without
# Docker installed). When `docker` IS present — OrbStack-Docker on
# macOS, Docker Desktop on macOS or Linux, native Linux Docker — the
# prune operates on whichever engine the CLI is talking to. The
# image/volume/build-cache accumulation pattern is not OrbStack-specific,
# so a Linux contributor using `--docker dev` benefits from the same
# prune as a Mac contributor.
#
# Best-effort: log a warning and continue on failure. Never aborts the
# deploy.
def _docker_system_prune []: nothing -> nothing {
    let docker_check = (do { ^which docker } | complete)
    if $docker_check.exit_code != 0 {
        log debug "docker system prune skipped — `docker` CLI not present on PATH"
        return
    }
    let result = (do { ^docker system prune -af --volumes } | complete)
    if $result.exit_code == 0 {
        # Parse the "Total reclaimed space: X" line. Format is stable
        # across Docker versions (e.g. "Total reclaimed space: 12.3GB").
        let lines_with_match = ($result.stdout | lines | where { |l| ($l | str lowercase | str contains "total reclaimed space") })
        let reclaimed = if ($lines_with_match | is-empty) { "" } else { $lines_with_match | first | str trim }
        if ($reclaimed | is-not-empty) {
            log info $"docker system prune complete — ($reclaimed)"
        } else {
            log info "docker system prune complete — host Docker engine cache reclaimed"
        }
    } else {
        log warning $"docker system prune exited non-zero \(best-effort, continuing\): ($result.stderr | str trim)"
    }
}

# Threshold constants. Override via env vars at the call site for the
# reclaim plan. The byte threshold is REAL bytes from `du -sb`, NOT a `df`
# percentage — `df` is poisoned on OrbStack (see modules/dapple/reclaim.nu
# and PR #488).
const RECLAIM_HIGH_WATER_BYTES = 26843545600  # 25 GiB
const RECLAIM_DEPLOY_COUNT = 25

def _high_water_bytes []: nothing -> int {
    let override = ($env.DAPPLE_RECLAIM_HIGH_WATER_GIB? | default "")
    if ($override | is-empty) {
        return $RECLAIM_HIGH_WATER_BYTES
    }
    try {
        ($override | into int) * 1024 * 1024 * 1024
    } catch {
        log warning $"DAPPLE_RECLAIM_HIGH_WATER_GIB=[($override)] is not an integer — using default 25 GiB"
        $RECLAIM_HIGH_WATER_BYTES
    }
}

def _deploy_count_threshold []: nothing -> int {
    let override = ($env.DAPPLE_RECLAIM_DEPLOY_COUNT? | default "")
    if ($override | is-empty) {
        return $RECLAIM_DEPLOY_COUNT
    }
    try {
        $override | into int
    } catch {
        log warning $"DAPPLE_RECLAIM_DEPLOY_COUNT=[($override)] is not an integer — using default 25"
        $RECLAIM_DEPLOY_COUNT
    }
}

# Host-side OrbStack sparse-file high-water mark. When this trips, the only
# reclaim is delete-and-re-provision — guest cleanup CANNOT shrink the host
# file (TRIM doesn't pass through on observed OrbStack versions, verified
# May 2026: 337 GiB host-side post-`fstrim`). This is a DIFFERENT remediation
# path from the guest-side containerd-reset warning, so it gets its own
# threshold and its own warning block. Override via
# DAPPLE_RECLAIM_HOST_HIGH_WATER_GIB.
const RECLAIM_HOST_HIGH_WATER_BYTES = 64424509440  # 60 GiB

def _host_high_water_bytes []: nothing -> int {
    let override = ($env.DAPPLE_RECLAIM_HOST_HIGH_WATER_GIB? | default "")
    if ($override | is-empty) {
        return $RECLAIM_HOST_HIGH_WATER_BYTES
    }
    try {
        ($override | into int) * 1024 * 1024 * 1024
    } catch {
        log warning $"DAPPLE_RECLAIM_HOST_HIGH_WATER_GIB=[($override)] is not an integer — using default 60 GiB"
        $RECLAIM_HOST_HIGH_WATER_BYTES
    }
}

# Compute and execute the reclaim plan at the end of a deploy_all.
#
# AUTO-RUNS the SAFE actions (registry GC with dry-run probe, ctr content
# prune). EMITS A WARNING — never auto-executes — for the destructive
# containerd reset: that wipes the local image cache and the re-pull storm
# that follows produced the "missing parent ... bucket" corruption during
# PR #488's originating incident. The reset is a one-way door — operator
# decision only.
def _run_reclaim_plan [] {
    let bytes = (reclaim containerd_store_bytes)
    let state = (reclaim read_state)

    # This deploy counts as one full deploy_all. Persist the bump now so an
    # abort later still increments the counter.
    let bumped_count = (($state.deploys_since_reset? | default 0) + 1)

    log info "reclaiming orphaned in-cluster registry blobs (post-deploy GC)"
    registry_garbage_collect

    log info "pruning unreferenced containerd content blobs (post-deploy)"
    _ctr_content_prune

    # Host-side Docker engine cache (APFS clones, NOT inside data.img.raw).
    # This is typically the largest single reclaimable bucket after a
    # series of --docker deploys — see _docker_system_prune for the full
    # rationale. Ordered last because it has the broadest scope of the
    # three caches (registry pod → in-VM containerd → host Docker engine)
    # and reads naturally in increasing scope.
    log info "pruning host-side Docker engine cache (post-deploy)"
    _docker_system_prune

    let high_water = (_high_water_bytes)
    let deploy_limit = (_deploy_count_threshold)
    let over_size = ($bytes > $high_water)
    let over_count = ($bumped_count >= $deploy_limit)
    let recommend = ($over_size or $over_count)

    if $recommend {
        let bytes_gib = (($bytes / 1024 / 1024 / 1024) | into int)
        let high_gib = (($high_water / 1024 / 1024 / 1024) | into int)
        # Bracketed parentheticals — NOT `($var)` interpolation — for prose.
        # See gotcha #1: bare parens inside `$"..."` parse as nu expressions.
        let size_reason = $"containerd store at ($bytes_gib) GiB [threshold ($high_gib) GiB]"
        let count_reason = $"($bumped_count) deploys since last reset [threshold ($deploy_limit)]"
        let reason = if $over_size and $over_count {
            $"($size_reason); ($count_reason)"
        } else if $over_size {
            $size_reason
        } else {
            $count_reason
        }
        log warning "Containerd image store is large — recommend running a maintenance-window reset."
        log warning $"  reason: ($reason)"
        log warning "  to reclaim: nu assets/nu/k8s/reclaim_disk.nu containerd-reset --confirm"
        log warning "  this is intentionally NOT auto-executed — see PR #488 for the corruption-during-re-pull rationale."
    }

    # HOST-SIDE check — remediation paths differ from the guest-side
    # containerd-reset warning above. There are TWO host-side caches
    # backing OrbStack's data dir:
    #
    #  (a) APFS clones — back OrbStack's host-side Docker engine
    #      (images, volumes, build cache). `docker system prune`
    #      reclaims these IMMEDIATELY; we already ran it above as part
    #      of the safe reclaim plan, so any remaining bloat is NOT in
    #      this layer.
    #  (b) `data.img.raw` — the sparse VM disk backing microk8s and
    #      everything inside the guest. Guest TRIM does NOT pass
    #      through to host APFS hole-punching on observed OrbStack
    #      versions, and per-VM `orb delete` does NOT reclaim the host
    #      file either (verified May 2026: deleting the only VM only
    #      dropped the data dir from 337 to 334 GiB, because OrbStack
    #      uses one shared `data.img.raw` for ALL its content). The
    #      only reclaim that shrinks it is to quit OrbStack and `rm`
    #      the file.
    #
    # If the warning trips it means (a) was already pruned this run and
    # the residual bloat is in (b). Recommend the docker-prune tier
    # first anyway in case the operator inspects the situation cold
    # (e.g. another deploy hasn't run since the data accumulated), then
    # escalate to the full reset only if Layer 1 didn't help.
    #
    # host_bytes == 0 means "couldn't measure" (non-macOS, OrbStack
    # absent, du failed). Do NOT warn in that case — silence is correct.
    let host_bytes = (reclaim orbstack_img_bytes)
    let host_high_water = (_host_high_water_bytes)
    if $host_bytes > 0 and $host_bytes > $host_high_water {
        let host_gib = (($host_bytes / 1024 / 1024 / 1024) | into int)
        let host_hi_gib = (($host_high_water / 1024 / 1024 / 1024) | into int)
        log warning "OrbStack data dir is large — try the cheap reclaim first:"
        log warning $"  reason: host data dir at ($host_gib) GiB [threshold ($host_hi_gib) GiB]"
        log warning "  Layer 1 (cheap, immediate — APFS clones in host Docker engine):"
        log warning "    nu assets/nu/k8s/reclaim_disk.nu docker-prune"
        log warning "  If host disk is still tight after that, the sparse VM file itself may"
        log warning "  have grown; escalate only then to the full reset (Layer 3):"
        # The data.img.raw path contains a space ("Group Containers"). The
        # operator pasting these into a shell will need shell-level quoting
        # (already provided via the $HOME-rooted double-quoted form below).
        log warning "    1. nu assets/nu/k8s/backup_pvc.nu backup"
        log warning "    2. osascript -e 'tell application \"OrbStack\" to quit'"
        log warning "    3. pgrep -fl OrbStack    # verify no OrbStack process is still running"
        log warning "    4. rm \"$HOME/Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw\""
        log warning "    5. open -a OrbStack"
        log warning "    6. nu assets/nu/k8s/install_orbstack_vm.nu"
        log warning "    7. nu assets/nu/k8s/setup_microk8s.nu"
        log warning "    8. nu assets/nu/k8s/backup_pvc.nu restore ~/dapple-backups/<latest>.tar.gz --confirm"
        log warning "    9. nu assets/nu/k8s/deploy_dapple_core.nu"
        log warning "  see docs/books/journey-builder-guide/development/orbstack-setup.md"
    }

    # Persist updated state (size snapshot + bumped counter). Preserve the
    # last-reset fields verbatim; only `record_reset` (in the reclaim
    # module) zeroes them. The new `size_orbstack_img_bytes` field is
    # additive — existing readers tolerate missing fields via `?` default.
    let new_state = {
        last_reset_epoch: ($state.last_reset_epoch? | default 0)
        deploys_since_reset: $bumped_count
        size_at_last_reset_bytes: ($state.size_at_last_reset_bytes? | default 0)
        size_at_last_check_bytes: $bytes
        size_orbstack_img_bytes: $host_bytes
    }
    reclaim write_state $new_state
}

#TODO: move this to k8s module
def setup_dapple_svc [cfg: record, --force, --hostpath: string = "", --docker, --allow-dirty, --no-build] {
    if $docker {
        # Docker mode: build image, push to registry, deploy from image
        build_and_push_image $cfg $force --allow-dirty=$allow_dirty
    } else {
        log info $"hostPath mode for ($cfg.name) — using local binary from target/"

        # Cargo features this service needs (e.g. `oximedia`); empty for most.
        let svc_features = ($cfg.features? | default "")

        # Build the binary the hostPath patch is about to mount. Both host
        # platforms build here, for the same reason: in hostPath mode the
        # mounted artifact IS the deployment, so a stale target/ dir means the
        # rollout succeeds while the code you just wrote never runs. macOS
        # cross-compiles to aarch64-linux (Mach-O can't execute in the VM);
        # Linux builds natively. Cargo's freshness check makes an
        # already-current build cheap, so this is a fast no-op when the
        # operator did already run `cargo build`.
        let profile = (resolve_hostpath_profile $hostpath)
        if $no_build {
            log warning $"--no-build: skipping cargo build for ($cfg.name) — mounting whatever is in target/($profile)/"
        } else if ($nu.os-info.name | str contains "mac") {
            cross_compile_for_linux_target $cfg.name $profile $svc_features
        } else {
            build_for_hostpath $cfg.name $profile $svc_features
        }

        # Ensure the image exists in the registry — hostPath mode still needs
        # a pullable image (K8s pulls the image before applying the command override).
        # If the image has never been pushed, do a one-time Docker build+push.
        let image_exists = (check_registry_image $cfg.name)
        if not $image_exists {
            log warning $"Image for ($cfg.name) not found in registry — pushing placeholder image"
            # The hostPath patch overrides the entrypoint to run the local
            # binary, but that binary still runs on the image's filesystem: a
            # bare base image has no CA bundle [every outbound HTTPS call
            # fails] and no ONNX Runtime [conduit-onnx panics]. Build the
            # Dockerfile's runtime-base stage — the real image minus the binary.
            let tag = $"($env.KUBE_REGISTRY_HOST)/($cfg.name):($env.DOCKER_DAPPLE_TAG_LATEST)"
            let gpu_type = ($cfg | get -o gpu | default { type: "none" } | get type)
            (^docker build
                --target runtime-base
                --build-arg $"GPU_TYPE=($gpu_type)"
                --file ($env.DAPPLE_PROMPT_WORKSPACE_DIR | path join "Dockerfile")
                --tag $tag
                $env.DAPPLE_PROMPT_WORKSPACE_DIR)
            ^docker push $tag
            log info $"Placeholder image pushed: ($tag)"
        }
    }

    let registry_file = $"assets/k8s/registry/($cfg.name).yaml"
    if ($registry_file | path exists) {
        log info $"applying k8s registry manifest for ($cfg.name)"
        kubectl apply -n ($env.KUBE_DAPPLE_NAMESPACE) -f $registry_file

        # Clean stale hostPath artifacts when deploying via Docker
        if $docker {
            remove_hostpath_patch $cfg
        }

        # Patch ONNX models volume if onnx_cache_dir is specified (for conduit)
        let onnx_cache_dir = ($cfg | get -o onnx_cache_dir | default "")
        if ($onnx_cache_dir | is-not-empty) {
            apply_onnx_volume_patch $cfg $onnx_cache_dir
        }

        # Patch HF cache volume if hf_cache_dir is specified (for conduit)
        let hf_cache_dir = ($cfg | get -o hf_cache_dir | default "")
        if ($hf_cache_dir | is-not-empty) {
            apply_hf_cache_volume_patch $cfg $hf_cache_dir
        }

        # Patch Mistral env vars on Linux hosts only (adds MISTRALRS_* env + bumps memory to 8Gi)
        let mistral_enabled = ($cfg | get -o mistral_enabled | default false)
        if $mistral_enabled {
            apply_mistral_env_patch $cfg
        }

        # Apply hostPath patch when in hostPath mode (the default)
        if ($hostpath | is-not-empty) {
            apply_hostpath_patch $cfg $hostpath
        }

        # Patch the deployment's image to include the GPU suffix when one was
        # detected. build_and_push_image tags GPU-aware images as
        # `($latest)-($gpu_type)` (e.g. `dapple-latest-apple`) but the registry
        # manifests hardcode the bare `dapple-latest` tag — kubelet can't pull
        # the bare tag because it doesn't exist for GPU services. This rewrite
        # is a no-op when no GPU was detected (gpu.type == "none").
        #
        # Skipped in hostPath mode: the placeholder push only tags the bare
        # `dapple-latest`, so rewriting to a GPU-suffixed tag would point at an
        # image that doesn't exist. The hostPath entrypoint override runs the
        # local binary anyway, so the image content is irrelevant.
        if $docker {
            apply_gpu_image_patch $cfg
        }

        # Force pod recreation
        log info $"restarting deployment ($cfg.name)"
        rollout_restart { name: $cfg.name, namespace: $cfg.namespace }

        # Wait for rollout — error on timeout instead of silent warning
        let rollout_result = (
            kubectl rollout status $"deployment/($cfg.name)" -n $cfg.namespace --timeout "300s"
            | complete
        )
        if $rollout_result.exit_code != 0 {
            log error $"Rollout failed for ($cfg.name): ($rollout_result.stderr)"
            error make { msg: $"Rollout timed out for ($cfg.name)" }
        }

        # Post-deploy verification only in Docker mode (hostPath has no digest to check)
        if $docker {
            verify_deploy $cfg
        }
    } else {
        # Only use template if no registry file exists
        log debug $"no registry manifest found for ($cfg.name), using template"
        deploy $cfg --wait
    }
}

# Verify the deployed pod is running the image we just pushed.
# The tag must include the GPU suffix for GPU-aware services — otherwise the
# `docker inspect` below queries a tag that was never pushed and reports
# "could not inspect", silently skipping verification.
def verify_deploy [cfg: record] {
    let ns = $cfg.namespace
    let name = $cfg.name
    let gpu_type = ($cfg | get -o gpu | default { type: "none" } | get type)
    let gpu_suffix = if $gpu_type == "none" { "" } else { $"-($gpu_type)" }
    let latest_tag = $"($env.KUBE_REGISTRY_HOST)/($name):($env.DOCKER_DAPPLE_TAG_LATEST)($gpu_suffix)"

    # Get the pushed digest from local docker metadata (RepoDigests is set after push)
    let inspect_result = (
        do { ^docker inspect $latest_tag --format "{{index .RepoDigests 0}}" } | complete
    )

    if $inspect_result.exit_code != 0 {
        log warning "Could not inspect local image for digest — skipping deploy verification"
        return
    }

    # RepoDigests format: registry/name@sha256:abc123...
    let expected_digest = ($inspect_result.stdout | str trim | split row "@" | last)

    # Poll Running pods until the expected digest appears (old pod may still be Terminating)
    let max_attempts = 10
    mut attempt = 0
    mut pod_images = ""

    loop {
        $attempt = $attempt + 1
        $pod_images = (kubectl get pod -n $ns -l $"app=($name)" --field-selector "status.phase=Running" -o "jsonpath={.items[*].status.containerStatuses[0].imageID}" | str trim)

        if ($pod_images | str contains $expected_digest) {
            log info $"Deploy verified: pod running expected image \(($expected_digest | str substring 7..19))"
            return
        }

        if $attempt >= $max_attempts {
            break
        }

        log debug $"Waiting for new pod to become Running \(attempt ($attempt)/($max_attempts))..."
        sleep 3sec
    }

    log error "Deploy mismatch!"
    log error $"  Expected: ($expected_digest | str substring 0..30)..."
    log error $"  Running pods: ($pod_images)"
    error make { msg: "Deploy verification failed: pod running stale image" }
}

# Check if an image exists in the registry by querying the registry API.
# Returns true if the image repo has at least one tag.
def check_registry_image [name: string]: nothing -> bool {
    let result = (do { curl -sk $"https://($env.KUBE_REGISTRY_HOST)/v2/($name)/tags/list" } | complete)
    if $result.exit_code != 0 {
        return false
    }
    # Response is {"name":"...","tags":["tag1",...]} or {"errors":[...]} if not found
    let body = ($result.stdout | str trim)
    if ($body | str contains '"tags"') and (not ($body | str contains '"tags":null')) {
        true
    } else {
        false
    }
}

# Build and push Docker image for a service.
# --allow-dirty skips the dirty-tree guard (used for bootstrap builds in hostPath mode).
def build_and_push_image [cfg: record, force: bool, --allow-dirty] {
    # Get GPU type from config or default to "none"
    let gpu = ($cfg.gpu? | default { type: "none" })
    let gpu_type = $gpu.type

    # Content hash covers EVERY input that can affect the produced binary.
    # Hashing only the immediate crate + Cargo.lock silently skips rebuilds
    # when a workspace dependency (e.g. `dapple-data`) changes without bumping
    # Cargo.lock — path dependencies don't bump versions, so the hash misses
    # the change and a stale binary keeps getting retagged as `:dapple-latest`.
    #
    # The set below mirrors what the Dockerfile actually uses to build:
    # - `crates/`       all workspace source (including transitive deps)
    # - `Cargo.toml`    workspace manifest (resolver, members, profile)
    # - `Cargo.lock`    exact dep versions
    # - `Dockerfile`    build instructions
    # - `.dockerignore` what gets shipped to the build daemon
    # - `.sqlx/`        sqlx offline metadata read at compile time
    #
    # Trade-off: any change to ANY workspace crate triggers a rebuild of EVERY
    # service. Slower than a per-service hash, but the alternative (silent
    # stale binary) is much worse. Mirrors the canonical logic in build_image.nu.
    let hash_inputs = ["crates/" "Cargo.toml" "Cargo.lock" "Dockerfile" ".dockerignore" ".sqlx/"]
    let content_hash = (
        git ls-tree -r HEAD -- ...$hash_inputs
        | ^sha256sum
        | split row " "
        | first
        | str substring 0..12
    )

    # Include GPU type in tag for GPU-enabled services
    let gpu_suffix = if $gpu_type == "none" { "" } else { $"-($gpu_type)" }
    let commit_tag = $"($env.KUBE_REGISTRY_HOST)/($cfg.name):($content_hash)($gpu_suffix)"
    let latest_tag = $"($env.KUBE_REGISTRY_HOST)/($cfg.name):($env.DOCKER_DAPPLE_TAG_LATEST)($gpu_suffix)"

    # Guard: refuse to Docker-build from a dirty working tree.
    # Dirty builds poison the content-hash cache — the image gets tagged with the
    # committed hash but contains uncommitted code. Later clean deploys skip the
    # build because the hash matches, silently running stale/broken code.
    # Mirrors `hash_inputs` so cross-crate dirt is caught the same way it
    # contributes to the hash.
    let dirty_paths = (git status --porcelain ...$hash_inputs | str trim)
    let is_dirty = ($dirty_paths | is-not-empty)

    if $is_dirty and (not $allow_dirty) {
        log error $"Workspace has uncommitted changes affecting ($cfg.name) — refusing Docker build"
        log error ""
        log error "A dirty build would poison the content-hash cache in the registry."
        log error "Dirty paths:"
        for line in ($dirty_paths | lines) {
            log error $"  ($line)"
        }
        log error ""
        log error "Options:"
        log error "  1. Commit or stash your changes, then re-run"
        log error "  2. Pass --allow-dirty to build anyway \(your committed hash will tag uncommitted bits)"
        log error "  3. Use hostPath mode \(Linux only) to run your local binary directly"
        log error ""
        error make { msg: $"Dirty tree: cannot Docker-build ($cfg.name). Commit first or pass --allow-dirty." }
    }
    if $is_dirty and $allow_dirty {
        log warning $"Workspace has uncommitted changes — building anyway because --allow-dirty was passed"
        log warning "  The pushed image will be tagged with the committed content_hash but"
        log warning "  contain uncommitted code. Future clean deploys with the same HEAD will"
        log warning "  reuse this image. Commit and re-tag once validated."
    }

    # Skip if: not forced and content-hash image already exists in registry
    let should_skip = if $force {
        false
    } else {
        (do { ^docker manifest inspect $commit_tag } | complete).exit_code == 0
    }

    if $should_skip {
        log info $"Image ($commit_tag) already exists, skipping build"
        # Pull the existing content-tagged image, retag as latest, and push
        ^docker pull $commit_tag
        ^docker tag $commit_tag $latest_tag
        ^docker push $latest_tag
    } else {
        # Pass the service's cargo features to the Docker build so a Docker-mode
        # deploy of a feature-gated service (e.g. conduit-oximedia → `oximedia`,
        # which its /compose-av + /analyze-audio endpoints depend on) ships the
        # same binary hostPath mode builds. Empty for most services ⇒ unchanged.
        # NOTE: features are NOT part of `content_hash` above (which hashes only
        # source/lock/Dockerfile). A service's features are hard-coded per
        # setup_dapple_svc call, so this is stable in practice — but if you ever
        # change a service's feature set without touching source, pass --force to
        # bypass the "image already exists" skip and rebuild.
        let features = ($cfg.features? | default "")
        let build_args = [
            --network=host
            --build-arg $"DAPPLE_PROMPT_DATABASE_URL=($env.DAPPLE_PROMPT_INTERNAL_POSTGRES_URL)"
            --build-arg $"BIN=($cfg.name)"
            --build-arg $"PROFILE=($cfg.profile)"
            --build-arg $"GPU_TYPE=($gpu_type)"
            --build-arg $"FEATURES=($features)"
            --tag $commit_tag
            --tag $latest_tag
            .
        ]

        # Remove stale local images BEFORE building so a failed build
        # cannot fall through to push an old cached image with the same tag.
        do { ^docker rmi $commit_tag } | ignore
        do { ^docker rmi $latest_tag } | ignore

        log debug $"building docker image with tag ($commit_tag) \(GPU: ($gpu_type))"
        ^docker build ...$build_args

        # Get image ID using Docker's --format for reliable parsing
        let image_id = (^docker images -f $"reference=($commit_tag)" --format "{{.ID}}" | str trim)

        if ($image_id | is-empty) {
            error make { msg: $"No image found matching ($commit_tag)" }
        }

        log info $"built image ($commit_tag) with ID ($image_id)"

        log debug $"pushing images to registry"

        # Push commit-tagged image first
        let push_commit_result = (^docker push $commit_tag | complete)
        if $push_commit_result.exit_code != 0 {
            let error_msg = $push_commit_result.stderr
            let cert_file = ($env.DOCKER_DIR | path join "certs.d" | path join $env.KUBE_REGISTRY_HOST | path join "ca.crt")
            let is_cert_error = (($error_msg | str contains "certificate") or ($error_msg | str contains "x509") or ($error_msg | str contains "unknown authority"))
            if $is_cert_error {
                let uses_desktop = (($nu.os-info.name | str contains "mac") or ($env.DAPPLE_DOCKER_DESKTOP? | default "" | str trim) == "true")
                log error "Docker push failed due to certificate verification error"
                if $uses_desktop {
                    log error $"Certificate file: ($cert_file)"
                    log error "This usually means Docker Desktop needs to be restarted to pick up the certificate"
                    log error "Please restart Docker Desktop and try again"
                } else {
                    log error $"Check that /etc/docker/certs.d/($env.KUBE_REGISTRY_HOST)/ca.crt exists"
                    log error "Run `k8s setup_docker_registry_cert` if missing"
                }
                error make { msg: $"Docker push failed: ($error_msg)" }
            } else {
                error make { msg: $"Docker push failed: ($error_msg)" }
            }
        }

        # Push latest-tagged image
        let push_latest_result = (^docker push $latest_tag | complete)
        if $push_latest_result.exit_code != 0 {
            error make { msg: $"Docker push failed for latest tag: ($push_latest_result.stderr)" }
        }
    }

    # Prune old local images, keeping only current + latest
    prune_old_images $cfg.name $"($content_hash)($gpu_suffix)"
}

# Remove old local images for a deployment, keeping only current + latest tags
def prune_old_images [deployment: string, keep_tag: string] {
    let repo = $"($env.KUBE_REGISTRY_HOST)/($deployment)"
    let all_tags = (
        ^docker images $repo --format "{{.Tag}}" | lines | where {|tag| $tag != "<none>" }
    )
    let remove = ($all_tags | where {|tag|
        $tag != $env.DOCKER_DAPPLE_TAG_LATEST and $tag != $keep_tag
    })
    for tag in $remove {
        log info $"Removing old image ($deployment):($tag)"
        do { ^docker rmi $"($repo):($tag)" } | ignore
    }
}

# Patch the deployment's container image tag to include the GPU suffix that
# build_and_push_image used. Without this, GPU-aware services (conduit-onnx
# with `gpu: { type: "apple" }`, etc.) fail to start with ImagePullBackOff
# because the deployment manifest references `dapple-latest` but only
# `dapple-latest-apple` (or `-amd`, `-nvidia`) exists in the registry.
#
# No-op when cfg.gpu is unset or gpu.type is "none" — non-GPU services keep
# the bare `dapple-latest` tag exactly as the manifest specifies.
def apply_gpu_image_patch [cfg: record] {
    let gpu_type = ($cfg | get -o gpu | default { type: "none" } | get type)
    if $gpu_type == "none" {
        return
    }

    let image = $"($env.KUBE_REGISTRY_HOST)/($cfg.name):($env.DOCKER_DAPPLE_TAG_LATEST)-($gpu_type)"
    log info $"patching ($cfg.name) image to GPU-suffixed tag: ($image)"
    kubectl set image $"deployment/($cfg.name)" $"($cfg.name)=($image)" -n $cfg.namespace
}

# Patch the hf-cache volume to use the correct host path
# This ensures host prefetch and pod share the same HuggingFace cache
def apply_hf_cache_volume_patch [cfg: record, hf_cache_dir: string] {
    log info $"patching hf-cache volume to use: ($hf_cache_dir)"

    # Find the index of the hf-cache volume (varies by service)
    let volumes_json = (kubectl get deployment $cfg.name -n $cfg.namespace -o "jsonpath={.spec.template.spec.volumes}")
    let volumes = ($volumes_json | from json)
    let idx = ($volumes | enumerate | where {|v| $v.item.name == "hf-cache"} | get 0?.index? | default (-1))

    if $idx == -1 {
        log warning "No hf-cache volume found in deployment, skipping patch"
        return
    }

    let patch = $'[{"op": "replace", "path": "/spec/template/spec/volumes/($idx)/hostPath/path", "value": "($hf_cache_dir)"}]'
    kubectl patch deployment $cfg.name -n $cfg.namespace --type=json --patch $patch
}

# Patch Mistral env vars and bump memory limit for Linux hosts.
# On macOS the VM can't spare ~4GB for the 7B GGUF model on top of ONNX models.
def apply_mistral_env_patch [cfg: record] {
    log info "applying Mistral env vars + memory limit (Linux host)"
    let container_name = $cfg.name
    let patch = $'{"spec":{"template":{"spec":{"containers":[{"name":"($container_name)","env":[{"name":"MISTRALRS_MODEL_ID","value":"bartowski/Mistral-7B-Instruct-v0.3-GGUF"},{"name":"MISTRALRS_GGUF_FILE","value":"Mistral-7B-Instruct-v0.3-Q4_K_M.gguf"},{"name":"MISTRALRS_TOK_MODEL_ID","value":"mistralai/Mistral-7B-Instruct-v0.3"}],"resources":{"limits":{"memory":"16Gi"}}}]}}}}'
    kubectl patch deployment $cfg.name -n $cfg.namespace --type=strategic --patch $patch
}

# Patch the onnx-models volume to use the correct host path
# This ensures the path works across different users and platforms (Linux/macOS)
def apply_onnx_volume_patch [cfg: record, onnx_cache_dir: string] {
    if not ($onnx_cache_dir | path exists) {
        log warning $"ONNX cache directory not found: ($onnx_cache_dir)"
        log warning "Run 'nu assets/nu/k8s/convert_onnx_models.nu' to create and populate it"
    }

    log info $"patching onnx-models volume to use: ($onnx_cache_dir)"

    let volumes_json = (kubectl get deployment $cfg.name -n $cfg.namespace -o "jsonpath={.spec.template.spec.volumes}")
    let volumes = ($volumes_json | from json)
    let idx = ($volumes | enumerate | where {|v| $v.item.name == "onnx-models"} | get 0?.index? | default (-1))

    if $idx == (-1) {
        log warning "No onnx-models volume found in deployment, skipping patch"
        return
    }

    let patch = $'[{"op": "replace", "path": "/spec/template/spec/volumes/($idx)/hostPath/path", "value": "($onnx_cache_dir)"}]'
    kubectl patch deployment $cfg.name -n $cfg.namespace --type=json --patch $patch
}

# Resolve whether a given service should be deployed via Docker rather than
# hostPath. The user's explicit --docker flag always wins; otherwise on macOS
# we auto-promote services that can't cross-compile (the conduit family with
# its mistralrs/hf-hub native deps) to Docker, while clean services fall
# through to hostPath mode.
def resolve_use_docker [
    user_wants_docker: bool,
    is_mac: bool,
    cross_compile_supported: bool,
    svc_name: string,
] {
    if $user_wants_docker { return true }
    if $is_mac and (not $cross_compile_supported) {
        log warning $"macOS: ($svc_name) is not cross-compile-eligible \(third-party native deps\); auto-promoting to Docker"
        return true
    }
    false
}

# Resolve the cargo profile a hostPath deploy builds and mounts. The --hostpath
# flag doubles as the profile carrier: empty (or the bare-flag "true") means the
# default debug build. Shared by the build step and apply_hostpath_patch so the
# binary we compile can never be a different profile than the one we mount.
def resolve_hostpath_profile [hostpath_flag: string]: nothing -> string {
    if ($hostpath_flag | is-empty) or ($hostpath_flag == "true") {
        "debug"
    } else {
        $hostpath_flag
    }
}

# Build a workspace binary natively for a Linux hostPath deploy.
#
# hostPath mode mounts target/<profile>/ straight into the pod, so whatever
# binary sits there IS the deployment. Leaving the build to the operator meant a
# forgotten `cargo build --workspace` silently redeployed a stale binary: the pod
# comes up healthy, the rollout succeeds, and the change under test isn't
# running. Building here makes the deploy self-consistent — the artifact we mount
# is always compiled from the current source tree. Pass --no-build to opt out.
#
# Deliberately NOT wrapped in SQLX_OFFLINE=true, unlike the macOS cross-compile:
# a Linux hostPath build compiles against the live DB via the env overlay's
# DATABASE_URL, exactly like the `cargo build` the operator would have run by
# hand. That also means a query change that outran `dapple sqlx prepare` fails
# here the same way it fails in their own terminal, rather than compiling against
# a stale committed cache.
def build_for_hostpath [pkg: string, profile: string, features: string = ""] {
    let release_args = if $profile == "release" { ["--release"] } else { [] }
    let feature_args = if ($features | is-empty) { [] } else { ["--features" $features] }
    log info $"building ($pkg) for hostPath \(profile: ($profile))"
    cargo build -p $pkg ...$feature_args ...$release_args
}

# Cross-compile a workspace binary to aarch64-unknown-linux-gnu via cargo
# zigbuild. Called from the hostPath branch of setup_dapple_svc on macOS so
# Mac contributors don't have to remember the cross-compile command — the
# deploy script just builds the right artifact for them. Linux runs `cargo
# build` natively, so this helper is a no-op there.
#
# SQLX_OFFLINE is set to "true" so the build uses the committed `.sqlx/` query
# cache and doesn't need a live database connection. Devs whose source has
# drifted past the cached queries get a sqlx compile error pointing them at
# `dapple sqlx prepare`.
def cross_compile_for_linux_target [pkg: string, profile: string, features: string = ""] {
    let release_args = if $profile == "release" { ["--release"] } else { [] }
    let feature_args = if ($features | is-empty) { [] } else { ["--features" $features] }
    log info $"cross-compiling ($pkg) for aarch64-unknown-linux-gnu \(profile: ($profile))"
    with-env { SQLX_OFFLINE: "true" } {
        cargo zigbuild -p $pkg --target aarch64-unknown-linux-gnu ...$feature_args ...$release_args
    }
}

# Verify the macOS cross-compile toolchain is installed. Called up front so the
# deploy fails with actionable install instructions instead of dying inside a
# zigbuild invocation later.
def ensure_cross_compile_prereqs [] {
    let zig_ok = (which zig | is-not-empty)
    let zigbuild_ok = (which cargo-zigbuild | is-not-empty)
    let installed_targets = (rustup target list --installed | lines | each { str trim })
    let target_ok = ($installed_targets | any {|t| $t == "aarch64-unknown-linux-gnu" })

    if $zig_ok and $zigbuild_ok and $target_ok { return }

    log error "macOS cross-compile prerequisites missing:"
    if not $zig_ok { log error "  - zig          (install: brew install zig)" }
    if not $zigbuild_ok { log error "  - cargo-zigbuild (install: cargo install cargo-zigbuild)" }
    if not $target_ok { log error "  - rustup target aarch64-unknown-linux-gnu (install: rustup target add aarch64-unknown-linux-gnu)" }
    log error ""
    log error "Run `nu assets/nu/init/setup.nu --environment sandbox` to install all prerequisites automatically,"
    log error "or pass --docker dev to skip cross-compile and build a Docker image instead."
    error make { msg: "Cross-compile prerequisites missing" }
}

# Apply hostPath patch to mount local target directory.
#
# On macOS the binary lives under target/aarch64-unknown-linux-gnu/<profile>/
# (cross-compiled via cargo zigbuild, since Mach-O can't run in the Linux VM).
# On Linux the native target/<profile>/ is mounted directly.
def apply_hostpath_patch [cfg: record, hostpath_profile: string] {
    # Validate source path exists before applying hostPath patch
    if not ($cfg.source_path | path exists) {
        log error $"Source directory not found for hostPath: ($cfg.source_path)"
        log error "hostPath mode requires the repo to be cloned at the expected location."
        log error ""
        let expected = if ($nu.os-info.name | str contains "mac") {
            "~/Source/Dapple/dapple-prompt"
        } else {
            "~/src/dapple/dapple-prompt"
        }
        log error $"Expected: ($expected)"
        return
    }

    # Default to debug if empty or just flag provided
    let profile = (resolve_hostpath_profile $hostpath_profile)

    # Resolve the target subdirectory. On macOS we mount the cross-compiled
    # aarch64-linux build; on Linux we mount the native build.
    let is_mac = ($nu.os-info.name | str contains "mac")
    let target_dir = if $is_mac { $"aarch64-unknown-linux-gnu/($profile)" } else { $profile }

    # Validate the binary actually exists in the target dir before patching —
    # otherwise the pod will hostPath-mount an empty directory and crashloop
    # with an unhelpful "exec format error" or "no such file." The build step in
    # setup_dapple_svc should have produced this artifact on both platforms, so
    # reaching this branch means either --no-build was passed with an empty
    # target/ or the package name doesn't match the binary name.
    let bin_path = ($cfg.source_path | path join "target" | path join $target_dir | path join $cfg.name)
    if not ($bin_path | path exists) {
        log error $"Expected binary not found: ($bin_path)"
        log error "The hostPath build step should have produced this binary — did you pass --no-build?"
        log error "Re-run the deploy without --no-build, or build manually:"
        if $is_mac {
            log error $"  cargo zigbuild -p ($cfg.name) --target aarch64-unknown-linux-gnu"
        } else {
            log error $"  cargo build -p ($cfg.name)"
        }
        error make { msg: $"Binary missing for ($cfg.name) at ($bin_path)" }
    }

    # Get current user's UID/GID for proper file permissions
    let run_as_user = (id -u | str trim)
    let run_as_group = (id -g | str trim)

    log info $"applying hostPath patch for ($cfg.name) with target_dir ($target_dir) as uid ($run_as_user)"

    let patch = (k8s render "dapple-prompt-hostpath.yaml" {
        name: $cfg.name,
        namespace: $cfg.namespace,
        source_path: $cfg.source_path,
        target_dir: $target_dir,
        run_as_user: $run_as_user,
        run_as_group: $run_as_group
    })

    kubectl patch deployment $cfg.name -n $cfg.namespace --type=strategic --patch $patch
}

# Setup rust-docs documentation service
def setup_rust_docs [cfg: record, --force] {
    build_and_push_docs_image $cfg $force

    let registry_file = $"assets/k8s/registry/($cfg.name).yaml"
    if ($registry_file | path exists) {
        log info $"applying k8s registry manifest for ($cfg.name)"
        kubectl apply -n ($env.KUBE_DAPPLE_NAMESPACE) -f $registry_file

        # Force pod recreation to pull fresh image (kubectl apply doesn't trigger rollout with same tag)
        log info $"restarting deployment ($cfg.name) to pull fresh image"
        rollout_restart { name: $cfg.name, namespace: $cfg.namespace }

        # Wait for rollout
        rollout_status { name: $cfg.name, namespace: $cfg.namespace } --timeout "300s"
    }
}

# Build and push Docker image for documentation service
def build_and_push_docs_image [cfg: record, force: bool] {
    # Track changes to docs/ directory and guides
    let docs_commit = (git log -1 --format=%h -- docs/ | str trim)
    let commit_tag = $"($env.KUBE_REGISTRY_HOST)/($cfg.name):($docs_commit)"
    let latest_tag = $"($env.KUBE_REGISTRY_HOST)/($cfg.name):($env.DOCKER_DAPPLE_TAG_LATEST)"

    # Check if docs directory has uncommitted changes
    let is_dirty = ((git status --porcelain docs/ | str trim) | is-not-empty)

    if $is_dirty {
        log info $"docs/ has uncommitted changes, forcing rebuild"
    }

    let should_skip = if $force or $is_dirty {
        false
    } else {
        (do { ^docker manifest inspect $commit_tag } | complete).exit_code == 0
    }

    if $should_skip {
        log info $"Image ($commit_tag) already exists, skipping build"
        ^docker pull $commit_tag
        ^docker tag $commit_tag $latest_tag
        ^docker push $latest_tag
    } else {
        let build_args = [
            --network=host
            --file docs/service/Dockerfile.docs
            --tag $commit_tag
            --tag $latest_tag
            .
        ]

        log debug $"building docs image with tag ($commit_tag)"
        ^docker build ...$build_args

        log debug $"pushing images to registry"
        ^docker push $commit_tag
        ^docker push $latest_tag
    }
}

# Build and push prompt-db Docker image (PG18 + PostGIS + pgvector).
# Uses content hash of the Dockerfile to skip rebuilds when unchanged.
def ensure_prompt_db_image [] {
    let dockerfile_dir = "assets/docker/prompt-db"

    # Content hash from the Dockerfile
    let content_hash = (
        git ls-tree -r HEAD -- $dockerfile_dir
        | ^sha256sum
        | split row " "
        | first
        | str substring 0..12
    )

    let commit_tag = $"($env.KUBE_REGISTRY_HOST)/prompt-db:($content_hash)"
    let latest_tag = $"($env.KUBE_REGISTRY_HOST)/prompt-db:($env.DOCKER_DAPPLE_TAG_LATEST)"

    # Skip if content-hash image already exists in registry
    let should_skip = (do { ^docker manifest inspect $commit_tag } | complete).exit_code == 0

    if $should_skip {
        log info $"prompt-db image ($commit_tag) already exists, skipping build"
        ^docker pull $commit_tag
        ^docker tag $commit_tag $latest_tag
        ^docker push $latest_tag
    } else {
        log debug $"building prompt-db image with tag ($commit_tag)"
        ^docker build --tag $commit_tag --tag $latest_tag $dockerfile_dir

        log debug "pushing prompt-db images to registry"
        ^docker push $commit_tag
        ^docker push $latest_tag
    }
}