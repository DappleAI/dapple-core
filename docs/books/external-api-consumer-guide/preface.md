# Preface

<div style="text-align: center; margin: 1em 0 2em 0;">
<strong style="font-size: 1.2em;">Version 0.1.0</strong><br/>
<a href="./CHANGES.md">What's new in this version?</a>
</div>

Welcome to the **External API Consumer Guide** for dapple-prompt.

## Who This Guide Is For

This guide is for **customer companies** integrating with dapple-prompt through the public external surface (`external-api.dapple.x`). If you are a third party building on top of dapple, this is your book:

- **Customer Integration Engineers** wiring a customer application into dapple-prompt
- **Backend Developers** at a customer company calling the external REST surface
- **DevOps Engineers** managing the Ed25519 keypair and token rotation
- **AI Agents** acting on behalf of a customer company

> **Are you a dapple-internal service?** Then you want the **internal** surface (`api.dapple.x`) instead. See the [API Consumer Guide](../api-consumer-guide/preface.md). The two surfaces are cryptographically isolated: a token minted for one cannot be used on the other.

## What Makes the External Surface Different

The external surface is deliberately narrow and strictly isolated:

- **Cryptographic isolation.** The external surface signs JWTs with its own key. A JWT from the internal surface fails signature verification here, and vice versa.
- **Per-company data isolation.** Every request is scoped to your company. There are no "list all" endpoints, and content from another company is structurally invisible to you.
- **Automatic execution.** You do not drive tasks by hand. Creating a dispatch (or submitting the value-slot bindings it needs) triggers execution automatically once the dispatch is ready.
- **A smaller endpoint set.** The external V1 surface covers token exchange, guide listing and ingest, journeys, dispatches, and bindings. It does not expose the internal admin/configuration endpoints.

## What You'll Learn

- How to exchange an Ed25519-signed token for a JWT
- The journey-centric vocabulary and the generate → pick → refine step model
- How to list and ingest your guides
- How to create journeys, collect debrief input, and let dispatches execute automatically
- How to poll runtime status across steps, stages, and dispatches
- How to interpret the surface's deliberate 404-for-foreign-ids no-leak behavior

## Prerequisites

- Basic understanding of REST APIs and JSON
- Familiarity with HTTP methods (GET, POST, PATCH)
- An onboarded company record and a registered Ed25519 keypair (see [Authentication](./getting-started/authentication.md))

## How to Use This Guide

1. Start with [Introduction](./getting-started/intro.md) for orientation and a quick start
2. Read [Authentication](./getting-started/authentication.md) to exchange a token for a JWT
3. Review [Core Concepts](./getting-started/concepts.md) for the journey vocabulary
4. Use [Endpoint Reference](./endpoint-reference.md) for the full V1 surface
5. Follow [Customer Runtime Flow](./runtime-operations.md) to run a journey end to end

## Feedback

If you find errors or have suggestions, please reach out to your dapple contact.
