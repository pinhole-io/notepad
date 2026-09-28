use thiserror::Error;

#[derive(Debug, Error)]
pub enum NotepadError {
    #[error("entry not found: {0}")]
    EntryNotFound(String),
}

#[derive(Debug, Error)]
pub enum LifeTaskError {
    #[error("task not found: {0}")]
    TaskNotFound(String),
    #[error("task deleted: {0}")]
    TaskDeleted(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("failed to parse registry: {message}")]
    Parse { message: String },
    #[error("io error at {path}: {message}")]
    Io { path: String, message: String },
}

#[derive(Debug, Error)]
pub enum ArtifactError {
    #[error("artifact not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Error)]
pub enum PlanError {
    #[error("plan not found: {0}")]
    PlanNotFound(String),
    #[error("task not found: {0}")]
    TaskNotFound(String),
    #[error("invalid plan draft: {0}")]
    InvalidDraft(String),
    #[error("failed to parse registry: {message}")]
    Parse { message: String },
    #[error("io error at {path}: {message}")]
    Io { path: String, message: String },
}

#[cfg(feature = "synthesize")]
#[derive(Debug, Error)]
pub enum SynthesisError {
    #[error("LLM completion failed: {0}")]
    Chat(#[from] superglue::chat::ChatError),
    #[error("empty LLM response")]
    EmptyResponse,
    #[error("failed to parse extraction JSON: {0}")]
    ParseExtraction(String),
    #[error("exceeded max extraction rounds ({0})")]
    MaxRoundsExceeded(u32),
}
