//! Shared helpers for the `edgar` command family.
//!
//! The EDGAR commands are opt-in: they only work when `DAPPLE_EDGAR_USER_AGENT`
//! is set (the SEC fair-access policy requires every client to declare a
//! User-Agent with contact information, so the required declaration doubles as
//! the on/off switch — same pattern as `DAPPLE_MEDIA_GATEWAY_URL` for `vod`).

use edgarkit::{CompanyOperations, Edgar, EdgarError, FilingOperations};
use nu_engine::command_prelude::*;

/// Env var carrying the SEC-mandated User-Agent, e.g. `"dapple ops@dapple.ai"`.
pub const EDGAR_USER_AGENT_ENV: &str = "DAPPLE_EDGAR_USER_AGENT";

/// Read the declared SEC User-Agent. `Err` when unset/empty — EDGAR commands are
/// opt-in and fail with a shaped "not configured" error until it is set.
pub fn get_edgar_user_agent() -> Result<String, String> {
    std::env::var(EDGAR_USER_AGENT_ENV)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            format!(
                "EDGAR access not configured (set {EDGAR_USER_AGENT_ENV} to \"AppName contact@example.com\")"
            )
        })
}

pub fn not_configured(msg: String, span: Span) -> ShellError {
    ShellError::GenericError {
        error: "EDGAR not configured".into(),
        msg,
        span: Some(span),
        help: Some(format!(
            "Set {EDGAR_USER_AGENT_ENV} to a declared User-Agent with contact info, e.g. \
             \"dapple ops@example.com\" — the SEC fair-access policy requires it"
        )),
        inner: vec![],
    }
}

/// Build the rate-limited EDGAR client from the declared User-Agent.
#[allow(clippy::result_large_err)]
pub fn build_client(span: Span) -> Result<Edgar, ShellError> {
    let user_agent = get_edgar_user_agent().map_err(|msg| not_configured(msg, span))?;
    Edgar::new(&user_agent).map_err(|e| edgar_error("initialize EDGAR client", &e, span))
}

/// Resolve a company argument to a numeric CIK: all-digits input is taken as a
/// CIK directly; anything else is looked up as a ticker symbol.
pub async fn resolve_cik(edgar: &Edgar, company: &str) -> Result<u64, EdgarError> {
    let trimmed = company.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
        return trimmed
            .parse::<u64>()
            .map_err(|_| EdgarError::InvalidFormat(format!("CIK out of range: {trimmed}")));
    }
    edgar.company_cik(trimmed).await
}

/// One filing located in EDGAR's browse feed.
pub struct LocatedFiling {
    pub accession: String,
    pub filing_date: String,
    /// The PARENT trust CIK, which is what the Archives path needs even when the
    /// query was by series id.
    pub cik: String,
    /// The exact filed form, e.g. `13F-HR` / `13F-HR/A` / `13F-NT`. A `type=`
    /// query PREFIX-matches, so asking for `13F` returns all four variants and
    /// the caller — not the query — is what tells them apart.
    pub form: String,
}

/// A browse-edgar atom response, which has two shapes depending on whether the
/// query resolved to one company.
///
/// - A **CIK query**, or a `company=` query matching exactly ONE filer, returns
///   the FILINGS feed: a single `company-info` block (carrying `conformed-name`)
///   plus one entry per filing.
/// - A `company=` query matching SEVERAL filers returns a company-list feed:
///   one entry per company, each with its own `cik` and **no** `conformed-name`.
///
/// So `conformed_name.is_some()` is the discriminator for "resolved to exactly
/// one filer", and it is the only reliable one — see the warning on
/// [`fetch_filing_feed`].
pub struct FilingFeed {
    /// Present iff the query resolved to exactly one filer.
    pub conformed_name: Option<String>,
    /// Every CIK the feed named, in document order. One when resolved.
    pub ciks: Vec<String>,
    pub filings: Vec<LocatedFiling>,
}

