//! Configuration loading and merging.
//!
//! Loads `.codereview.yaml` from the repo root, merges with built-in defaults.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::language::Language;
use crate::language::rules::{Rule, builtin_rules};
use crate::review::models::Severity;

/// Top-level configuration from `.codereview.yaml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Override the default Ollama model.
    #[serde(default)]
    pub model: Option<String>,

    /// Default output format.
    #[serde(default)]
    pub output_format: Option<String>,

    /// Languages to review (auto-detected if omitted).
    #[serde(default)]
    pub languages: Option<Vec<String>>,

    /// Rule overrides per language.
    #[serde(default)]
    pub rules: HashMap<String, HashMap<String, RuleOverride>>,

    /// Custom rules.
    #[serde(default)]
    pub custom_rules: Vec<CustomRule>,

    /// Files and directories to exclude from review.
    /// Supports exact paths, glob patterns (*.log), and directory prefixes (vendor/).
    #[serde(default)]
    pub exclude: Vec<String>,

    /// Custom review agents with specialized prompts.
    #[serde(default)]
    pub agents: Vec<AgentConfig>,

    /// Maximum context window (in tokens) to request from the model. The tool
    /// auto-detects the model's architectural maximum and uses the smaller of
    /// the two. Set this to cap memory use (a large `num_ctx` grows the KV
    /// cache) or to override detection. Defaults to 32768 when unset.
    #[serde(default)]
    pub max_context_tokens: Option<usize>,

    /// Timeout in seconds for each LLM request. Raise this on slow hardware
    /// where a single review call legitimately needs more time. `0` disables
    /// the timeout entirely. Defaults to 300 when unset.
    #[serde(default)]
    pub llm_timeout_seconds: Option<u64>,

    /// PHP_CodeSniffer (Drupal coding standards) as a deterministic finding
    /// source for the rule-based Drupal/PHP checks.
    #[serde(default)]
    pub phpcs: PhpcsConfig,
}

/// Configuration for the phpcs (Drupal coding standards) finding source.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PhpcsConfig {
    /// `None` (absent) = auto: run phpcs when it's installed. `Some(false)`
    /// disables it; `Some(true)` forces it on (still only runs if installed).
    #[serde(default)]
    pub enabled: Option<bool>,

    /// phpcs standard(s). Defaults to `Drupal,DrupalPractice`.
    #[serde(default)]
    pub standard: Option<String>,

    /// How to invoke phpcs. A space-separated command — e.g. `vendor/bin/phpcs`,
    /// or a container wrapper like `ddev exec phpcs` / `lando phpcs` /
    /// `docker compose exec -T php phpcs` when PHP lives in a container. When
    /// unset, phpcs is located at `vendor/bin/phpcs` or on `PATH`.
    #[serde(default)]
    pub command: Option<String>,
}

/// Override for a built-in rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleOverride {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub severity: Option<Severity>,
}

/// A custom rule defined by the team.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomRule {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default = "default_severity")]
    pub severity: Severity,
}

/// A custom review agent defined in .codereview.yaml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Display name (e.g., "PCI-DSS Compliance").
    pub name: String,

    /// System prompt text. JSON output schema is appended automatically.
    pub prompt: String,

    /// Languages this agent applies to. Empty = all languages.
    #[serde(default)]
    pub languages: Vec<String>,

    /// Inline rules for this agent.
    #[serde(default)]
    pub rules: Vec<AgentRule>,

    /// Whether this agent is enabled. Default: true.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// An inline rule within a custom agent definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRule {
    pub id: String,
    pub description: String,
    #[serde(default = "default_severity")]
    pub severity: Severity,
}

fn default_severity() -> Severity {
    Severity::Warning
}

fn default_true() -> bool {
    true
}

impl Config {
    /// Check if a file path should be excluded from review.
    pub fn is_excluded(&self, path: &str) -> bool {
        for pattern in &self.exclude {
            // Exact match
            if path == pattern {
                return true;
            }

            // Directory prefix (pattern ends with /)
            if pattern.ends_with('/') && path.starts_with(pattern) {
                return true;
            }

            // Glob pattern with * — simple matching
            if pattern.contains('*') && Self::glob_matches(pattern, path) {
                return true;
            }
        }
        false
    }

    /// Simple glob matching supporting * and **/
    fn glob_matches(pattern: &str, path: &str) -> bool {
        // Handle */.../* patterns (match any directory segment)
        if pattern.starts_with("*/") && pattern.ends_with("/*") {
            let middle = &pattern[2..pattern.len() - 2];
            return path.contains(&format!("/{middle}/"));
        }

        // Handle *.ext patterns (match file extension)
        if pattern.starts_with("*.") {
            let ext = &pattern[1..]; // includes the dot
            return path.ends_with(ext);
        }

        // Handle prefix*.suffix patterns
        if let Some(star_pos) = pattern.find('*') {
            let prefix = &pattern[..star_pos];
            let suffix = &pattern[star_pos + 1..];
            // Check the filename (last segment) against the pattern
            let filename = path.rsplit('/').next().unwrap_or(path);
            return filename.starts_with(prefix) && filename.ends_with(suffix);
        }

        false
    }

