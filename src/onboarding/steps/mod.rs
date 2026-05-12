pub mod done;
pub mod model_selection;
pub mod ollama_check;
pub mod preferences;
pub mod repo_platform;
pub mod team_config;
pub mod welcome;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::error::OnboardingError;
use super::state::{OnboardingState, StepData};

/// Identifies each onboarding step. Ordering defines the canonical sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum StepId {
    Welcome = 1,
    OllamaCheck = 2,
    ModelSelection = 3,
    RepoPlatform = 4,
    Preferences = 5,
    TeamConfig = 6,
    Done = 7,
}

impl StepId {
    pub fn all() -> &'static [StepId] {
        &[
            StepId::Welcome,
            StepId::OllamaCheck,
            StepId::ModelSelection,
            StepId::RepoPlatform,
            StepId::Preferences,
            StepId::TeamConfig,
            StepId::Done,
        ]
    }

    pub fn next(self) -> Option<StepId> {
        let all = Self::all();
        let idx = all.iter().position(|&s| s == self)?;
        all.get(idx + 1).copied()
    }

    pub fn number(self) -> u8 {
        let all = Self::all();
        all.iter()
            .position(|&s| s == self)
            .map(|i| i as u8 + 1)
            .unwrap_or(0)
    }

    pub fn total() -> u8 {
        Self::all().len() as u8
    }
}

impl std::fmt::Display for StepId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepId::Welcome => write!(f, "Welcome"),
            StepId::OllamaCheck => write!(f, "Ollama Check"),
            StepId::ModelSelection => write!(f, "Model Selection"),
            StepId::RepoPlatform => write!(f, "Repository Platform"),
            StepId::Preferences => write!(f, "Preferences"),
            StepId::TeamConfig => write!(f, "Team Configuration"),
            StepId::Done => write!(f, "Done"),
        }
    }
}

/// Outcome of executing a single step.
#[derive(Debug)]
pub enum StepOutcome {
    /// Step completed successfully.
    Completed(StepData),
    /// User explicitly skipped this step.
    Skipped,
    /// User interrupted (Ctrl-C). Save and exit.
    Interrupted,
}

/// Injected dependencies available to every step.
pub struct StepContext<'a> {
    pub ui: &'a dyn TerminalUi,
    pub ollama: &'a dyn OllamaClient,
    pub git: &'a dyn GitContext,
    pub fs: &'a dyn FileSystem,
    pub app_info: &'a AppInfo,
    pub credentials: Option<&'a (dyn crate::credentials::CredentialStore + Send + Sync)>,
}

pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
}

// -- Trait definitions for dependency injection --

#[async_trait]
pub trait TerminalUi: Send + Sync {
    /// Display a message to the user.
    fn print(&self, message: &str);

    /// Display a styled header/title.
    fn print_header(&self, title: &str);

    /// Prompt the user for text input. Returns None on Ctrl-C/Ctrl-D.
    fn prompt(&self, message: &str) -> Option<String>;

    /// Prompt with a default value.
    fn prompt_with_default(&self, message: &str, default: &str) -> Option<String>;

    /// Yes/no confirmation. Returns None on Ctrl-C.
    fn confirm(&self, message: &str, default: bool) -> Option<bool>;

    /// Let the user pick from a list. Returns the index, or None on Ctrl-C.
    fn select(&self, message: &str, items: &[&str]) -> Option<usize>;

    /// Let the user pick multiple items. Returns indices, or None on Ctrl-C.
    fn multi_select(&self, message: &str, items: &[&str]) -> Option<Vec<usize>>;

    /// Prompt for a password/token (hidden input). Returns None on Ctrl-C.
    fn password(&self, message: &str) -> Option<String>;

    /// Show a spinner with a message. Returns a handle to stop it.
    fn start_spinner(&self, message: &str) -> Box<dyn SpinnerHandle>;
}

/// Handle to stop a running spinner.
pub trait SpinnerHandle: Send + Sync {
    /// Stop the spinner and show a final message.
    fn finish(&self, message: &str);
}

#[async_trait]
pub trait OllamaClient: Send + Sync {
    /// Check if the Ollama binary is installed (on PATH).
    fn is_installed(&self) -> bool;

    /// Check if Ollama is reachable.
    async fn is_running(&self) -> bool;

    /// Start the Ollama server process.
    async fn start(&self) -> Result<(), OnboardingError>;

    /// Get the Ollama version string.
    async fn version(&self) -> Result<String, OnboardingError>;

    /// List locally available models.
    async fn list_models(&self) -> Result<Vec<String>, OnboardingError>;

    /// Pull a model by name.
    async fn pull_model(&self, name: &str) -> Result<(), OnboardingError>;

    /// Send a chat message and get a response. Used by the review pipeline.
    async fn chat(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, OnboardingError>;
}

#[async_trait]
pub trait GitContext: Send + Sync {
    /// Whether the current directory is inside a git repository.
    fn is_repo(&self) -> bool;

    /// The root directory of the git repository, if any.
    fn repo_root(&self) -> Option<std::path::PathBuf>;
}

pub trait FileSystem: Send + Sync {
    /// Check if a file exists.
    fn exists(&self, path: &std::path::Path) -> bool;

    /// Write content to a file, creating parent directories as needed.
    fn write(&self, path: &std::path::Path, content: &str) -> Result<(), std::io::Error>;
}

/// Trait that every onboarding step implements.
#[async_trait]
pub trait OnboardingStep: Send + Sync {
    fn id(&self) -> StepId;
    fn title(&self) -> &'static str;

    async fn execute(
        &self,
        ctx: &StepContext<'_>,
        prior_state: &OnboardingState,
    ) -> Result<StepOutcome, OnboardingError>;
}
