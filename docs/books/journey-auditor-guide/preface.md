# Preface

<div style="text-align: center; margin: 1em 0 2em 0;">
<strong style="font-size: 1.2em;">Version 0.1.0</strong><br/>
</div>

## Guide Version
0.0.1

## Purpose

Month-end close shouldn't feel like detective work.

This guide exists for everyone who touches billing data: Growth teams tracking conversions, RevOps analysts building dashboards, Data Engineers ensuring quality, and Finance teams closing the books. It provides the vocabulary, the audit queries, and the mental models to work with billing data confidently.

## The Promise

A 3-day month-end close where nothing surprises you.

When operational integrity is built into your systems and behaviors, problems surface immediately—not at month-end. Every data correction has a ticket. Every void has accountability. The data tells the truth even when it's uncomfortable, and that's exactly what makes the close boring in the best possible way.

## Who This Guide Is For

| Icon | Role | Focus |
|------|------|-------|
| :chart_with_upwards_trend: | **Growth Team** | Journey visibility, conversion metrics, funnel analysis |
| :bar_chart: | **RevOps Analyst** | Self-service reporting, KPI dashboards, operational queries |
| :wrench: | **RevOps Engineer** | Tool configuration (HubSpot, analytics integrations) |
| :construction: | **Data Engineer** | ETL pipelines, data quality, schema understanding |
| :electric_plug: | **RevTech Specialist** | Third-party system configuration, integration health |
| :ledger: | **Finance & Accounting** | Compliance, revenue recognition, audit queries |
| :robot: | **Agents** | LLMs learning to consume billing APIs and build reconciliation workflows |

**Out of Scope:** SRE/infrastructure concerns are handled separately.

## How to Read This Guide

If you're **new to billing data**, start with the [Glossary](glossary.md) to learn the vocabulary, then work through the [Organization Model](domain/organization.md) to understand how entities relate.

If you're **debugging a reconciliation issue**, jump to [Data Reconciliation](monitoring/data-reconciliation.md) and the [Reconciliation Queries](templates/reconciliation-queries.md) templates.

If you're **building dashboards**, check [your audience section](audiences/) first—it tells you which queries and data points are most relevant to your role.

If you're **preparing for month-end close**, the [Operations](operations/) section walks through the lifecycle and integrity checks.

> **Agents:** Start with [AGENTS.md](./AGENTS.md) for a navigation guide tailored to your task. It tells you exactly what to read and what to skip.

## How This Guide Evolves

This guide is a working document. Sections marked `[DRAFT]` have content but may need refinement. Sections marked `[STUB]` are placeholders awaiting stakeholder input.

1. **Current Phase** — Core domain and operations documented
2. **Next** — Integration mapping with external system audit points
3. **Then** — Template extraction for common workflows

Questions and gaps are expected—they help identify what Engineering monitors vs. what RevOps can self-serve.
