#!/usr/bin/env nu
# Build and push Docker image for a service
#
# Usage:
#   nu build_image.nu api           # Build dapple-prompt-api
#   nu build_image.nu conduit -f    # Force rebuild
#   nu build_image.nu rust-docs     # Build docs image

use std/log

$env.NU_LOG_LEVEL = "info"

use ../modules/dapple_env_profiles dapple_sandbox_tools
overlay use dapple_sandbox_tools as dapple_env

use ../modules/k8s
use k8s setup_docker_registry_cert

# Service alias to deployment name mapping
def resolve-service [alias: string]: nothing -> string {
    match $alias {
        "api" => "dapple-prompt-api"
        "conduit" => "dapple-prompt-conduit"
        "flow" => "dapple-prompt-flow"
        "rust-docs" | "docs" => "rust-docs"
        _ => $alias  # Pass through full names
    }
}

# Check if a service is the docs service
def is-docs-service [service: string]: nothing -> bool {
    $service in ["rust-docs", "docs"]
}

def main [
    service: string  # Service alias (api, conduit, flow, rust-docs)
    --force (-f)     # Force rebuild even if image exists
    --profile: string = "dev"  # Build profile (dev, release)
] {
    let deployment = (resolve-service $service)

    # Ensure Docker registry certificate is configured
    setup_docker_registry_cert $env.KUBE_REGISTRY_HOST $env.KUBE_ROOT_CA_CERT_NAME

    cd $env.DAPPLE_PROMPT_WORKSPACE_DIR

    if (is-docs-service $service) {
        build_and_push_docs_image $deployment $force
    } else {
        build_and_push_image $deployment $profile $force
    }
}

