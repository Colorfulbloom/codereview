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

pub struct LiveOllamaClient {
    /// Per-request timeout for chat calls, from `llm_timeout_seconds` in
    /// `.codereview.yaml` (300s when unconfigured).
    timeout_secs: u64,
}

impl Default for LiveOllamaClient {
    fn default() -> Self {
        Self { timeout_secs: 300 }
    }
}

impl LiveOllamaClient {
    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }

    fn chat_client(&self) -> Result<reqwest::Client, OnboardingError> {
        let mut builder = reqwest::Client::builder();
        if let Some(timeout) = timeout_duration(self.timeout_secs) {
            builder = builder.timeout(timeout);
        }
        builder
            .build()
            .map_err(|e| OnboardingError::Other(e.into()))
    }
}

/// Per-request timeout from the configured seconds; `0` means no timeout.
fn timeout_duration(secs: u64) -> Option<std::time::Duration> {
    (secs > 0).then(|| std::time::Duration::from_secs(secs))
}

/// User-facing message for a chat request that hit the client timeout.
fn timeout_message(timeout_secs: u64) -> String {
    format!(
        "Request timed out after {timeout_secs}s. Try a smaller model, lower max_context_tokens, or raise llm_timeout_seconds in .codereview.yaml."
    )
}

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
            // Flash attention lowers attention memory/compute on long prompts
            // (and is required for KV-cache quantization). Only takes effect
            // when this app is the one starting the server.
            .env("OLLAMA_FLASH_ATTENTION", "1")
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
        let client = self.chat_client()?;
        let body = build_chat_body(model, system_prompt, user_prompt, None, None);

        post_chat(&client, body, self.timeout_secs).await
    }

    async fn model_context_limit(&self, model: &str) -> Option<usize> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .ok()?;

        let resp = client
            .post("http://127.0.0.1:11434/api/show")
            .json(&serde_json::json!({ "model": model }))
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            return None;
        }

        let json: serde_json::Value = resp.json().await.ok()?;

        // The context length lives under model_info as "<arch>.context_length"
        // (e.g. "llama.context_length", "qwen2.context_length"). Scan for any
        // key ending in ".context_length" rather than guessing the arch.
        let info = json.get("model_info")?.as_object()?;
        info.iter()
            .find(|(k, _)| k.ends_with(".context_length"))
            .and_then(|(_, v)| v.as_u64())
            .map(|n| n as usize)
    }

    async fn model_supports_thinking(&self, model: &str) -> bool {
        let Ok(client) = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
        else {
            return false;
        };

        let Ok(resp) = client
            .post("http://127.0.0.1:11434/api/show")
            .json(&serde_json::json!({ "model": model }))
            .send()
            .await
        else {
            return false;
        };

        if !resp.status().is_success() {
            return false;
        }

        match resp.json::<serde_json::Value>().await {
            Ok(json) => show_response_supports_thinking(&json),
            Err(_) => false,
        }
    }

    async fn chat_sized(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        num_ctx: usize,
        think: Option<bool>,
    ) -> Result<String, OnboardingError> {
        let client = self.chat_client()?;
        let body = build_chat_body(model, system_prompt, user_prompt, Some(num_ctx), think);

        post_chat(&client, body, self.timeout_secs).await
    }
}

/// Build the JSON body for an Ollama /api/chat request.
///
/// `num_ctx` lands under `options`; `think` is a top-level field and is only
/// included when explicitly set — Ollama rejects it for models without the
/// thinking capability. Review calls (those with a `num_ctx`) also cap
/// generation at the response headroom so a looping model fails bounded
/// instead of generating until the context window fills.
fn build_chat_body(
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    num_ctx: Option<usize>,
    think: Option<bool>,
) -> serde_json::Value {
    let mut options = serde_json::json!({ "temperature": 0.1 });
    if let Some(n) = num_ctx {
        options["num_ctx"] = n.into();
        // Hard generation cap, decoupled from the chunking headroom: bounds a
        // looping model without truncating a legitimate findings array.
        options["num_predict"] = REVIEW_NUM_PREDICT.into();
    }

    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ],
        "stream": false,
        "options": options
    });
    if let Some(t) = think {
        body["think"] = t.into();
    }
    // Keep the model resident between review calls (and runs) so the iterate
    // loop doesn't pay a model reload each time. Only on review calls.
    if num_ctx.is_some() {
        body["keep_alive"] = REVIEW_KEEP_ALIVE.into();
    }

    body
}

