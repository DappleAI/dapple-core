//! mdbook-admonish fence recognition.
//!
//! Journey builders put life into report markdown with
//! [mdbook-admonish](https://github.com/tommilligan/mdbook-admonish)-style
//! fenced blocks:
//!
//! ````markdown
//! ```admonish warning "Custom title"
//! Body is **markdown** and is re-parsed by the emitters.
//! ```
//! ````
//!
//! Both emitters (Typst → PDF, HTML) intercept a fenced code block whose info
//! string starts with `admonish` and render a themed callout instead of a code
//! block. Because the syntax rides on a plain code fence, any renderer that
//! does NOT know it degrades to a visible code block — contained, never
//! syntax garbage in prose.
//!
//! Grammar accepted here: `admonish [directive] ["Custom title"] [key=value…]`.
//! A bare ```` ```admonish ```` defaults to `note` (mdbook-admonish behavior);
//! `key=value` flags (e.g. `collapsible=true`) are tolerated and ignored —
//! they are meaningless in PDF/static-HTML output.

/// The twelve canonical mdbook-admonish directive families, plus the Dapple
/// color-band extensions (`orange`, `teal`) — persistent identity bands that
/// documents assign to an entity (e.g. one company per color in a two-company
/// report) and wrap around whole subsections, with ordinary callouts nested
/// inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdmonitionKind {
    Note,
    Abstract,
    Info,
    Tip,
    Success,
    Question,
    Warning,
    Failure,
    Danger,
    Bug,
    Example,
    Quote,
    Orange,
    Teal,
}

impl AdmonitionKind {
    /// Resolve a directive token (canonical name or alias) to its family.
    /// Alias table mirrors the mdbook-admonish reference.
    pub fn from_directive(directive: &str) -> Option<Self> {
        use AdmonitionKind::*;
        Some(match directive.to_ascii_lowercase().as_str() {
            "note" => Note,
            "abstract" | "summary" | "tldr" => Abstract,
            "info" | "todo" => Info,
            "tip" | "hint" | "important" => Tip,
            "success" | "check" | "done" => Success,
            "question" | "help" | "faq" => Question,
            "warning" | "caution" | "attention" => Warning,
            "failure" | "fail" | "missing" => Failure,
            "danger" | "error" => Danger,
            "bug" => Bug,
            "example" => Example,
            "quote" | "cite" => Quote,
            "orange" => Orange,
            "teal" => Teal,
            _ => return None,
        })
    }

    /// Default title (directive in title case), used when no custom title is given.
    pub fn default_title(self) -> &'static str {
        use AdmonitionKind::*;
        match self {
            Note => "Note",
            Abstract => "Abstract",
            Info => "Info",
            Tip => "Tip",
            Success => "Success",
            Question => "Question",
            Warning => "Warning",
            Failure => "Failure",
            Danger => "Danger",
            Bug => "Bug",
            Example => "Example",
            Quote => "Quote",
            Orange => "Orange",
            Teal => "Teal",
        }
    }

    /// Canonical lowercase name — CSS class suffix in the HTML emitter.
    pub fn key(self) -> &'static str {
        use AdmonitionKind::*;
        match self {
            Note => "note",
            Abstract => "abstract",
            Info => "info",
            Tip => "tip",
            Success => "success",
            Question => "question",
            Warning => "warning",
            Failure => "failure",
            Danger => "danger",
            Bug => "bug",
            Example => "example",
            Quote => "quote",
            Orange => "orange",
            Teal => "teal",
        }
    }
}

/// A recognized admonition fence: which family, and the title to render.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmonitionSpec {
    pub kind: AdmonitionKind,
    pub title: String,
}

/// Parse a fence info string. Returns `None` when the fence is not an
/// admonition (an ordinary code block — `rust`, `nushell`, empty, …).
///
/// An unknown directive after `admonish` (e.g. a typo like `admonish warnign`)
/// still renders as a callout — `note` family — rather than silently becoming
/// a code block; the typo is logged.
pub fn parse_admonish_info(info: &str) -> Option<AdmonitionSpec> {
    let info = info.trim();
    let rest = info.strip_prefix("admonish")?;
    // `admonishfoo` is not an admonition fence — the keyword must stand alone.
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();

    // Optional directive: the first token, unless it is a quoted title or a
    // key=value flag.
    let (kind, after_directive) = if rest.is_empty() || rest.starts_with('"') {
        (AdmonitionKind::Note, rest)
    } else {
        let token: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
        if token.contains('=') {
            // A flag like `collapsible=true` in directive position: default kind.
            (AdmonitionKind::Note, rest)
        } else {
            let kind = AdmonitionKind::from_directive(&token).unwrap_or_else(|| {
                log::warn!("Unknown admonish directive '{token}', rendering as note");
                AdmonitionKind::Note
            });
            (kind, rest[token.len()..].trim_start())
        }
    };

    // Optional custom title: the first double-quoted span. Everything else
    // (key=value flags) is ignored.
    let title = extract_quoted(after_directive).unwrap_or_else(|| kind.default_title().to_string());

    Some(AdmonitionSpec { kind, title })
}

