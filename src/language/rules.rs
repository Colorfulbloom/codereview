//! Built-in review rules per language.
//!
//! These are prompt instructions telling the LLM what to check for,
//! not static analysis rules. They're compiled into the binary.

use serde::{Deserialize, Serialize};

use super::Language;
use crate::review::models::Severity;

/// A review rule — an instruction for the LLM about what to check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub language: Language,
    pub severity: Severity,
    pub description: String,
    /// Whether this rule is enabled by default.
    pub enabled: bool,
}

/// Get all built-in rules for a language.
pub fn builtin_rules(lang: Language) -> Vec<Rule> {
    match lang {
        Language::Php => php_rules(),
        Language::Drupal => drupal_rules(),
        Language::JavaScript => javascript_rules(),
        Language::Css => css_rules(),
        Language::Html => html_rules(),
        Language::Yaml => yaml_rules(),
    }
}

/// Get all built-in rules across all languages.
pub fn all_builtin_rules() -> Vec<Rule> {
    let mut rules = Vec::new();
    for lang in &[
        Language::Php,
        Language::Drupal,
        Language::JavaScript,
        Language::Css,
        Language::Html,
        Language::Yaml,
    ] {
        rules.extend(builtin_rules(*lang));
    }
    rules
}

fn php_rules() -> Vec<Rule> {
    vec![
        Rule {
            id: "php-psr12-style".into(),
            language: Language::Php,
            severity: Severity::Warning,
            description: "Follow PSR-12 coding style: 4-space indentation, brace placement, line length (soft 80, hard 120 chars)".into(),
            enabled: true,
        },
        Rule {
            id: "php-type-declarations".into(),
            language: Language::Php,
            severity: Severity::Warning,
            description: "Use type declarations for function parameters and return types".into(),
            enabled: true,
        },
        Rule {
            id: "php-error-handling".into(),
            language: Language::Php,
            severity: Severity::Error,
            description: "Use proper error handling — no empty catch blocks, no silenced errors (@)".into(),
            enabled: true,
        },
        Rule {
            id: "php-sql-injection".into(),
            language: Language::Php,
            severity: Severity::Error,
            description: "No raw SQL concatenation — use parameterized queries or prepared statements".into(),
            enabled: true,
        },
        Rule {
            id: "php-no-eval".into(),
            language: Language::Php,
            severity: Severity::Error,
            description: "Never use eval() — it enables arbitrary code execution".into(),
            enabled: true,
        },
        Rule {
            id: "php-no-hardcoded-secrets".into(),
            language: Language::Php,
            severity: Severity::Error,
            description: "No hardcoded passwords, API keys, or secrets — use environment variables".into(),
            enabled: true,
        },
    ]
}

fn drupal_rules() -> Vec<Rule> {
    let mut rules = php_rules();
    // Change language to Drupal for inherited PHP rules
    for rule in &mut rules {
        rule.language = Language::Drupal;
        rule.id = rule.id.replace("php-", "drupal-");
    }

    rules.extend(vec![
        Rule {
            id: "drupal-dependency-injection".into(),
            language: Language::Drupal,
            severity: Severity::Error,
            description: "Use dependency injection via constructors — never use \\Drupal::service() static calls".into(),
            enabled: true,
        },
        Rule {
            id: "drupal-hook-attributes".into(),
            language: Language::Drupal,
            severity: Severity::Warning,
            description: "Use #[Hook] attribute-based hooks (Drupal 11+) instead of procedural hooks in .module files".into(),
            enabled: true,
        },
        Rule {
            id: "drupal-coding-standards".into(),
            language: Language::Drupal,
            severity: Severity::Warning,
            description: "Follow Drupal coding standards — US English in comments, proper docblocks, namespace conventions".into(),
            enabled: true,
        },
        Rule {
            id: "drupal-no-direct-db".into(),
            language: Language::Drupal,
            severity: Severity::Warning,
            description: "Use Entity API and services instead of direct database queries where possible".into(),
            enabled: true,
        },
    ]);

    rules
}

fn javascript_rules() -> Vec<Rule> {
    vec![
        Rule {
            id: "js-no-var".into(),
            language: Language::JavaScript,
            severity: Severity::Error,
            description: "Use const or let instead of var".into(),
            enabled: true,
        },
        Rule {
            id: "js-strict-equality".into(),
            language: Language::JavaScript,
            severity: Severity::Warning,
            description: "Use === and !== instead of == and !=".into(),
            enabled: true,
        },
        Rule {
            id: "js-no-unused-vars".into(),
            language: Language::JavaScript,
            severity: Severity::Warning,
            description: "Remove unused variables and imports".into(),
            enabled: true,
        },
        Rule {
            id: "js-error-handling".into(),
            language: Language::JavaScript,
            severity: Severity::Error,
            description:
                "Handle errors in async/await with try-catch, no unhandled promise rejections"
                    .into(),
            enabled: true,
        },
        Rule {
            id: "js-no-console-log".into(),
            language: Language::JavaScript,
            severity: Severity::Warning,
            description: "Remove console.log() from production code".into(),
            enabled: true,
        },
        Rule {
            id: "js-xss-prevention".into(),
            language: Language::JavaScript,
            severity: Severity::Error,
            description:
                "Sanitize user input before rendering in DOM — avoid innerHTML with untrusted data"
                    .into(),
            enabled: true,
        },
    ]
}

