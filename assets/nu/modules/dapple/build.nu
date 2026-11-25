# Build and deploy commands for dapple services
#
# Examples:
#   dapple build api              # Build and push docker image
#   dapple deploy api             # Full deploy (manifests + rollout)
#   dapple patch api              # Quick image update
#   dapple patch api --tag abc123 # Patch with specific image tag

use std/log

use ./aliases.nu [resolve-service buildable-services]

# Get the path to the k8s scripts directory
def get-k8s-scripts-dir []: nothing -> string {
    $env.DAPPLE_ASSETS_DIR | path join "nu" | path join "k8s"
}

# Build and push Docker image for a service
export def image [
    service: string     # Service alias (api, conduit, flow, rust-docs)
    --force (-f)        # Force rebuild even if image exists
    --profile: string = "dev"  # Build profile (dev, release)
] {
    let buildable = (buildable-services)
    let deployment = (resolve-service $service)

    # Validate service is buildable
    if not ($service in $buildable) and not ($deployment in $buildable) {
        log error $"Service ($service) is not buildable. Buildable services: ($buildable | str join ', ')"
        return
    }

    let script = (get-k8s-scripts-dir) | path join "build_image.nu"

    mut args = [$script $service "--profile" $profile]
    if $force {
        $args = ($args | append "--force")
    }

    nu ...$args
}

# Full deploy (manifests + rollout) using deploy_dapple_core.nu
export def deploy [
    service: string     # Service alias or comma-separated list
    --force (-f)        # Force rebuild
    --hostpath: string = ""  # Enable hostPath mode (Linux only)
] {
    let script = (get-k8s-scripts-dir) | path join "deploy_dapple_core.nu"

    mut args = [$script "--svc" $service]
    if $force {
        $args = ($args | append "--force")
    }
    if ($hostpath | is-not-empty) {
        $args = ($args | append ["--hostpath" $hostpath])
    }

    nu ...$args
}

# Quick image update (no manifest changes)
export def patch [
    service: string        # Service alias (api, conduit, flow, etc.)
    --tag: string = ""     # Specific image tag (default: dapple-latest)
    --timeout: string = "120s"  # Rollout timeout
    --restart (-r)         # hostPath mode: just restart (binary already built)
    --build (-b)           # hostPath mode: build the binary (cross-compiles on macOS) then restart
    --profile: string = "debug"  # Cargo profile for --build (debug or release)
] {
    let script = (get-k8s-scripts-dir) | path join "patch_image.nu"

    mut args = [$script $service "--timeout" $timeout]
    if ($tag | is-not-empty) {
        $args = ($args | append ["--tag" $tag])
    }
    if $build {
        $args = ($args | append ["--build" "--profile" $profile])
    }
    if $restart {
        $args = ($args | append ["--restart"])
    }

    nu ...$args
}

# Prune old Docker images, build cache, and optionally registry tags
export def prune [
    --dry-run (-d)    # Show what would be removed without removing
    --cache           # Also prune Docker build cache
    --registry        # Also prune old tags from registry
] {
    let deployments = (buildable-services | each {|svc| resolve-service $svc })

    # Prune old local images per deployment (keep latest + most recent content tag)
    for deployment in $deployments {
        let repo = $"($env.KUBE_REGISTRY_HOST)/($deployment)"
        let all_tags = (
            do { ^docker images $repo --format "{{.Tag}}" } | complete
        )
        if $all_tags.exit_code != 0 { continue }

        let tags = ($all_tags.stdout | lines | where {|tag| $tag != "<none>" and ($tag | is-not-empty) })
        if ($tags | is-empty) { continue }

        # Keep the latest tag and the most recently created content tag
        let keep_latest = $env.DOCKER_DAPPLE_TAG_LATEST
        let non_latest = ($tags | where {|tag| $tag != $keep_latest })
        # Keep the first non-latest tag (most recent by docker images ordering)
        let keep_content = if ($non_latest | is-not-empty) { $non_latest | first } else { "" }

        let remove = ($tags | where {|tag|
            $tag != $keep_latest and $tag != $keep_content
        })

        for tag in $remove {
            if $dry_run {
                print $"Would remove: ($repo):($tag)"
            } else {
                log info $"Removing ($repo):($tag)"
                do { ^docker rmi $"($repo):($tag)" } | ignore
            }
        }
    }

    # Prune Docker build cache
    if $cache {
        if $dry_run {
            print "Would prune Docker build cache"
        } else {
            log info "Pruning Docker build cache"
            ^docker builder prune --all --force
        }
    }

    # Prune old tags from registry
    if $registry {
        for deployment in $deployments {
            let catalog_result = (
                do {
                    curl -sk $"https://($env.KUBE_REGISTRY_HOST)/v2/($deployment)/tags/list"
                } | complete
            )
            if $catalog_result.exit_code != 0 { continue }

            let tag_list = (try {
                $catalog_result.stdout | from json | get tags | default []
            } catch {
                []
            })
            if ($tag_list | is-empty) { continue }

            let keep_latest = $env.DOCKER_DAPPLE_TAG_LATEST
            let remove_tags = ($tag_list | where {|tag| $tag != $keep_latest })

            for tag in $remove_tags {
                # Get manifest digest for this tag
                let manifest_result = (
                    do {
                        ^curl -sk -I -H "Accept: application/vnd.docker.distribution.manifest.v2+json" $"https://($env.KUBE_REGISTRY_HOST)/v2/($deployment)/manifests/($tag)"
                    } | complete
                )
                if $manifest_result.exit_code != 0 { continue }

                let digest_line = ($manifest_result.stdout | lines | where {|line| $line =~ "(?i)docker-content-digest" })
                if ($digest_line | is-empty) { continue }
                let digest = ($digest_line | first | split row ": " | last | str trim)

                if $dry_run {
                    print $"Would delete registry tag: ($deployment):($tag) \(($digest | str substring 7..19))"
                } else {
                    log info $"Deleting registry tag ($deployment):($tag)"
                    let del_result = (
                        do {
                            curl -sk -X DELETE $"https://($env.KUBE_REGISTRY_HOST)/v2/($deployment)/manifests/($digest)"
                        } | complete
                    )
                    if $del_result.exit_code != 0 {
                        log warning $"Failed to delete ($deployment):($tag): ($del_result.stderr)"
                    }
                }
            }
        }
    }

    if $dry_run {
        print ""
        print "Run without --dry-run to apply changes"
    } else {
        log info "Prune complete"
    }
}

# Build and patch in one command (common workflow)
export def quick [
    service: string     # Service alias
    --force (-f)        # Force rebuild
] {
    log info $"Building and patching ($service)..."

    # Build first
    if $force {
        image $service --force
    } else {
        image $service
    }

    # Then patch
    patch $service

    log info $"($service) build and patch complete"
}
