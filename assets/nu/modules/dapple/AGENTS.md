# AGENTS.md - Dapple CLI Module

This document provides guidance for AI agents working with the dapple CLI module.

## Module Overview

The dapple CLI is a nushell module that provides developer-friendly wrappers around kubectl and deployment scripts. It simplifies Kubernetes operations for the Dapple development cluster.

**Location:** `assets/nu/modules/dapple/`

**Purpose:** Enable developers (especially web team on Mac) to perform common k8s operations without memorizing kubectl commands.

## Architecture

```
dapple/
├── mod.nu           # Main module entry point, exports all commands
├── aliases.nu       # Service alias resolution (api -> dapple-prompt-api)
├── logs.nu          # Log viewing (wraps kubectl logs)
├── status.nu        # Status/describe (wraps kubectl get/describe)
├── lifecycle.nu     # restart/cleanup/scale operations
├── build.nu         # Build/deploy/patch image operations
├── access.nu        # shell/port-forward/url commands
├── guide.nu         # Guide authoring: local pre-sync validation
├── README.md        # User documentation
└── AGENTS.md        # This file
```

### `dapple guide validate` (local pre-sync check)

`guide.nu` wraps the `guide-validate` binary
(`crates/dapple-prompt-api/src/bin/guide-validate.rs`, built with
`SQLX_OFFLINE=true cargo build -p dapple-prompt-api --bin guide-validate`),
which calls `dapple_prompt_api::handler::guide_ingest::validate_guide_directory`
— the exact `parse_guide` → `plan_guide` → `validate_ingest_plan` pipeline the
ingest endpoints run, DB-free. Task scripts are parsed by the **embedded**
StepEngine nu-parser, which is stricter than an interactive system nushell
(e.g. `str lowercase` — real command `str downcase` — parses interactively but
422s at ingest as "Extra positional argument"). Run it on a guide directory
before syncing; it itemizes every failing task script, not just the first.

## Key Patterns

### Service Alias Resolution

All commands accept short aliases instead of full deployment names:

```nu
# In aliases.nu
export def resolve-service [alias: string] -> string {
    match $alias {
        "api" => "dapple-prompt-api"
        "conduit" => "dapple-prompt-conduit"
        ...
        _ => $alias  # Pass through full names
    }
}
```

When adding new services, update `aliases.nu` in three places:
1. `resolve-service` match statement
2. `list-aliases` function
3. `buildable-services` if the service can be built

### Environment Variables

The module depends on environment variables set by `dapple_sandbox` overlay:
- `KUBE_DAPPLE_NAMESPACE` - Target namespace (usually `dapple-sandbox`)
- `KUBE_REGISTRY_HOST` - Docker registry host
- `DOCKER_DAPPLE_TAG_LATEST` - Default image tag
- `DAPPLE_ASSETS_DIR` - Path to assets directory

### Command Export Pattern

Commands are exported using `export module` for submodules with `main`, and wrapper functions for additional exports:

```nu
# In status.nu - main becomes the module command
export def main [...] { ... }           # Becomes `dapple status` via `export module`
export def describe [...] { ... }       # Additional command

# In mod.nu
export module ./status.nu               # Makes `main` available as `dapple status`

# Wrapper for additional functions (needed due to spread arg limitations)
use ./status.nu
export def describe [service: string] {
    status describe $service
}
```

**Nushell 0.110.0 Notes:**
- The `[main as alias]` import syntax is no longer supported
- Use `export module ./file.nu` to re-export a module's `main` as the module name
- A module cannot export a function with the same name as the module file (e.g., `build.nu` cannot export `def build`)
- For modules with conflicting names, rename the internal function (e.g., `image` in build.nu, wrapped as `build` in mod.nu)

## Common Tasks for Agents

### Adding a New Command

1. Choose the appropriate submodule based on command category
2. Add the exported function with proper documentation
3. Update `mod.nu` to re-export the command
4. Update the help text in `mod.nu main`
5. Add examples to README.md

Example:
```nu
# In lifecycle.nu
export def drain [
    service: string
    --timeout: string = "60s"
] {
    let deployment = (resolve-service $service)
    let ns = $env.KUBE_DAPPLE_NAMESPACE
    # ... implementation
}
```

### Adding a New Service Alias

1. Update `aliases.nu`:
   - Add case to `resolve-service`
   - Add entry to `list-aliases`
   - Add to `buildable-services` if applicable

2. Update README.md service alias table

### Modifying Build/Deploy Logic

The build commands in `build.nu` delegate to standalone scripts:
- `nu/k8s/build_image.nu` - Builds Docker images
- `nu/k8s/patch_image.nu` - Updates deployments with new images
- `nu/k8s/deploy_dapple_core.nu` - Full deployment

When modifying build logic, consider:
- Should the change be in the CLI wrapper or the underlying script?
- Does it affect both `dapple build` and `dapple deploy`?
- Is the change specific to certain services?

## Testing Changes

After modifying the module:

1. **Syntax check:**
   ```nu
   nu -c "use modules/dapple"
   ```

2. **Verify commands load:**
   ```nu
   dapple --help
   dapple status
   ```

3. **Test against cluster (if connected):**
   ```nu
   dapple verify
   dapple status api
   ```

## Error Handling

Commands should:
- Use `std/log` for status messages (`log info`, `log warning`, `log error`)
- Return early with `log error` for validation failures
- Use `error make { msg: "..." }` for fatal errors
- Provide helpful troubleshooting hints in error messages

## Dependencies

The module depends on:
- **nushell stdlib:** `use std/log`
- **kubectl:** Must be configured and authenticated
- **dapple_sandbox overlay:** Provides environment variables
- **Build scripts:** `nu/k8s/*.nu` for build/deploy operations

## Integration Points

### With setup.nu

The setup script installs this module and configures `config.nu`:
```nu
use modules/dapple
```

### With dapple_env_profiles

The module expects the `dapple_sandbox` overlay to be active, which sets up:
- Cluster connection details
- Registry authentication
- Database URLs

### With deploy_dapple_core.nu

The `dapple deploy` command wraps this script. The standalone script handles:
- SQLx migrations
- Service token setup
- Full manifest application
- Image building

## Common Issues

### Missing Environment Variables

If commands fail with "variable not found", the dapple overlay may not be loaded:
```nu
# Check if overlay is active
overlay list | find dapple

# Reload if needed
use modules/dapple_env_profiles dapple_sandbox_tools
overlay use dapple_sandbox_tools as dapple_env
```

### kubectl Context Issues

Commands may fail if kubectl is pointing to wrong cluster:
```bash
kubectl config current-context
kubectl config use-context microk8s
```

### Module Not Found

After modifying module files, nushell may cache old versions:
```nu
# Force reload
exec nu
```

## Style Guidelines

1. **Command naming:** Use verbs (`build`, `restart`, `cleanup`)
2. **Flag naming:** Use `--long-name` with `-s` short form
3. **Documentation:** Include examples in function comments
4. **Defaults:** Prefer sensible defaults over required flags
5. **Output:** Use `print` for user-facing output, `log` for status
