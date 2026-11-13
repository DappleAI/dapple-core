# Preface

<div style="text-align: center; margin: 1em 0 2em 0;">
<strong style="font-size: 1.2em;">Version 0.2.0</strong><br/>
<a href="./CHANGES.md">What's new in this version?</a>
</div>

Welcome to the **API Consumer Guide** for dapple-prompt.

## Who This Guide Is For

This guide is designed for developers and agents who **programmatically interact** with the dapple-prompt platform:

- **SDK Consumers** using the `dapple-sdk` Rust crate for typed platform access
- **Frontend Developers** building web or mobile applications that consume the REST API
- **Integration Engineers** connecting external systems to dapple-prompt
- **Automation Engineers** writing scripts and CI/CD pipelines that interact with the API
- **AI Agents** that need to understand API patterns for programmatic access

> **For guide authoring and workflow design** (writing markdown directives, guide.toml, Fluent localization, Nushell tasks, inference profiles), see the **[Journey Planner Guide](../journey-planner-guide/preface.md)**.

## What You'll Learn

- How to authenticate with the API using different token types
- How to use the Dapple SDK for typed Rust access
- API patterns including batch operations, query semantics, and response formats
- How to ingest guides using the SDK or HTTP API
- How to work with guide cards, favorites, and PDF rendering
- How to create and manage directives for flow control
- How to work with Journeys, Briefs, and Dispatches at runtime
- Common recipes for typical integration scenarios

## Prerequisites

- Basic understanding of REST APIs and JSON
- Familiarity with HTTP methods (GET, POST, PATCH, DELETE)
- Access to a dapple-prompt instance (development or production)

## How to Use This Guide

1. Start with [Getting Started](./getting-started/intro.md) for orientation
2. Review [Authentication](./getting-started/authentication.md) to set up API access
3. Read [SDK Overview](./sdk-overview.md) for the typed Rust client
4. Study [API Patterns](./api-patterns.md) to understand request/response semantics
5. Use the API Reference chapters for specific endpoint documentation
6. Reference the [API Cookbook](./api-cookbook.md) for copy-paste examples

## Related Guides

- **Journey Planner Guide** -- For guide authoring, workflow design, and runtime internals
- **Journey Builder Guide** -- For backend development and system internals

## Feedback

If you find errors or have suggestions, please open an issue on GitHub.
