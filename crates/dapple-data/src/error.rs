use std::fmt::Display;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DataError {
    #[error("sqlx: {0}")]
    SqlX(#[from] sqlx::Error),
    #[error("model ordering mismatch: {0}")]
    ModelColumnOrderMismatch(#[from] ModelColumnMismatchError),
    #[error("ambiguous id lookup: {0}")]
    AmbiguousIdLookup(String),
    #[error("missing id lookup: {0}")]
    MissingIdLookup(String),
    #[error("insert argument column mismatch: {0}")]
    InsertArgColumnMismatch(#[from] InsertArgColumnMismatch),
    #[error("result set problems: {0}")]
    ResultSetProblems(#[from] ResultSetProblems),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("blocked reference: {0}")]
    BlockedReference(#[from] BlockedReferenceError),
    #[error("{0}")]
    Custom(String),
}

/// Error when attempting to reference retired or quarantined objects.
#[derive(Error, Debug)]
pub struct BlockedReferenceError {
    /// The type of object that was being created (e.g., "brief_group").
    pub creating_type: String,
    /// The type of object being referenced (e.g., "brief").
    pub referenced_type: String,
    /// List of blocked object_ids and their deprecation levels.
    pub blocked_objects: Vec<BlockedObjectInfo>,
}

impl std::fmt::Display for BlockedReferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let blocked_list: Vec<String> = self
            .blocked_objects
            .iter()
            .map(|b| format!("{} ({})", b.object_id, b.level))
            .collect();
        write!(
            f,
            "cannot create {} referencing {} {}(s): {}",
            self.creating_type,
            self.blocked_objects.len(),
            self.referenced_type,
            blocked_list.join(", ")
        )
    }
}

/// Information about a blocked object reference.
#[derive(Clone, Debug)]
pub struct BlockedObjectInfo {
    pub object_id: uuid::Uuid,
    pub level: String,
}

#[derive(Error, Debug)]
pub enum InsertArgColumnMismatch {
    #[error("no columns provide for insert args struct")]
    EmptyColumnSet {},

    #[error("column `{column}` has invalid size: expected {expected}, found {found}")]
    InvalidColumnSize {
        column: String,
        expected: usize,
        found: usize,
    },
}

#[derive(Error, Debug)]
pub enum ModelColumnMismatchError {
    #[error("child key `{child_key}` appears before its parent key `{expected_at_least}`")]
    ChildBeforeParent {
        child_key: String,
        expected_at_least: String,
    },

    #[error("child key `{child_key}` has no matching parent")]
    ChildWithoutParent { child_key: String },

    #[error("column {column_name} is NULL but must be included when primary key is not NULL")]
    MissingNonNullColumn { column_name: String },
}

#[derive(Error, Debug)]
pub enum ResultSetProblems {
    #[error("set id issues:\n {0}")]
    InvalidIds(InvalidIdErrors),
}

#[derive(Clone, Debug, Default)]
pub struct InvalidIdErrors {
    pub missing_ids: Vec<String>,
    pub duplicated_ids: Vec<String>,
    pub unexpected_ids: Vec<String>,
}

impl Display for InvalidIdErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = vec![];

        if !self.missing_ids.is_empty() {
            parts.push(format!("missing ids: {:?}", self.missing_ids));
        }

        if !self.duplicated_ids.is_empty() {
            parts.push(format!("duplicated ids: {:?}", self.duplicated_ids));
        }

        if !self.unexpected_ids.is_empty() {
            parts.push(format!("unexpected ids: {:?}", self.unexpected_ids));
        }

        write!(f, "{}", parts.join(", "))
    }
}
