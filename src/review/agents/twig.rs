//! Twig template review agent.
//!
//! Specialized agent for Drupal Twig templates. Runs only when .twig files
//! are in the diff. Has deep knowledge of Twig syntax, Drupal conventions,
//! and common template mistakes.

use async_trait::async_trait;

use super::{AgentError, ContextBudget, JSON_SCHEMA, ReviewAgent, execute_agent, format_rules};
use crate::git::FileDiff;
use crate::language::rules::Rule;
use crate::onboarding::steps::OllamaClient;
use crate::review::models::ReviewFinding;

const TWIG_RULE_PREFIX: &str = "twig-";

pub struct TwigAgent {
    rules: Vec<Rule>,
}

impl TwigAgent {
    pub fn new(all_rules: &[Rule]) -> Self {
        let rules = all_rules
            .iter()
            .filter(|r| r.id.starts_with(TWIG_RULE_PREFIX))
            .cloned()
            .collect();
        Self { rules }
    }

    /// Check if any diffs contain .twig files.
    pub fn has_twig_files(diffs: &[FileDiff]) -> bool {
        diffs.iter().any(|d| d.path.ends_with(".twig"))
    }

    /// Filter diffs to only .twig files.
    pub fn filter_twig_diffs(diffs: &[FileDiff]) -> Vec<FileDiff> {
        diffs
            .iter()
            .filter(|d| d.path.ends_with(".twig"))
            .cloned()
            .collect()
    }

    fn system_prompt(&self) -> String {
        format!(
            "You are an expert Drupal Twig template reviewer. Your ONLY job is to find issues in Twig template files.\n\n\
            You have deep expertise in:\n\
            - Twig syntax (variables, filters, blocks, includes, extends, macros)\n\
            - Drupal Twig conventions (preprocess variables, render arrays, attach_library)\n\
            - Twig security (autoescaping, raw filter abuse, XSS via unescaped output)\n\
            - Common Twig mistakes (undefined variables, wrong filter usage, missing trans)\n\n\
            When reviewing a Twig template diff, check EVERY added or modified line for:\n\
            - Variables that look undefined, misspelled, or not provided by the component/preprocess\n\
            - Use of |raw filter without justification (XSS risk)\n\
            - Hardcoded strings that should use {{% trans %}} for translation\n\
            - Inline <style> or <script> tags instead of attach_library()\n\
            - PHP code embedded in templates\n\
            - Incorrect Twig syntax that will cause render errors\n\
            - Missing or malformed Twig comments\n\
            - Accessibility issues in the HTML output (missing alt, form labels)\n\n\
            {JSON_SCHEMA}\n\n\
            ## Twig review rules\n\n\
            {rules}\n\n\
            Be thorough. Check every changed line. Report real issues with specific line numbers.\n\
            Output a JSON array. If no issues found, output: []",
            rules = format_rules(&self.rules),
        )
    }
}

#[async_trait]
impl ReviewAgent for TwigAgent {
    fn name(&self) -> &str {
        "Twig Templates"
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
        if self.rules.is_empty() || diffs.is_empty() {
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
    use crate::review::testutil::MockOllama;

    #[test]
    fn filters_only_twig_rules() {
        let all = builtin_rules(Language::Html);
        let agent = TwigAgent::new(&all);
        assert!(!agent.rules().is_empty());
        assert!(agent.rules().iter().all(|r| r.id.starts_with("twig-")));
    }

    #[test]
    fn includes_all_twig_rules() {
        let all = builtin_rules(Language::Html);
        let agent = TwigAgent::new(&all);
        let ids: Vec<&str> = agent.rules().iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"twig-no-raw"));
        assert!(ids.contains(&"twig-autoescape"));
        assert!(ids.contains(&"twig-trans"));
        assert!(ids.contains(&"twig-no-php"));
        assert!(ids.contains(&"twig-attach-library"));
        assert!(ids.contains(&"twig-undefined-vars"));
    }

