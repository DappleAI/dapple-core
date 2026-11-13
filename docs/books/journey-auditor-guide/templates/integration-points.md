# Integration Points

Overview of external system connections and data flows.

---

## Overview

<!-- ANCHOR: templates-integration-points -->
This section documents where external systems connect to the platform, what data flows through each integration, and audit considerations for each.

**Integration Types:**
- **Inbound:** Data coming into the platform
- **Outbound:** Data sent to external systems
- **Bidirectional:** Two-way synchronization
<!-- /ANCHOR -->

---

## Third-Party Philosophy

<!-- ANCHOR: third-party-philosophy -->
**All external integrations are treated as third-party vendor relationships**, even when self-hosted:

- We do NOT modify external system source code
- Feature requests go through vendor/community channels
- Support contracts for production issues (where applicable)
- Our investment is in **integration**, not in replicating vendor capabilities

**Why this matters for auditing:**
- Clear boundary between internal and external system responsibilities
- Reconciliation procedures assume external systems are authoritative for their domain
- Audit trails must correlate internal events with external system identifiers

**Scalability Note:** The current integration stack (Lago for billing) is appropriate for **up to ~50,000 customers**. Beyond this scale, consider adding Kafka/ClickHouse for high-volume event processing.
<!-- /ANCHOR -->

---

## System Map

<!-- ANCHOR: integration-map -->
```
                    ┌─────────────────┐
                    │   HubSpot       │
                    │  (Marketing)    │
                    └────────┬────────┘
                             │ Leads
                             ▼
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   HubSpot       │◄──►│                 │◄──►│      Lago       │
│   (Sales)       │    │   Internal      │    │   (Billing)     │
└─────────────────┘    │   Platform      │    └────────┬────────┘
                       │                 │             │
┌─────────────────┐    │                 │    ┌────────▼────────┐
│   HubSpot       │◄──►│                 │    │  Zuora Revenue  │
│  (Service)      │    │                 │    │  (Recognition)  │
└─────────────────┘    └────────┬────────┘    └────────┬────────┘
                                │                      │
                       ┌────────▼────────┐    ┌────────▼────────┐
                       │    Avalara      │    │  Accounting/ERP │
                       │     (Tax)       │    │                 │
                       └─────────────────┘    └─────────────────┘
```
<!-- /ANCHOR -->

---

## Integration Details

<!-- ANCHOR: integration-details -->
### Lago (Recurring Billing)

| Direction | Data | Frequency | Audit Point |
|-----------|------|-----------|-------------|
| Outbound | Customer creation | Real-time | Customer ID mapping |
| Outbound | Subscription changes | Real-time | State sync |
| Outbound | Usage events | Real-time | Event correlation |
| Inbound | Webhook events | Real-time | Event processing |
| Inbound | Invoice data | Daily | Amount reconciliation |

**Key Fields:**
- `external_subscription_id` — Links to Lago subscription
- `external_invoice_id` — Links to Lago invoice (lago_id)
- `external_event_id` — Links to Lago event (transaction_id)

**Scalability:** Appropriate for < 50K customers with minute-level billing latency.

---

### HubSpot (CRM/CMS)

| Direction | Data | Frequency | Audit Point |
|-----------|------|-----------|-------------|
| Bidirectional | Contacts | Real-time | Contact matching |
| Bidirectional | Deals | Real-time | Deal-subscription link |
| Outbound | Subscription status | Real-time | Customer health |
| Inbound | Campaign data | Daily | Attribution |

**Key Fields:**
- `external_hubspot_id` — Links to HubSpot contact
- `external_hubspot_deal_id` — Links to HubSpot deal

---

### Zuora Revenue

| Direction | Data | Frequency | Audit Point |
|-----------|------|-----------|-------------|
| Outbound | Invoice data | Daily | Recognition input |
| Inbound | Recognition data | Monthly | Revenue booking |

**Key Fields:**
- Recognition period alignment
- Revenue amount reconciliation

---

### Avalara (Tax)

| Direction | Data | Frequency | Audit Point |
|-----------|------|-----------|-------------|
| Outbound | Transaction data | Real-time | Tax calculation |
| Inbound | Tax rates | Real-time | Rate accuracy |

**Key Fields:**
- Tax jurisdiction
- Tax amount on invoices
<!-- /ANCHOR -->

---

## Sync Monitoring

<!-- ANCHOR: integration-monitoring -->
### Health Checks

For each integration, monitor:
- Last successful sync timestamp
- Error rate in last 24 hours
- Queue depth (for async processing)
- Response time / latency

### Alert Thresholds

| Integration | Error Rate | Latency | Queue Depth |
|-------------|------------|---------|-------------|
| Lago | > 1% | > 5s | > 100 |
| HubSpot | > 5% | > 10s | > 500 |
| Zuora | N/A | N/A | N/A |
| Avalara | > 0.1% | > 2s | N/A |

### Daily Reconciliation

```sql
-- Check integration health summary
SELECT
    integration_name,
    COUNT(*) AS total_events,
    COUNT(*) FILTER (WHERE success = true) AS successful,
    COUNT(*) FILTER (WHERE success = false) AS failed,
    MAX(processed_at) AS last_processed
FROM rev.integration_events
WHERE processed_at > CURRENT_DATE - INTERVAL '1 day'
GROUP BY integration_name;
```
<!-- /ANCHOR -->

---

## Troubleshooting

### Common Issues

**Webhook Processing Delays:**
- Check queue depth
- Verify webhook endpoint health
- Review error logs

**Data Mismatch:**
- Compare record counts
- Check for sync timing issues
- Verify field mapping

**Missing External IDs:**
- Review integration logs
- Check for API errors
- Verify create/update flow

---

## Related Sections

- [Data Reconciliation](../monitoring/data-reconciliation.md) — Comparison procedures
- [Recurring Billing (Lago)](../integrations/recurring-billing.md) — Lago details
- [CRM & CMS (HubSpot)](../integrations/crm-cms.md) — HubSpot details
- [Event Correlation](../operations/event-correlation.md) — Event tracking
