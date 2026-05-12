//! Mock implementations for testing onboarding steps and orchestrator.

#![cfg(test)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;

use super::error::OnboardingError;
use super::progress::OnboardingPersistence;
use super::state::OnboardingState;
use super::steps::{FileSystem, GitContext, OllamaClient, SpinnerHandle, TerminalUi};

// -- MockUi --

/// A mock terminal UI that replays scripted responses.
pub struct MockUi {
    responses: Mutex<VecDeque<MockResponse>>,
    pub output: Mutex<Vec<String>>,
}

#[derive(Debug, Clone)]
pub enum MockResponse {
    Text(String),
    Bool(bool),
    Index(usize),
    Indices(Vec<usize>),
    Interrupt,
}

impl MockUi {
    pub fn new(responses: Vec<MockResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            output: Mutex::new(Vec::new()),
        }
    }

    fn next_response(&self) -> Option<MockResponse> {
        self.responses.lock().unwrap().pop_front()
    }
}

impl TerminalUi for MockUi {
    fn print(&self, message: &str) {
        self.output.lock().unwrap().push(message.to_string());
    }

    fn print_header(&self, title: &str) {
        self.output
            .lock()
            .unwrap()
            .push(format!("[HEADER] {title}"));
    }

    fn prompt(&self, _message: &str) -> Option<String> {
        match self.next_response()? {
            MockResponse::Text(s) => Some(s),
            MockResponse::Interrupt => None,
            _ => panic!("MockUi: expected Text response for prompt"),
        }
    }

    fn prompt_with_default(&self, _message: &str, default: &str) -> Option<String> {
        match self.next_response() {
            Some(MockResponse::Text(s)) => Some(s),
            Some(MockResponse::Interrupt) => None,
            None => Some(default.to_string()),
            _ => panic!("MockUi: expected Text response for prompt_with_default"),
        }
    }

    fn confirm(&self, _message: &str, default: bool) -> Option<bool> {
        match self.next_response() {
            Some(MockResponse::Bool(b)) => Some(b),
            Some(MockResponse::Interrupt) => None,
            None => Some(default),
            _ => panic!("MockUi: expected Bool response for confirm"),
        }
    }

    fn select(&self, _message: &str, _items: &[&str]) -> Option<usize> {
        match self.next_response()? {
            MockResponse::Index(i) => Some(i),
            MockResponse::Interrupt => None,
            _ => panic!("MockUi: expected Index response for select"),
        }
    }

    fn multi_select(&self, _message: &str, _items: &[&str]) -> Option<Vec<usize>> {
        match self.next_response()? {
            MockResponse::Indices(v) => Some(v),
            MockResponse::Interrupt => None,
            _ => panic!("MockUi: expected Indices response for multi_select"),
        }
    }

    fn password(&self, _message: &str) -> Option<String> {
        match self.next_response()? {
            MockResponse::Text(s) => Some(s),
            MockResponse::Interrupt => None,
            _ => panic!("MockUi: expected Text response for password"),
        }
    }

    fn start_spinner(&self, message: &str) -> Box<dyn SpinnerHandle> {
        self.output
            .lock()
            .unwrap()
            .push(format!("[SPINNER] {message}"));
        Box::new(NoopSpinner)
    }
}

struct NoopSpinner;

impl SpinnerHandle for NoopSpinner {
    fn finish(&self, _message: &str) {}
}

// -- MockOllamaClient --

pub struct MockOllamaClient {
    pub installed: bool,
    pub running: Mutex<bool>,
    pub version: String,
    pub models: Vec<String>,
    pub pull_succeeds: bool,
}

impl MockOllamaClient {
    pub fn running() -> Self {
        Self {
            installed: true,
            running: Mutex::new(true),
            version: "0.5.0".to_string(),
            models: vec!["gemma4:latest".to_string()],
            pull_succeeds: true,
        }
    }

    pub fn not_running() -> Self {
        Self {
            installed: true,
            running: Mutex::new(false),
            version: "0.5.0".to_string(),
            models: vec![],
            pull_succeeds: true,
        }
    }

    pub fn not_installed() -> Self {
        Self {
            installed: false,
            running: Mutex::new(false),
            version: String::new(),
            models: vec![],
            pull_succeeds: false,
        }
    }
}

#[async_trait]
impl OllamaClient for MockOllamaClient {
    fn is_installed(&self) -> bool {
        self.installed
    }

