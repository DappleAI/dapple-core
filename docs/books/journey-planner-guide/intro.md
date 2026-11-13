# Introduction

Intelligence alone never built anything worthwhile. Collaboration did.

Dapple's Playbooks bring together different kinds of intelligence: LLMs for pattern recognition and language, scripts for deterministic logic, and people for judgment and creativity. Each contributes what it does best. The result is greater than any individual part.

Playbooks ensure that useful things can be done when an LLM is not the right solution, as is very often the case. They define a flexible workflow designed so that a skilled Nushell user can do near anything, while also providing the deterministic footing that makes everything predictable, debuggable, and trustworthy.

The vast majority of Playbooks will be written to help regular users navigate complex tasks without sitting through endless vague prompt engineering tutorials. A user experiences these Playbooks through **Journeys**: a structured path through every task in a Playbook. As you create a Playbook, you're crafting countless different and unique Journeys through the Dapple workflow system. This guide is intended to help you master the art of Journey Planning from both a conceptual and technical perspective to build amazing Playbooks.

> **For programmatic API consumption** -- using the SDK, making HTTP requests, managing journeys and dispatches at runtime -- see the **[API Consumer Guide](../api-consumer-guide/getting-started/intro.md)**.

---

## What Makes Playbooks Work

Playbooks are built on three principles that hold up in practice.

**The right tool for the job.** Sometimes you need Claude to extract meaning from messy text. Sometimes you need a three-line script to format a date. Playbooks let you use both exactly when and where they make sense.

**Humans are part of the intelligence.** Review points, approvals, and course corrections are first-class concepts because real workflows need real judgment at key moments.

**Deterministic does not mean rigid.** A Journey follows a predictable path even when LLM results vary. You know what happens, when, and in what order. That footing is what makes flexibility useful instead of chaotic.

Most of the Journey is deterministic code doing deterministic things. The LLM gets called at specific points where you actually need intelligence and flexibility. This is deliberate collaboration between different kinds of intelligence, all working in concert.

---

## What You'll Learn

This guide walks you through the full lifecycle of building Playbooks:

1. **Authoring guides.** How to write `guide.toml`, directive blocks, and Fluent localization files that define your workflow content.
2. **The building blocks.** Tasks, Steps, Stages, and how directive blocks organize them into Playbooks.
3. **Workflow design.** Using JSONLogic directives for flow control, writing Nushell tasks, and designing custom commands.
4. **Ingesting and validating.** How the ingest pipeline parses, transforms, and materializes your guides into platform entities.
5. **The runtime.** How the task execution engine and prompt conduit run your workflows.
6. **A complete walkthrough.** The Resume Guide Walkthrough takes you from an empty directory to a working Playbook.

By the end, you'll understand not just *how* to build Playbooks, but *why* the system works the way it does. That's the difference between fighting the framework and making it sing.

---

## Who This Guide Is For

This guide serves **Journey Planners** who design how users move through Playbooks:

| Audience | Role |
|----------|------|
| **Guide Authors** | Technical builders writing guide content, directive blocks, Nushell scripts, and LLM prompts |
| **Domain Experts** | Subject-matter experts who understand the real-world process being automated |
| **Product** | Strategic readers building vocabulary and mental models to engage meaningfully with the system |

Chapters are marked with audience tags showing who they're written for. Technical chapters include notes when Product readers can skip them.

---

## How to Read This Book

If you're the type who reads documentation front-to-back, start with the [Glossary](./glossary.md) and [Concepts](./concepts.md), then work through Part II (Authoring Guides) beginning with [Guide Directory Structure](./guide-directory-structure.md). You'll have a complete mental model by the end.

If you learn by doing, skip straight to the [Resume Guide Walkthrough](./resume-guide-walkthrough.md) and refer back to earlier chapters when something doesn't make sense.

If you're here to debug a specific problem, the [Task Execution Runtime](./dapple-prompt-flow.md) and [Prompt Execution and Routing](./dapple-prompt-conduit.md) chapters have the technical details you need.

---

Let's get started.
