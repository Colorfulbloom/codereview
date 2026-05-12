//! Language-specific style and best-practice review agent.

use async_trait::async_trait;

use super::accessibility::A11Y_IDS;
use super::bugs::BUG_SUFFIXES;
use super::security::SECURITY_SUFFIXES;
use super::{AgentError, JSON_SCHEMA, ReviewAgent, execute_agent, format_rules};
use crate::git::FileDiff;
use crate::language::Language;
use crate::language::rules::Rule;
use crate::onboarding::steps::OllamaClient;
use crate::review::models::ReviewFinding;

pub struct LanguageStyleAgent {
    language: Language,
    rules: Vec<Rule>,
    display_name: String,
}

impl LanguageStyleAgent {
    pub fn new(language: Language, all_rules: &[Rule]) -> Self {
        let rules = all_rules
            .iter()
            .filter(|r| r.language == language)
            .filter(|r| !SECURITY_SUFFIXES.iter().any(|s| r.id.ends_with(s)))
            .filter(|r| !BUG_SUFFIXES.iter().any(|s| r.id.ends_with(s)))
            .filter(|r| !A11Y_IDS.contains(&r.id.as_str()))
            .cloned()
            .collect();

        let display_name = format!("Style ({language})");
        Self {
            language,
            rules,
            display_name,
        }
    }

    pub(crate) fn language_context(&self) -> &'static str {
        match self.language {
            Language::Php => "You are an expert in PSR-12 and modern PHP best practices.",
            Language::Drupal => {
                "You are an expert in Drupal 10/11 coding standards, dependency injection, and the Hook attribute system."
            }
            Language::JavaScript => {
                "You are an expert in modern ES6+ JavaScript and TypeScript best practices."
            }
            Language::Css => {
                "You are an expert in CSS architecture, specificity management, and maintainable stylesheets."
            }
            Language::Html => {
                "You are an expert in semantic HTML and Twig template best practices. You review both HTML structure and Twig template syntax including variable usage, filters, blocks, and Drupal-specific Twig conventions."
            }
            Language::Yaml => {
                "You are an expert in YAML syntax and configuration files. You review YAML for valid structure, proper indentation, correct key-value formatting, and Drupal-specific YAML conventions (services.yml, routing.yml, libraries.yml, config schemas)."
            }
        }
    }

    fn system_prompt(&self) -> String {
        format!(
            "You are a {lang} coding standards reviewer. Your job is to find style, best-practice, and correctness violations specific to {lang}.\n\n\
            {context}\n\n\
            Look carefully at every added or modified line in the diff. Flag any issues including:\n\
            - Violations of the rules listed below\n\
            - Undefined or suspicious variables\n\
            - Incorrect syntax or misused functions\n\
            - Code that will cause runtime errors\n\n\
            {JSON_SCHEMA}\n\n\
            ## {lang} style and correctness rules\n\n\
            {rules}\n\n\
            Review every changed line. If you find issues, report them. If the code is clean, output: []",
            lang = self.language,
            context = self.language_context(),
            rules = format_rules(&self.rules),
        )
    }
}

#[async_trait]
impl ReviewAgent for LanguageStyleAgent {
    fn name(&self) -> &str {
        &self.display_name
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
    use crate::language::rules::builtin_rules;

    #[test]
    fn filters_style_rules_only_for_php() {
        let all = builtin_rules(Language::Php);
        let agent = LanguageStyleAgent::new(Language::Php, &all);
        let ids: Vec<&str> = agent.rules().iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"php-psr12-style"));
        // Security and bug rules excluded
        assert!(!ids.contains(&"php-sql-injection"));
        assert!(!ids.contains(&"php-error-handling"));
        assert!(!ids.contains(&"php-type-declarations"));
    }

    #[test]
    fn filters_style_rules_for_drupal() {
        let all = builtin_rules(Language::Drupal);
        let agent = LanguageStyleAgent::new(Language::Drupal, &all);
        let ids: Vec<&str> = agent.rules().iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"drupal-dependency-injection"));
        assert!(ids.contains(&"drupal-hook-attributes"));
        assert!(ids.contains(&"drupal-coding-standards"));
        // Security/bug excluded
        assert!(!ids.contains(&"drupal-sql-injection"));
        assert!(!ids.contains(&"drupal-error-handling"));
    }

    #[test]
    fn filters_style_rules_for_javascript() {
        let all = builtin_rules(Language::JavaScript);
        let agent = LanguageStyleAgent::new(Language::JavaScript, &all);
        let ids: Vec<&str> = agent.rules().iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"js-no-var"));
        assert!(ids.contains(&"js-strict-equality"));
        assert!(ids.contains(&"js-no-console-log"));
        // Bug/security excluded
        assert!(!ids.contains(&"js-error-handling"));
        assert!(!ids.contains(&"js-xss-prevention"));
    }

    #[test]
    fn filters_style_rules_for_css() {
        let all = builtin_rules(Language::Css);
        let agent = LanguageStyleAgent::new(Language::Css, &all);
        assert!(!agent.rules().is_empty());
        // All CSS rules are style rules (no security/bug)
        assert_eq!(agent.rules().len(), all.len());
    }

    #[test]
    fn excludes_html_a11y_rules() {
        let all = builtin_rules(Language::Html);
        let agent = LanguageStyleAgent::new(Language::Html, &all);
        let ids: Vec<&str> = agent.rules().iter().map(|r| r.id.as_str()).collect();
        assert!(!ids.contains(&"html-alt-text"));
        assert!(!ids.contains(&"html-form-labels"));
    }

    #[test]
    fn display_name_includes_language() {
        let agent = LanguageStyleAgent::new(Language::Php, &builtin_rules(Language::Php));
        assert_eq!(agent.name(), "Style (PHP)");
    }

    #[test]
    fn system_prompt_has_language_context() {
        let agent = LanguageStyleAgent::new(Language::Drupal, &builtin_rules(Language::Drupal));
        let prompt = agent.system_prompt();
        assert!(prompt.contains("Drupal 10/11"));
        assert!(prompt.contains("dependency injection"));
    }

    #[test]
    fn ignores_rules_from_other_languages() {
        let mut all = builtin_rules(Language::Php);
        all.extend(builtin_rules(Language::JavaScript));
        let agent = LanguageStyleAgent::new(Language::Php, &all);
        assert!(agent.rules().iter().all(|r| r.language == Language::Php));
    }

    #[test]
    fn html_prompt_mentions_twig_expertise() {
        let agent = LanguageStyleAgent::new(Language::Html, &builtin_rules(Language::Html));
        let context = agent.language_context();
        assert!(
            context.contains("Twig"),
            "HTML language context should mention Twig: got '{context}'"
        );
    }

    #[test]
    fn html_prompt_includes_twig_rules() {
        let agent = LanguageStyleAgent::new(Language::Html, &builtin_rules(Language::Html));
        let prompt = agent.system_prompt();
        assert!(prompt.contains("twig-no-raw"));
        assert!(prompt.contains("twig-trans"));
    }

    #[test]
    fn html_style_agent_includes_twig_rules() {
        let all = builtin_rules(Language::Html);
        let agent = LanguageStyleAgent::new(Language::Html, &all);
        let ids: Vec<&str> = agent.rules().iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"twig-no-raw"));
        assert!(ids.contains(&"twig-autoescape"));
        assert!(ids.contains(&"twig-trans"));
        assert!(ids.contains(&"twig-no-php"));
        assert!(ids.contains(&"twig-attach-library"));
    }
}
