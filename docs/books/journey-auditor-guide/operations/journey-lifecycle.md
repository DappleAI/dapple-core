# Journey Lifecycle

Playbook selection, Journey states, and Dispatch execution tracking.

---

## A Complete Journey: Resume Generator

Let's follow Sarah through a real Journey to see how user activity connects to billing.

### Day 1: Getting Started

**10:00 AM — Sarah Selects a Playbook**

Sarah visits Dapple and picks the "Resume Generator" Playbook. Here's what happens:

| Event | System Action | Billing Impact |
|-------|---------------|----------------|
| Playbook selected | Journey created, BriefPack assigned | None yet |
| Journey ID: `j_abc123` | State: `created` | Clock starts for engagement metrics |

**10:02 AM — Debrief Begins**

Sarah starts the interview. The first BriefGroup asks about her work history.

| Event | System Action | Billing Impact |
|-------|---------------|----------------|
| BriefGroup 1 displayed | Session marked `active` | View ability logged (free) |
| Sarah enters 3 jobs | Responses captured | Input saved, no charge |
| BriefGroup 1 completed | Next group loaded | Completion rate: 33% |

**10:15 AM — Debrief Complete**

Sarah finishes all three BriefGroups (work history, skills, target role).

| Event | System Action | Billing Impact |
|-------|---------------|----------------|
| All BriefGroups done | Debrief complete, ready for dispatch | Total debrief time: 13 min |

### Day 1: Generation

**10:16 AM — Dispatch Runs**

Sarah clicks "Generate." The system executes three LLM tasks:

| Task | What It Does | Token Usage | Cost (Approx) |
|------|--------------|-------------|---------------|
| Extract achievements | Analyzes work history | 2,100 in / 850 out | $0.09 |
| Match skills to role | Compares skills vs job target | 1,800 in / 620 out | $0.07 |
| Generate resume | Creates final document | 3,200 in / 1,900 out | $0.16 |
| **Total** | | **7,100 in / 3,370 out** | **$0.32** |

Behind the scenes:
- 3 resource claims created (one per LLM call)
- 3 `dispatch_task_attempt_event_llm_usages` records after completion
- 3 Lago usage events queued

**10:16:45 AM — Review Ready**

| Event | System Action | Billing Impact |
|-------|---------------|----------------|
| Dispatch complete | State: `review` | $0.32 in LLM tokens recorded |
| PDF generated | Asset record created | Asset generation ability logged |

### Day 1: Review and Revision

**10:20 AM — Sarah Reviews**

She likes the structure but wants changes to the skills section.

| Event | System Action | Billing Impact |
|-------|---------------|----------------|
| View results | Display generated content | View ability (free) |
| Request revision | New dispatch triggered | Another round of billing |

**10:21 AM — Revision Dispatch**

Only the skills-related tasks re-run:

| Task | Token Usage | Cost |
|------|-------------|------|
| Revise skills section | 1,500 in / 700 out | $0.07 |
| Regenerate PDF | 3,200 in / 1,900 out | $0.16 |
| **Total** | **4,700 in / 2,600 out** | **$0.23** |

**10:22 AM — Final Approval**

| Event | System Action | Billing Impact |
|-------|---------------|----------------|
| Sarah approves | State: `completed` | Journey total: $0.55 |
| Results delivered | Final assets available | Revenue recognition begins |

### Revenue Recognition

Sarah's $0.55 in LLM costs happened instantly (we paid Anthropic). But how we *recognize* that revenue depends on her subscription:

| Subscription Type | Recognition Pattern |
|-------------------|---------------------|
| Pay-as-you-go | Recognized immediately when invoiced |
| Monthly subscription | Recognized daily over the subscription period |
| Annual prepaid | Recognized daily over 365 days |

For a $29/month subscription, Sarah's January usage ($0.55) is part of the value delivered that month. Revenue is recognized at ~$0.97/day ($29 ÷ 30 days).

---

## Overview

<!-- ANCHOR: operations-journey -->
The Journey is the central runtime concept connecting user activity to billing:

```
User → Select Playbook → Journey Created → Debrief → Dispatch → Review → Complete
                              ↓               ↓          ↓
                         Ability Usage    Token Consumption   Billing Events
```

Tracking Journey lifecycle provides visibility into user engagement and billable activity.
<!-- /ANCHOR -->

---

## Journey States

<!-- ANCHOR: operations-journey-states -->
### State Definitions

| State | Description | Audit Point |
|-------|-------------|-------------|
| **Created** | Journey initialized | Track creation rate |
| **Active** | User engaged in debrief | Session activity |
| **In Progress** | Dispatch executing | Token consumption |
| **Review** | Results awaiting user | Completion rate |
| **Completed** | Journey finished | Conversion metric |
| **Abandoned** | User left without completing | Drop-off analysis |
| **Failed** | Dispatch error | Error tracking |