/// Fetch and parse a browse-edgar atom feed.
///
/// ⚠️ **A company-list feed carries no names.** On a multi-match, EDGAR's CGI
/// emits `<company-info name="ARRAY(0x5589e41ff350)">` — a stringified Perl
/// array reference leaking where the company name belongs — and there is no
/// other name anywhere in the response. Candidate names must be resolved with a
/// follow-up request per CIK. The one mercy is that the bug is unmistakable, so
/// there is no plausible-looking wrong name to accidentally trust; do NOT try to
/// salvage that attribute.
pub async fn fetch_filing_feed(
    edgar: &Edgar,
    url: &str,
    context: &str,
    span: Span,
) -> Result<FilingFeed, ShellError> {
    let xml = edgar
        .get(url)
        .await
        .map_err(|e| edgar_error(context, &e, span))?;

    let doc = roxmltree::Document::parse(&xml).map_err(|e| ShellError::GenericError {
        error: "Unreadable EDGAR filing index".into(),
        msg: format!("{context}: {e}"),
        span: Some(span),
        help: None,
        inner: vec![],
    })?;

    let text_of = |node: roxmltree::Node, tag: &str| -> Option<String> {
        node.descendants()
            .find(|n| n.has_tag_name(tag))
            .and_then(|n| n.text())
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
    };

    let ciks: Vec<String> = doc
        .descendants()
        .filter(|n| n.has_tag_name("cik"))
        .filter_map(|n| n.text())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    let conformed_name = doc
        .descendants()
        .find(|n| n.has_tag_name("conformed-name"))
        .and_then(|n| n.text())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    let feed_cik = ciks.first().cloned().unwrap_or_default();

    let filings = doc
        .descendants()
        .filter(|n| n.has_tag_name("entry"))
        .filter_map(|entry| {
            Some(LocatedFiling {
                accession: text_of(entry, "accession-number")?,
                filing_date: text_of(entry, "filing-date").unwrap_or_default(),
                cik: feed_cik.clone(),
                form: text_of(entry, "filing-type").unwrap_or_default(),
            })
        })
        .collect();

    Ok(FilingFeed {
        conformed_name,
        ciks,
        filings,
    })
}

/// Locate a fund filing, newest first.
///
/// `identifier` may be a CIK, a series id (`S000######`) or a class id — EDGAR's
/// `browse-edgar` accepts all three in its `CIK` parameter and scopes the result
/// accordingly. This matters because the submissions feed carries **no series
/// information at all**, so filtering a trust's submissions would return every
/// fund in the trust indiscriminately.
pub async fn locate_fund_filings(
    edgar: &Edgar,
    identifier: &str,
    form: &str,
    limit: usize,
    span: Span,
) -> Result<Vec<LocatedFiling>, ShellError> {
    let url = browse_by_cik_url(identifier, form, limit);
    let feed = fetch_filing_feed(
        edgar,
        &url,
        &format!("list {form} filings for {identifier}"),
        span,
    )
    .await?;
    Ok(feed.filings)
}

/// Browse a filer's own filings. `identifier` may be a CIK, a series id
/// (`S000######`) or a class id.
pub fn browse_by_cik_url(identifier: &str, form: &str, limit: usize) -> String {
    format!(
        "https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={identifier}\
         &type={form}&dateb=&owner=include&count={limit}&output=atom"
    )
}

/// Search filers BY NAME, scoped to a form.
///
/// The form scope is what makes a name search usable: `company=vanguard` alone
/// returns 100+ companies, and `&type=13F` narrows it to the 12 that actually
/// file one (measured 2026-08-01). `type=` prefix-matches, so pass the FAMILY
/// (`13F`) rather than one variant (`13F-HR`) or notices are silently dropped.
pub fn browse_by_name_url(name: &str, form: &str, limit: usize) -> String {
    format!(
        "https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&company={}\
         &type={form}&dateb=&owner=include&count={limit}&output=atom",
        query_escape(name)
    )
}

/// Percent-encode a query-string value.
///
/// Hand-rolled because the workspace pulls no URL crate directly, and because
/// the character that matters most here is `&`: manager names contain it
/// routinely ("National Liability & Fire Insurance Co"), and passing it raw
/// truncates the query at that point and silently searches for a prefix.
pub fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Fetch a filing's XBRL instance document, or `None` if it has none readable.
///
/// The extracted instance for an inline-XBRL filing sits beside the primary document with
/// `.htm` swapped for `_htm.xml`. Pre-iXBRL filings (roughly before 2019) instead carry a
/// plain `.xml` of the same stem, so both are tried — one extra miss on an old filing is
/// cheaper than losing the early half of a ten-year window.
///
/// Shared by `edgar segments` (dimensional segment facts) and `edgar portfolio` (a BDC's
/// schedule of investments). They read different axes out of the same document, and an
/// instance locator duplicated per caller is exactly the copy that drifts.
pub async fn fetch_instance(
    edgar: &Edgar,
    cik: &str,
    accession: &str,
    primary: &str,
) -> Option<String> {
    for candidate in instance_candidates(primary) {
        let id = format!("{accession}:{candidate}");
        if let Ok(xml) = edgar.get_filing_content_by_id(cik, &id).await
            && xml.contains("contextRef")
        {
            return Some(xml);
        }
    }
    None
}

