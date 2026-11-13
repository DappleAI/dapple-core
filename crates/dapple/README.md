# dapple

CLI utilities crate for Dapple Prompt.

## Overview

The `dapple` crate is intended to provide command-line utilities for development and operations tasks related to the Dapple ecosystem.

## Status

This crate is currently a placeholder. Future functionality may include:

- Database migration utilities
- Data seeding tools
- Development helpers
- Operations scripts

## Usage

```bash
# Build the crate
cargo build -p dapple
```

## Development

When adding new functionality to this crate:

1. Add dependencies to `Cargo.toml`
2. Implement CLI commands or library functions in `src/`
3. Add tests for new functionality
4. Update this README with usage instructions

## References

- [CLAUDE.md](../../CLAUDE.md) — Project conventions
- [dapple-data](../dapple-data/README.md) — Data layer for database operations
- [dapple-prompt-api](../dapple-prompt-api/README.md) — REST API service
