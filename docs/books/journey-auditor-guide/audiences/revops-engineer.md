# RevOps Engineer

RevOps Engineers focused on tool configuration, automation, and system integration for revenue operations.

---

## Role Definition

<!-- ANCHOR: audience-revops-engineer -->
RevOps Engineers maintain the technical infrastructure of revenue operations:
- Configure and maintain RevOps tools (HubSpot, analytics platforms)
- Build automation workflows between systems
- Ensure data flows correctly between touchpoints
- Implement tracking and attribution systems

### Key Decisions
- Tool selection and configuration
- Automation workflow design
- Data synchronization strategies
- Tracking implementation standards
<!-- /ANCHOR -->

---

## Audit Questions

Questions RevOps Engineers typically need answered:

### System Health
- Are all integrations syncing correctly?
- What's the data latency between systems?
- Are there sync failures or backlogs?
- Is tracking firing on all expected events?

### Data Quality
- Are all required fields being populated?
- What's the match rate between systems?
- Are there duplicate or orphaned records?
- Is attribution data complete?

### Configuration Accuracy
- Are product catalogs in sync?
- Are pricing tiers configured correctly?
- Are automation rules firing as expected?
- Is segmentation logic correct?

---

## Tool Responsibilities

### HubSpot Suite
- Marketing Hub: Campaign tracking, lead scoring
- Sales Hub: Pipeline stages, deal tracking
- Service Hub: Ticket correlation, NPS tracking
- Content Hub: Content analytics, form tracking

### Analytics Platforms
- Event tracking implementation
- Custom dimension configuration
- Goal and conversion setup
- Attribution model configuration

### Integration Points
- Webhook configuration and monitoring
- API connection health
- Data transformation rules
- Error handling and alerting

---

## Self-Service vs. Escalation

### Self-Service
- Tool configuration within established patterns
- Standard automation workflows
- Dashboard and report creation
- Basic troubleshooting

### Escalation to Engineering
- Custom API development
- Database schema changes
- Complex data transformations
- Production incident response

---

## Related Sections

- [CRM & CMS Integration](../integrations/crm-cms.md) — HubSpot configuration details
- [Integration Points](../templates/integration-points.md) — System connection overview
- [Event Correlation](../operations/event-correlation.md) — Tracking requirements
