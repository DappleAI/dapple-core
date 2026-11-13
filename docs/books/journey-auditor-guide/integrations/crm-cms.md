# CRM & CMS (HubSpot)

HubSpot integration for marketing, sales, service, and content management.

---

## Overview

<!-- ANCHOR: integration-hubspot -->
HubSpot serves multiple functions:
- **Marketing Hub:** Campaign management, lead tracking, attribution
- **Sales Hub:** CRM, pipeline management, deal tracking
- **Service Hub:** Customer support, ticket management
- **Content Hub:** Website content, analytics, forms

**Data Flow:**
```
Marketing Hub → Leads → Sales Hub → Customers → Internal System
                                         ↓
                                   Billing (Maxio)
                                         ↓
                                   Service Hub
```
<!-- /ANCHOR -->

---

## Marketing Hub Integration

<!-- ANCHOR: integration-hubspot-marketing -->
### Data Points
| HubSpot Field | Internal Use | Audit Point |
|---------------|--------------|-------------|
| Contact source | Attribution | Campaign correlation |
| Campaign ID | Revenue attribution | Cost/revenue tracking |
| Form submissions | Lead capture | Conversion tracking |
| Email engagement | Lead scoring | Activity correlation |

### Audit Concerns
- UTM parameters captured correctly
- Campaign costs tracked for ROI
- Attribution model consistent
- Form submissions linked to contacts
<!-- /ANCHOR -->

---

## Sales Hub Integration

<!-- ANCHOR: integration-hubspot-sales -->
### Data Points
| HubSpot Field | Internal Use | Audit Point |
|---------------|--------------|-------------|
| Deal ID | Opportunity tracking | 1:1 with subscription |
| Deal amount | Forecast accuracy | Match actual revenue |
| Close date | Sales cycle metrics | Accurate timing |
| Deal owner | Commission tracking | Attribution |

### Audit Concerns
- Deals linked to subscriptions
- Deal amounts match actual billing
- Close dates align with subscription start
- Pipeline stages reflect reality
<!-- /ANCHOR -->

---

## Service Hub Integration

<!-- ANCHOR: integration-hubspot-service -->
### Data Points
| HubSpot Field | Internal Use | Audit Point |
|---------------|--------------|-------------|
| Ticket ID | Support correlation | Issue tracking |
| Customer ID | Account linkage | 1:1 mapping |
| Resolution time | SLA tracking | Credit eligibility |
| Satisfaction score | Churn prediction | Retention signals |

### Audit Concerns
- Tickets linked to customers
- SLA violations trigger credits
- Customer health scoring accurate
<!-- /ANCHOR -->

---

## Content Hub Integration

<!-- ANCHOR: integration-hubspot-content -->
### Data Points
| HubSpot Field | Internal Use | Audit Point |
|---------------|--------------|-------------|
| Page views | Engagement tracking | Product interest |
| Content offers | Lead generation | Conversion tracking |
| Blog engagement | Content ROI | Attribution |

### Audit Concerns
- Analytics tracking accurate
- Form captures linked to contacts
- Content attribution tracked
<!-- /ANCHOR -->

---

## Reconciliation

> **Note:** Customer data lives in `auth.users` and subscription data lives in Lago. CRM reconciliation requires joining across these sources.

### Contact/Billable Entity Match
```sql
-- Verify billable entities can be matched to CRM
SELECT
    be.id,
    be.object_id,
    be.lago_customer_external_id,
    be.created_at
FROM rev.billable_entities be
WHERE be.created_at > CURRENT_DATE - INTERVAL '30 days';
-- Cross-reference with CRM contact export by email or external ID
```

### Deal/Subscription Match

Subscription data lives in Lago. To reconcile with CRM deals:

1. Export deals from CRM (HubSpot, etc.)
2. Query Lago: `GET /subscriptions?external_customer_id={id}`
3. Compare deal amounts with Lago subscription plan amounts
4. Document variances for sales/finance review

---

## Sync Requirements

### Real-Time (Webhooks)
- Contact creation/update
- Deal stage changes
- Form submissions

### Batch (Scheduled)
- Full contact sync
- Deal reconciliation
- Activity aggregation

### Frequency
| Data Type | Frequency |
|-----------|-----------|
| Contact updates | Real-time |
| Deal updates | Real-time |
| Activity sync | Hourly |
| Full reconciliation | Daily |

---

## Related Sections

- [Growth Team](../audiences/growth-team.md) — Marketing/sales use cases
- [RevOps Engineer](../audiences/revops-engineer.md) — Configuration responsibility
- [Event Correlation](../operations/event-correlation.md) — Tracking requirements