/// First `"…"` span in `s`, with `\"` unescaped. `None` when no quoted span.
fn extract_quoted(s: &str) -> Option<String> {
    let start = s.find('"')?;
    let mut out = String::new();
    let mut escaped = false;
    for c in s[start + 1..].chars() {
        if escaped {
            out.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    None // unterminated quote: treat as no title
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_admonish_defaults_to_note() {
        let spec = parse_admonish_info("admonish").unwrap();
        assert_eq!(spec.kind, AdmonitionKind::Note);
        assert_eq!(spec.title, "Note");
    }

    #[test]
    fn directive_and_default_title() {
        let spec = parse_admonish_info("admonish warning").unwrap();
        assert_eq!(spec.kind, AdmonitionKind::Warning);
        assert_eq!(spec.title, "Warning");
    }

    #[test]
    fn aliases_resolve() {
        assert_eq!(
            parse_admonish_info("admonish tldr").unwrap().kind,
            AdmonitionKind::Abstract
        );
        assert_eq!(
            parse_admonish_info("admonish important").unwrap().kind,
            AdmonitionKind::Tip
        );
        assert_eq!(
            parse_admonish_info("admonish error").unwrap().kind,
            AdmonitionKind::Danger
        );
        assert_eq!(
            parse_admonish_info("admonish cite").unwrap().kind,
            AdmonitionKind::Quote
        );
    }

    #[test]
    fn color_band_kinds_resolve() {
        let spec = parse_admonish_info("admonish orange title=\"ZIP\"").unwrap();
        assert_eq!(spec.kind, AdmonitionKind::Orange);
        assert_eq!(spec.title, "ZIP");
        assert_eq!(spec.kind.key(), "orange");
        let spec = parse_admonish_info("admonish teal title=\"NOW\"").unwrap();
        assert_eq!(spec.kind, AdmonitionKind::Teal);
        assert_eq!(spec.title, "NOW");
        assert_eq!(spec.kind.key(), "teal");
    }

    #[test]
    fn custom_title() {
        let spec = parse_admonish_info("admonish tip \"Read the cash flow first\"").unwrap();
        assert_eq!(spec.kind, AdmonitionKind::Tip);
        assert_eq!(spec.title, "Read the cash flow first");
    }

    #[test]
    fn custom_title_without_directive() {
        let spec = parse_admonish_info("admonish \"Just a title\"").unwrap();
        assert_eq!(spec.kind, AdmonitionKind::Note);
        assert_eq!(spec.title, "Just a title");
    }

    #[test]
    fn escaped_quote_in_title() {
        let spec = parse_admonish_info(r#"admonish note "He said \"hold\"""#).unwrap();
        assert_eq!(spec.title, r#"He said "hold""#);
    }

    #[test]
    fn flags_are_ignored() {
        let spec = parse_admonish_info("admonish warning \"T\" collapsible=true").unwrap();
        assert_eq!(spec.kind, AdmonitionKind::Warning);
        assert_eq!(spec.title, "T");
        // Flag in directive position.
        let spec = parse_admonish_info("admonish collapsible=true").unwrap();
        assert_eq!(spec.kind, AdmonitionKind::Note);
    }

    #[test]
    fn unknown_directive_becomes_note() {
        let spec = parse_admonish_info("admonish warnign").unwrap();
        assert_eq!(spec.kind, AdmonitionKind::Note);
    }

    #[test]
    fn non_admonish_infos_are_none() {
        assert!(parse_admonish_info("rust").is_none());
        assert!(parse_admonish_info("nushell").is_none());
        assert!(parse_admonish_info("").is_none());
        assert!(parse_admonish_info("admonishment").is_none());
    }

    #[test]
    fn unterminated_title_falls_back_to_default() {
        let spec = parse_admonish_info("admonish danger \"oops").unwrap();
        assert_eq!(spec.kind, AdmonitionKind::Danger);
        assert_eq!(spec.title, "Danger");
    }
}
