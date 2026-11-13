# Operations

The heartbeat of a healthy billing system.

This section covers the runtime behavior of billing: how Journeys move through their lifecycle, how events correlate between systems, and how operational integrity keeps everything honest. If the Domain section is anatomy, Operations is physiology.

Start with [Operational Integrity](./integrity.md) to understand the principles that make auditing possible. Then read [Journey Lifecycle](./journey-lifecycle.md) to see how user activity flows into billing events. The remaining chapters cover specific operational concerns: routing, event correlation, testing, attribution, and compliance.

**Reading order for understanding the system:**
1. Operational Integrity — The principles that make everything else work
2. Journey Lifecycle — How user actions become billing events
3. Event Correlation — How we match internal events to Lago events
4. Conduit Routing — How LLM calls get directed and tracked

**For debugging:**
- Start with [Event Correlation](./event-correlation.md) if you're investigating missing or mismatched events
- Check [Conduit Routing](./conduit-routing.md) if LLM usage doesn't match expectations