    /// Per-request LLM timeout in seconds (configured or the 300s default).
    pub fn llm_timeout(&self) -> u64 {
        self.llm_timeout_seconds.unwrap_or(300)
    }

    /// Whether phpcs is explicitly disabled (`phpcs.enabled: false`). Absent or
    /// `true` means "run it if installed".
    pub fn phpcs_disabled(&self) -> bool {
        self.phpcs.enabled == Some(false)
    }

    /// phpcs standard(s) to run (configured or the Drupal default).
    pub fn phpcs_standard(&self) -> &str {
        self.phpcs.standard.as_deref().unwrap_or("Drupal,DrupalPractice")
    }

    /// Explicit phpcs invocation command, if configured (e.g. `ddev exec phpcs`).
    pub fn phpcs_command(&self) -> Option<&str> {
        self.phpcs.command.as_deref()
    }

    /// Load config from a YAML file, falling back to defaults on any error.
    ///
    /// A missing file is normal (no warning). An unreadable or invalid file
    /// returns the warning message the caller should show the user — a broken
    /// config silently behaving like no config has hidden real bugs before.
    pub fn load_lenient(path: &Path) -> (Self, Option<String>) {
        if !path.exists() {
            return (Self::default(), None);
        }

        match Self::load_from_file(path) {
            Ok(config) => (config, None),
            Err(e) => {
                let warning = format!(
                    "Warning: {} could not be loaded ({e}). Using default configuration.",
                    path.display()
                );
                crate::logging::warn(&warning);
                (Self::default(), Some(warning))
            }
        }
    }

    /// Load config from a YAML file path.
    pub fn load_from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::Io(path.display().to_string(), e))?;
        Self::parse(&content)
    }

    /// Parse config from a YAML string.
    pub fn parse(yaml: &str) -> Result<Self, ConfigError> {
        yaml_serde::from_str(yaml).map_err(ConfigError::Yaml)
    }

    /// Get the effective rules for a language, merging built-in with overrides.
    pub fn effective_rules(&self, lang: Language) -> Vec<Rule> {
        let lang_key = lang.to_string().to_lowercase();
        let overrides = self.rules.get(&lang_key);

        let mut rules = builtin_rules(lang);

        // Apply overrides
        if let Some(overrides) = overrides {
            for rule in &mut rules {
                if let Some(ov) = overrides.get(&rule.id) {
                    if let Some(enabled) = ov.enabled {
                        rule.enabled = enabled;
                    }
                    if let Some(severity) = ov.severity {
                        rule.severity = severity;
                    }
                }
            }
        }

        // Add custom rules for this language
        for custom in &self.custom_rules {
            if custom.languages.is_empty()
                || custom
                    .languages
                    .iter()
                    .any(|l| l.to_lowercase() == lang_key)
            {
                rules.push(Rule {
                    id: custom.id.clone(),
                    language: lang,
                    severity: custom.severity,
                    description: custom.description.clone(),
                    enabled: true,
                });
            }
        }

        // Only return enabled rules
        rules.into_iter().filter(|r| r.enabled).collect()
    }
}

