//! Render a run-the-10k-shaped sample to files for visual inspection.
//!
//! Usage: `cargo run -p dapple-render --example render_sample -- <outdir>`

use dapple_render::{DocumentRenderInput, render_document_html, render_document_pdf};

fn main() {
    let outdir = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let markdown = r##"## What They Filed

The cloud segment doubled and now carries the mix. See [The Timeline](#the-timeline)
for the record of how it got here, and [the 2024 crossover](#tl-bz-2024-cloud-3) in
particular. Revenue grew 12.9% to $103.2B while free cash flow **fell** 4%.

```admonish tip "Read the cash flow first"
Operating cash covers all of capex this year, but the gap narrowed from
2.1x to 1.4x coverage. That trend matters more than the EPS beat.
```

| Metric | FY24 | FY25 | Change |
|--------|-----:|-----:|-------:|
| Revenue | $91.4B | $103.2B | +12.9% |
| Operating margin | 24.1% | 26.3% | +220bps |
| DSO | 44 days | 53 days | +9 days |
| Free cash flow | $18.2B | $17.5B | -3.8% |

```admonish warning
DSO rose 9 days while revenue accelerated. Receivables are growing faster
than sales — [Review the Signals](#review-the-signals) walks the components.
```

> Management characterizes the receivables build as "a timing artifact of
> enterprise contract seasonality." The last three years each contained the
> same explanation.

### How the segments split

- Cloud infrastructure
  - Compute: up 61% year over year
  - Storage: up 34%
- Legacy licensing
  - Perpetual: down 18%, now sub-10% of revenue

```nushell
let facts = open facts.json | get us-gaap
$facts | where concept == "AccountsReceivableNetCurrent" | last 3
```

```chart
{ "type": "line", "title": "Revenue and net income, FY2022 to FY2024",
  "subtitle": "Billions of dollars, as reported",
  "x": ["FY2022", "FY2023", "FY2024"],
  "series": [
    { "name": "Revenue", "data": [1.1, 1.4, 1.3] },
    { "name": "Net income", "data": [0.088, 0.012, -0.04] }
  ],
  "unit": "usd" }
```

## Review the Signals

Deterministic cross-checks over the XBRL facts. Two fired this year.

```chart
{ "type": "stacked", "title": "What each year tripped",
  "x": ["FY2022", "FY2023", "FY2024"],
  "series": [
    { "name": "Integrity flags", "data": [1, 2, 4] },
    { "name": "Misfilings", "data": [0, 1, 2] },
    { "name": "SEC staff flags", "data": [0, 0, 1] }
  ] }
```

```admonish success "Coverage holds"
Interest coverage stayed above 12x through the debt raise.
```

```admonish danger "Sloan ratio drifting"
Accruals hit 9.1% of assets — past the 8% line for the second
consecutive year. Earnings quality depends increasingly on non-cash items.
```

---

## Only One of Them Carries This

The company bands: each side owns a color everywhere — bands, chart series,
diagram elements. This one wraps a long body to prove it breaks across pages.

````admonish orange title="ZIP"
##### 2024 One customer at 39 percent of revenue {#tl-band-zip-1}

Concentration doubled across the window while revenue fell by more than a
third. The pair of movements is the finding; either alone would be routine.

```admonish info title="A mention is a mention"
The record does not say which role the counterparty plays. Two people can
share a name; two filings can share a customer without saying so.
```

| Year | Share | Revenue |
|------|------:|--------:|
| FY2023 | 21% | $645.7M |
| FY2024 | 39% | $474.0M |

```chart
{ "type": "bar", "title": "Top-customer share of revenue",
  "x": ["FY2023", "FY2024"],
  "series": [ { "name": "ZIP", "data": [21, 39], "color": "#ea580c" } ],
  "unit": "pct",
  "caption": "Concentration as filed in each year's Item 1A. The customer is not named in either filing, so the share is the only comparable figure." }
```

More prose to push the band across a page boundary. The rail should continue
on the next page and the title bar should not repeat. Filler follows: the
disclosure grew from one sentence to three paragraphs; the named risk moved
from boilerplate to specific; the counterparty language changed twice.
````

````admonish teal title="NOW"
##### 2025 Backlog disclosure begins {#tl-band-now-1}

The first year the record states remaining performance obligations by tier.

```admonish warning title="A count of filings, not of businesses"
A new disclosure is a change to the record, not necessarily to the business.
```
````

```chart
{ "type": "line", "title": "Revenue, indexed to 100 at FY2021",
  "subtitle": "Direction, never size",
  "x": ["FY2021", "FY2022", "FY2023", "FY2024", "FY2025"],
  "series": [
    { "name": "ZIP", "data": [100, 122, 87, 64, 61], "color": "#ea580c" },
    { "name": "NOW", "data": [100, 123, 152, 186, 225], "color": "#0d9488" }
  ],
  "caption": "Indexed to 100 at each company's first filed year. The two operate at different scales (ZIP FY2021 revenue $741M, NOW $5.9B), so the lines compare direction, never size." }
```

```diagram
{ "type": "venn2", "title": "What the two records cover",
  "a": { "label": "ZIP", "color": "#ea580c", "count": 60,
         "items": ["Segment detail", "Employer count"] },
  "b": { "label": "NOW", "color": "#0d9488", "count": 96,
         "items": ["Backlog by tier", "RPO detail"] },
  "shared": { "count": 163, "items": ["Revenue", "One auditor signs both"] },
  "caption": "Counts are dimensions the two annual records describe, not things the businesses have." }
```

```diagram
{ "type": "lanes", "title": "Two histories, one clock",
  "lanes": [
    { "label": "ZIP", "color": "#ea580c",
      "events": [ { "when": "2019-06-01", "text": "Series B" },
                  { "when": "2021", "text": "Direct listing" },
                  { "when": "2022", "text": "First buyback" },
                  { "when": "2024-11-01", "text": "CFO change" } ] },
    { "label": "NOW", "color": "#0d9488",
      "events": [ { "when": "2004", "text": "Founded" },
                  { "when": "2012", "text": "IPO" },
                  { "when": "2023", "text": "First $1B quarter" } ] } ],
  "caption": "Dated from each company's own filings; spacing is time, not importance." }
```

```diagram
{ "type": "flow", "title": "Who names whom",
  "boxes": [ { "id": "a", "label": "ZIP", "color": "#ea580c", "col": 0, "row": 0 },
             { "id": "pwc", "label": "One auditor", "col": 1, "row": 1 },
             { "id": "b", "label": "NOW", "color": "#0d9488", "col": 2, "row": 0 } ],
  "arrows": [ { "from": "pwc", "to": "a", "label": "signs" },
              { "from": "pwc", "to": "b", "label": "signs" } ],
  "caption": "Arrows are statements in filings, not relationships this review asserts." }
```

Five lanes on a year axis — the widest a lane stack goes, and the case the risk
register needs so `escalated` and `softened` can both show.

```diagram
{ "type": "lanes", "title": "When the risk register moved, and how",
  "lanes": [
    { "label": "New", "color": "#49a013",
      "events": [ { "when": "2021", "text": "4 risks" }, { "when": "2023", "text": "2 risks" } ] },
    { "label": "Escalated", "color": "#3b82f6",
      "events": [ { "when": "2022", "text": "3 risks" }, { "when": "2024", "text": "1 risk" } ] },
    { "label": "Softened", "color": "#f59e0b",
      "events": [ { "when": "2023", "text": "2 risks" }, { "when": "2025", "text": "3 risks" } ] },
    { "label": "Dropped", "color": "#7c3aed",
      "events": [ { "when": "2024", "text": "1 risk" }, { "when": "2025", "text": "2 risks" } ] },
    { "label": "Restated", "color": "#0891b2",
      "events": [ { "when": "2022", "text": "1 risk" } ] } ],
  "caption": "One dot per year carrying how many risks moved that way, never one dot per risk: a register runs to dozens of lines and would be truncated to whichever ten sorted earliest." }
```

A month-granular axis: every event carries a month and the whole trail spans
under two years, so the ticks are months and the pad is one month rather than a
full year either side.

```diagram
{ "type": "lanes", "title": "What has arrived since, and when",
  "lanes": [
    { "label": "Contradicts", "color": "#49a013",
      "events": [ { "when": "2026-02-11", "text": "Auditor change" },
                  { "when": "2026-05-02", "text": "Guidance withdrawn" } ] },
    { "label": "Extends", "color": "#3b82f6",
      "events": [ { "when": "2026-03-19", "text": "New facility" },
                  { "when": "2026-06-08", "text": "Segment renamed" },
                  { "when": "2026-09-30", "text": "Buyback raised" } ] } ],
  "caption": "Every kept 8-K on the month it was filed. Spacing is time and nothing else: a crowded stretch is a crowded stretch, never a judgement." }
```

A two-node middle column, which is where a flow stops being legible: a third
node would sit three rows below the subjects and its arrows would pass behind
every box between.

```diagram
{ "type": "flow", "title": "Who names whom on the record",
  "boxes": [ { "id": "a", "label": "ZIP", "color": "#ea580c", "col": 0, "row": 0 },
             { "id": "b", "label": "NOW", "color": "#0d9488", "col": 2, "row": 0 },
             { "id": "s0", "label": "One auditor", "col": 1, "row": 1 },
             { "id": "s1", "label": "3 customers", "col": 1, "row": 2 } ],
  "arrows": [ { "from": "a", "to": "b", "label": "names in filings" },
              { "from": "s0", "to": "a", "label": "signs" },
              { "from": "s0", "to": "b", "label": "signs" },
              { "from": "a", "to": "s1", "label": "names as customer" },
              { "from": "b", "to": "s1", "label": "names as customer" } ],
  "caption": "The arrow runs the way the fact does: an auditor signs FOR a company, and a company NAMES its own customer." }
```

## The Timeline

The truth of record. Every entry cites its filing.

### 2024 [Business] Cloud is now half of revenue {#tl-bz-2024-cloud-3}

The mix crossed 50% in Q3 FY2024, three quarters ahead of guidance. Source:
[FY2024 10-K, Item 7](https://www.sec.gov/cgi-bin/browse-edgar).

### 2025 [Numbers] Margin inflection {#tl-nm-2025-margin-1}

Operating margin recovered 220bps on cloud scale economics, partially offset
by the receivables build flagged in [Review the Signals](#review-the-signals).

### 2025 [Risk] Concentration disclosure expands {#tl-rk-2025-conc-2}

The top-customer concentration note went from one sentence to three
paragraphs. New language names "a small number of hyperscale partners."
"##;

    let input = DocumentRenderInput {
        title: "Run the 10-K — ZIP FY2025".to_string(),
        subtitle: Some("What they filed, and what it means".to_string()),
        author: Some("Dapple".to_string()),
        date: Some("2026-07-25".to_string()),
        markdown: markdown.to_string(),
        branding: "technical".to_string(),
        toc: Some(true),
    };

    let pdf = render_document_pdf(&input).expect("pdf");
    let html = render_document_html(&input).expect("html");
    std::fs::write(format!("{outdir}/sample.pdf"), &pdf).unwrap();
    std::fs::write(format!("{outdir}/sample.html"), &html).unwrap();
    println!(
        "wrote {outdir}/sample.pdf ({} bytes) and sample.html ({} bytes)",
        pdf.len(),
        html.len()
    );
}
