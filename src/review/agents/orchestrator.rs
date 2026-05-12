//! Orchestrator — coordinates specialized sub-agents for code review.

use std::collections::{BTreeSet, HashSet};

use crate::config::Config;
use crate::git::FileDiff;
use crate::language::rules::Rule;
use crate::language::{self, Language};
use crate::onboarding::steps::OllamaClient;
use crate::review::models::ReviewFinding;

use super::accessibility::AccessibilityAgent;
use super::bugs::BugDetectionAgent;
use super::config_defined::ConfigDefinedAgent;
use super::custom::CustomRulesAgent;
use super::security::SecurityAgent;
use super::style::LanguageStyleAgent;
use super::twig::TwigAgent;
use super::{AgentError, ReviewAgent};

/// Result of a single agent's run.
pub struct AgentRun {
    pub agent_name: String,
    pub finding_count: usize,
    pub rules_count: usize,
}

/// Coordinate specialized sub-agents for a review.
pub async fn run_agents(
    diffs: &[FileDiff],
    languages: &BTreeSet<Language>,
    config: &Config,
    model: &str,
    ollama: &dyn OllamaClient,
    on_agent_start: impl Fn(&str),
) -> Result<(Vec<ReviewFinding>, Vec<AgentRun>), AgentError> {
    // Collect all effective rules across detected languages
    let all_rules: Vec<Rule> = languages
        .iter()
        .flat_map(|lang| config.effective_rules(*lang))
        .collect();

    let mut all_findings: Vec<ReviewFinding> = Vec::new();
    let mut runs: Vec<AgentRun> = Vec::new();

    let is_drupal =
        language::is_drupal_project(&diffs.iter().map(|d| d.path.as_str()).collect::<Vec<_>>());

    // 1. SecurityAgent — cross-language, all diffs
    run_agent_if_applicable(
        &SecurityAgent::new(&all_rules),
        diffs,
        model,
        ollama,
        &on_agent_start,
        &mut all_findings,
        &mut runs,
    )
    .await?;

    // 2. BugDetectionAgent — cross-language, all diffs
    run_agent_if_applicable(
        &BugDetectionAgent::new(&all_rules),
        diffs,
        model,
        ollama,
        &on_agent_start,
        &mut all_findings,
        &mut runs,
    )
    .await?;

    // 3. LanguageStyleAgent — one per detected language, filtered diffs
    for &lang in languages {
        let style = LanguageStyleAgent::new(lang, &all_rules);
        if style.rules().is_empty() {
            continue;
        }

        let lang_diffs = filter_diffs_by_language(diffs, lang, is_drupal);
        if lang_diffs.is_empty() {
            continue;
        }

        run_agent_if_applicable(
            &style,
            &lang_diffs,
            model,
            ollama,
            &on_agent_start,
            &mut all_findings,
            &mut runs,
        )
        .await?;
    }

    // 4. AccessibilityAgent — only for HTML/CSS
    if languages.contains(&Language::Html) || languages.contains(&Language::Css) {
        let a11y = AccessibilityAgent::new(&all_rules);
        if !a11y.rules().is_empty() {
            let a11y_diffs =
                filter_diffs_by_languages(diffs, &[Language::Html, Language::Css], is_drupal);
            if !a11y_diffs.is_empty() {
                run_agent_if_applicable(
                    &a11y,
                    &a11y_diffs,
                    model,
                    ollama,
                    &on_agent_start,
                    &mut all_findings,
                    &mut runs,
                )
                .await?;
            }
        }
    }

    // 5. TwigAgent — only when .twig files are in the diff
    if TwigAgent::has_twig_files(diffs) {
        let twig = TwigAgent::new(&all_rules);
        if !twig.rules().is_empty() {
            let twig_diffs = TwigAgent::filter_twig_diffs(diffs);
            run_agent_if_applicable(
                &twig,
                &twig_diffs,
                model,
                ollama,
                &on_agent_start,
                &mut all_findings,
                &mut runs,
            )
            .await?;
        }
    }

    // 6. CustomRulesAgent — only when custom rules exist
    let custom_ids: HashSet<&str> = config.custom_rules.iter().map(|c| c.id.as_str()).collect();
    let custom_rules: Vec<Rule> = all_rules
        .iter()
        .filter(|r| custom_ids.contains(r.id.as_str()))
        .cloned()
        .collect();
    if !custom_rules.is_empty() {
        run_agent_if_applicable(
            &CustomRulesAgent::new(custom_rules),
            diffs,
            model,
            ollama,
            &on_agent_start,
            &mut all_findings,
            &mut runs,
        )
        .await?;
    }

    // 6. Config-defined custom agents
    for agent_config in &config.agents {
        if !agent_config.enabled {
            continue;
        }

        let agent = ConfigDefinedAgent::from_config(agent_config);

        // Filter diffs by agent's language config (empty = all)
        let agent_diffs = if agent_config.languages.is_empty() {
            diffs.to_vec()
        } else {
            diffs
                .iter()
                .filter(|d| {
                    let mut detected = language::detect_language(&d.path);
                    if is_drupal && detected == Some(Language::Php) {
                        detected = Some(Language::Drupal);
                    }
                    detected.is_some_and(|l| {
                        agent_config
                            .languages
                            .iter()
                            .any(|al| al.to_lowercase() == l.to_string().to_lowercase())
                    })
                })
                .cloned()
                .collect()
        };

        if agent_diffs.is_empty() {
            continue;
        }

        on_agent_start(agent.name());
        let findings = agent.review(&agent_diffs, model, ollama).await?;
        runs.push(AgentRun {
            agent_name: agent.name().to_string(),
            finding_count: findings.len(),
            rules_count: agent_config.rules.len(),
        });
        all_findings.extend(findings);
    }

    deduplicate_findings(&mut all_findings);

    Ok((all_findings, runs))
}

