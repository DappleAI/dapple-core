# Domain

The vocabulary of billing—what happens when a user clicks "Generate."

This section covers the data structures and relationships that make billing work. When a company signs up, when a user consumes LLM tokens, when revenue gets recognized—these chapters explain what's actually happening in the database and how the pieces connect.

Start with [Organization Model](./organization.md) to understand how users, companies, and billable entities relate. Then work through [Usage Tracking](./usage-tracking.md) to see how actions become invoices. The remaining chapters cover specific billing mechanics: memos for adjustments, revenue recognition for GAAP compliance, subscriptions for recurring billing, and pricing for rate calculations.

**Reading order for newcomers:**
1. Organization Model — How entities are structured
2. Usage Tracking — How actions become billable events
3. Subscriptions — How recurring billing works
4. Revenue Recognition — How money is recognized over time

The other chapters are reference material you can read as needed.
