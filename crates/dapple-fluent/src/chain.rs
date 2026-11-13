//! Fallback chain builders for locale and engine resolution
//!
//! These functions construct priority-ordered fallback chains that are passed
//! to PostgreSQL for resolution using `array_position()`.

/// Build locale fallback chain from a locale tag.
///
/// Strips region codes to create a fallback hierarchy:
/// - "pt-BR" → ["pt-BR", "pt", "en"]
/// - "en-US" → ["en-US", "en"]
/// - "en" → ["en"]
///
/// The default locale is appended if not already present.
///
/// # Examples
///
/// ```
/// use dapple_fluent::locale_chain;
///
/// let chain = locale_chain("pt-BR", "en");
/// assert_eq!(chain, vec!["pt-BR", "pt", "en"]);
///
/// let chain = locale_chain("en", "en");
/// assert_eq!(chain, vec!["en"]);
/// ```
pub fn locale_chain(locale: &str, default: &str) -> Vec<String> {
    let mut chain = vec![locale.to_string()];

    // Strip region: "pt-BR" → "pt"
    if let Some(base) = locale.split('-').next() {
        if base != locale {
            chain.push(base.to_string());
        }
    }

    // Add default if not already present
    if !chain.contains(&default.to_string()) {
        chain.push(default.to_string());
    }

    chain
}

/// Build engine fallback chain from an engine key and optional version.
///
/// Creates a priority chain where more specific matches come first:
/// - With version: ["engine:version", "engine", ...fallbacks]
/// - Without version: ["engine", ...fallbacks]
///
/// Fallbacks are engine keys that should be tried if no match is found
/// for the primary engine. These are typically configured per-engine
/// (e.g., "claude-3.5-sonnet" falls back to "claude-3.5", then "claude").
///
/// # Examples
///
/// ```
/// use dapple_fluent::engine_chain;
///
/// let chain = engine_chain("claude-3.5-sonnet", Some("20241022"), &["claude-3.5".into()]);
/// assert_eq!(chain, vec!["claude-3.5-sonnet:20241022", "claude-3.5-sonnet", "claude-3.5"]);
///
/// let chain = engine_chain("gpt-4", None, &[]);
/// assert_eq!(chain, vec!["gpt-4"]);
/// ```
pub fn engine_chain(engine_key: &str, version: Option<&str>, fallbacks: &[String]) -> Vec<String> {
    let mut chain = Vec::new();

    // Full key with version if present
    if let Some(v) = version {
        chain.push(format!("{}:{}", engine_key, v));
    }
    chain.push(engine_key.to_string());

    // Add configured fallbacks
    chain.extend(fallbacks.iter().cloned());

    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_chain_with_region() {
        assert_eq!(locale_chain("pt-BR", "en"), vec!["pt-BR", "pt", "en"]);
    }

    #[test]
    fn test_locale_chain_no_region() {
        assert_eq!(locale_chain("en", "en"), vec!["en"]);
    }

    #[test]
    fn test_locale_chain_different_default() {
        assert_eq!(locale_chain("pt", "en"), vec!["pt", "en"]);
    }

    #[test]
    fn test_locale_chain_default_already_in_chain() {
        assert_eq!(locale_chain("en-US", "en"), vec!["en-US", "en"]);
    }

    #[test]
    fn test_locale_chain_complex_tag() {
        // Extended language subtags should still work
        assert_eq!(
            locale_chain("zh-Hans-CN", "en"),
            vec!["zh-Hans-CN", "zh", "en"]
        );
    }

    #[test]
    fn test_engine_chain_with_version() {
        let chain = engine_chain(
            "claude-3.5-sonnet",
            Some("20241022"),
            &["claude-3.5".into()],
        );
        assert_eq!(
            chain,
            vec![
                "claude-3.5-sonnet:20241022",
                "claude-3.5-sonnet",
                "claude-3.5"
            ]
        );
    }

    #[test]
    fn test_engine_chain_without_version() {
        let chain = engine_chain("gpt-4", None, &["gpt".into()]);
        assert_eq!(chain, vec!["gpt-4", "gpt"]);
    }

    #[test]
    fn test_engine_chain_no_fallbacks() {
        let chain = engine_chain("mistral-large", None, &[]);
        assert_eq!(chain, vec!["mistral-large"]);
    }

    #[test]
    fn test_engine_chain_multiple_fallbacks() {
        let chain = engine_chain(
            "claude-3.5-sonnet",
            None,
            &["claude-3.5".into(), "claude-3".into(), "claude".into()],
        );
        assert_eq!(
            chain,
            vec!["claude-3.5-sonnet", "claude-3.5", "claude-3", "claude"]
        );
    }
}