    async fn is_running(&self) -> bool {
        *self.running.lock().unwrap()
    }

    async fn start(&self) -> Result<(), OnboardingError> {
        *self.running.lock().unwrap() = true;
        Ok(())
    }

    async fn version(&self) -> Result<String, OnboardingError> {
        Ok(self.version.clone())
    }

    async fn list_models(&self) -> Result<Vec<String>, OnboardingError> {
        Ok(self.models.clone())
    }

    async fn pull_model(&self, _name: &str) -> Result<(), OnboardingError> {
        if self.pull_succeeds {
            Ok(())
        } else {
            Err(OnboardingError::OllamaUnavailable(
                "pull failed".to_string(),
            ))
        }
    }

    async fn chat(
        &self,
        _model: &str,
        _system_prompt: &str,
        _user_prompt: &str,
    ) -> Result<String, OnboardingError> {
        Ok("[]".to_string())
    }
}

// -- MockGitContext --

pub struct MockGitContext {
    pub is_repo: bool,
    pub root: Option<PathBuf>,
}

impl MockGitContext {
    pub fn in_repo(root: PathBuf) -> Self {
        Self {
            is_repo: true,
            root: Some(root),
        }
    }

    pub fn not_in_repo() -> Self {
        Self {
            is_repo: false,
            root: None,
        }
    }
}

#[async_trait]
impl GitContext for MockGitContext {
    fn is_repo(&self) -> bool {
        self.is_repo
    }

    fn repo_root(&self) -> Option<PathBuf> {
        self.root.clone()
    }
}

// -- MockFileSystem --

pub struct MockFileSystem {
    pub existing_files: Mutex<Vec<PathBuf>>,
    pub written_files: Mutex<Vec<(PathBuf, String)>>,
}

impl MockFileSystem {
    pub fn empty() -> Self {
        Self {
            existing_files: Mutex::new(Vec::new()),
            written_files: Mutex::new(Vec::new()),
        }
    }

    pub fn with_existing(files: Vec<PathBuf>) -> Self {
        Self {
            existing_files: Mutex::new(files),
            written_files: Mutex::new(Vec::new()),
        }
    }
}

impl FileSystem for MockFileSystem {
    fn exists(&self, path: &Path) -> bool {
        self.existing_files
            .lock()
            .unwrap()
            .iter()
            .any(|p| p == path)
    }

    fn write(&self, path: &Path, content: &str) -> Result<(), std::io::Error> {
        self.written_files
            .lock()
            .unwrap()
            .push((path.to_path_buf(), content.to_string()));
        Ok(())
    }
}

// -- MockPersistence --

pub struct MockPersistence {
    pub state: Mutex<Option<OnboardingState>>,
    pub save_count: Mutex<usize>,
}

impl MockPersistence {
    pub fn empty() -> Self {
        Self {
            state: Mutex::new(None),
            save_count: Mutex::new(0),
        }
    }

    pub fn with_state(state: OnboardingState) -> Self {
        Self {
            state: Mutex::new(Some(state)),
            save_count: Mutex::new(0),
        }
    }
}

impl OnboardingPersistence for MockPersistence {
    fn load_state(&self) -> Result<Option<OnboardingState>, OnboardingError> {
        Ok(self.state.lock().unwrap().clone())
    }

    fn save_state(&self, state: &OnboardingState) -> Result<(), OnboardingError> {
        *self.state.lock().unwrap() = Some(state.clone());
        *self.save_count.lock().unwrap() += 1;
        Ok(())
    }

    fn clear_state(&self) -> Result<(), OnboardingError> {
        *self.state.lock().unwrap() = None;
        Ok(())
    }

    fn has_completed_onboarding(&self) -> Result<bool, OnboardingError> {
        match self.load_state()? {
            Some(state) => Ok(state.is_complete()),
            None => Ok(false),
        }
    }
}

// -- Helper to build a StepContext --

use super::steps::AppInfo;

pub const TEST_APP_INFO: AppInfo = AppInfo {
    name: "code-review",
    version: "0.1.0-test",
};

pub fn make_context<'a>(
    ui: &'a dyn TerminalUi,
    ollama: &'a dyn OllamaClient,
    git: &'a dyn GitContext,
    fs: &'a dyn FileSystem,
) -> super::steps::StepContext<'a> {
    super::steps::StepContext {
        ui,
        ollama,
        git,
        fs,
        app_info: &TEST_APP_INFO,
        credentials: None,
    }
}
