use thiserror::Error;

#[derive(Debug, Error)]
pub enum TupiError {
    #[error("failed to read catalog: {0}")]
    CatalogRead(String),
    #[error("failed to parse catalog yaml: {0}")]
    CatalogParse(String),
    #[error("catalog validation failed: {0}")]
    CatalogValidation(String),
    #[error("failed to read agent: {0}")]
    AgentRead(String),
    #[error("agent validation failed: {0}")]
    AgentValidation(String),
    #[error("profile validation failed: {0}")]
    ProfileValidation(String),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("url error: {0}")]
    Url(#[from] url::ParseError),
}

pub type Result<T> = std::result::Result<T, TupiError>;