/// Errors from config loading.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file {0}: {1}")]
    Io(String, std::io::Error),

    #[error("Invalid YAML: {0}")]
    Yaml(#[from] yaml_serde::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phpcs_config_defaults_and_overrides() {
        // Absent → auto (not disabled), default Drupal standard.
        let c = Config::parse("").unwrap();
        assert!(!c.phpcs_disabled());
        assert_eq!(c.phpcs_standard(), "Drupal,DrupalPractice");

        // Explicit disable + custom standard.
        let c = Config::parse("phpcs:\n  enabled: false\n  standard: Drupal\n").unwrap();
        assert!(c.phpcs_disabled());
        assert_eq!(c.phpcs_standard(), "Drupal");
        assert_eq!(c.phpcs_command(), None);

        // Container command override.
        let c = Config::parse("phpcs:\n  command: \"ddev exec phpcs\"\n").unwrap();
        assert_eq!(c.phpcs_command(), Some("ddev exec phpcs"));
    }

    #[test]
    fn llm_timeout_parsed_and_defaulted() {
        let config = Config::parse("llm_timeout_seconds: 600\n").unwrap();
        assert_eq!(config.llm_timeout_seconds, Some(600));
        assert_eq!(config.llm_timeout(), 600);

        let config = Config::parse("").unwrap();
        assert_eq!(config.llm_timeout_seconds, None);
        assert_eq!(config.llm_timeout(), 300);
    }

    #[test]
    fn legacy_init_languages_string_fails_parse() {
        // Old `init` builds wrote this uncommented prose line; it must be a
        // parse error (languages expects a list), never silently half-parse.
        let yaml = "model: qwen3.5:9b-mlx\nlanguages: auto-detected from file extensions\n";
        assert!(Config::parse(yaml).is_err());
    }

    #[test]
    fn load_lenient_missing_file_is_silent_default() {
        let dir = tempfile::TempDir::new().unwrap();
        let (config, warning) = Config::load_lenient(&dir.path().join(".codereview.yaml"));
        assert!(config.model.is_none());
        assert!(warning.is_none());
    }

    #[test]
    fn load_lenient_valid_file_is_silent() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(".codereview.yaml");
        std::fs::write(&path, "model: test-model\n").unwrap();

        let (config, warning) = Config::load_lenient(&path);
        assert_eq!(config.model.as_deref(), Some("test-model"));
        assert!(warning.is_none());
    }

    #[test]
    fn load_lenient_invalid_file_warns_and_defaults() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(".codereview.yaml");
        std::fs::write(
            &path,
            "model: m\nlanguages: auto-detected from file extensions\n",
        )
        .unwrap();

        let (config, warning) = Config::load_lenient(&path);
        // Falls back to defaults — the broken file must not half-apply.
        assert!(config.model.is_none());
        let msg = warning.expect("invalid config must produce a warning");
        assert!(msg.contains(".codereview.yaml"));
    }

    #[test]
    fn empty_config_parses() {
        let config = Config::parse("").unwrap();
        assert!(config.model.is_none());
        assert!(config.rules.is_empty());
        assert!(config.custom_rules.is_empty());
    }

    #[test]
    fn minimal_config_parses() {
        let yaml = r#"
model: qwen3-coder:30b
output_format: terminal
"#;
        let config = Config::parse(yaml).unwrap();
        assert_eq!(config.model.as_deref(), Some("qwen3-coder:30b"));
        assert_eq!(config.output_format.as_deref(), Some("terminal"));
    }

    #[test]
    fn rule_overrides_parsed() {
        let yaml = r#"
rules:
  php:
    php-psr12-style:
      enabled: false
    php-error-handling:
      severity: warning
"#;
        let config = Config::parse(yaml).unwrap();
        let php_rules = config.rules.get("php").unwrap();
        assert!(!php_rules["php-psr12-style"].enabled.unwrap());
        assert_eq!(
            php_rules["php-error-handling"].severity,
            Some(Severity::Warning)
        );
    }

    #[test]
    fn effective_rules_without_overrides() {
        let config = Config::default();
        let rules = config.effective_rules(Language::Php);
        assert!(!rules.is_empty());
        // All built-in rules should be present and enabled
        assert!(rules.iter().all(|r| r.enabled));
    }

    #[test]
    fn effective_rules_with_disabled_rule() {
        let yaml = r#"
rules:
  php:
    php-psr12-style:
      enabled: false
"#;
        let config = Config::parse(yaml).unwrap();
        let rules = config.effective_rules(Language::Php);
        // psr12 should be filtered out
        assert!(!rules.iter().any(|r| r.id == "php-psr12-style"));
    }

    #[test]
    fn effective_rules_with_severity_override() {
        let yaml = r#"
rules:
  javascript:
    js-no-var:
      severity: info
"#;
        let config = Config::parse(yaml).unwrap();
        let rules = config.effective_rules(Language::JavaScript);
        let no_var = rules.iter().find(|r| r.id == "js-no-var").unwrap();
        assert_eq!(no_var.severity, Severity::Info);
    }

    #[test]
    fn custom_rules_added() {
        let yaml = r#"
custom_rules:
  - id: no-debug-code
    description: "No debug statements in production code"
    languages: [php, javascript]
    severity: error
"#;
        let config = Config::parse(yaml).unwrap();

        let php_rules = config.effective_rules(Language::Php);
        assert!(php_rules.iter().any(|r| r.id == "no-debug-code"));

        let js_rules = config.effective_rules(Language::JavaScript);
        assert!(js_rules.iter().any(|r| r.id == "no-debug-code"));

        // Not in CSS
        let css_rules = config.effective_rules(Language::Css);
        assert!(!css_rules.iter().any(|r| r.id == "no-debug-code"));
    }

    #[test]
    fn custom_rules_no_languages_applies_to_all() {
        let yaml = r#"
custom_rules:
  - id: global-rule
    description: "Applies everywhere"
    severity: info
"#;
        let config = Config::parse(yaml).unwrap();

        // Should appear in all languages
        for lang in &[
            Language::Php,
            Language::JavaScript,
            Language::Css,
            Language::Html,
        ] {
            let rules = config.effective_rules(*lang);
            assert!(
                rules.iter().any(|r| r.id == "global-rule"),
                "Missing in {lang}"
            );
        }
    }

    #[test]
    fn languages_filter_parsed() {
        let yaml = r#"
languages:
  - php
  - javascript
"#;
        let config = Config::parse(yaml).unwrap();
        let langs = config.languages.unwrap();
        assert_eq!(langs.len(), 2);
        assert!(langs.contains(&"php".to_string()));
    }

    #[test]
    fn invalid_yaml_returns_error() {
        let result = Config::parse("{{{{invalid yaml");
        assert!(result.is_err());
    }

    #[test]
    fn config_serializes_roundtrip() {
        let yaml = r#"
model: test-model
rules:
  php:
    php-no-eval:
      enabled: false
custom_rules:
  - id: my-rule
    description: My custom rule
    severity: warning
"#;
        let config = Config::parse(yaml).unwrap();
        let serialized = serde_json::to_string(&config).unwrap();
        let _restored: Config = serde_json::from_str(&serialized).unwrap();
    }

    #[test]
    fn agents_parsed() {
        let yaml = r#"
agents:
  - name: "PCI Compliance"
    prompt: "You are a PCI-DSS reviewer."
    languages: [php, javascript]
    rules:
      - id: pci-no-pan
        description: "No card numbers in logs"
        severity: error
"#;
        let config = Config::parse(yaml).unwrap();
        assert_eq!(config.agents.len(), 1);
        assert_eq!(config.agents[0].name, "PCI Compliance");
        assert_eq!(config.agents[0].languages.len(), 2);
        assert_eq!(config.agents[0].rules.len(), 1);
        assert!(config.agents[0].enabled);
    }

    #[test]
    fn agents_default_enabled() {
        let yaml = r#"
agents:
  - name: "Test Agent"
    prompt: "Review the code."
"#;
        let config = Config::parse(yaml).unwrap();
        assert!(config.agents[0].enabled);
        assert!(config.agents[0].languages.is_empty());
        assert!(config.agents[0].rules.is_empty());
    }

    #[test]
    fn agents_can_be_disabled() {
        let yaml = r#"
agents:
  - name: "Disabled Agent"
    prompt: "Review the code."
    enabled: false
"#;
        let config = Config::parse(yaml).unwrap();
        assert!(!config.agents[0].enabled);
    }

    #[test]
    fn empty_config_has_no_agents() {
        let config = Config::parse("").unwrap();
        assert!(config.agents.is_empty());
    }

    // ── Exclude tests ──

    #[test]
    fn exclude_parsed() {
        let yaml = r#"
exclude:
  - .lando.yml
  - "*.log"
  - vendor/
  - node_modules/
"#;
        let config = Config::parse(yaml).unwrap();
        assert_eq!(config.exclude.len(), 4);
        assert!(config.exclude.contains(&".lando.yml".to_string()));
        assert!(config.exclude.contains(&"vendor/".to_string()));
    }

    #[test]
    fn empty_config_has_no_excludes() {
        let config = Config::parse("").unwrap();
        assert!(config.exclude.is_empty());
    }

    #[test]
    fn is_excluded_exact_match() {
        let yaml = r#"
exclude:
  - .lando.yml
  - docker-compose.yml
"#;
        let config = Config::parse(yaml).unwrap();
        assert!(config.is_excluded(".lando.yml"));
        assert!(config.is_excluded("docker-compose.yml"));
        assert!(!config.is_excluded("app.php"));
    }

    #[test]
    fn is_excluded_glob_pattern() {
        let yaml = r#"
exclude:
  - "*.log"
  - "*.min.js"
"#;
        let config = Config::parse(yaml).unwrap();
        assert!(config.is_excluded("error.log"));
        assert!(config.is_excluded("app.min.js"));
        assert!(!config.is_excluded("app.js"));
    }

    #[test]
    fn is_excluded_directory_prefix() {
        let yaml = r#"
exclude:
  - vendor/
  - node_modules/
"#;
        let config = Config::parse(yaml).unwrap();
        assert!(config.is_excluded("vendor/autoload.php"));
        assert!(config.is_excluded("node_modules/lodash/index.js"));
        assert!(!config.is_excluded("src/vendor_helper.php"));
    }

    #[test]
    fn is_excluded_path_contains() {
        let yaml = r#"
exclude:
  - "*/test/*"
"#;
        let config = Config::parse(yaml).unwrap();
        assert!(config.is_excluded("src/test/helper.php"));
        assert!(!config.is_excluded("src/main.php"));
    }
}
