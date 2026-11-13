use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("IO error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("SUMMARY.md not found in guide directory: {0}")]
    MissingSummary(String),

    #[error(
        "CONDUITS.md not found in guide directory: {0} (every guide requires a conduit-facing context document at the guide root)"
    )]
    MissingConduits(String),

    #[error("Invalid SUMMARY.md link on line {line}: {detail}")]
    InvalidSummaryLink { line: usize, detail: String },

    #[error("Chapter file not found: {0}")]
    MissingChapterFile(String),

    #[error("Unclosed fence directive starting at line {line} in {file}")]
    UnclosedFence { file: String, line: usize },

    #[error("Invalid attribute syntax at line {line} in {file}: {detail}")]
    InvalidAttributes {
        file: String,
        line: usize,
        detail: String,
    },

    #[error("Unknown directive type '{directive}' at line {line} in {file}")]
    UnknownDirective {
        file: String,
        line: usize,
        directive: String,
    },

    #[error("guide.toml not found in guide directory: {0}")]
    MissingGuideToml(String),

    #[error("Invalid guide.toml: {detail}")]
    InvalidGuideToml { detail: String },

    #[error("Invalid Fluent file '{file}' for locale '{locale}': {detail}")]
    InvalidFtlFile {
        locale: String,
        file: String,
        detail: String,
    },

    #[error("ref and name attributes are mutually exclusive")]
    RefAndNameConflict,

    #[error("unwired brief system:\n{detail}")]
    UnwiredBriefSystem { detail: String },

    #[error("invalid canonical.toml: {detail}")]
    InvalidCanonicalToml { detail: String },
}