### State Transitions
```
Created → Active → In Progress → Review → Completed
    ↓         ↓          ↓          ↓
 Abandoned  Abandoned  Failed   Abandoned
```

### Transition Events
Each state change should log:
- Timestamp
- Previous state
- New state
- Trigger (user action, system event, timeout)
- Associated ability usage (if any)
<!-- /ANCHOR -->

---

## Playbook to Journey

<!-- ANCHOR: operations-playbook-journey -->
### Selection Tracking
When a user selects a Playbook:
1. Record Playbook selection event
2. Create Journey with Playbook reference
3. Assign BriefPack(s) to Journey
4. Initialize Journey state

### Audit Points
- Playbook popularity (selection counts)
- Playbook-to-Journey conversion rate
- Time from selection to first activity
- BriefPack assignment accuracy
<!-- /ANCHOR -->

---

## Debrief Phase

<!-- ANCHOR: operations-debrief -->
### What Happens
User completes interview-style data gathering:
- BriefGroups presented atomically
- BriefUnits collect input/display content
- User responses captured
- Validation applied

### Audit Points
- Debrief completion rate
- Time to complete debrief
- Drop-off by BriefGroup
- Validation failure rate

### Ability Usage
Debrief may trigger abilities:
- View content (typically free)
- Submit responses (may be free or tracked)
- Request assistance (may consume tokens)
<!-- /ANCHOR -->

---

## Dispatch Phase

<!-- ANCHOR: operations-dispatch -->
### What Happens
Background execution of workflow steps:
- Steps execute sequentially or in parallel
- Tasks within steps may call LLMs
- Results stored for review
- Errors handled and logged

### Audit Points
- Dispatch success/failure rate
- Execution time by step
- Token consumption per dispatch
- Error categorization

### Ability Usage (Critical for Billing)
Dispatch typically consumes tokens:
- Each LLM call = ability usage
- Each step execution may = ability usage
- Track `dispatch_id` ↔ `ability_usage` ↔ `token_consumption`
<!-- /ANCHOR -->

---

## Review Phase

<!-- ANCHOR: operations-review -->
### What Happens
User reviews dispatch results:
- View generated outputs
- Provide feedback
- Request revisions (may trigger new dispatch)
- Approve results

### Audit Points
- Review completion rate
- Revision request rate
- Time in review
- Approval rate

### Ability Usage
Review may trigger:
- View outputs (typically free)
- Request revision (token consumption)
- Export/download (may be tracked)
<!-- /ANCHOR -->

---

## Completion

### What Happens
Journey reaches terminal state:
- All required steps completed
- User confirms completion
- Results delivered
- Journey marked complete

### Audit Points
- Overall completion rate
- Time from creation to completion
- Total token consumption per journey
- Revenue attribution

---

## Audit Queries

### Journey Funnel
```sql
-- Journey completion funnel
SELECT
    j.playbook_id,
    COUNT(*) FILTER (WHERE j.state = 'created') AS created,
    COUNT(*) FILTER (WHERE j.state = 'active') AS active,
    COUNT(*) FILTER (WHERE j.state = 'in_progress') AS in_progress,
    COUNT(*) FILTER (WHERE j.state = 'review') AS review,
    COUNT(*) FILTER (WHERE j.state = 'completed') AS completed,
    COUNT(*) FILTER (WHERE j.state = 'abandoned') AS abandoned
FROM web.journeys j
WHERE j.created_at > CURRENT_DATE - INTERVAL '30 days'
GROUP BY j.playbook_id;
```

### Token Consumption by Journey
```sql
-- Tokens consumed per journey
SELECT
    j.id AS journey_id,
    j.playbook_id,
    COUNT(e.id) AS ability_usages,
    COUNT(t.id) AS tokens_consumed
FROM web.journeys j
LEFT JOIN rev.events e ON e.journey_id = j.id
LEFT JOIN rev.tokens t ON t.id = e.token_id
WHERE j.created_at > CURRENT_DATE - INTERVAL '30 days'
GROUP BY j.id, j.playbook_id;
```

### Drop-off Analysis
```sql
-- Where journeys are abandoned
SELECT
    j.state AS abandoned_state,
    COUNT(*) AS count,
    AVG(EXTRACT(EPOCH FROM (j.updated_at - j.created_at))/3600) AS avg_hours_before_abandon
FROM web.journeys j
WHERE j.state = 'abandoned'
  AND j.created_at > CURRENT_DATE - INTERVAL '30 days'
GROUP BY j.state;
```

---

## Related Sections

- [Usage Tracking](../domain/usage-tracking.md) — Ability and token tracking
- [Event Correlation](event-correlation.md) — Event tracking
- [Growth Team](../audiences/growth-team.md) — Funnel analysis use cases
