//! Fluent Translation List parsing utilities
//!
//! FTL resources can be parsed using one of two methods:
//! * [`parse`] - parses an input into a complete Abstract Syntax Tree representation with all source information preserved.
//! * [`parse_runtime`] - parses an input into a runtime optimized Abstract Syntax Tree
//!   representation with comments stripped.
//!
//! # Example
//!
//! ```
//! use fluent_syntax::parser;
//! use fluent_syntax::ast;
//!
//! let ftl = r#"
//! #### Resource Level Comment
//!
//! ## This is a message comment
//! hello-world = Hello World!
//!
//! "#;
//!
//! let resource = parser::parse(ftl)
//!     .expect("Failed to parse an FTL resource.");
//!
//! assert_eq!(
//!     resource.body[0],
//!     ast::Entry::ResourceComment(
//!         ast::Comment {
//!             content: vec![
//!                 "Resource Level Comment"
//!             ]
//!         }
//!     )
//! );
//! assert_eq!(
//!     resource.body[1],
//!     ast::Entry::Message(
//!         ast::Message {
//!             id: ast::Identifier {
//!                 name: "hello-world"
//!             },
//!             value: Some(ast::Pattern {
//!                 elements: vec![
//!                     ast::PatternElement::TextElement {
//!                         value: "Hello World!"
//!                     },
//!                 ]
//!             }),
//!             attributes: vec![],
//!             comment: Some(
//!                 ast::Comment {
//!                     content: vec!["This is a message comment"]
//!                 }
//!             )
//!         }
//!     ),
//! );
//! ```
//!
//! # Error Recovery
//!
//! In both modes the parser is lenient, attempting to recover from errors.
//!
//! The [`Result`] return the resulting AST in both scenarios, and in the
//! error scenario a vector of [`ParserError`] elements is returned as well.
//!
//! Any unparsed parts of the input are returned as [`ast::Entry::Junk`] elements.
#[macro_use]
mod errors;
#[macro_use]
mod macros;
mod comment;
mod core;
mod expression;
mod helper;
mod pattern;
mod runtime;
mod slice;

use crate::ast;
pub use errors::{ErrorKind, ParserError};
pub(crate) use slice::matches_fluent_ws;
pub use slice::Slice;

/// Parser result always returns an AST representation of the input,
/// and if parsing errors were encountered, a list of [`ParserError`] elements
/// is also returned.
///
/// # Example
///
/// ```
/// use fluent_syntax::parser;
/// use fluent_syntax::ast;
///
/// let ftl = r#"
/// key1 = Value 1
///
/// g@Rb@ge = #2y ds
///
/// key2 = Value 2
///
/// "#;
///
/// let (resource, errors) = parser::parse_runtime(ftl)
///     .expect_err("Resource should contain errors.");
///
/// assert_eq!(
///     errors,
///     vec![
///         parser::ParserError {
///             pos: 18..19,
///             slice: Some(17..35),
///             kind: parser::ErrorKind::ExpectedToken('=')
///         }
///     ]
/// );
///
/// assert_eq!(
///     resource.body[0],
///     ast::Entry::Message(
///         ast::Message {
///             id: ast::Identifier {
///                 name: "key1"
///             },
///             value: Some(ast::Pattern {
///                 elements: vec![
///                     ast::PatternElement::TextElement {
///                         value: "Value 1"
///                     },
///                 ]
///             }),
///             attributes: vec![],
///             comment: None,
///         }
///     ),
/// );
///
/// assert_eq!(
///     resource.body[1],
///     ast::Entry::Junk {
///         content: "g@Rb@ge = #2y ds\n\n"
///     }
/// );
///
/// assert_eq!(
///     resource.body[2],
///     ast::Entry::Message(
///         ast::Message {
///             id: ast::Identifier {
///                 name: "key2"
///             },
///             value: Some(ast::Pattern {
///                 elements: vec![
///                     ast::PatternElement::TextElement {
///                         value: "Value 2"
///                     },
///                 ]
///             }),
///             attributes: vec![],
///             comment: None,
///         }
///     ),
/// );
/// ```
pub type Result<S> = std::result::Result<ast::Resource<S>, (ast::Resource<S>, Vec<ParserError>)>;

/// Parses an input into a complete Abstract Syntax Tree representation with
/// all source information preserved.
///
/// This mode is intended for tooling, linters and other scenarios where
/// complete representation, with comments, is preferred over speed and memory
/// utilization.
///
/// # Example
///
/// ```
/// use fluent_syntax::parser;
/// use fluent_syntax::ast;
///
/// let ftl = r#"
/// #### Resource Level Comment
///
/// ## This is a message comment
/// hello-world = Hello World!
///
/// "#;
///
/// let resource = parser::parse(ftl)
///     .expect("Failed to parse an FTL resource.");
///
/// assert_eq!(
///     resource.body[0],
///     ast::Entry::ResourceComment(
///         ast::Comment {
///             content: vec![
///                 "Resource Level Comment"
///             ]
///         }
///     )
/// );
/// assert_eq!(
///     resource.body[1],
///     ast::Entry::Message(
///         ast::Message {
///             id: ast::Identifier {
///                 name: "hello-world"
///             },
///             value: Some(ast::Pattern {
///                 elements: vec![
///                     ast::PatternElement::TextElement {
///                         value: "Hello World!"
///                     },
///                 ]
///             }),
///             attributes: vec![],
///             comment: Some(
///                 ast::Comment {
///                     content: vec!["This is a message comment"]
///                 }
///             )
///         }
///     ),
/// );
/// ```
pub fn parse<'s, S>(input: S) -> Result<S>
where
    S: Slice<'s>,
{
    core::Parser::new(input).parse()
}

