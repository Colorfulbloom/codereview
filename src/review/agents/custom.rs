//! Custom rules review agent — runs team-defined rules from .codereview.yaml.

use async_trait::async_trait;

use super::{AgentError, ContextBudget, JSON_SCHEMA, ReviewAgent, execute_agent, format_rules};
use crate::git::FileDiff;
use crate::language::rules::Rule;
use crate::onboarding::steps::OllamaClient;
use crate::review::models::ReviewFinding;

pub struct CustomRulesAgent {
    rules: Vec<Rule>,
}

impl CustomRulesAgent {
    pub fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }

    fn system_prompt(&self) -> String {
        format!(
            "You are a code reviewer enforcing team-specific rules defined by the project configuration.\n\n\
            {JSON_SCHEMA}\n\n\
            ## Custom team rules\n\n\
            {rules}\n\n\
            IMPORTANT: Only check for violations of the custom rules listed above.\n\
            Output a JSON array. If no violations found, output: []",
            rules = format_rules(&self.rules),
        )
    }
}

#[async_trait]
impl ReviewAgent for CustomRulesAgent {
    fn name(&self) -> &str {
        "Custom Rules"
    }

    fn rules(&self) -> &[Rule] {
        &self.rules
    }

    async fn review(
        &self,
        diffs: &[FileDiff],
        model: &str,
        ollama: &dyn OllamaClient,
        budget: ContextBudget,
    ) -> Result<Vec<ReviewFinding>, AgentError> {
        if self.rules.is_empty() {
            return Ok(vec![]);
        }
        execute_agent(self.name(), &self.system_prompt(), diffs, model, ollama, budget).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::Language;
    use crate::review::models::Severity;

    fn sample_custom_rules() -> Vec<Rule> {
        vec![
            Rule {
                id: "no-debug-code".into(),
                language: Language::Php,
                severity: Severity::Error,
                description: "No dd() or var_dump() in production".into(),
                enabled: true,
            },
            Rule {
                id: "require-docblocks".into(),
                language: Language::Php,
                severity: Severity::Warning,
                description: "All public methods must have docblocks".into(),
                enabled: true,
            },
        ]
    }

    #[test]
    fn stores_all_provided_rules() {
        let rules = sample_custom_rules();
        let agent = CustomRulesAgent::new(rules.clone());
        assert_eq!(agent.rules().len(), 2);
    }

    #[test]
    fn system_prompt_includes_custom_rules() {
        let agent = CustomRulesAgent::new(sample_custom_rules());
        let prompt = agent.system_prompt();
        assert!(prompt.contains("team-specific rules"));
        assert!(prompt.contains("no-debug-code"));
        assert!(prompt.contains("require-docblocks"));
    }

    #[test]
    fn empty_rules_agent() {
        let agent = CustomRulesAgent::new(vec![]);
        assert!(agent.rules().is_empty());
    }
}
