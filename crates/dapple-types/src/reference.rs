use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Trait representing a lightweight reference to another top-level object.
/// Consumer-facing references should implement this trait.
pub trait ObjectReference {
    fn id(&self) -> Uuid;
    fn endpoint(&self) -> &'static str;
}

// ----------------------
// Consumer-facing references
// ----------------------

/// A consumer-facing reference to a Category object.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct CategoryReference {
    pub id: Uuid,
}

impl CategoryReference {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl ObjectReference for CategoryReference {
    #[inline]
    fn id(&self) -> Uuid {
        self.id
    }

    #[inline]
    fn endpoint(&self) -> &'static str {
        "categories"
    }
}

impl From<Uuid> for CategoryReference {
    fn from(u: Uuid) -> Self {
        CategoryReference { id: u }
    }
}

/// A consumer-facing reference to a Step object.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct StepReference {
    pub id: Uuid,
}

impl StepReference {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl ObjectReference for StepReference {
    #[inline]
    fn id(&self) -> Uuid {
        self.id
    }

    #[inline]
    fn endpoint(&self) -> &'static str {
        "steps"
    }
}

impl From<Uuid> for StepReference {
    fn from(u: Uuid) -> Self {
        StepReference { id: u }
    }
}

/// A consumer-facing reference to a Task object.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct TaskReference {
    pub id: Uuid,
}

impl TaskReference {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl ObjectReference for TaskReference {
    #[inline]
    fn id(&self) -> Uuid {
        self.id
    }

    #[inline]
    fn endpoint(&self) -> &'static str {
        "tasks"
    }
}

impl From<Uuid> for TaskReference {
    fn from(u: Uuid) -> Self {
        TaskReference { id: u }
    }
}

/// A consumer-facing reference to a Path object.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct PathReference {
    pub id: Uuid,
}

impl PathReference {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl ObjectReference for PathReference {
    #[inline]
    fn id(&self) -> Uuid {
        self.id
    }

    // Generic endpoint for path-like objects.
    #[inline]
    fn endpoint(&self) -> &'static str {
        "paths"
    }
}

impl From<Uuid> for PathReference {
    fn from(u: Uuid) -> Self {
        PathReference { id: u }
    }
}

// ----------------------
// Typed consumer-facing Path references
// ----------------------

#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct PlaybookPathReference {
    pub id: Uuid,
}

impl PlaybookPathReference {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl ObjectReference for PlaybookPathReference {
    #[inline]
    fn id(&self) -> Uuid {
        self.id
    }

    #[inline]
    fn endpoint(&self) -> &'static str {
        "playbook_paths"
    }
}

impl From<Uuid> for PlaybookPathReference {
    fn from(u: Uuid) -> Self {
        PlaybookPathReference { id: u }
    }
}

#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct HatPathReference {
    pub id: Uuid,
}

impl HatPathReference {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl ObjectReference for HatPathReference {
    #[inline]
    fn id(&self) -> Uuid {
        self.id
    }

    #[inline]
    fn endpoint(&self) -> &'static str {
        "hat_paths"
    }
}

impl From<Uuid> for HatPathReference {
    fn from(u: Uuid) -> Self {
        HatPathReference { id: u }
    }
}

#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct StepPathReference {
    pub id: Uuid,
}

impl StepPathReference {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl ObjectReference for StepPathReference {
    #[inline]
    fn id(&self) -> Uuid {
        self.id
    }

    #[inline]
    fn endpoint(&self) -> &'static str {
        "step_paths"
    }
}

impl From<Uuid> for StepPathReference {
    fn from(u: Uuid) -> Self {
        StepPathReference { id: u }
    }
}

#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct TaskPathReference {
    pub id: Uuid,
}

impl TaskPathReference {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl ObjectReference for TaskPathReference {
    #[inline]
    fn id(&self) -> Uuid {
        self.id
    }

    #[inline]
    fn endpoint(&self) -> &'static str {
        "task_paths"
    }
}

impl From<Uuid> for TaskPathReference {
    fn from(u: Uuid) -> Self {
        TaskPathReference { id: u }
    }
}

#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct CategoryPathReference {
    pub id: Uuid,
}

impl CategoryPathReference {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl ObjectReference for CategoryPathReference {
    #[inline]
    fn id(&self) -> Uuid {
        self.id
    }

    #[inline]
    fn endpoint(&self) -> &'static str {
        "category_paths"
    }
}

impl From<Uuid> for CategoryPathReference {
    fn from(u: Uuid) -> Self {
        CategoryPathReference { id: u }
    }
}

// Fluent Message references

/// A consumer-facing reference to a FluentMessage object.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct FluentMessageReference {
    pub id: Uuid,
}

impl FluentMessageReference {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl ObjectReference for FluentMessageReference {
    #[inline]
    fn id(&self) -> Uuid {
        self.id
    }

    #[inline]
    fn endpoint(&self) -> &'static str {
        "fluent-messages"
    }
}

impl From<Uuid> for FluentMessageReference {
    fn from(u: Uuid) -> Self {
        FluentMessageReference { id: u }
    }
}

/// A consumer-facing reference to a FluentMessagePath object.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct FluentMessagePathReference {
    pub id: Uuid,
    pub name: String,
}

impl FluentMessagePathReference {
    pub fn new(id: Uuid, name: String) -> Self {
        Self { id, name }
    }
}

impl ObjectReference for FluentMessagePathReference {
    #[inline]
    fn id(&self) -> Uuid {
        self.id
    }

    #[inline]
    fn endpoint(&self) -> &'static str {
        "fluent-message-paths"
    }
}

impl From<(Uuid, String)> for FluentMessagePathReference {
    fn from(v: (Uuid, String)) -> Self {
        FluentMessagePathReference { id: v.0, name: v.1 }
    }
}

// Prompt references

/// A consumer-facing reference to a Prompt object.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct PromptReference {
    pub id: Uuid,
}

impl PromptReference {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl ObjectReference for PromptReference {
    #[inline]
    fn id(&self) -> Uuid {
        self.id
    }

    #[inline]
    fn endpoint(&self) -> &'static str {
        "prompts"
    }
}

impl From<Uuid> for PromptReference {
    fn from(u: Uuid) -> Self {
        PromptReference { id: u }
    }
}

/// A consumer-facing reference to a PromptPath object.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct PromptPathReference {
    pub id: Uuid,
}

impl PromptPathReference {
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

impl ObjectReference for PromptPathReference {
    #[inline]
    fn id(&self) -> Uuid {
        self.id
    }

    #[inline]
    fn endpoint(&self) -> &'static str {
        "prompt_paths"
    }
}

impl From<Uuid> for PromptPathReference {
    fn from(u: Uuid) -> Self {
        PromptPathReference { id: u }
    }
}
