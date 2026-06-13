use thiserror::Error;

#[derive(Debug, Error)]
pub enum OnboardingError {
    #[error("Database error during onboarding: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Ollama is not reachable: {0}")]
    OllamaUnavailable(String),

    /// An LLM request reached Ollama but failed (timeout, HTTP error, bad
    /// response). Distinct from [`OllamaUnavailable`](Self::OllamaUnavailable)
    /// so a slow request is never reported as a connectivity problem.
    #[error("{0}")]
    LlmRequest(String),

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_request_error_displays_bare_message() {
        // A timed-out or failed request must NOT claim Ollama is unreachable —
        // that wording sent a user debugging connectivity instead of timeouts.
        let err = OnboardingError::LlmRequest("Request timed out after 300s.".into());
        assert_eq!(err.to_string(), "Request timed out after 300s.");
    }

    #[test]
    fn unreachable_error_reserved_for_connectivity() {
        let err = OnboardingError::OllamaUnavailable("connection refused".into());
        assert!(err.to_string().starts_with("Ollama is not reachable"));
    }
}