# Build and push Docker image for a Rust service
def build_and_push_image [deployment: string, profile: string, force: bool] {
    let crate_path = $"crates/($deployment)"

    # Content hash covers EVERY input that can affect the produced binary.
    # The previous implementation hashed only `$crate_path` + `Cargo.lock`,
    # which silently skipped rebuilds when a workspace dependency crate
    # (e.g. `dapple-data`) changed without bumping `Cargo.lock` — path
    # dependencies don't bump versions, so Cargo.lock stays stable and
    # the hash didn't notice the change. Result: a stale binary kept
    # being retagged as `:dapple-latest` and pushed.
    #
    # The set below mirrors what the Dockerfile actually uses to build:
    # - `crates/`     all workspace source (including transitive deps)
    # - `Cargo.toml`  workspace manifest (resolver, members, profile)
    # - `Cargo.lock`  exact dep versions
    # - `Dockerfile`  build instructions
    # - `.dockerignore`  what gets shipped to the build daemon
    # - `.sqlx/`      sqlx offline metadata read at compile time
    #
    # Trade-off: any change to ANY workspace crate triggers a rebuild
    # of EVERY service. That's slower than a per-service hash, but the
    # alternative (silent stale binary) is much worse.
    let hash_inputs = ["crates/" "Cargo.toml" "Cargo.lock" "Dockerfile" ".dockerignore" ".sqlx/"]
    let content_hash = (
        git ls-tree -r HEAD -- ...$hash_inputs
        | ^sha256sum
        | split row " "
        | first
        | str substring 0..12
    )
    let commit_tag = $"($env.KUBE_REGISTRY_HOST)/($deployment):($content_hash)"
    let latest_tag = $"($env.KUBE_REGISTRY_HOST)/($deployment):($env.DOCKER_DAPPLE_TAG_LATEST)"

    # Check if any of the hash-relevant paths have uncommitted changes.
    # Mirrors the content_hash inputs so the dirty check catches local
    # edits to workspace deps, not just the immediate service crate.
    let is_dirty = ((git status --porcelain ...$hash_inputs | str trim) | is-not-empty)

    if $is_dirty {
        log info $"Workspace has uncommitted changes affecting ($deployment), forcing rebuild"
    }

    # Skip only if: not forced, not dirty, and content-hash image exists
    let should_skip = if $force or $is_dirty {
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
        prune_old_images $deployment $content_hash
        print $commit_tag
        return
    }

    mut build_args = [
        --build-arg $"DAPPLE_PROMPT_DATABASE_URL=($env.DAPPLE_PROMPT_INTERNAL_POSTGRES_URL)"
        --build-arg $"BIN=($deployment)"
        --build-arg $"PROFILE=($profile)"
        --tag $commit_tag
        --tag $latest_tag
    ]

    # --network=host only works on Linux; on macOS Docker Desktop it breaks
    # DNS resolution inside BuildKit, causing wget/curl failures.
    if $nu.os-info.name == "linux" {
        $build_args = ($build_args | prepend "--network=host")
    }

    # Dirty tree: purge all BuildKit caches (layer cache AND cache mounts like sccache)
    # --no-cache alone does NOT clear --mount=type=cache volumes
    if $is_dirty {
        log info "dirty tree detected — pruning BuildKit cache and building fresh"
        do { ^docker builder prune --all --force } | ignore
        $build_args = ($build_args | prepend "--no-cache")
    }

    $build_args = ($build_args | append ".")

    # Remove stale local images BEFORE building so a failed build
    # cannot fall through to push an old cached image with the same tag.
    do { ^docker rmi $commit_tag } | ignore
    do { ^docker rmi $latest_tag } | ignore

    log info $"Building docker image ($deployment) with tag ($commit_tag)"
    ^docker build ...$build_args

    # Get image ID using Docker's --format for reliable parsing
    let image_id = (^docker images -f $"reference=($commit_tag)" --format "{{.ID}}" | str trim)

    if ($image_id | is-empty) {
        error make { msg: $"No image found matching ($commit_tag)" }
    }

    log info $"Built image ($commit_tag) with ID ($image_id)"

    log info "Pushing images to registry"

    # Push commit-tagged image first
    let push_commit_result = (^docker push $commit_tag | complete)
    if $push_commit_result.exit_code != 0 {
        let error_msg = $push_commit_result.stderr
        let is_cert_error = (($error_msg | str contains "certificate") or ($error_msg | str contains "x509") or ($error_msg | str contains "unknown authority"))
        if $is_cert_error {
            let uses_desktop = (($nu.os-info.name | str contains "mac") or ($env.DAPPLE_DOCKER_DESKTOP? | default "" | str trim) == "true")
            log error "Docker push failed due to certificate verification error"
            if $uses_desktop {
                log error "Docker Desktop may need to be restarted to pick up the certificate"
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

    log info $"Successfully pushed ($commit_tag)"
    prune_old_images $deployment $content_hash
    print $commit_tag
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

# Build and push Docker image for documentation service
def build_and_push_docs_image [deployment: string, force: bool] {
    # Track changes to docs/ directory
    let docs_commit = (git log -1 --format=%h -- docs/ | str trim)
    let commit_tag = $"($env.KUBE_REGISTRY_HOST)/($deployment):($docs_commit)"
    let latest_tag = $"($env.KUBE_REGISTRY_HOST)/($deployment):($env.DOCKER_DAPPLE_TAG_LATEST)"

    # Check if docs directory has uncommitted changes
    let is_dirty = ((git status --porcelain docs/ | str trim) | is-not-empty)

    if $is_dirty {
        log info "docs/ has uncommitted changes, forcing rebuild"
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
        print $commit_tag
        return
    }

    let build_args = [
        --network=host
        --file docs/service/Dockerfile.docs
        --tag $commit_tag
        --tag $latest_tag
        .
    ]

    log info $"Building docs image with tag ($commit_tag)"
    ^docker build ...$build_args

    log info "Pushing images to registry"
    ^docker push $commit_tag
    ^docker push $latest_tag

    log info $"Successfully pushed ($commit_tag)"
    print $commit_tag
}