async fn run_agent_if_applicable(
    agent: &dyn ReviewAgent,
    diffs: &[FileDiff],
    model: &str,
    ollama: &dyn OllamaClient,
    on_start: &impl Fn(&str),
    all_findings: &mut Vec<ReviewFinding>,
    runs: &mut Vec<AgentRun>,
) -> Result<(), AgentError> {
    if agent.rules().is_empty() || diffs.is_empty() {
        return Ok(());
    }

    on_start(agent.name());
    let findings = agent.review(diffs, model, ollama).await?;
    runs.push(AgentRun {
        agent_name: agent.name().to_string(),
        finding_count: findings.len(),
        rules_count: agent.rules().len(),
    });
    all_findings.extend(findings);
    Ok(())
}

/// Filter diffs to files matching a specific language.
fn filter_diffs_by_language(diffs: &[FileDiff], lang: Language, is_drupal: bool) -> Vec<FileDiff> {
    diffs
        .iter()
        .filter(|d| {
            let mut detected = language::detect_language(&d.path);
            if is_drupal && detected == Some(Language::Php) {
                detected = Some(Language::Drupal);
            }
            detected == Some(lang)
        })
        .cloned()
        .collect()
}

/// Filter diffs to files matching any of the given languages.
fn filter_diffs_by_languages(
    diffs: &[FileDiff],
    langs: &[Language],
    is_drupal: bool,
) -> Vec<FileDiff> {
    diffs
        .iter()
        .filter(|d| {
            let mut detected = language::detect_language(&d.path);
            if is_drupal && detected == Some(Language::Php) {
                detected = Some(Language::Drupal);
            }
            detected.is_some_and(|l| langs.contains(&l))
        })
        .cloned()
        .collect()
}

