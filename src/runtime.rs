//! Concrete implementations of the trait interfaces for production use.
//! Some types here are used only by the binary crate (main.rs), not by tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use console::Style;
use dialoguer::{Confirm, Input, MultiSelect, Password, Select};
use indicatif::{ProgressBar, ProgressStyle};

use code_review::onboarding::error::OnboardingError;
use code_review::onboarding::steps::{
    FileSystem, GitContext, OllamaClient, SpinnerHandle, TerminalUi,
};

// -- TerminalUi --

pub struct ConsoleUi;

impl TerminalUi for ConsoleUi {
    fn print(&self, message: &str) {
        println!("{message}");
    }

    fn print_header(&self, title: &str) {
        let style = Style::new().bold().cyan();
        // Use ASCII fallback if terminal doesn't support UTF-8
        let bar = if supports_unicode() {
            "━━━"
        } else {
            "---"
        };
        println!("\n{}", style.apply_to(format!("{bar} {title} {bar}")));
    }

    fn prompt(&self, message: &str) -> Option<String> {
        Input::new().with_prompt(message).interact_text().ok()
    }

    fn prompt_with_default(&self, message: &str, default: &str) -> Option<String> {
        Input::new()
            .with_prompt(message)
            .default(default.to_string())
            .interact_text()
            .ok()
    }

    fn confirm(&self, message: &str, default: bool) -> Option<bool> {
        Confirm::new()
            .with_prompt(message)
            .default(default)
            .interact()
            .ok()
    }

    fn select(&self, message: &str, items: &[&str]) -> Option<usize> {
        Select::new()
            .with_prompt(message)
            .items(items)
            .interact()
            .ok()
    }

    fn multi_select(&self, message: &str, items: &[&str]) -> Option<Vec<usize>> {
        MultiSelect::new()
            .with_prompt(message)
            .items(items)
            .interact()
            .ok()
    }

    fn password(&self, message: &str) -> Option<String> {
        Password::new().with_prompt(message).interact().ok()
    }

    fn start_spinner(&self, message: &str) -> Box<dyn SpinnerHandle> {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message(message.to_string());
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        Box::new(ConsoleSpinner(pb))
    }
}

struct ConsoleSpinner(ProgressBar);

impl SpinnerHandle for ConsoleSpinner {
    fn finish(&self, message: &str) {
        self.0.finish_with_message(message.to_string());
    }
}

/// Check if the terminal likely supports Unicode.
fn supports_unicode() -> bool {
    if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
        return false;
    }
    // Windows cmd.exe without UTF-8 codepage
    if cfg!(target_os = "windows") {
        return std::env::var("WT_SESSION").is_ok() // Windows Terminal
            || std::env::var("TERM_PROGRAM").is_ok(); // Other modern terminals
    }
    true
}

// -- OllamaClient --

pub struct LiveOllamaClient;

#[async_trait]
impl OllamaClient for LiveOllamaClient {
    fn is_installed(&self) -> bool {
        let cmd = if cfg!(target_os = "windows") {
            "where"
        } else {
            "which"
        };
        std::process::Command::new(cmd)
            .arg("ollama")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    async fn is_running(&self) -> bool {
        reqwest::get("http://127.0.0.1:11434").await.is_ok()
    }

    async fn start(&self) -> Result<(), OnboardingError> {
        let cmd = if cfg!(target_os = "windows") {
            "ollama.exe"
        } else {
            "ollama"
        };

        std::process::Command::new(cmd)
            .arg("serve")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| {
                OnboardingError::OllamaUnavailable(format!(
                    "Failed to start Ollama: {e}. Is it installed? https://ollama.com"
                ))
            })?;

        // Poll until Ollama responds, up to 10 seconds
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if self.is_running().await {
                return Ok(());
            }
        }

        Err(OnboardingError::OllamaUnavailable(
            "Ollama did not respond within 10 seconds. Try running `ollama serve` manually.".into(),
        ))
    }

    async fn version(&self) -> Result<String, OnboardingError> {
        let resp = reqwest::get("http://127.0.0.1:11434/api/version")
            .await
            .map_err(|e| OnboardingError::OllamaUnavailable(e.to_string()))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| OnboardingError::OllamaUnavailable(e.to_string()))?;

        Ok(body["version"].as_str().unwrap_or("unknown").to_string())
    }

    async fn list_models(&self) -> Result<Vec<String>, OnboardingError> {
        let resp = reqwest::get("http://127.0.0.1:11434/api/tags")
            .await
            .map_err(|e| OnboardingError::OllamaUnavailable(e.to_string()))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| OnboardingError::OllamaUnavailable(e.to_string()))?;

        let models = body["models"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    async fn pull_model(&self, name: &str) -> Result<(), OnboardingError> {
        let client = reqwest::Client::new();
        let resp = client
            .post("http://127.0.0.1:11434/api/pull")
            .json(&serde_json::json!({ "name": name, "stream": false }))
            .send()
            .await
            .map_err(|e| OnboardingError::OllamaUnavailable(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(OnboardingError::OllamaUnavailable(format!(
                "Failed to pull model {name}: HTTP {}",
                resp.status()
            )));
        }

        Ok(())
    }

    async fn chat(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, OnboardingError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| OnboardingError::OllamaUnavailable(e.to_string()))?;

        let body = serde_json::json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
            ],
            "stream": false,
            "options": { "temperature": 0.1 }
        });

        let resp = client
            .post("http://127.0.0.1:11434/api/chat")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    OnboardingError::OllamaUnavailable(
                        "Request timed out. Try a smaller model or increase timeout.".into(),
                    )
                } else if e.is_connect() {
                    OnboardingError::OllamaUnavailable(
                        "Cannot connect to Ollama. Is it running?".into(),
                    )
                } else {
                    OnboardingError::OllamaUnavailable(e.to_string())
                }
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            return Err(OnboardingError::OllamaUnavailable(format!(
                "Ollama API error {status}: {body_text}"
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| OnboardingError::OllamaUnavailable(e.to_string()))?;

        Ok(json["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string())
    }
}

// -- GitContext --

pub struct LiveGitContext;

#[async_trait]
impl GitContext for LiveGitContext {
    fn is_repo(&self) -> bool {
        git2::Repository::discover(".").is_ok()
    }

    fn repo_root(&self) -> Option<PathBuf> {
        git2::Repository::discover(".")
            .ok()
            .and_then(|repo| repo.workdir().map(Path::to_path_buf))
    }
}

// -- FileSystem --

pub struct LiveFileSystem;

impl FileSystem for LiveFileSystem {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn write(&self, path: &Path, content: &str) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)
    }
}
