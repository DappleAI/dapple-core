# Introduction

Some intro prose.

:::scope { name: "onboarding-scope" }
audience: new business customers
goals: collect business details, validate information
boundaries: not covering payment setup
tone: professional, welcoming
:::

:::objective { name: "gather-business-info" }
Collect all required business details from the customer
expectations:
- Customer provides company name and contact info
- Customer uploads required documents
:::

:::deliverable { name: "verified-profile" }
A verified business profile ready for account activation
:::

:::stage { name: "Onboarding" }

:::step { name: "Gather Info", tags: [agent, product] }

Please provide your business details.

:::nushell { name: "process-info", tags: [coder] }
def main [] { { payload: {}, signals: {} } }
:::

:::

:::

More prose after the stage.