/// Remove duplicate findings (same file, line, title).
fn deduplicate_findings(findings: &mut Vec<ReviewFinding>) {
    // Use a hash of borrowed values to avoid cloning strings
    use std::hash::{Hash, Hasher};
    let mut seen = HashSet::new();
    findings.retain(|f| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        f.file_path.hash(&mut hasher);
        f.line_number.hash(&mut hasher);
        f.title.hash(&mut hasher);
        seen.insert(hasher.finish())
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::FileStatus;
    use crate::git::testutil::make_file_diff;
    use crate::review::testutil::SequentialMockOllama;
    use std::sync::Mutex;

    #[tokio::test]
    async fn runs_security_and_bug_agents_for_php() {
        let diffs = vec![make_file_diff("app.php", FileStatus::Modified, "+$x = 1;")];
        let languages = BTreeSet::from([Language::Php]);
        let config = Config::default();
        let ollama = SequentialMockOllama::with_responses(vec!["[]", "[]", "[]"]);
        let started = Mutex::new(Vec::new());

        let (findings, runs) = run_agents(&diffs, &languages, &config, "test", &ollama, |name| {
            started.lock().unwrap().push(name.to_string())
        })
        .await
        .unwrap();

        assert!(findings.is_empty());
        let names: Vec<&str> = runs.iter().map(|r| r.agent_name.as_str()).collect();
        assert!(names.contains(&"Security"));
        assert!(names.contains(&"Bug Detection"));
        assert!(names.iter().any(|n| n.starts_with("Style")));
    }

    #[tokio::test]
    async fn runs_a11y_agent_for_html() {
        let diffs = vec![make_file_diff(
            "index.html",
            FileStatus::Modified,
            "+<img src='x'>",
        )];
        let languages = BTreeSet::from([Language::Html]);
        let config = Config::default();
        let ollama = SequentialMockOllama::with_responses(vec!["[]", "[]", "[]", "[]"]);

        let (_, runs) = run_agents(&diffs, &languages, &config, "test", &ollama, |_| {})
            .await
            .unwrap();

        let names: Vec<&str> = runs.iter().map(|r| r.agent_name.as_str()).collect();
        assert!(names.contains(&"Accessibility"));
    }

    #[tokio::test]
    async fn skips_a11y_for_php_only() {
        let diffs = vec![make_file_diff("app.php", FileStatus::Modified, "+echo 1;")];
        let languages = BTreeSet::from([Language::Php]);
        let config = Config::default();
        let ollama = SequentialMockOllama::with_responses(vec!["[]", "[]", "[]"]);

        let (_, runs) = run_agents(&diffs, &languages, &config, "test", &ollama, |_| {})
            .await
            .unwrap();

        let names: Vec<&str> = runs.iter().map(|r| r.agent_name.as_str()).collect();
        assert!(!names.contains(&"Accessibility"));
    }

    #[tokio::test]
    async fn deduplicates_findings() {
        let finding_json = r#"[{"file_path":"a.php","line_number":1,"severity":"error","category":"security","title":"Same Issue","description":"Desc","suggestion":"Fix"}]"#;
        let diffs = vec![make_file_diff("a.php", FileStatus::Modified, "+bad();")];
        let languages = BTreeSet::from([Language::Php]);
        let config = Config::default();
        // Multiple agents return the same finding
        let ollama =
            SequentialMockOllama::with_responses(vec![finding_json, finding_json, finding_json]);

        let (findings, _) = run_agents(&diffs, &languages, &config, "test", &ollama, |_| {})
            .await
            .unwrap();

        // Should be deduplicated to 1
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn runs_custom_agent_when_custom_rules_exist() {
        let diffs = vec![make_file_diff("app.php", FileStatus::Modified, "+dd($x);")];
        let languages = BTreeSet::from([Language::Php]);
        let config = Config::parse(
            r#"
custom_rules:
  - id: no-dd
    description: "No dd() calls"
    languages: [php]
    severity: error
"#,
        )
        .unwrap();
        let ollama = SequentialMockOllama::with_responses(vec!["[]", "[]", "[]", "[]"]);

        let (_, runs) = run_agents(&diffs, &languages, &config, "test", &ollama, |_| {})
            .await
            .unwrap();

        let names: Vec<&str> = runs.iter().map(|r| r.agent_name.as_str()).collect();
        assert!(names.contains(&"Custom Rules"));
    }

    #[tokio::test]
    async fn skips_custom_agent_when_no_custom_rules() {
        let diffs = vec![make_file_diff("app.php", FileStatus::Modified, "+echo 1;")];
        let languages = BTreeSet::from([Language::Php]);
        let config = Config::default();
        let ollama = SequentialMockOllama::with_responses(vec!["[]", "[]", "[]"]);

        let (_, runs) = run_agents(&diffs, &languages, &config, "test", &ollama, |_| {})
            .await
            .unwrap();

        let names: Vec<&str> = runs.iter().map(|r| r.agent_name.as_str()).collect();
        assert!(!names.contains(&"Custom Rules"));
    }

    #[test]
    fn dedup_removes_exact_duplicates() {
        let mut findings = vec![
            ReviewFinding {
                file_path: "a.rs".into(),
                line_number: 1,
                end_line: None,
                severity: crate::review::models::Severity::Error,
                category: crate::review::models::Category::Bug,
                title: "Bug".into(),
                description: "Desc1".into(),
                suggestion: "Fix1".into(),
            },
            ReviewFinding {
                file_path: "a.rs".into(),
                line_number: 1,
                end_line: None,
                severity: crate::review::models::Severity::Warning,
                category: crate::review::models::Category::Security,
                title: "Bug".into(), // same title + file + line
                description: "Desc2".into(),
                suggestion: "Fix2".into(),
            },
        ];
        deduplicate_findings(&mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn runs_config_defined_agents() {
        let diffs = vec![make_file_diff(
            "payment.php",
            FileStatus::Modified,
            "+charge();",
        )];
        let languages = BTreeSet::from([Language::Php]);
        let config = Config::parse(
            r#"
agents:
  - name: "PCI Check"
    prompt: "Check for PCI compliance."
    languages: [php]
    rules:
      - id: pci-no-pan
        description: "No card numbers"
        severity: error
"#,
        )
        .unwrap();
        let ollama = SequentialMockOllama::with_responses(vec!["[]", "[]", "[]", "[]"]);

        let (_, runs) = run_agents(&diffs, &languages, &config, "test", &ollama, |_| {})
            .await
            .unwrap();

        let names: Vec<&str> = runs.iter().map(|r| r.agent_name.as_str()).collect();
        assert!(names.contains(&"PCI Check"));
    }

    #[tokio::test]
    async fn skips_disabled_config_agents() {
        let diffs = vec![make_file_diff("app.php", FileStatus::Modified, "+echo 1;")];
        let languages = BTreeSet::from([Language::Php]);
        let config = Config::parse(
            r#"
agents:
  - name: "Disabled Agent"
    prompt: "Review code."
    enabled: false
"#,
        )
        .unwrap();
        let ollama = SequentialMockOllama::with_responses(vec!["[]", "[]", "[]"]);

        let (_, runs) = run_agents(&diffs, &languages, &config, "test", &ollama, |_| {})
            .await
            .unwrap();

        let names: Vec<&str> = runs.iter().map(|r| r.agent_name.as_str()).collect();
        assert!(!names.contains(&"Disabled Agent"));
    }

    #[tokio::test]
    async fn config_agent_filtered_by_language() {
        let diffs = vec![make_file_diff(
            "style.css",
            FileStatus::Modified,
            "+body {}",
        )];
        let languages = BTreeSet::from([Language::Css]);
        let config = Config::parse(
            r#"
agents:
  - name: "PHP Only Agent"
    prompt: "Review PHP code."
    languages: [php]
"#,
        )
        .unwrap();
        let ollama = SequentialMockOllama::with_responses(vec!["[]", "[]", "[]"]);

        let (_, runs) = run_agents(&diffs, &languages, &config, "test", &ollama, |_| {})
            .await
            .unwrap();

        // PHP-only agent should NOT run on CSS files
        let names: Vec<&str> = runs.iter().map(|r| r.agent_name.as_str()).collect();
        assert!(!names.contains(&"PHP Only Agent"));
    }

    #[tokio::test]
    async fn runs_twig_agent_for_twig_files() {
        let diffs = vec![make_file_diff(
            "node.html.twig",
            FileStatus::Modified,
            "+{{ afsadasdf }}",
        )];
        let languages = BTreeSet::from([Language::Html]);
        let config = Config::default();
        // Security, Bug, Style(HTML), A11y, Twig = 5 agents
        let ollama = SequentialMockOllama::with_responses(vec!["[]", "[]", "[]", "[]", "[]"]);

        let (_, runs) = run_agents(&diffs, &languages, &config, "test", &ollama, |_| {})
            .await
            .unwrap();

        let names: Vec<&str> = runs.iter().map(|r| r.agent_name.as_str()).collect();
        assert!(
            names.contains(&"Twig Templates"),
            "Twig agent should run. Got: {names:?}"
        );
    }

    #[tokio::test]
    async fn skips_twig_agent_for_non_twig_files() {
        let diffs = vec![make_file_diff(
            "style.css",
            FileStatus::Modified,
            "+body {}",
        )];
        let languages = BTreeSet::from([Language::Css]);
        let config = Config::default();
        let ollama = SequentialMockOllama::with_responses(vec!["[]", "[]", "[]"]);

        let (_, runs) = run_agents(&diffs, &languages, &config, "test", &ollama, |_| {})
            .await
            .unwrap();

        let names: Vec<&str> = runs.iter().map(|r| r.agent_name.as_str()).collect();
        assert!(!names.contains(&"Twig Templates"));
    }
}
