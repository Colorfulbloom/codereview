//! Accessibility (WCAG) review agent.

use async_trait::async_trait;

use super::{AgentError, ContextBudget, JSON_SCHEMA, ReviewAgent, execute_agent, format_rules};
use crate::git::FileDiff;
use crate::language::rules::Rule;
use crate::onboarding::steps::OllamaClient;
use crate::review::models::ReviewFinding;

pub const A11Y_IDS: &[&str] = &[
    "html-alt-text",
    "html-semantic-elements",
    "html-heading-hierarchy",
    "html-form-labels",
    "html-link-text",
    "html-contrast",
];

pub struct AccessibilityAgent {
    rules: Vec<Rule>,
}

impl AccessibilityAgent {
    pub fn new(all_rules: &[Rule]) -> Self {
        let rules = all_rules
            .iter()
            .filter(|r| A11Y_IDS.contains(&r.id.as_str()))
            .cloned()
            .collect();
        Self { rules }
    }

    fn system_prompt(&self) -> String {
        format!(
            "You are a web accessibility (WCAG 2.2) reviewer. Your ONLY job is to find accessibility violations.\n\n\
            Focus areas:\n\
            - Missing alt text on images\n\
            - Non-semantic HTML (div soup instead of proper elements)\n\
            - Skipped heading levels\n\
            - Form inputs without labels\n\
            - Links with non-descriptive text (\"click here\")\n\
            - Insufficient color contrast\n\n\
            {JSON_SCHEMA}\n\n\
            ## WCAG / Accessibility rules\n\n\
            {rules}\n\n\
            IMPORTANT: Only report ACCESSIBILITY issues (category: \"accessibility\"). Ignore code style and bugs.\n\
            Output a JSON array. If no accessibility issues found, output: []",
            rules = format_rules(&self.rules),
        )
    }
}

#[async_trait]
impl ReviewAgent for AccessibilityAgent {
    fn name(&self) -> &str {
        "Accessibility"
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
    use crate::language::rules::builtin_rules;

    #[test]
    fn filters_only_a11y_rules() {
        let all = builtin_rules(Language::Html);
        let agent = AccessibilityAgent::new(&all);
        assert_eq!(agent.rules().len(), 6);
        assert!(
            agent
                .rules()
                .iter()
                .all(|r| A11Y_IDS.contains(&r.id.as_str()))
        );
    }

    #[test]
    fn no_a11y_rules_from_other_languages() {
        let all = builtin_rules(Language::Php);
        let agent = AccessibilityAgent::new(&all);
        assert!(agent.rules().is_empty());
    }

    #[test]
    fn system_prompt_focuses_on_wcag() {
        let all = builtin_rules(Language::Html);
        let agent = AccessibilityAgent::new(&all);
        let prompt = agent.system_prompt();
        assert!(prompt.contains("WCAG 2.2"));
        assert!(prompt.contains("accessibility"));
        assert!(prompt.contains("alt text"));
    }
}