fn css_rules() -> Vec<Rule> {
    vec![
        Rule {
            id: "css-no-important".into(),
            language: Language::Css,
            severity: Severity::Warning,
            description: "Avoid !important — refactor selector specificity instead".into(),
            enabled: true,
        },
        Rule {
            id: "css-no-duplicate-selectors".into(),
            language: Language::Css,
            severity: Severity::Warning,
            description: "No duplicate selectors in the same stylesheet".into(),
            enabled: true,
        },
        Rule {
            id: "css-max-nesting".into(),
            language: Language::Css,
            severity: Severity::Info,
            description: "Limit nesting depth to 3 levels for maintainability".into(),
            enabled: true,
        },
        Rule {
            id: "css-color-format".into(),
            language: Language::Css,
            severity: Severity::Info,
            description: "Use consistent color format — prefer hex or CSS custom properties over named colors".into(),
            enabled: true,
        },
    ]
}

fn html_rules() -> Vec<Rule> {
    vec![
        Rule {
            id: "html-alt-text".into(),
            language: Language::Html,
            severity: Severity::Error,
            description: "All <img> elements must have descriptive alt text (WCAG 2.2)".into(),
            enabled: true,
        },
        Rule {
            id: "html-semantic-elements".into(),
            language: Language::Html,
            severity: Severity::Warning,
            description: "Use semantic HTML elements (<header>, <nav>, <main>, <footer>, <article>, <section>)".into(),
            enabled: true,
        },
        Rule {
            id: "html-heading-hierarchy".into(),
            language: Language::Html,
            severity: Severity::Warning,
            description: "Heading levels must be properly nested (h1 → h2 → h3, no skipped levels)".into(),
            enabled: true,
        },
        Rule {
            id: "html-form-labels".into(),
            language: Language::Html,
            severity: Severity::Error,
            description: "All form inputs must have associated <label> elements".into(),
            enabled: true,
        },
        Rule {
            id: "html-link-text".into(),
            language: Language::Html,
            severity: Severity::Warning,
            description: "Links must have descriptive text — avoid 'click here' or 'read more' without context".into(),
            enabled: true,
        },
        Rule {
            id: "html-contrast".into(),
            language: Language::Html,
            severity: Severity::Warning,
            description: "Ensure text has sufficient color contrast (4.5:1 minimum per WCAG 2.2 AA)".into(),
            enabled: true,
        },
        // Twig template rules (Twig files are detected as HTML)
        Rule {
            id: "twig-no-raw".into(),
            language: Language::Html,
            severity: Severity::Error,
            description: "Do not use the {{ raw }} filter — it disables autoescaping and creates XSS vulnerabilities".into(),
            enabled: true,
        },
        Rule {
            id: "twig-autoescape".into(),
            language: Language::Html,
            severity: Severity::Warning,
            description: "Ensure variables in Twig templates are autoescaped. Do not wrap output in {% autoescape false %}".into(),
            enabled: true,
        },
        Rule {
            id: "twig-trans".into(),
            language: Language::Html,
            severity: Severity::Warning,
            description: "Use {% trans %} or |t filter for user-facing strings in Twig templates to support translations".into(),
            enabled: true,
        },
        Rule {
            id: "twig-no-php".into(),
            language: Language::Html,
            severity: Severity::Error,
            description: "Never embed PHP code in Twig templates — use Twig syntax, filters, and functions instead".into(),
            enabled: true,
        },
        Rule {
            id: "twig-attach-library".into(),
            language: Language::Html,
            severity: Severity::Warning,
            description: "Use {{ attach_library('module/library') }} to include CSS/JS — do not use inline <style> or <script> tags in Twig".into(),
            enabled: true,
        },
        Rule {
            id: "twig-undefined-vars".into(),
            language: Language::Html,
            severity: Severity::Error,
            description: "Flag Twig variables that appear undefined, misspelled, or not passed from the parent template/preprocess function. Variables like {{ afsadasdf }} that are not standard Drupal variables should be reported.".into(),
            enabled: true,
        },
    ]
}