    #[test]
    fn no_non_twig_rules() {
        let all = builtin_rules(Language::Html);
        let agent = TwigAgent::new(&all);
        assert!(!agent.rules().iter().any(|r| r.id.starts_with("html-")));
    }

    #[test]
    fn no_twig_rules_from_php() {
        let all = builtin_rules(Language::Php);
        let agent = TwigAgent::new(&all);
        assert!(agent.rules().is_empty());
    }

    #[test]
    fn system_prompt_has_twig_expertise() {
        let all = builtin_rules(Language::Html);
        let agent = TwigAgent::new(&all);
        let prompt = agent.system_prompt();
        assert!(prompt.contains("Twig template"));
        assert!(prompt.contains("undefined variables"));
        assert!(prompt.contains("raw filter"));
        assert!(prompt.contains("trans"));
        assert!(prompt.contains("attach_library"));
    }

    #[test]
    fn has_twig_files_detects_twig() {
        let diffs = vec![
            make_file_diff("app.php", FileStatus::Modified, "+echo 1;"),
            make_file_diff("node.html.twig", FileStatus::Modified, "+{{ var }}"),
        ];
        assert!(TwigAgent::has_twig_files(&diffs));
    }

    #[test]
    fn has_twig_files_false_when_no_twig() {
        let diffs = vec![
            make_file_diff("app.php", FileStatus::Modified, "+echo 1;"),
            make_file_diff("style.css", FileStatus::Modified, "+body {}"),
        ];
        assert!(!TwigAgent::has_twig_files(&diffs));
    }

    #[test]
    fn filter_twig_diffs_only_twig() {
        let diffs = vec![
            make_file_diff("app.php", FileStatus::Modified, "+echo 1;"),
            make_file_diff("node.html.twig", FileStatus::Modified, "+{{ var }}"),
            make_file_diff("style.css", FileStatus::Modified, "+body {}"),
        ];
        let twig_only = TwigAgent::filter_twig_diffs(&diffs);
        assert_eq!(twig_only.len(), 1);
        assert_eq!(twig_only[0].path, "node.html.twig");
    }

    #[tokio::test]
    async fn returns_findings_from_twig_review() {
        let all = builtin_rules(Language::Html);
        let agent = TwigAgent::new(&all);
        let ollama = MockOllama::with_response(
            r#"[{"file_path":"node.html.twig","line_number":5,"severity":"error","category":"bug","title":"Undefined variable","description":"{{ afsadasdf }} is not a known Twig variable","suggestion":"Remove or replace with a valid variable","evidence":"{{ afsadasdf }}"}]"#,
        );
        let diffs = vec![make_file_diff(
            "node.html.twig",
            FileStatus::Modified,
            "+{{ afsadasdf }}",
        )];
        let findings = agent.review(&diffs, "test", &ollama, crate::review::chunking::ContextBudget::unlimited()).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].title, "Undefined variable");
    }

    #[tokio::test]
    async fn prompt_sent_mentions_twig() {
        let all = builtin_rules(Language::Html);
        let agent = TwigAgent::new(&all);
        let ollama = MockOllama::with_response("[]");
        let diffs = vec![make_file_diff(
            "page.html.twig",
            FileStatus::Modified,
            "+{{ content }}",
        )];
        agent.review(&diffs, "test", &ollama, crate::review::chunking::ContextBudget::unlimited()).await.unwrap();
        assert!(ollama.system_prompt_contains("Twig template"));
        assert!(ollama.system_prompt_contains("undefined variables"));
    }

    #[tokio::test]
    async fn empty_diffs_skips() {
        let all = builtin_rules(Language::Html);
        let agent = TwigAgent::new(&all);
        let ollama = MockOllama::with_response("should not be called");
        let findings = agent.review(&[], "test", &ollama, crate::review::chunking::ContextBudget::unlimited()).await.unwrap();
        assert!(findings.is_empty());
        assert_eq!(ollama.call_count(), 0);
    }
}
