# Local Development

> **Audience:** Backend Developers · Platform Engineers

Setting up and running the dapple-prompt stack locally.

## Prerequisites

- Rust toolchain (latest stable)
- PostgreSQL 14+
- Redis (optional, uses noop fallback)
- Node.js (for web frontend)
- Nushell (for deployment scripts)

## Platform Differences

### Linux

MicroK8s runs natively on the host. hostPath volumes work directly with the filesystem.

### macOS

MicroK8s runs inside an [OrbStack](https://orbstack.dev)-hosted Ubuntu VM.
OrbStack auto-mounts `/Users/$USER` two-way into the VM at the same path,
so hostPath volumes referencing your home directory work transparently.
ONNX model paths under `~/.cache/dapple-onnx` are visible inside the VM
without per-path registration. GPU acceleration is limited compared to the
Linux native path.

OrbStack is a paid product — see
[orbstack-setup.md](./orbstack-setup.md) for the full onboarding walkthrough,
including the one-shot provisioning script
(`nu assets/nu/k8s/install_orbstack_vm.nu`).

## Nushell Environment Setup

Before running deployment scripts, set up the nushell environment:

```bash
# Run the setup script
nu assets/nu/init/setup.nu --environment sandbox

# Restart nushell to pick up changes
exec nu

# Verify environment is configured correctly
dapple verify
```

This installs:
- Dapple CLI module with service aliases
- Environment profile overlays
- K8s utility functions

## Environment Setup

### Database

```bash
# Create database
createdb dapple_dev

# Run migrations (NEVER use psql -f)
cargo sqlx migrate run

# Check migration status
cargo sqlx migrate info
```

### Environment Variables

Create `.env` or `.env.test`:

```bash
# Required
DAPPLE_PROMPT_DATABASE_URL=postgresql://user:pass@localhost/dapple_dev

# LLM Backends
ANTHROPIC_API_KEY=sk-ant-...
MISTRAL_ENABLED=false

# Optional
REDIS_URL=redis://localhost:6379
RUST_LOG=debug
```

## Building

```bash
# Format and lint
cargo fmt
cargo clippy -- -D warnings

# Build workspace
cargo build --workspace

# Build specific crate
cargo build -p dapple-data
cargo build -p dapple-prompt-api
cargo build -p dapple-prompt-conduit
```

## Running Services

### API Server

```bash
cargo run -p dapple-prompt-api
# Runs on http://localhost:8080
```

### Conduit Service

```bash
# With Anthropic
ANTHROPIC_API_KEY=sk-ant-... cargo run -p dapple-prompt-conduit

# With local Mistral
MISTRAL_ENABLED=true cargo run -p dapple-prompt-conduit

# Testing mode (stub only)
cargo run -p dapple-prompt-conduit
```

### ONNX Models for Conduit

Conduit uses local ONNX models for embedding, summarization, and TTS. Convert models before first use:

```bash
# Convert all models (embedding, mlm, summarization, tts)
nu assets/nu/k8s/convert_onnx_models.nu

# Verify models are ready
nu assets/nu/k8s/convert_onnx_models.nu verify

# Download specific TTS model
nu assets/nu/k8s/convert_onnx_models.nu tts vits-ljspeech
```

Models are stored in `~/.cache/dapple-onnx/` by default. Override with:

```bash
export DAPPLE_ONNX_CACHE_DIR=/custom/path
export ONNX_MODEL_PATH=$DAPPLE_ONNX_CACHE_DIR
```

## Testing

```bash
# Run all tests
cargo test --workspace

# Run specific test
cargo test --test user-lifecycle --package dapple-prompt-api

# Run with logging
RUST_LOG=debug cargo test

# Run with strict Redis requirement
DAPPLE_REDIS_STRICT=1 cargo test
```

### Test Database

Tests use `#[sqlx::test]` which creates isolated test databases automatically.

## SQLx Offline Mode

For CI/CD or when database is unavailable:

```bash
# Generate query metadata (requires DAPPLE_PROMPT_DATABASE_URL).
# Preferred: `dapple sqlx prepare`. `-- --all-targets` keeps the cache covering
# test-target inline `sqlx::query!` macros, which CI's offline build needs.
SQLX_OFFLINE=false cargo sqlx prepare --workspace -- --all-targets

# Build with offline mode
SQLX_OFFLINE=true cargo build
```

After changing `.sqlc` files:

```bash
# Regenerate SQL from sources
cd crates/dapple-data
cargo sqlc compose --source assets/sqlc/ --target assets/.sql/

# Then regenerate SQLx metadata (preferred: `dapple sqlx prepare`)
cargo sqlx prepare --workspace -- --all-targets

# Commit both generated directories
git add assets/.sql/ .sqlx/
```

## API Documentation

Access documentation when API server is running:

- OpenAPI Spec: `http://localhost:8080/api.json`
- ReDoc: `http://localhost:8080/docs`

## Directory Structure

```
dapple-prompt/
├── crates/
│   ├── dapple-data/           # Data access layer
│   │   ├── assets/sqlc/       # SQL source files (.sqlc — edit these)
│   │   ├── assets/.sql/       # Generated SQL output (never edit)
│   │   ├── migrations/        # Database migrations
│   │   └── src/
│   │       ├── model/         # Type definitions
│   │       └── query/         # Query functions
│   ├── dapple-prompt-api/     # REST API
│   │   ├── src/
│   │   │   ├── handler/       # Request handlers
│   │   │   ├── route/         # Route definitions
│   │   │   └── schema/        # Request/response types
│   │   └── tests/             # Integration tests
│   ├── dapple-prompt-conduit/ # LLM routing
│   │   └── src/
│   │       └── service/       # Backend services
│   └── dapple-prompt-flow/    # Workflow engine
│       └── src/
│           └── engine/        # StepEngine
└── docs/
    └── guides/                # Documentation
```

## Common Tasks

### Add a New Endpoint

1. Create `.sqlc` source files in `assets/sqlc/{resource}/`
2. Run `cargo sqlc compose --source assets/sqlc/ --target assets/.sql/`
3. Add models to `src/model/{resource}.rs`
4. Add query functions to `src/query/{resource}.rs` (alphabetical bind order)
5. Add handler to `src/handler/{resource}.rs`
6. Add route to `src/route/{resource}.rs`
7. Add test to `tests/`
8. Run `dapple sqlx prepare` (raw: `cargo sqlx prepare --workspace -- --all-targets`)

### Add a Database Migration

1. Create `migrations/YYYYMMDD_description.sql`
2. Run migration: `cargo sqlx migrate run`
3. Run `dapple sqlx prepare` (raw: `cargo sqlx prepare --workspace -- --all-targets`)
4. Test: `cargo test`

### Update a Query

1. Edit `.sqlc` source file in `assets/sqlc/`
2. Run `cargo sqlc compose --source assets/sqlc/ --target assets/.sql/`
3. Run `dapple sqlx prepare` (raw: `cargo sqlx prepare --workspace -- --all-targets`)
4. Fix any compile errors
5. Run tests
