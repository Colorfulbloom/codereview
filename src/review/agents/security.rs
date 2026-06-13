//! Security-focused review agent.

use async_trait::async_trait;

use super::{AgentError, ContextBudget, JSON_SCHEMA, ReviewAgent, execute_agent, format_rules};
use crate::git::FileDiff;
use crate::language::rules::Rule;
use crate::onboarding::steps::OllamaClient;
use crate::review::models::ReviewFinding;

pub const SECURITY_SUFFIXES: &[&str] = &[
    "sql-injection",
    "no-eval",
    "no-hardcoded-secrets",
    "xss-prevention",
];

pub struct SecurityAgent {
    rules: Vec<Rule>,
}

impl SecurityAgent {
    pub fn new(all_rules: &[Rule]) -> Self {
        let rules = all_rules
            .iter()
            .filter(|r| SECURITY_SUFFIXES.iter().any(|s| r.id.ends_with(s)))
            .cloned()
            .collect();
        Self { rules }
    }

    fn system_prompt(&self) -> String {
        format!(
            "You are a security-focused code reviewer. Your ONLY job is to find security vulnerabilities.\n\n\
            Focus areas:\n\
            - SQL injection (string concatenation in queries)\n\
            - Cross-site scripting (XSS) via unsanitized DOM insertion\n\
            - Hardcoded secrets (API keys, passwords, tokens in source)\n\
            - Code injection via eval() or similar dynamic execution\n\n\
            {JSON_SCHEMA}\n\n\
            ## Security rules to enforce\n\n\
            {rules}\n\n\
            IMPORTANT: Only report SECURITY issues. Ignore style, performance, and other concerns.\n\
            Output a JSON array. If no security issues found, output: []",
            rules = format_rules(&self.rules),
        )
    }
}

#[async_trait]
impl ReviewAgent for SecurityAgent {
    fn name(&self) -> &str {
        "Security"
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
    use crate::git::FileStatus;
    use crate::git::testutil::make_file_diff;
    use crate::language::Language;
    use crate::language::rules::builtin_rules;
    use crate::review::models::Category;
    use crate::review::testutil::MockOllama;

    #[test]
    fn filters_only_security_rules() {
        let all = builtin_rules(Language::Php);
        let agent = SecurityAgent::new(&all);
        let ids: Vec<&str> = agent.rules().iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"php-sql-injection"));
        assert!(ids.contains(&"php-no-eval"));
        assert!(ids.contains(&"php-no-hardcoded-secrets"));
        assert!(!ids.contains(&"php-psr12-style"));
        assert!(!ids.contains(&"php-error-handling"));
    }

    #[test]
    fn system_prompt_focuses_on_security() {
        let all = builtin_rules(Language::Php);
        let agent = SecurityAgent::new(&all);
        let prompt = agent.system_prompt();
        assert!(prompt.contains("security vulnerabilities"));
        assert!(prompt.contains("SQL injection"));
        assert!(!prompt.contains("PSR-12"));
    }

    #[tokio::test]
    async fn returns_findings_from_llm() {
        let all = builtin_rules(Language::Php);
        let agent = SecurityAgent::new(&all);
        let ollama = MockOllama::with_response(
            r#"[{"file_path":"a.php","line_number":5,"severity":"error","category":"security","title":"SQL injection","description":"Raw query","suggestion":"Use prepared stmt","evidence":"$db->query($sql);"}]"#,
        );
        let diffs = vec![make_file_diff(
            "a.php",
            FileStatus::Modified,
            "+$db->query($sql);",
        )];
        let findings = agent.review(&diffs, "test", &ollama, crate::review::chunking::ContextBudget::unlimited()).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, Category::Security);
    }

    #[tokio::test]
    async fn empty_rules_returns_empty() {
        let agent = SecurityAgent::new(&[]);
        let ollama = MockOllama::with_response("should not be called");
        let diffs = vec![make_file_diff("a.css", FileStatus::Modified, "+body {}")];
        let findings = agent.review(&diffs, "test", &ollama, crate::review::chunking::ContextBudget::unlimited()).await.unwrap();
        assert!(findings.is_empty());
        assert_eq!(ollama.call_count(), 0);
    }

    #[tokio::test]
    async fn empty_diffs_returns_empty() {
        let all = builtin_rules(Language::Php);
        let agent = SecurityAgent::new(&all);
        let ollama = MockOllama::with_response("should not be called");
        let findings = agent.review(&[], "test", &ollama, crate::review::chunking::ContextBudget::unlimited()).await.unwrap();
        assert!(findings.is_empty());
        assert_eq!(ollama.call_count(), 0);
    }

    #[test]
    fn includes_js_xss_rule() {
        let all = builtin_rules(Language::JavaScript);
        let agent = SecurityAgent::new(&all);
        assert!(agent.rules().iter().any(|r| r.id == "js-xss-prevention"));
    }
}
