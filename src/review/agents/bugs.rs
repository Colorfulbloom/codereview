//! Bug detection review agent.

use async_trait::async_trait;

use super::{AgentError, JSON_SCHEMA, ReviewAgent, execute_agent, format_rules};
use crate::git::FileDiff;
use crate::language::rules::Rule;
use crate::onboarding::steps::OllamaClient;
use crate::review::models::ReviewFinding;

pub const BUG_SUFFIXES: &[&str] = &["error-handling", "type-declarations", "no-unused-vars"];

pub struct BugDetectionAgent {
    rules: Vec<Rule>,
}

impl BugDetectionAgent {
    pub fn new(all_rules: &[Rule]) -> Self {
        let rules = all_rules
            .iter()
            .filter(|r| BUG_SUFFIXES.iter().any(|s| r.id.ends_with(s)))
            .cloned()
            .collect();
        Self { rules }
    }

    fn system_prompt(&self) -> String {
        format!(
            "You are a bug-detection code reviewer. Your ONLY job is to find bugs and error handling problems.\n\n\
            Focus areas:\n\
            - Empty catch blocks or silenced errors\n\
            - Missing error handling in async code (unhandled promise rejections)\n\
            - Missing type declarations that could lead to null/type errors\n\
            - Unused variables that indicate dead or incomplete code\n\n\
            {JSON_SCHEMA}\n\n\
            ## Bug detection rules\n\n\
            {rules}\n\n\
            IMPORTANT: Only report BUGS and error-handling issues. Ignore style and security.\n\
            Output a JSON array. If no bugs found, output: []",
            rules = format_rules(&self.rules),
        )
    }
}

#[async_trait]
impl ReviewAgent for BugDetectionAgent {
    fn name(&self) -> &str {
        "Bug Detection"
    }

    fn rules(&self) -> &[Rule] {
        &self.rules
    }

    async fn review(
        &self,
        diffs: &[FileDiff],
        model: &str,
        ollama: &dyn OllamaClient,
    ) -> Result<Vec<ReviewFinding>, AgentError> {
        if self.rules.is_empty() {
            return Ok(vec![]);
        }
        execute_agent(self.name(), &self.system_prompt(), diffs, model, ollama).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::Language;
    use crate::language::rules::builtin_rules;

    #[test]
    fn filters_only_bug_rules() {
        let all = builtin_rules(Language::Php);
        let agent = BugDetectionAgent::new(&all);
        let ids: Vec<&str> = agent.rules().iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"php-error-handling"));
        assert!(ids.contains(&"php-type-declarations"));
        assert!(!ids.contains(&"php-sql-injection")); // security, not bug
        assert!(!ids.contains(&"php-psr12-style")); // style, not bug
    }

    #[test]
    fn includes_js_error_handling() {
        let all = builtin_rules(Language::JavaScript);
        let agent = BugDetectionAgent::new(&all);
        assert!(agent.rules().iter().any(|r| r.id == "js-error-handling"));
        assert!(agent.rules().iter().any(|r| r.id == "js-no-unused-vars"));
    }

    #[test]
    fn system_prompt_focuses_on_bugs() {
        let all = builtin_rules(Language::Php);
        let agent = BugDetectionAgent::new(&all);
        let prompt = agent.system_prompt();
        assert!(prompt.contains("bug-detection"));
        assert!(prompt.contains("error handling"));
        // Should not contain security-focused instructions (JSON schema mentions security as a category, which is fine)
        assert!(!prompt.contains("security vulnerabilities"));
    }

    #[test]
    fn no_rules_produces_empty_agent() {
        let agent = BugDetectionAgent::new(&[]);
        assert!(agent.rules().is_empty());
    }
}
