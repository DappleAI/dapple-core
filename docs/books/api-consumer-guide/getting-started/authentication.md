# Authentication

> **Audience:** Frontend Developers · Integration Engineers · Agents

This chapter covers the authentication methods available for the dapple-prompt API.

## Overview

{{#include ../../_foundation/authentication/overview.md}}

---

## API Tokens (Recommended for Integrations)

For programmatic access, we recommend using Opaque Personal Access Tokens (PATs).

{{#include ../../_foundation/authentication/api-tokens.md}}

---

## Web Sessions

For browser-based applications using NextAuth.js:

{{#include ../../_foundation/authentication/web-sessions.md}}

---

## Best Practices

{{#include ../../_foundation/authentication/best-practices.md:security-checklist}}

{{#include ../../_foundation/authentication/best-practices.md:token-storage}}
