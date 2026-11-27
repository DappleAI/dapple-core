//! Render a provenance-tagged sample — the report and its JSON provenance
//! manifest — for inspection.
//!
//! Usage: `cargo run -p dapple-render --example render_provenance -- <outdir>`
//!
//! Writes `report.pdf`, `report.html` and `provenance.json`.
//! The sample mirrors what a filing guide emits: figures tagged with the
//! EDGAR filing they came from and their values as filed (`md fact`), model-written passages attributed
//! to the prompt that wrote them (`md attribute`), a chart whose data is filed,
//! a computed growth rate, the operator's ticker, and the guide's own template
//! text as the document default (`md provenance --default guide`).

use dapple_render::{
    DocumentRenderInput, ProvenanceContext, render_document_html, render_document_pdf,
    render_document_provenance,
};

fn main() {
    let outdir = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let markdown = r##"## What They Filed

Prepared for ticker [ACME](src:operator "id=ticker"). Revenue was [$103.2B](src:k10-fy25 "id=revenue-fy25 amount=103213000000 unit=USD decimals=-6 start=2024-09-29 end=2025-09-27 concept=us-gaap:Revenues") in
fiscal 2025, up from [$91.4B](src:k10-fy24 "id=revenue-fy24 amount=91417000000 unit=USD decimals=-6 start=2023-10-01 end=2024-09-28 concept=us-gaap:Revenues"), a change of [+12.9%](src:growth "id=revenue-growth amount=0.1290 unit=pure").

<!-- src:mda id=mda-intro -->
The cloud segment doubled and now carries the mix. Operating cash still covers
all of capex, but the coverage ratio narrowed, and that trend matters more than
the earnings beat. Receivables grew faster than sales, which management again
attributes to contract seasonality.
<!-- /src -->

| Metric | FY24 | FY25 |
|--------|-----:|-----:|
| Revenue | [$91.4B](src:k10-fy24) | [$103.2B](src:k10-fy25) |
| Free cash flow | [$18.2B](src:k10-fy24 "amount=18212000000 unit=USD end=2024-09-28") | [$17.5B](src:k10-fy25 "amount=17498000000 unit=USD end=2025-09-27") |

<!-- src:k10-fy25 -->
```chart
{"type":"bar","title":"Revenue (USD billions)","x":["FY2024","FY2025"],
 "series":[{"name":"Revenue","data":[91.4,103.2]}],"unit":"usd"}
```
<!-- /src -->

## Review the Signals

```admonish warning "Receivables"
DSO rose from [44 days](src:k10-fy24) to [53 days](src:k10-fy25) while revenue accelerated.
```

<!-- src:risks id=risk-summary -->
The largest exposure is customer concentration: the top three customers are a
growing share of the cloud backlog.
<!-- /src -->

```provenance
{ "default": "guide",
  "sources": {
    "k10-fy25": { "kind": "filing", "label": "ACME 10-K FY2025", "publisher": "SEC EDGAR",
                  "form": "10-K", "accession": "0000000001-25-000010", "filed": "2025-11-01",
                  "url": "https://www.sec.gov/Archives/edgar/data/1/000000000125000010/0000000001-25-000010-index.htm" },
    "k10-fy24": { "kind": "filing", "label": "ACME 10-K FY2024", "publisher": "SEC EDGAR",
                  "form": "10-K", "accession": "0000000001-24-000010", "filed": "2024-11-01" },
    "growth":   { "kind": "computed", "label": "Year-over-year growth", "from": ["k10-fy24", "k10-fy25"] },
    "mda":      { "kind": "generated", "label": "Filing narrative", "model": "claude-sonnet", "prompt": "rt10k-mda" },
    "risks":    { "kind": "generated", "label": "Risk summary", "model": "claude-sonnet", "prompt": "rt10k-risks" },
    "operator": { "kind": "operator", "label": "Ticker entered by the operator" },
    "guide":    { "kind": "authored", "label": "Report template" } } }
```
"##;

    let input = DocumentRenderInput {
        title: "ACME filing review".to_string(),
        subtitle: Some("Fiscal 2025 10-K".to_string()),
        author: Some("Dapple".to_string()),
        date: Some("November 2026".to_string()),
        markdown: markdown.to_string(),
        branding: "technical".to_string(),
        toc: None,
    };

    let write = |name: &str, bytes: &[u8]| {
        let path = std::path::Path::new(&outdir).join(name);
        std::fs::write(&path, bytes).expect("write output");
        println!("wrote {} ({} bytes)", path.display(), bytes.len());
    };
    write(
        "report.pdf",
        &render_document_pdf(&input).expect("render pdf"),
    );
    write(
        "report.html",
        render_document_html(&input)
            .expect("render html")
            .as_bytes(),
    );
    // A corrected version of an earlier render, to show `derived_from`.
    let context = ProvenanceContext {
        document_id: Some("urn:dapple:document:01937a2e-0000-7000-8000-000000000002".to_string()),
        derived_from: Some("urn:dapple:document:01937a2e-0000-7000-8000-000000000001".to_string()),
    };
    let manifest = render_document_provenance(&input, &context).expect("the sample is tagged");
    write("provenance.json", manifest.to_json().as_bytes());
    for w in &manifest.warnings {
        println!("warning: {w}");
    }
}
