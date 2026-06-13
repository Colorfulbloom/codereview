//! Config-defined custom agent — user-defined agents from .codereview.yaml.

use async_trait::async_trait;

use super::{AgentError, ContextBudget, JSON_SCHEMA, ReviewAgent, execute_agent, format_rules};
use crate::config::AgentConfig;
use crate::git::FileDiff;
use crate::language::rules::Rule;
use crate::onboarding::steps::OllamaClient;
use crate::review::models::ReviewFinding;

pub struct ConfigDefinedAgent {
    name: String,
    user_prompt: String,
    rules: Vec<Rule>,
}

impl ConfigDefinedAgent {
    /// Create from a config-defined agent definition.
    pub fn from_config(config: &AgentConfig) -> Self {
        let rules: Vec<Rule> = config
            .rules
            .iter()
            .map(|r| Rule {
                id: r.id.clone(),
                language: crate::language::Language::Php, // placeholder — custom agents are cross-language
                severity: r.severity,
                description: r.description.clone(),
                enabled: true,
            })
            .collect();

        Self {
            name: config.name.clone(),
            user_prompt: config.prompt.clone(),
            rules,
        }
    }

    fn system_prompt(&self) -> String {
        let mut prompt = self.user_prompt.clone();

        // Auto-append JSON schema — the critical safety rail
        prompt.push_str("\n\n");
        prompt.push_str(JSON_SCHEMA);

        if !self.rules.is_empty() {
            prompt.push_str("\n\n## Rules to enforce\n\n");
            prompt.push_str(&format_rules(&self.rules));
        }

        prompt.push_str("\n\nOutput a JSON array of issue objects. If no issues found, output: []");

        prompt
    }
}

#[async_trait]
impl ReviewAgent for ConfigDefinedAgent {
    fn name(&self) -> &str {
        &self.name
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
        // Config-defined agents always run (even with no rules — the prompt itself is the instruction)
        if diffs.is_empty() {
            return Ok(vec![]);
        }
        execute_agent(self.name(), &self.system_prompt(), diffs, model, ollama, budget).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentConfig, AgentRule};
    use crate::git::FileStatus;
    use crate::git::testutil::make_file_diff;
    use crate::review::models::Severity;
    use crate::review::testutil::MockOllama;

    fn sample_agent_config() -> AgentConfig {
        AgentConfig {
            name: "PCI-DSS Compliance".into(),
            prompt: "You are a PCI-DSS compliance reviewer.\nFocus on card data security.".into(),
            languages: vec!["php".into(), "javascript".into()],
            rules: vec![
                AgentRule {
                    id: "pci-no-pan-logging".into(),
                    description: "Never log full payment card numbers".into(),
                    severity: Severity::Error,
                },
                AgentRule {
                    id: "pci-mask-output".into(),
                    description: "Mask card numbers in all output".into(),
                    severity: Severity::Warning,
                },
            ],
            enabled: true,
        }
    }

    #[test]
    fn creates_from_config() {
        let config = sample_agent_config();
        let agent = ConfigDefinedAgent::from_config(&config);
        assert_eq!(agent.name(), "PCI-DSS Compliance");
        assert_eq!(agent.rules().len(), 2);
    }

    #[test]
    fn system_prompt_includes_user_prompt() {
        let agent = ConfigDefinedAgent::from_config(&sample_agent_config());
        let prompt = agent.system_prompt();
        assert!(prompt.contains("PCI-DSS compliance reviewer"));
        assert!(prompt.contains("card data security"));
    }

    #[test]
    fn system_prompt_auto_appends_json_schema() {
        let agent = ConfigDefinedAgent::from_config(&sample_agent_config());
        let prompt = agent.system_prompt();
        assert!(prompt.contains("file_path"));
        assert!(prompt.contains("line_number"));
        assert!(prompt.contains("severity"));
        assert!(prompt.contains("JSON array"));
    }

    #[test]
    fn system_prompt_includes_rules() {
        let agent = ConfigDefinedAgent::from_config(&sample_agent_config());
        let prompt = agent.system_prompt();
        assert!(prompt.contains("pci-no-pan-logging"));
        assert!(prompt.contains("pci-mask-output"));
        assert!(prompt.contains("Never log full payment card numbers"));
    }

    #[test]
    fn agent_with_no_rules_still_has_prompt() {
        let config = AgentConfig {
            name: "General Review".into(),
            prompt: "Review this code for quality.".into(),
            languages: vec![],
            rules: vec![],
            enabled: true,
        };
        let agent = ConfigDefinedAgent::from_config(&config);
        assert!(agent.rules().is_empty());
        let prompt = agent.system_prompt();
        assert!(prompt.contains("Review this code for quality"));
        assert!(prompt.contains("JSON array")); // schema still appended
        assert!(!prompt.contains("Rules to enforce")); // no rules section
    }

    #[tokio::test]
    async fn returns_findings_from_llm() {
        let agent = ConfigDefinedAgent::from_config(&sample_agent_config());
        let ollama = MockOllama::with_response(
            r#"[{"file_path":"payment.php","line_number":42,"severity":"error","category":"security","title":"PAN logged","description":"Full card number in log","suggestion":"Mask it","evidence":"error_log($card_number);"}]"#,
        );
        let diffs = vec![make_file_diff(
            "payment.php",
            FileStatus::Modified,
            "+error_log($card_number);",
        )];
        let findings = agent.review(&diffs, "test", &ollama, crate::review::chunking::ContextBudget::unlimited()).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].title, "PAN logged");
    }

    #[tokio::test]
    async fn empty_diffs_returns_empty() {
        let agent = ConfigDefinedAgent::from_config(&sample_agent_config());
        let ollama = MockOllama::with_response("should not be called");
        let findings = agent.review(&[], "test", &ollama, crate::review::chunking::ContextBudget::unlimited()).await.unwrap();
        assert!(findings.is_empty());
        assert_eq!(ollama.call_count(), 0);
    }

    #[tokio::test]
    async fn prompt_sent_to_llm_contains_json_schema() {
        let agent = ConfigDefinedAgent::from_config(&sample_agent_config());
        let ollama = MockOllama::with_response("[]");
        let diffs = vec![make_file_diff("app.php", FileStatus::Modified, "+echo 1;")];
        agent.review(&diffs, "test", &ollama, crate::review::chunking::ContextBudget::unlimited()).await.unwrap();

        // Verify the system prompt sent to the LLM contains JSON schema
        assert!(ollama.system_prompt_contains("file_path"));
        assert!(ollama.system_prompt_contains("JSON array"));
        // And the user's prompt text
        assert!(ollama.system_prompt_contains("PCI-DSS"));
    }
}