fn yaml_rules() -> Vec<Rule> {
    vec![
        Rule {
            id: "yaml-valid-syntax".into(),
            language: Language::Yaml,
            severity: Severity::Error,
            description: "YAML must have valid syntax — check for incorrect indentation, missing colons, tab characters, and malformed values".into(),
            enabled: true,
        },
        Rule {
            id: "yaml-consistent-indent".into(),
            language: Language::Yaml,
            severity: Severity::Warning,
            description: "Use consistent indentation (2 spaces preferred). Never use tabs in YAML files".into(),
            enabled: true,
        },
        Rule {
            id: "yaml-no-duplicate-keys".into(),
            language: Language::Yaml,
            severity: Severity::Error,
            description: "No duplicate keys at the same level — duplicate keys silently override earlier values".into(),
            enabled: true,
        },
        Rule {
            id: "yaml-quote-special-values".into(),
            language: Language::Yaml,
            severity: Severity::Warning,
            description: "Quote strings that look like booleans (yes/no/true/false), nulls, or numbers when a string is intended".into(),
            enabled: true,
        },
        Rule {
            id: "yaml-no-hardcoded-secrets".into(),
            language: Language::Yaml,
            severity: Severity::Error,
            description: "No hardcoded passwords, API keys, tokens, or database credentials in YAML config files".into(),
            enabled: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_rules_not_empty() {
        let rules = builtin_rules(Language::Php);
        assert!(!rules.is_empty());
        assert!(rules.iter().all(|r| r.language == Language::Php));
    }

    #[test]
    fn drupal_rules_include_php_rules() {
        let php = builtin_rules(Language::Php);
        let drupal = builtin_rules(Language::Drupal);
        // Drupal should have more rules than PHP (PHP + Drupal-specific)
        assert!(drupal.len() > php.len());
    }

    #[test]
    fn drupal_rules_all_have_drupal_language() {
        let rules = builtin_rules(Language::Drupal);
        assert!(rules.iter().all(|r| r.language == Language::Drupal));
    }

    #[test]
    fn drupal_rules_include_di_rule() {
        let rules = builtin_rules(Language::Drupal);
        assert!(rules.iter().any(|r| r.id == "drupal-dependency-injection"));
    }

    #[test]
    fn javascript_rules_include_no_var() {
        let rules = builtin_rules(Language::JavaScript);
        assert!(rules.iter().any(|r| r.id == "js-no-var"));
    }

    #[test]
    fn css_rules_include_no_important() {
        let rules = builtin_rules(Language::Css);
        assert!(rules.iter().any(|r| r.id == "css-no-important"));
    }

    #[test]
    fn html_rules_include_alt_text() {
        let rules = builtin_rules(Language::Html);
        assert!(rules.iter().any(|r| r.id == "html-alt-text"));
    }

    #[test]
    fn yaml_rules_not_empty() {
        let rules = builtin_rules(Language::Yaml);
        assert!(!rules.is_empty());
        assert!(rules.iter().all(|r| r.language == Language::Yaml));
    }

    #[test]
    fn yaml_rules_include_syntax_check() {
        let rules = builtin_rules(Language::Yaml);
        assert!(rules.iter().any(|r| r.id == "yaml-valid-syntax"));
        assert!(rules.iter().any(|r| r.id == "yaml-no-duplicate-keys"));
    }

    #[test]
    fn all_rules_have_unique_ids() {
        let rules = all_builtin_rules();
        let mut ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        let total = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), total, "Duplicate rule IDs found");
    }

    #[test]
    fn all_rules_have_descriptions() {
        for rule in all_builtin_rules() {
            assert!(
                !rule.description.is_empty(),
                "Rule {} has empty description",
                rule.id
            );
        }
    }

    #[test]
    fn all_rules_enabled_by_default() {
        for rule in all_builtin_rules() {
            assert!(
                rule.enabled,
                "Rule {} should be enabled by default",
                rule.id
            );
        }
    }

    #[test]
    fn rule_serializes() {
        let rule = &builtin_rules(Language::Php)[0];
        let json = serde_json::to_string(rule).unwrap();
        assert!(json.contains("php-psr12"));

        let restored: Rule = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, rule.id);
    }

    #[test]
    fn html_rules_include_twig() {
        let rules = builtin_rules(Language::Html);
        assert!(rules.iter().any(|r| r.id == "twig-no-raw"));
        assert!(rules.iter().any(|r| r.id == "twig-trans"));
        assert!(rules.iter().any(|r| r.id == "twig-no-php"));
        assert!(rules.iter().any(|r| r.id == "twig-autoescape"));
        assert!(rules.iter().any(|r| r.id == "twig-undefined-vars"));
    }

    #[test]
    fn twig_rules_have_html_language() {
        let rules = builtin_rules(Language::Html);
        let twig_rules: Vec<&Rule> = rules.iter().filter(|r| r.id.starts_with("twig-")).collect();
        assert!(!twig_rules.is_empty());
        assert!(twig_rules.iter().all(|r| r.language == Language::Html));
    }

    #[test]
    fn all_builtin_rules_covers_all_languages() {
        let rules = all_builtin_rules();
        let languages: std::collections::BTreeSet<Language> =
            rules.iter().map(|r| r.language).collect();

        assert!(languages.contains(&Language::Php));
        assert!(languages.contains(&Language::Drupal));
        assert!(languages.contains(&Language::JavaScript));
        assert!(languages.contains(&Language::Css));
        assert!(languages.contains(&Language::Html));
        assert!(languages.contains(&Language::Yaml));
    }
}
