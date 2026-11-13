# RevTech Specialist

RevTech Specialists focused on third-party system configuration, particularly Salesforce and CRM integrations.

---

## Role Definition

<!-- ANCHOR: audience-revtech-specialist -->
RevTech Specialists manage the configuration and integration of third-party revenue technology systems:
- Configure CRM systems (Salesforce, HubSpot)
- Manage billing system integrations (Maxio)
- Ensure data synchronization between platforms
- Maintain product catalog consistency

### Key Decisions
- System configuration standards
- Integration architecture
- Data mapping between systems
- Workflow automation rules
<!-- /ANCHOR -->

---

## Audit Questions

Questions RevTech Specialists typically need answered:

### System Synchronization
- Are products in sync across all systems?
- Do customer records match between CRM and billing?
- Are subscription states consistent?
- Is pricing synchronized correctly?

### Integration Health
- What's the sync frequency and latency?
- Are there failed or stuck syncs?
- What's the error rate by integration?
- Are all required fields mapping correctly?

### Configuration Accuracy
- Are workflow rules firing correctly?
- Is segmentation logic consistent?
- Are automation triggers reliable?
- Is the product hierarchy correct?

---

## System Responsibilities

### Salesforce
- Account and contact management
- Opportunity pipeline
- Product and price book configuration
- Custom object relationships

### Maxio (Recurring Billing)
- Subscription management
- Product catalog configuration
- Invoice and payment processing
- Usage metering

### HubSpot
- Marketing automation
- Sales pipeline (if primary CRM)
- Customer success workflows
- Analytics and reporting

---

## Integration Patterns

### Bidirectional Sync
- Customer data between CRM and billing
- Subscription status updates
- Usage data for sales visibility

### One-Way Sync
- Product catalog from master source
- Pricing updates from billing
- Event data to analytics

### Webhook Events
- Real-time status changes
- Payment notifications
- Subscription lifecycle events

---

## Audit Procedures

### Daily Checks
- Sync job completion status
- Error log review
- Record count comparisons

### Weekly Checks
- Product catalog alignment
- Pricing verification
- Orphaned record identification

### Monthly Checks
- Full reconciliation runs
- Configuration review
- Integration performance metrics

---

## Related Sections

- [Recurring Billing (Maxio)](../integrations/recurring-billing.md) — Billing system details
- [CRM & CMS (HubSpot)](../integrations/crm-cms.md) — HubSpot integration
- [Data Reconciliation](../monitoring/data-reconciliation.md) — Comparison procedures
