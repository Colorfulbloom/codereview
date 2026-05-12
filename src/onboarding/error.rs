use thiserror::Error;

#[derive(Debug, Error)]
pub enum OnboardingError {
    #[error("Database error during onboarding: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Ollama is not reachable: {0}")]
    OllamaUnavailable(String),

    #[error("No Ollama models available and user declined to pull one")]
    NoModels,

    #[error("Git repository error: {0}")]
    Git(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Credential storage error: {0}")]
    Credential(String),

    #[error("User cancelled onboarding")]
    Cancelled,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}