/// Parses an input into an Abstract Syntax Tree representation with comments stripped.
///
/// This mode is intended for runtime use of Fluent. It currently strips all
/// comments improving parsing performance and reducing the size of the AST tree.
///
/// # Example
///
/// ```
/// use fluent_syntax::parser;
/// use fluent_syntax::ast;
///
/// let ftl = r#"
/// #### Resource Level Comment
///
/// ## This is a message comment
/// hello-world = Hello World!
///
/// "#;
///
/// let resource = parser::parse_runtime(ftl)
///     .expect("Failed to parse an FTL resource.");
///
/// assert_eq!(
///     resource.body[0],
///     ast::Entry::Message(
///         ast::Message {
///             id: ast::Identifier {
///                 name: "hello-world"
///             },
///             value: Some(ast::Pattern {
///                 elements: vec![
///                     ast::PatternElement::TextElement {
///                         value: "Hello World!"
///                     },
///                 ]
///             }),
///             attributes: vec![],
///             comment: None,
///         }
///     ),
/// );
/// ```
pub fn parse_runtime<'s, S>(input: S) -> Result<S>
where
    S: Slice<'s>,
{
    core::Parser::new(input).parse_runtime()
}

#[cfg(test)]
mod unicode_identifier_tests {
    use super::*;

    fn parse_or_panic(ftl: &str) -> ast::Resource<&str> {
        match parse(ftl) {
            Ok(r) => r,
            Err((_, errors)) => panic!("expected clean parse, got {errors:?}"),
        }
    }

    fn message_keys<'a>(resource: &'a ast::Resource<&'a str>) -> Vec<&'a str> {
        resource
            .body
            .iter()
            .filter_map(|e| match e {
                ast::Entry::Message(m) => Some(m.id.name),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn accepts_latin_diacritics_in_message_keys() {
        let ftl = "\
esboçar-o-restaurante-pack = Esboçar o restaurante
padrão-diário-output = Padrão diário
as-horas-de-operação-intro = As horas de operação
o-tráfego-esperado-padrão-semanal-input = Tráfego semanal
";
        let resource = parse_or_panic(ftl);
        assert_eq!(
            message_keys(&resource),
            vec![
                "esboçar-o-restaurante-pack",
                "padrão-diário-output",
                "as-horas-de-operação-intro",
                "o-tráfego-esperado-padrão-semanal-input",
            ]
        );
    }

    #[test]
    fn accepts_unicode_in_attribute_keys() {
        let ftl = "esboçar-restaurante-pack = Body\n    .descrição = Descrição da carta\n";
        let resource = parse_or_panic(ftl);
        let attrs = match &resource.body[0] {
            ast::Entry::Message(m) => &m.attributes,
            _ => panic!("expected message"),
        };
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs[0].id.name, "descrição");
    }

    #[test]
    fn rejects_leading_digit() {
        let ftl = "123-leading-digit = bad\n";
        assert!(parse(ftl).is_err());
    }

    #[test]
    fn rejects_punctuation() {
        let ftl = "o-que-serviria? = bad\n";
        assert!(parse(ftl).is_err());
    }

    #[test]
    fn ascii_only_keys_still_parse() {
        let ftl = "ascii-only-key = value\nkey-with-numbers-2025 = ok\n";
        let resource = parse_or_panic(ftl);
        assert_eq!(
            message_keys(&resource),
            vec!["ascii-only-key", "key-with-numbers-2025"]
        );
    }

    #[test]
    fn junk_line_with_non_alpha_unicode_start_terminates() {
        // Regression: a junk line starting with a non-letter non-ASCII char used
        // to spin skip_to_next_entry_start forever (it broke on any `*b >= 0x80`
        // while get_identifier only advances on is_alphabetic codepoints). Each
        // parse MUST return, not hang, and recovery must still find the valid
        // message after the junk line.
        for ftl in [
            "😀 not an entry\nreal-message = ok\n",
            "— em dash junk\nreal-message = ok\n",
            "« guillemet junk\nreal-message = ok\n",
        ] {
            let (resource, errors) = parse(ftl).expect_err("junk line should error");
            assert!(!errors.is_empty(), "expected a recovery error: {ftl:?}");
            assert!(
                message_keys(&resource).contains(&"real-message"),
                "recovery should still parse the message after junk: {ftl:?}",
            );
        }
    }
}
