//! Specialized review sub-agents.
//!
//! Each agent makes a focused LLM call with a domain-specific system prompt
//! and only the rules relevant to its domain.

pub mod accessibility;
pub mod bugs;
pub mod config_defined;
pub mod custom;
pub mod orchestrator;
pub mod security;
pub mod style;
pub mod twig;

use async_trait::async_trait;

use crate::git::FileDiff;
use crate::language::rules::Rule;
use crate::onboarding::steps::OllamaClient;
use crate::review::models::ReviewFinding;
use crate::review::parser::parse_review_response;
use crate::review::prompt::build_user_prompt;

/// Error type for agent operations.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("LLM call failed: {0}")]
    Llm(String),
}

/// A specialized review sub-agent.
#[async_trait]
pub trait ReviewAgent: Send + Sync {
    /// Human-readable name (e.g., "Security", "Style (PHP)").
    fn name(&self) -> &str;

    /// The rules this agent checks.
    fn rules(&self) -> &[Rule];

    /// Run the review and return findings.
    async fn review(
        &self,
        diffs: &[FileDiff],
        model: &str,
        ollama: &dyn OllamaClient,
    ) -> Result<Vec<ReviewFinding>, AgentError>;
}

/// JSON schema shared by all agent system prompts.
pub const JSON_SCHEMA: &str = r#"For each issue found, output a JSON object with these fields:
- "file_path": string — path to the file
- "line_number": integer — line number where the issue occurs (in the new file)
- "severity": "error" | "warning" | "info"
- "category": "bug" | "security" | "performance" | "style" | "best_practice" | "accessibility"
- "title": string — short title (under 80 chars)
- "description": string — detailed explanation of the issue
- "suggestion": string — how to fix the issue

Output a JSON array of issue objects. Only output valid JSON, no explanations or markdown."#;

/// Format rules into a bullet list for system prompts.
pub fn format_rules(rules: &[Rule]) -> String {
    rules
        .iter()
        .map(|r| format!("- [{}] {}: {}", r.severity, r.id, r.description))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Shared execution: build user prompt from diffs, call LLM, parse response.
pub async fn execute_agent(
    name: &str,
    system_prompt: &str,
    diffs: &[FileDiff],
    model: &str,
    ollama: &dyn OllamaClient,
) -> Result<Vec<ReviewFinding>, AgentError> {
    if diffs.is_empty() {
        return Ok(vec![]);
    }

    let user_prompt = build_user_prompt(diffs);

    let response = ollama
        .chat(model, system_prompt, &user_prompt)
        .await
        .map_err(|e| AgentError::Llm(format!("{name}: {e}")))?;

    Ok(parse_review_response(&response))
}
