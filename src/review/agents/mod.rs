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
use crate::review::chunking::{chunk_diffs, ContextBudget};
use crate::review::models::ReviewFinding;
use crate::review::parser::parse_and_verify_response;
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
        budget: ContextBudget,
    ) -> Result<Vec<ReviewFinding>, AgentError>;
}

/// JSON schema and accuracy contract shared by all agent system prompts.
pub const JSON_SCHEMA: &str = r#"For each issue found, output a JSON object with these fields:
- "file_path": string — path to the file
- "line_number": integer — the number shown at the start of the offending line
- "evidence": string — the offending line, copied exactly from the code (without the line-number prefix). REQUIRED.
- "severity": "error" | "warning" | "info"
- "category": "bug" | "security" | "performance" | "style" | "best_practice" | "accessibility"
- "title": string — short title (under 80 chars)
- "description": string — detailed explanation of the issue
- "suggestion": string — how to fix the issue

Accuracy requirements:
- Only report an issue you can prove by quoting the offending line in "evidence". Findings whose evidence does not appear in the code are discarded automatically.
- Use the line numbers shown at the start of each line; never estimate or invent them.
- "suggestion" must describe a change. A suggestion that repeats the existing code is discarded.
- If you are not certain a class, interface, or API exists, describe the fix in words instead of naming it.
- Apply each rule only to the language and layer it belongs to.
- Severity: "error" only for definite bugs or vulnerabilities, "warning" for probable issues, "info" for style preferences.

Output a JSON array of issue objects. If no issues are found, output an empty array: []
If there are many issues, report only the 25 most important.
Only output valid JSON, no explanations or markdown."#;

/// Format rules into a bullet list for system prompts.
pub fn format_rules(rules: &[Rule]) -> String {
    rules
        .iter()
        .map(|r| format!("- [{}] {}: {}", r.severity, r.id, r.description))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Shared execution: split diffs to fit the context window, call the LLM once
/// per chunk, and merge the parsed findings.
pub async fn execute_agent(
    name: &str,
    system_prompt: &str,
    diffs: &[FileDiff],
    model: &str,
    ollama: &dyn OllamaClient,
    budget: ContextBudget,
) -> Result<Vec<ReviewFinding>, AgentError> {
    if diffs.is_empty() {
        return Ok(vec![]);
    }

    let input_budget = budget.input_token_budget(system_prompt);
    let chunks = chunk_diffs(diffs, input_budget);

    let mut findings = Vec::new();
    for chunk in &chunks {
        let user_prompt = build_user_prompt(chunk);

        let response = ollama
            .chat_sized(model, system_prompt, &user_prompt, budget.num_ctx, budget.think)
            .await
            .map_err(|e| AgentError::Llm(format!("{name}: {e}")))?;

        findings.extend(parse_and_verify_response(&response, chunk));
    }

    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::FileStatus;
    use crate::git::testutil::make_file_diff;
    use crate::review::testutil::MockOllama;

    #[tokio::test]
    async fn execute_agent_passes_think_to_llm() {
        let ollama = MockOllama::with_response("[]");
        let budget = ContextBudget {
            num_ctx: 4096,
            think: Some(false),
        };
        let diffs = vec![make_file_diff("a.php", FileStatus::Modified, "+echo 1;")];

        execute_agent("T", "sys", &diffs, "m", &ollama, budget)
            .await
            .unwrap();

        assert_eq!(
            ollama.captured_think.lock().unwrap().as_slice(),
            &[Some(false)]
        );
    }
}
