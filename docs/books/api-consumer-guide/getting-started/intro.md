# Introduction

> **Audience:** SDK Consumers · Frontend Developers · Agents

Welcome to the dapple-prompt API. This guide will help you understand how to authenticate, use the SDK, consume the REST API, and build integrations with the platform.

> **For guide authoring and workflow design** -- writing markdown directives, guide.toml, Fluent localization, Nushell tasks, and inference profiles -- see the **[Journey Planner Guide](../../journey-planner-guide/intro.md)**.

## What is dapple-prompt?

dapple-prompt is a workflow orchestration platform for LLM-powered applications. Content authors write **structured guides** that automatically derive the platform's core entities:

- **Guides** -- Structured documentation with chapters, blocks, and audience tags
- **Playbooks** -- Workflow templates derived from `:::stage` and `:::step` directives
- **Tasks** -- Workflow task definitions derived from `:::nushell` blocks
- **Inference Profiles** -- Model configuration derived from `:::inference_profile` blocks
- **Brief Packs** -- Data collection experiences derived from `:::brief_pack` blocks
- **Journeys** -- User-facing workflow sessions (created at runtime)
- **Dispatches** -- Workflow execution runs (created at runtime)

> **Guide-centric architecture:** Most entities are derived automatically from guide ingest. Direct API creation of tasks, steps, stages, and playbooks is an advanced path for cases where guide authoring doesn't apply.

## API Overview

The dapple-prompt API is a REST API that uses JSON for request and response bodies.

| Property | Value |
|----------|-------|
| Base URL | `/api/` |
| Content-Type | `application/json` |
| Authentication | `Authorization: Bearer <token>` |
| OpenAPI Spec | `GET /api.json` |
| Documentation | `GET /docs` (ReDoc) |

> **Typed SDK:** The `dapple-sdk` crate provides typed Rust methods for all API operations. If you are a Rust consumer, prefer the SDK over raw HTTP. See [SDK Overview](../sdk-overview.md).

## Quick Start

### 1. Get an API Token

Create a Personal Access Token (PAT) for programmatic access:

```bash
# Using a web session JWT
curl -X POST https://api.example.com/api/api-tokens/ \
  -H "Authorization: Bearer <your-jwt>" \
  -H "Content-Type: application/json" \
  -d '{"name": "My Integration", "expires_in_secs": 7776000}'
```

Save the returned `token` value (it's only shown once).

### 2. Ingest a Guide

The primary way to populate the platform is by ingesting a guide:

**SDK (recommended):**

```rust
use dapple_sdk::{DappleClient, IngestParams};

let client = DappleClient::with_api_token(
    "https://api.example.com",
    "dpl_your_token",
);

let response = client.guides().ingest_from_dir(
    std::path::Path::new("./my-guide"),
    IngestParams { dry_run: false, source_path: None },
).await?;
```

**curl:**

```bash
# Package the guide directory
tar czh -f my-guide.tar.gz my-guide/

# Ingest via multipart upload
curl -X POST "https://api.example.com/api/guides/ingest" \
  -H "Authorization: Bearer dpl_your_token" \
  -F "file=@my-guide.tar.gz"
```

This parses the guide, derives all entities (playbook, tasks, inference profiles, etc.), and creates them atomically.

### 3. Browse Available Guides

Discover guides through their cards:

**SDK:**

```rust
let cards = client.guides().cards(None).await?;
```

**curl:**

```bash
curl "https://api.example.com/api/guide-cards/?query=all" \
  -H "Authorization: Bearer dpl_your_token"
```

### 4. Start a Journey

Create a journey from a guide's derived playbook:

`POST /api/journeys/` requires both `guide_id` and `playbook_id` (specific version ids). The backend validates that the guide exposes that playbook version and returns 400 `GUIDE_PLAYBOOK_MISMATCH` if not.

```bash
curl -X POST https://api.example.com/api/journeys/ \
  -H "Authorization: Bearer dpl_your_token" \
  -H "Content-Type: application/json" \
  -d '{
    "objs": [{
      "name": "My First Journey",
      "guide_id": "derived-guide-uuid",
      "playbook_id": "derived-playbook-uuid",
      "brief_pack_ids": ["derived-briefpack-uuid"],
      "inference_profile_ids": ["derived-profile-uuid"]
    }]
  }'
```

## Next Steps

- [Glossary](./glossary.md) -- Learn the terminology
- [Core Concepts](./concepts.md) -- Understand the data model
- [Authentication](./authentication.md) -- Deep dive into auth methods
- [SDK Overview](../sdk-overview.md) -- The typed Rust client
- [API Patterns](../api-patterns.md) -- Request/response conventions