/// Hard cap on generation tokens for a review call. Sized for the prompt's
/// "report only the 25 most important" limit (~150 tokens/finding); large
/// enough not to truncate a real findings array, small enough to bound a
/// runaway/looping model.
const REVIEW_NUM_PREDICT: usize = 4096;

/// `keep_alive` requested on review calls so the model stays loaded across the
/// review -> fix -> review loop instead of reloading each invocation.
const REVIEW_KEEP_ALIVE: &str = "30m";

/// Whether an Ollama /api/show response lists the thinking capability.
fn show_response_supports_thinking(json: &serde_json::Value) -> bool {
    json.get("capabilities")
        .and_then(|c| c.as_array())
        .is_some_and(|caps| caps.iter().any(|c| c.as_str() == Some("thinking")))
}

/// Shared POST to Ollama's /api/chat plus response parsing.
///
/// Connectivity failures map to `OllamaUnavailable`; everything else (timeout,
/// HTTP error, bad response body) is an `LlmRequest` failure — the server was
/// reachable, the request just didn't succeed.
async fn post_chat(
    client: &reqwest::Client,
    body: serde_json::Value,
    timeout_secs: u64,
) -> Result<String, OnboardingError> {
    let resp = client
        .post("http://127.0.0.1:11434/api/chat")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            let err = if e.is_timeout() {
                OnboardingError::LlmRequest(timeout_message(timeout_secs))
            } else if e.is_connect() {
                OnboardingError::OllamaUnavailable(
                    "Cannot connect to Ollama. Is it running?".into(),
                )
            } else {
                OnboardingError::LlmRequest(e.to_string())
            };
            code_review::logging::error(format!("LLM request failed: {err}"));
            err
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        return Err(OnboardingError::LlmRequest(format!(
            "Ollama API error {status}: {body_text}"
        )));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| OnboardingError::LlmRequest(e.to_string()))?;

    Ok(json["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
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

#[cfg(test)]
mod chat_body_tests {
    use super::*;

    #[test]
    fn think_omitted_when_none() {
        let body = build_chat_body("m", "sys", "user", Some(4096), None);
        assert!(body.get("think").is_none());
        assert_eq!(body["options"]["num_ctx"], 4096);
        assert_eq!(body["options"]["temperature"], 0.1);
    }

    #[test]
    fn review_calls_bound_generation_length() {
        // Without num_predict a looping model generates until the whole
        // context window fills — observed as a 20-minute hang on real
        // hardware. Review calls must cap the response at a dedicated
        // generation budget (decoupled from the chunking headroom).
        let body = build_chat_body("m", "sys", "user", Some(32768), None);
        assert_eq!(body["options"]["num_predict"], REVIEW_NUM_PREDICT);

        // Plain chats (onboarding, commit messages) stay uncapped.
        let body = build_chat_body("m", "sys", "user", None, None);
        assert!(body["options"].get("num_predict").is_none());
    }

    #[test]
    fn review_calls_keep_model_warm() {
        // Review calls request a long keep_alive so the iterate loop
        // (review -> fix -> review) doesn't reload the model each run.
        let body = build_chat_body("m", "sys", "user", Some(32768), None);
        assert_eq!(body["keep_alive"], REVIEW_KEEP_ALIVE);

        // Plain chats use Ollama's default keep_alive.
        let body = build_chat_body("m", "sys", "user", None, None);
        assert!(body.get("keep_alive").is_none());
    }

    #[test]
    fn think_false_included_when_set() {
        let body = build_chat_body("m", "sys", "user", Some(4096), Some(false));
        assert_eq!(body["think"], false);
    }

    #[test]
    fn num_ctx_omitted_when_none() {
        let body = build_chat_body("m", "sys", "user", None, None);
        assert!(body["options"].get("num_ctx").is_none());
    }

    #[test]
    fn zero_timeout_means_unlimited() {
        assert_eq!(timeout_duration(0), None);
        assert_eq!(
            timeout_duration(300),
            Some(std::time::Duration::from_secs(300))
        );
    }

    #[test]
    fn timeout_message_names_duration_and_remedies() {
        let msg = timeout_message(450);
        assert!(msg.contains("450s"));
        assert!(msg.contains("llm_timeout_seconds"));
        assert!(!msg.contains("not reachable"));
    }

    #[test]
    fn show_response_thinking_capability_detected() {
        let with = serde_json::json!({"capabilities": ["completion", "thinking", "tools"]});
        let without = serde_json::json!({"capabilities": ["completion", "tools"]});
        let missing = serde_json::json!({});

        assert!(show_response_supports_thinking(&with));
        assert!(!show_response_supports_thinking(&without));
        assert!(!show_response_supports_thinking(&missing));
    }
}