/// `zip-20251231.htm` -> [`zip-20251231_htm.xml`, `zip-20251231.xml`].
pub fn instance_candidates(primary: &str) -> Vec<String> {
    let stem = primary
        .strip_suffix(".htm")
        .or_else(|| primary.strip_suffix(".html"))
        .unwrap_or(primary);
    vec![format!("{stem}_htm.xml"), format!("{stem}.xml")]
}

/// Build an `Archives/edgar/data/{cik}/{accession}/{file}` URL.
pub fn archive_url(cik: &str, accession: &str, file: &str) -> String {
    format!(
        "https://www.sec.gov/Archives/edgar/data/{}/{}/{file}",
        cik.trim_start_matches('0'),
        accession.replace('-', "")
    )
}

/// Map an [`EdgarError`] into a shaped command error with actionable help.
pub fn edgar_error(context: &str, e: &EdgarError, span: Span) -> ShellError {
    let (error, help) = match e {
        EdgarError::TickerNotFound => (
            "Unknown ticker".to_string(),
            Some("Pass a ticker symbol the SEC lists (e.g. AAPL) or a numeric CIK".to_string()),
        ),
        EdgarError::NotFound => (
            "EDGAR resource not found".to_string(),
            Some(
                "Check the ticker/CIK — the SEC has no data at the requested location".to_string(),
            ),
        ),
        EdgarError::RateLimitExceeded => (
            "SEC rate limit exceeded".to_string(),
            Some("EDGAR enforces ~10 requests/second per client; retry shortly".to_string()),
        ),
        _ => (format!("EDGAR request failed: {e}"), None),
    };
    ShellError::GenericError {
        error,
        msg: format!("Could not {context}"),
        span: Some(span),
        help,
        inner: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_candidates_try_ixbrl_then_legacy() {
        assert_eq!(
            instance_candidates("zip-20251231.htm"),
            vec!["zip-20251231_htm.xml", "zip-20251231.xml"]
        );
        // A primary document that is already XML is tried unchanged rather than mangled.
        assert_eq!(
            instance_candidates("aapl-20250927.xml"),
            vec!["aapl-20250927.xml_htm.xml", "aapl-20250927.xml.xml"]
        );
    }

    #[test]
    fn a_name_with_an_ampersand_does_not_truncate_the_query() {
        // Raw, this would end the `company=` value at the `&` and search for
        // "National Liability " — a prefix match that quietly returns the wrong
        // filer instead of failing.
        assert_eq!(
            query_escape("National Liability & Fire Insurance Co"),
            "National+Liability+%26+Fire+Insurance+Co"
        );
        // Reserved characters that would otherwise re-shape the query string.
        assert_eq!(query_escape("a=b#c?d/e"), "a%3Db%23c%3Fd%2Fe");
        // Unreserved characters pass through unchanged.
        assert_eq!(
            query_escape("Fool-Asset_Mgmt.LLC~1"),
            "Fool-Asset_Mgmt.LLC~1"
        );
    }

    #[test]
    fn name_search_scopes_by_form_family_not_one_variant() {
        // `type=` prefix-matches, so the FAMILY keeps notices in the result.
        let url = browse_by_name_url("Motley Fool Asset Management", "13F", 12);
        assert!(
            url.contains("company=Motley+Fool+Asset+Management"),
            "{url}"
        );
        assert!(url.contains("&type=13F&"), "{url}");
        assert!(url.contains("count=12"), "{url}");
    }

    #[test]
    fn archive_url_strips_leading_zeros_and_dashes() {
        assert_eq!(
            archive_url("0001287750", "0001287750-26-000006", "primary_doc.xml"),
            "https://www.sec.gov/Archives/edgar/data/1287750/000128775026000006/primary_doc.xml"
        );
    }
}
