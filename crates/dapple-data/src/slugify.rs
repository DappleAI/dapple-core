/// Generate a URL-friendly slug from a name string.
///
/// Mirrors `rev.slugify()` in PostgreSQL (see
/// `crates/dapple-data/migrations/00000000000042_unicode_slugify.sql`).
/// SQL and Rust slugs MUST match byte-for-byte — any divergence breaks slug-
/// based lookups against `*_path_details.slug` columns (rows inserted by SQL
/// can't be found by Rust queries, materializers fall through to INSERT, and
/// the UNIQUE-on-`name` constraint then collides because the raw name is
/// unchanged across re-ingests).
///
/// Transformations (matching SQL `[^[:alnum:]\s_-]` strip + `\s+` → `-`):
/// 1. Convert to lowercase (Unicode case folding)
/// 2. Strip every character that is not a Unicode letter, Unicode digit,
///    ASCII whitespace, `-`, or `_`. Accented Latin characters (`ç`, `ã`,
///    `é`, …) and other Unicode letters are preserved.
/// 3. Replace ASCII whitespace runs with a single `-`
/// 4. Trim leading/trailing `-`
///
/// `char::is_alphanumeric` (Unicode `L`/`N` general categories) matches PG
/// `[:alnum:]` in a UTF-8 database. `\s` in PG regex is ASCII whitespace,
/// matched here by `char::is_ascii_whitespace`.
///
/// # Examples
///
/// ```
/// use dapple_data::slugify;
/// assert_eq!(slugify("Journey Builder Guide"), "journey-builder-guide");
/// assert_eq!(slugify("  Multiple   Spaces  "), "multiple-spaces");
/// assert_eq!(slugify("Special! Ch@rs (removed)"), "special-chrs-removed");
/// // Accented Latin letters are preserved:
/// assert_eq!(slugify("Como Lida com Alergênios"), "como-lida-com-alergênios");
/// ```
pub fn slugify(input: &str) -> String {
    let lowered = input.to_lowercase();

    // Strip every char outside SQL's `[[:alnum:]\s_-]` class. `is_alphanumeric`
    // is Unicode-aware (matches Unicode L + N categories), which aligns with
    // PG `[:alnum:]` in a UTF-8 database. `\s` in PG regex is ASCII whitespace.
    let cleaned: String = lowered
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_ascii_whitespace() || *c == '-' || *c == '_')
        .collect();

    // Replace ASCII whitespace runs with a single hyphen.
    let mut result = String::with_capacity(cleaned.len());
    let mut prev_was_whitespace = false;

    for c in cleaned.chars() {
        if c.is_ascii_whitespace() {
            if !prev_was_whitespace {
                result.push('-');
            }
            prev_was_whitespace = true;
        } else {
            result.push(c);
            prev_was_whitespace = false;
        }
    }

    // Trim leading/trailing hyphens.
    result.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_slug() {
        assert_eq!(slugify("Journey Builder Guide"), "journey-builder-guide");
    }

    #[test]
    fn test_multiple_spaces() {
        assert_eq!(slugify("  Multiple   Spaces  "), "multiple-spaces");
    }

    #[test]
    fn test_special_characters() {
        assert_eq!(slugify("Special! Ch@rs (removed)"), "special-chrs-removed");
    }

    #[test]
    fn test_already_slug() {
        assert_eq!(slugify("already-a-slug"), "already-a-slug");
    }

    #[test]
    fn test_underscores_preserved() {
        assert_eq!(slugify("snake_case name"), "snake_case-name");
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn test_single_word() {
        assert_eq!(slugify("Word"), "word");
    }

    #[test]
    fn test_leading_trailing_hyphens() {
        assert_eq!(slugify("- Leading Hyphen -"), "leading-hyphen");
    }

    /// Accented Latin characters are preserved (Unicode-aware slugify). Each
    /// pair below was verified against `SELECT rev.slugify($1)` post-migration
    /// 42.
    #[test]
    fn test_preserves_accented_latin() {
        let cases = &[
            ("Como Lida com Alergênios", "como-lida-com-alergênios"),
            ("E se contratasse um padeiro", "e-se-contratasse-um-padeiro"),
            (
                "Correspondência aproximada de linhas de fatura",
                "correspondência-aproximada-de-linhas-de-fatura",
            ),
            (
                "Menu para um evento específico",
                "menu-para-um-evento-específico",
            ),
            (
                "Onde está a adivinhar vs a medir",
                "onde-está-a-adivinhar-vs-a-medir",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(slugify(input), *expected, "input={:?}", input);
        }
    }
}
