# Introduction

Operational integrity isn't compliance theater. It's the foundation that makes everything else possible.

When your billing data tells the truth, month-end close becomes routine instead of frantic. When every correction has a ticket, you can explain any number to any auditor. When problems surface in real-time instead of at close, you fix them while context is fresh instead of reconstructing history.

This guide teaches you how to build and maintain that foundation—not through heroic effort, but through systems and behaviors that make integrity the path of least resistance.

---

## What Makes This Work

Operational integrity rests on three principles that hold up in practice.

**Problems surface immediately, not at month-end.** Every billable action flows through a pipeline that validates, correlates, and reconciles. When something doesn't match, you know within hours—not weeks. Real-time monitoring means small problems stay small.

**Every fix has a ticket, every void has accountability.** No silent corrections. No "we'll figure it out later." When data changes, there's a record of who changed it, why, and what ticket authorized it. This isn't bureaucracy—it's how you sleep well at night and answer questions from auditors with confidence.

**The data tells the truth even when it's uncomfortable.** Usage records are append-only. Voids don't delete—they mark and explain. Amendments create new records with full lineage. The historical record stays intact even as you correct forward. Reconciliation queries prove consistency, not just assert it.

---

## What You'll Learn

This guide walks you through billing operations from the ground up:

1. **The vocabulary.** [Glossary](./glossary.md) establishes shared terminology so everyone speaks the same language—Lago terms, internal terms, and how they map.

2. **The domain.** How organizations, users, and billable entities relate. How usage flows from user actions to invoices. How revenue gets recognized.

3. **The operations.** Journey lifecycle from selection to completion. Event correlation between our systems and Lago. The integrity framework that keeps everything honest.

4. **The monitoring.** Audit queries that prove your data is consistent. Reconciliation checks between internal records and external systems.

5. **The templates.** Copy-paste queries for common scenarios. Dashboards for each role. Checklists for month-end.

By the end, you'll understand not just *what* the billing data means, but *how* to verify it's correct and *why* the system is designed for auditability.

---

## Who This Guide Is For

This guide serves teams across the revenue operations spectrum:

| Audience | What They'll Find |
|----------|-------------------|
| **Growth & RevOps** | Funnel queries, conversion metrics, attribution tracking |
| **Data Engineering** | Schema reference, ETL patterns, data quality checks |
| **Finance** | Revenue recognition rules, month-end queries, audit trail |
| **Technical Teams** | Integration details, implementation specs, API patterns |

Chapters are written for human readers first. Implementation specifications (marked with `<!-- ANCHOR: impl-* -->`) provide the technical details agents and engineers need without cluttering the narrative.

---

## How to Read This Guide

If you're the type who reads documentation front-to-back, start with the [Glossary](./glossary.md) and work your way through. You'll have a complete mental model by the end.

If you learn by doing, jump to [Templates](./templates/) and find a query that matches what you're trying to accomplish. Work backwards to understand the concepts.

If you're here to debug a specific issue, the [Monitoring](./monitoring/) section has the diagnostic queries you need.

---

Let's get started.
