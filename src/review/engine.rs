//! Review engine — orchestrates the review pipeline.

use std::collections::BTreeSet;
use std::time::Instant;

use crate::config::Config;
use crate::git::{FileDiff, GitAgent};
use crate::language;
use crate::language::Language;
use crate::onboarding::steps::OllamaClient;
use crate::output::OutputFormatter;
use crate::review::agents;
use crate::review::claims;
use crate::review::models::ReviewResult;
use crate::review::source::{self, ReviewTarget};

/// Error type for review operations.
#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    #[error("Not in a git repository")]
    NotARepo,

    #[error("No changes to review")]
    NoChanges,

    #[error("Git error: {0}")]
    Git(#[from] crate::git::GitError),

    #[error("{0}")]
    Source(#[from] source::SourceError),

    #[error("Ollama error: {0}")]
    Ollama(String),
}

/// Gather the diffs to review for a given target, with excluded files removed.
///
/// Shared by the engine and the REPL pre-flight so the file list and language
/// detection shown to the user always match what is actually reviewed.
pub fn collect_review_diffs(
    git: &dyn GitAgent,
    target: &ReviewTarget<'_>,
    config: &Config,
) -> Result<Vec<FileDiff>, ReviewError> {
    let diffs = match target {
        ReviewTarget::WorkingTree => {
            if !git.is_repo() {
                return Err(ReviewError::NotARepo);
            }
            git.diff_all()?
        }
        ReviewTarget::Ref(base) => {
            if !git.is_repo() {
                return Err(ReviewError::NotARepo);
            }
            git.diff_branch(base)?
        }
        // Path review reads the filesystem directly — no repository required.
        ReviewTarget::Path(path) => source::read_path_as_diffs(path)?,
    };

    Ok(diffs
        .into_iter()
        .filter(|d| !config.is_excluded(&d.path))
        .collect())
}

/// Detect the languages for a set of review diffs.
///
/// In path mode the reviewed files may carry no Drupal markers of their own
/// (a single controller, say), so the project root is also consulted and PHP
/// promoted to Drupal when the project is a Drupal installation. Shared by
/// the engine and the REPL pre-flight so both report the same languages.
pub fn detect_review_languages(
    git: &dyn GitAgent,
    target: &ReviewTarget<'_>,
    diffs: &[FileDiff],
) -> BTreeSet<Language> {
    let paths: Vec<&str> = diffs.iter().map(|d| d.path.as_str()).collect();
    let mut languages = language::detect_languages(&paths);

    if matches!(target, ReviewTarget::Path(_))
        && languages.contains(&Language::Php)
        && let Ok(root) = git.repo_root()
        && language::is_drupal_project_root(&root)
    {
        languages.remove(&Language::Php);
        languages.insert(Language::Drupal);
    }

    languages
}

/// Roots to scan when proving/refuting an "API does not exist" claim.
///
/// In a repository this is the repo root — it contains the module, `vendor/`,
/// and framework `core/`, so any referenced symbol's definition is reachable.
/// For a path review outside a repo, fall back to the target itself.
fn existence_search_roots(git: &dyn GitAgent, target: &ReviewTarget<'_>) -> Vec<std::path::PathBuf> {
    if let Ok(root) = git.repo_root() {
        return vec![root];
    }
    if let ReviewTarget::Path(p) = target {
        let p = p.to_path_buf();
        let root = if p.is_file() {
            p.parent().map(std::path::Path::to_path_buf).unwrap_or(p)
        } else {
            p
        };
        return vec![root];
    }
    Vec::new()
}

/// Run a code review on the given target.
///
/// `on_agent` fires as each sub-agent starts — callers surface it as progress
/// (spinner message, stderr line) so long LLM calls aren't silent.
///
/// `cache`, when present, serves unchanged files from a prior run so a
/// re-review only sends changed files to the LLM. `None` disables caching.
///
/// Returns the formatted output string and the raw ReviewResult.
#[allow(clippy::too_many_arguments)]
pub async fn run_review(
    git: &dyn GitAgent,
    ollama: &dyn OllamaClient,
    formatter: &dyn OutputFormatter,
    model: &str,
    config: &Config,
    target: ReviewTarget<'_>,
    on_agent: impl Fn(&str),
    cache: Option<&dyn crate::review::cache::FindingCache>,
    phpcs: Option<&dyn crate::review::phpcs::PhpcsRunner>,
) -> Result<(String, ReviewResult), ReviewError> {
    let start = Instant::now();

    let diffs = collect_review_diffs(git, &target, config)?;

    if diffs.is_empty() {
        return Err(ReviewError::NoChanges);
    }

    let files_reviewed = diffs.len();

    // Detect languages from the changed files
    let languages = detect_review_languages(git, &target, &diffs);

    // phpcs owns the deterministic Drupal/PHP rules when it's installed and not
    // disabled. When active, the LLM agents drop those rules (no double-report /
    // re-hallucination); when phpcs can't run, the LLM keeps checking them.
    let phpcs_active = match phpcs {
        Some(r) => !config.phpcs_disabled() && r.available(),
        None => false,
    };

    crate::logging::info(format!(
        "review started: target={target:?} model={model} files={files_reviewed} languages={languages:?} phpcs={phpcs_active}"
    ));

    // PHPCS first — deterministic and fast — and surfaced as progress like an
    // agent so the user sees it running. Calling `on_agent` here borrows it; it
    // is still moved into `run_agents` below.
    let phpcs_findings = if phpcs_active && let Some(runner) = phpcs {
        on_agent("PHPCS (Drupal coding standards)");
        let files = crate::review::phpcs::php_review_files(&diffs);
        let f = crate::review::phpcs::collect_findings(runner, &files, config.phpcs_standard());
        crate::logging::info(format!("phpcs contributed {} finding(s)", f.len()));
        f
    } else {
        Vec::new()
    };

    // Run specialized sub-agents via the orchestrator
    let (findings, agent_runs) = agents::orchestrator::run_agents(
        &diffs,
        &languages,
        config,
        model,
        ollama,
        on_agent,
        cache,
        phpcs_active,
    )
    .await
    .map_err(|e| {
        crate::logging::error(format!("review failed: {e}"));
        ReviewError::Ollama(e.to_string())
    })?;

    // Deterministic hallucination gate: drop "API X does not exist → fatal
    // error" findings when X is actually defined in the project/framework
    // source. Only pays for a source scan when such a claim is present.
    let mut findings = if claims::any_existence_claim(&findings) {
        let index = claims::SourceIndex::build(&existence_search_roots(git, &target));
        claims::verify_existence_claims(findings, &index)
    } else {
        findings
    };

    // Merge phpcs's deterministic findings (collected above, before the agents).
    // They need no verification gate — phpcs reports only what its sniffs match.
    findings.extend(phpcs_findings);

    let rules_applied: usize = agent_runs.iter().map(|r| r.rules_count).sum();
    let languages_detected: Vec<String> = languages.iter().map(|l| l.to_string()).collect();
    let has_custom_config =
        !config.rules.is_empty() || !config.custom_rules.is_empty() || config.model.is_some();
    let agents_ran: Vec<String> = agent_runs.iter().map(|r| r.agent_name.clone()).collect();

    let result = ReviewResult {
        findings,
        files_reviewed,
        model_used: model.to_string(),
        duration: start.elapsed(),
        rules_applied,
        languages_detected,
        has_custom_config,
        agents_ran,
    };

    crate::logging::info(format!(
        "review finished: findings={} files={} duration={:.1}s agents={:?}",
        result.findings.len(),
        result.files_reviewed,
        result.duration.as_secs_f32(),
        result.agents_ran,
    ));

    let output = formatter.format(&result);

    Ok((output, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::FileStatus;
    use crate::git::testutil::{MockGitAgent, make_file_diff};
    use crate::output::terminal::TerminalFormatter;
    use crate::review::models::Severity;

    use std::sync::Mutex;

    /// Mock OllamaClient for review engine tests.
    struct MockReviewOllama {
        response: Mutex<String>,
    }

    impl MockReviewOllama {
        fn with_response(json: &str) -> Self {
            Self {
                response: Mutex::new(json.to_string()),
            }
        }
    }

    #[async_trait::async_trait]
    impl OllamaClient for MockReviewOllama {
        fn is_installed(&self) -> bool {
            true
        }
        async fn is_running(&self) -> bool {
            true
        }
        async fn start(&self) -> Result<(), crate::onboarding::error::OnboardingError> {
            Ok(())
        }
        async fn version(&self) -> Result<String, crate::onboarding::error::OnboardingError> {
            Ok("mock".into())
        }
        async fn list_models(
            &self,
        ) -> Result<Vec<String>, crate::onboarding::error::OnboardingError> {
            Ok(vec!["test".into()])
        }
        async fn pull_model(
            &self,
            _: &str,
        ) -> Result<(), crate::onboarding::error::OnboardingError> {
            Ok(())
        }
        async fn chat(
            &self,
            _model: &str,
            _system: &str,
            _user: &str,
        ) -> Result<String, crate::onboarding::error::OnboardingError> {
            Ok(self.response.lock().unwrap().clone())
        }
    }

    #[tokio::test]
    async fn review_not_a_repo_errors() {
        let git = MockGitAgent::not_a_repo();
        let ollama = MockReviewOllama::with_response("[]");
        let formatter = TerminalFormatter;

        let result = run_review(&git, &ollama, &formatter, "test", &Config::default(), ReviewTarget::WorkingTree, |_: &str| {}, None, None).await;
        assert!(matches!(result, Err(ReviewError::NotARepo)));
    }

    #[tokio::test]
    async fn review_no_changes_errors() {
        let git = MockGitAgent::in_repo();
        let ollama = MockReviewOllama::with_response("[]");
        let formatter = TerminalFormatter;

        let result = run_review(&git, &ollama, &formatter, "test", &Config::default(), ReviewTarget::WorkingTree, |_: &str| {}, None, None).await;
        assert!(matches!(result, Err(ReviewError::NoChanges)));
    }

    #[test]
    fn path_mode_promotes_php_to_drupal_via_project_root() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("web/core/lib")).unwrap();
        std::fs::write(dir.path().join("web/core/lib/Drupal.php"), "<?php\n").unwrap();

        let mut git = MockGitAgent::in_repo();
        git.root = dir.path().to_path_buf();

        let diffs = vec![make_file_diff(
            "src/Controller.php",
            FileStatus::Modified,
            "+<?php",
        )];
        let target = ReviewTarget::Path(std::path::Path::new("src"));

        let langs = detect_review_languages(&git, &target, &diffs);
        assert!(langs.contains(&Language::Drupal));
        assert!(!langs.contains(&Language::Php));
    }

    #[test]
    fn path_mode_without_drupal_root_stays_php() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut git = MockGitAgent::in_repo();
        git.root = dir.path().to_path_buf();

        let diffs = vec![make_file_diff(
            "src/Controller.php",
            FileStatus::Modified,
            "+<?php",
        )];
        let target = ReviewTarget::Path(std::path::Path::new("src"));

        let langs = detect_review_languages(&git, &target, &diffs);
        assert!(langs.contains(&Language::Php));
        assert!(!langs.contains(&Language::Drupal));
    }

    #[test]
    fn working_tree_mode_not_promoted_by_project_root() {
        // Root promotion is a path-mode affordance; working-tree reviews keep
        // relying on markers in the diff itself.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("web/core/lib")).unwrap();
        std::fs::write(dir.path().join("web/core/lib/Drupal.php"), "<?php\n").unwrap();

        let mut git = MockGitAgent::in_repo();
        git.root = dir.path().to_path_buf();

        let diffs = vec![make_file_diff(
            "src/Controller.php",
            FileStatus::Modified,
            "+<?php",
        )];

        let langs = detect_review_languages(&git, &ReviewTarget::WorkingTree, &diffs);
        assert!(langs.contains(&Language::Php));
    }

    #[test]
    fn collect_path_diffs_needs_no_repo() {
        // Path review must work even outside a git repository.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("widget.module"), "<?php\nfunction x() {}\n").unwrap();

        let git = MockGitAgent::not_a_repo();
        let diffs = collect_review_diffs(
            &git,
            &ReviewTarget::Path(dir.path()),
            &Config::default(),
        )
        .unwrap();

        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].path.ends_with("widget.module"));
        assert_eq!(diffs[0].status, FileStatus::Added);
    }

    #[tokio::test]
    async fn run_review_suppresses_existence_hallucination_via_source() {
        // Source on disk proves `setLoggerFactory` exists; a finding claiming
        // it's missing must be dropped even though its evidence is real.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("core")).unwrap();
        std::fs::write(
            dir.path().join("core/Trait.php"),
            "<?php\nclass T {\n  public function setLoggerFactory($f) {}\n}\n",
        )
        .unwrap();

        let mut git = MockGitAgent::in_repo().with_unstaged(
            vec![],
            vec![make_file_diff(
                "src/Foo.php",
                FileStatus::Modified,
                "+    $instance->setLoggerFactory($container->get('logger.factory'));",
            )],
        );
        git.root = dir.path().to_path_buf();

        let response = r#"[{"file_path":"src/Foo.php","line_number":1,"severity":"error","category":"bug","title":"Fatal error","description":"ControllerBase does not have a `setLoggerFactory()` method.","suggestion":"Remove the call.","evidence":"$instance->setLoggerFactory($container->get('logger.factory'));"}]"#;
        let ollama = MockReviewOllama::with_response(response);
        let formatter = TerminalFormatter;

        let (_, result) = run_review(
            &git,
            &ollama,
            &formatter,
            "test",
            &Config::default(),
            ReviewTarget::WorkingTree,
            |_: &str| {},
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            result.total_findings(),
            0,
            "existence hallucination must be suppressed by on-disk source proof"
        );
    }

    #[tokio::test]
    async fn run_review_keeps_existence_claim_when_source_lacks_symbol() {
        // Same shape, but the symbol is genuinely absent from the source — the
        // finding must survive (the gate never drops on uncertainty).
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("unrelated.php"), "<?php\n// nothing here\n").unwrap();

        let mut git = MockGitAgent::in_repo().with_unstaged(
            vec![],
            vec![make_file_diff(
                "src/Foo.php",
                FileStatus::Modified,
                "+    $instance->totallyMadeUpMethod();",
            )],
        );
        git.root = dir.path().to_path_buf();

        let response = r#"[{"file_path":"src/Foo.php","line_number":1,"severity":"error","category":"bug","title":"Fatal error","description":"This class does not have a `totallyMadeUpMethod()` method.","suggestion":"Remove the call.","evidence":"$instance->totallyMadeUpMethod();"}]"#;
        let ollama = MockReviewOllama::with_response(response);
        let formatter = TerminalFormatter;

        let (_, result) = run_review(
            &git,
            &ollama,
            &formatter,
            "test",
            &Config::default(),
            ReviewTarget::WorkingTree,
            |_: &str| {},
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.total_findings(), 1, "a genuinely-absent symbol's claim must survive");
    }

    #[tokio::test]
    async fn run_review_reports_agent_progress() {
        let git = MockGitAgent::in_repo().with_unstaged(
            vec![],
            vec![make_file_diff("app.php", FileStatus::Modified, "+echo 1;")],
        );
        let ollama = MockReviewOllama::with_response("[]");
        let formatter = TerminalFormatter;
        let started = Mutex::new(Vec::<String>::new());

        run_review(
            &git,
            &ollama,
            &formatter,
            "test",
            &Config::default(),
            ReviewTarget::WorkingTree,
            |agent| started.lock().unwrap().push(agent.to_string()),
            None,
            None,
        )
        .await
        .unwrap();

        let names = started.lock().unwrap();
        assert!(names.iter().any(|n| n == "Security"), "got: {names:?}");
        assert!(names.iter().any(|n| n == "Bug Detection"), "got: {names:?}");
    }

    #[tokio::test]
    async fn run_review_reports_phpcs_progress_and_merges_findings() {
        let git = MockGitAgent::in_repo().with_unstaged(
            vec![],
            vec![make_file_diff("src/Foo.php", FileStatus::Modified, "+<?php")],
        );
        let ollama = MockReviewOllama::with_response("[]");
        let formatter = TerminalFormatter;
        let phpcs_json = r#"{"files":{"src/Foo.php":{"messages":[
          {"message":"Use dependency injection.","source":"DrupalPractice.Objects.GlobalDrupal","type":"ERROR","line":3}
        ]}}}"#;
        let phpcs = crate::review::phpcs::MockPhpcsRunner { json: Some(phpcs_json.into()) };
        let started = Mutex::new(Vec::<String>::new());

        let (_, result) = run_review(
            &git,
            &ollama,
            &formatter,
            "m",
            &Config::default(),
            ReviewTarget::WorkingTree,
            |agent| started.lock().unwrap().push(agent.to_string()),
            None,
            Some(&phpcs),
        )
        .await
        .unwrap();

        // The phpcs step must be surfaced as progress, like the LLM agents.
        assert!(
            started.lock().unwrap().iter().any(|n| n.contains("PHPCS")),
            "phpcs progress not reported: {:?}",
            started.lock().unwrap()
        );
        // ...and its deterministic findings must appear in the result.
        assert!(
            result.findings.iter().any(|f| f.description.contains("dependency injection")),
            "phpcs finding missing from result"
        );
    }

    #[tokio::test]
    async fn review_with_changes_returns_findings() {
        let git = MockGitAgent::in_repo().with_unstaged(
            vec![],
            vec![make_file_diff(
                "src/app.php",
                FileStatus::Modified,
                "+eval($user_input);",
            )],
        );

        let response = r#"[{"file_path":"src/app.php","line_number":1,"severity":"error","category":"bug","title":"Eval usage","description":"Using eval is dangerous","suggestion":"Use safer alternative","evidence":"eval($user_input);"}]"#;
        let ollama = MockReviewOllama::with_response(response);
        let formatter = TerminalFormatter;

        let (output, result) = run_review(
            &git,
            &ollama,
            &formatter,
            "test-model",
            &Config::default(),
            ReviewTarget::WorkingTree,
            |_: &str| {},
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.total_findings(), 1);
        assert_eq!(result.count_by_severity(Severity::Error), 1);
        assert_eq!(result.model_used, "test-model");
        assert_eq!(result.files_reviewed, 1);
        assert!(output.contains("Eval usage"));
    }

    #[tokio::test]
    async fn review_falls_back_to_staged_diffs() {
        let git = MockGitAgent::in_repo().with_staged(
            vec![],
            vec![make_file_diff(
                "staged.php",
                FileStatus::Added,
                "+function create() {}",
            )],
        );

        let ollama = MockReviewOllama::with_response("[]");
        let formatter = TerminalFormatter;

        let (output, result) =
            run_review(&git, &ollama, &formatter, "test", &Config::default(), ReviewTarget::WorkingTree, |_: &str| {}, None, None)
                .await
                .unwrap();

        assert_eq!(result.files_reviewed, 1);
        assert!(output.contains("No issues found"));
    }

    #[tokio::test]
    async fn review_handles_empty_llm_response() {
        let git = MockGitAgent::in_repo().with_unstaged(
            vec![],
            vec![make_file_diff("a.php", FileStatus::Modified, "+echo 1;")],
        );

        let ollama = MockReviewOllama::with_response("");
        let formatter = TerminalFormatter;

        let (_, result) = run_review(&git, &ollama, &formatter, "test", &Config::default(), ReviewTarget::WorkingTree, |_: &str| {}, None, None)
            .await
            .unwrap();
        assert_eq!(result.total_findings(), 0);
    }

    #[tokio::test]
    async fn review_handles_malformed_llm_response() {
        let git = MockGitAgent::in_repo().with_unstaged(
            vec![],
            vec![make_file_diff("a.php", FileStatus::Modified, "+echo 1;")],
        );

        let ollama = MockReviewOllama::with_response("This is not JSON at all");
        let formatter = TerminalFormatter;

        let (_, result) = run_review(&git, &ollama, &formatter, "test", &Config::default(), ReviewTarget::WorkingTree, |_: &str| {}, None, None)
            .await
            .unwrap();
        assert_eq!(result.total_findings(), 0);
    }

    #[tokio::test]
    async fn review_combines_unstaged_and_staged_diffs() {
        // Simulate: .gitignore unstaged + .php staged (the real bug)
        let git = MockGitAgent::in_repo()
            .with_unstaged(
                vec![],
                vec![make_file_diff(
                    ".gitignore",
                    FileStatus::Modified,
                    "+.codereview/",
                )],
            )
            .with_staged(
                vec![],
                vec![make_file_diff(
                    "app.php",
                    FileStatus::Modified,
                    "+echo $user_input;",
                )],
            );

        let response = r#"[{"file_path":"app.php","line_number":1,"severity":"warning","category":"security","title":"Unsanitized output","description":"Echo raw input","suggestion":"Sanitize first"}]"#;
        let ollama = MockReviewOllama::with_response(response);
        let formatter = TerminalFormatter;

        let (_, result) = run_review(&git, &ollama, &formatter, "test", &Config::default(), ReviewTarget::WorkingTree, |_: &str| {}, None, None)
            .await
            .unwrap();

        // Should review BOTH files, not just unstaged
        assert_eq!(result.files_reviewed, 2);
        assert!(result.languages_detected.iter().any(|l| l == "PHP"));
    }

    #[tokio::test]
    async fn review_deduplicates_combined_diffs() {
        // Same file in both unstaged and staged — should appear once
        let git = MockGitAgent::in_repo()
            .with_unstaged(
                vec![],
                vec![make_file_diff("app.php", FileStatus::Modified, "+line1;")],
            )
            .with_staged(
                vec![],
                vec![make_file_diff("app.php", FileStatus::Modified, "+line2;")],
            );

        let ollama = MockReviewOllama::with_response("[]");
        let formatter = TerminalFormatter;

        let (_, result) = run_review(&git, &ollama, &formatter, "test", &Config::default(), ReviewTarget::WorkingTree, |_: &str| {}, None, None)
            .await
            .unwrap();

        // Should deduplicate — only count the file once
        assert_eq!(result.files_reviewed, 1);
    }

    #[tokio::test]
    async fn review_excludes_configured_files() {
        let git = MockGitAgent::in_repo().with_unstaged(
            vec![],
            vec![
                make_file_diff("app.php", FileStatus::Modified, "+echo 1;"),
                make_file_diff(".lando.yml", FileStatus::Modified, "+key: val"),
            ],
        );

        let config = Config::parse(
            r#"
exclude:
  - .lando.yml
"#,
        )
        .unwrap();

        let ollama = MockReviewOllama::with_response("[]");
        let formatter = TerminalFormatter;

        let (_, result) = run_review(&git, &ollama, &formatter, "test", &config, ReviewTarget::WorkingTree, |_: &str| {}, None, None)
            .await
            .unwrap();

        // .lando.yml should be excluded — only app.php reviewed
        assert_eq!(result.files_reviewed, 1);
    }

    #[tokio::test]
    async fn review_all_excluded_returns_no_changes() {
        let git = MockGitAgent::in_repo().with_unstaged(
            vec![],
            vec![make_file_diff(
                ".lando.yml",
                FileStatus::Modified,
                "+key: val",
            )],
        );

        let config = Config::parse(
            r#"
exclude:
  - .lando.yml
"#,
        )
        .unwrap();

        let ollama = MockReviewOllama::with_response("[]");
        let formatter = TerminalFormatter;

        let result = run_review(&git, &ollama, &formatter, "test", &config, ReviewTarget::WorkingTree, |_: &str| {}, None, None).await;
        assert!(matches!(result, Err(ReviewError::NoChanges)));
    }
}
