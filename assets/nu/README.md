# Dapple Nushell Scripts

Nushell scripts and modules for deploying and managing dapple-prompt infrastructure.

## Setup

Before running any scripts, install the modules to your nushell config:

```bash
nu assets/nu/init/setup.nu --environment sandbox
```

This copies modules from `assets/nu/modules/` into your nushell config dir's
`modules/` — `~/.config/nushell/modules/` on Linux,
`~/Library/Application Support/nushell/modules/` on macOS (the script targets
`$nu.default-config-dir` automatically).

## Deploy Services

```bash
nu assets/nu/k8s/deploy_dapple_prompt_pods.nu
```

**Note**: After deploying/updating secrets, you must restart deployments to pick up new values:
```bash
kubectl rollout restart deployment/<name> -n dapple-sandbox
```

## Available Tools

- `modules/tools/service_token.nu` - Service token management
- `modules/tools/endpoint.nu` - API endpoint testing

See `AGENTS.md` for detailed documentation.
