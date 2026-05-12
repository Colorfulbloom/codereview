//! Review engine — orchestrates the review pipeline.

use std::time::Instant;

use crate::config::Config;
use crate::git::GitAgent;
use crate::language;
use crate::onboarding::steps::OllamaClient;
use crate::output::OutputFormatter;
use crate::review::agents;
use crate::review::models::ReviewResult;

/// Error type for review operations.
#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    #[error("Not in a git repository")]
    NotARepo,

    #[error("No changes to review")]
    NoChanges,

    #[error("Git error: {0}")]
    Git(#[from] crate::git::GitError),

    #[error("Ollama error: {0}")]
    Ollama(String),
}

/// Run a code review on the current diff.
///
/// Returns the formatted output string and the raw ReviewResult.
pub async fn run_review(
    git: &dyn GitAgent,
    ollama: &dyn OllamaClient,
    formatter: &dyn OutputFormatter,
    model: &str,
    config: &Config,
    diff_ref: Option<&str>,
) -> Result<(String, ReviewResult), ReviewError> {
    let start = Instant::now();

    if !git.is_repo() {
        return Err(ReviewError::NotARepo);
    }

    // Get diffs based on mode
    let diffs = if let Some(base) = diff_ref {
        // Non-interactive: diff against specified ref
        git.diff_branch(base)?
    } else {
        // Interactive: all uncommitted changes (HEAD → working tree)
        git.diff_all()?
    };

    // Filter out excluded files
    let diffs: Vec<_> = diffs
        .into_iter()
        .filter(|d| !config.is_excluded(&d.path))
        .collect();

    if diffs.is_empty() {
        return Err(ReviewError::NoChanges);
    }

    let files_reviewed = diffs.len();

    // Detect languages from the changed files
    let file_paths: Vec<&str> = diffs.iter().map(|d| d.path.as_str()).collect();
    let languages = language::detect_languages(&file_paths);

    // Run specialized sub-agents via the orchestrator
    let (findings, agent_runs) = agents::orchestrator::run_agents(
        &diffs,
        &languages,
        config,
        model,
        ollama,
        |_agent_name| {
            // Progress callback — used by the REPL spinner
        },
    )
    .await
    .map_err(|e| ReviewError::Ollama(e.to_string()))?;

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

        let result = run_review(&git, &ollama, &formatter, "test", &Config::default(), None).await;
        assert!(matches!(result, Err(ReviewError::NotARepo)));
    }

    #[tokio::test]
    async fn review_no_changes_errors() {
        let git = MockGitAgent::in_repo();
        let ollama = MockReviewOllama::with_response("[]");
        let formatter = TerminalFormatter;

        let result = run_review(&git, &ollama, &formatter, "test", &Config::default(), None).await;
        assert!(matches!(result, Err(ReviewError::NoChanges)));
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

        let response = r#"[{"file_path":"src/app.php","line_number":1,"severity":"error","category":"bug","title":"Eval usage","description":"Using eval is dangerous","suggestion":"Use safer alternative"}]"#;
        let ollama = MockReviewOllama::with_response(response);
        let formatter = TerminalFormatter;

        let (output, result) = run_review(
            &git,
            &ollama,
            &formatter,
            "test-model",
            &Config::default(),
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
            run_review(&git, &ollama, &formatter, "test", &Config::default(), None)
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

        let (_, result) = run_review(&git, &ollama, &formatter, "test", &Config::default(), None)
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

        let (_, result) = run_review(&git, &ollama, &formatter, "test", &Config::default(), None)
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

        let (_, result) = run_review(&git, &ollama, &formatter, "test", &Config::default(), None)
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

        let (_, result) = run_review(&git, &ollama, &formatter, "test", &Config::default(), None)
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

        let (_, result) = run_review(&git, &ollama, &formatter, "test", &config, None)
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

        let result = run_review(&git, &ollama, &formatter, "test", &config, None).await;
        assert!(matches!(result, Err(ReviewError::NoChanges)));
    }
}
