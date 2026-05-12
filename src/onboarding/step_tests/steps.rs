//! Tests for individual onboarding steps.

use std::path::PathBuf;

use crate::onboarding::state::*;
use crate::onboarding::steps::*;
use crate::onboarding::testutil::*;

// -- Welcome Step --

mod welcome {
    use super::*;
    use crate::onboarding::steps::welcome::WelcomeStep;

    #[tokio::test]
    async fn confirms_ready_completes() {
        let ui = MockUi::new(vec![MockResponse::Bool(true)]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = WelcomeStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        assert!(matches!(result, StepOutcome::Completed(StepData::Welcome)));
    }

    #[tokio::test]
    async fn declines_ready_skips() {
        let ui = MockUi::new(vec![MockResponse::Bool(false)]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = WelcomeStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        assert!(matches!(result, StepOutcome::Skipped));
    }

    #[tokio::test]
    async fn ctrl_c_interrupts() {
        let ui = MockUi::new(vec![MockResponse::Interrupt]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = WelcomeStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        assert!(matches!(result, StepOutcome::Interrupted));
    }

    #[tokio::test]
    async fn prints_app_info() {
        let ui = MockUi::new(vec![MockResponse::Bool(true)]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = WelcomeStep;
        step.execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        let output = ui.output.lock().unwrap();
        assert!(output.iter().any(|l| l.contains("code-review")));
        assert!(output.iter().any(|l| l.contains("Ctrl-C")));
    }
}

// -- Ollama Check Step --

mod ollama_check {
    use super::*;
    use crate::onboarding::steps::ollama_check::OllamaCheckStep;

    #[tokio::test]
    async fn already_running_completes() {
        let ui = MockUi::new(vec![]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = OllamaCheckStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        match result {
            StepOutcome::Completed(StepData::OllamaCheck {
                was_already_running,
                ..
            }) => assert!(was_already_running),
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn not_installed_skips() {
        let ui = MockUi::new(vec![]);
        let ollama = MockOllamaClient::not_installed();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = OllamaCheckStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        assert!(matches!(result, StepOutcome::Skipped));
    }

    #[tokio::test]
    async fn not_running_user_starts_it() {
        let ui = MockUi::new(vec![MockResponse::Bool(true)]); // confirm start
        let ollama = MockOllamaClient::not_running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = OllamaCheckStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        match result {
            StepOutcome::Completed(StepData::OllamaCheck {
                was_already_running,
                ..
            }) => assert!(!was_already_running),
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn not_running_user_declines_skips() {
        let ui = MockUi::new(vec![MockResponse::Bool(false)]); // decline start
        let ollama = MockOllamaClient::not_running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = OllamaCheckStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        assert!(matches!(result, StepOutcome::Skipped));
    }
}

// -- Model Selection Step --

mod model_selection {
    use super::*;
    use crate::onboarding::steps::model_selection::ModelSelectionStep;

    #[tokio::test]
    async fn selects_existing_model() {
        let ui = MockUi::new(vec![MockResponse::Index(0)]); // pick first model
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = ModelSelectionStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        match result {
            StepOutcome::Completed(StepData::ModelSelection {
                selected_model,
                pulled_new,
            }) => {
                assert_eq!(selected_model, "gemma4:latest");
                assert!(!pulled_new);
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn no_models_pulls_selected() {
        let ui = MockUi::new(vec![MockResponse::Index(0)]); // pick first recommended
        let mut ollama = MockOllamaClient::running();
        ollama.models = vec![]; // no local models
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = ModelSelectionStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        match result {
            StepOutcome::Completed(StepData::ModelSelection { pulled_new, .. }) => {
                assert!(pulled_new);
            }
            other => panic!("Expected Completed with pulled_new, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn no_models_skip_option() {
        // The "Skip for now" option is after the 5 recommended models (index 5)
        let ui = MockUi::new(vec![MockResponse::Index(5)]); // skip
        let mut ollama = MockOllamaClient::running();
        ollama.models = vec![];
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = ModelSelectionStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        assert!(matches!(result, StepOutcome::Skipped));
    }
}

// -- Repo Platform Step --

mod repo_platform {
    use super::*;
    use crate::onboarding::steps::repo_platform::RepoPlatformStep;

    #[tokio::test]
    async fn skip_without_adding() {
        let ui = MockUi::new(vec![MockResponse::Index(2)]); // skip
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = RepoPlatformStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        assert!(matches!(result, StepOutcome::Skipped));
    }

    #[tokio::test]
    async fn add_github_pat_account() {
        let ui = MockUi::new(vec![
            MockResponse::Index(0),               // GitHub
            MockResponse::Text("alice".into()),   // username
            MockResponse::Text("ghp_xxx".into()), // token
            MockResponse::Index(2),               // done adding
        ]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = RepoPlatformStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        match result {
            StepOutcome::Completed(StepData::RepoPlatform { accounts }) => {
                assert_eq!(accounts.len(), 1);
                assert_eq!(accounts[0].platform, Platform::GitHub);
                assert_eq!(accounts[0].username, "alice");
                assert_eq!(accounts[0].host, "github.com");
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn add_gitlab_pat_account() {
        let ui = MockUi::new(vec![
            MockResponse::Index(1),                  // GitLab
            MockResponse::Text("gitlab.com".into()), // host (default)
            MockResponse::Text("bob".into()),        // username
            MockResponse::Text("glpat-xxx".into()),  // token
            MockResponse::Index(2),                  // done adding
        ]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = RepoPlatformStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        match result {
            StepOutcome::Completed(StepData::RepoPlatform { accounts }) => {
                assert_eq!(accounts.len(), 1);
                assert_eq!(accounts[0].platform, Platform::GitLab);
                assert_eq!(accounts[0].username, "bob");
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn add_multiple_accounts() {
        let ui = MockUi::new(vec![
            MockResponse::Index(0),                 // GitHub
            MockResponse::Text("alice".into()),     // username
            MockResponse::Text("ghp_xxx".into()),   // token
            MockResponse::Index(1),                 // GitLab (second account)
            MockResponse::Text("gitlab.co".into()), // host
            MockResponse::Text("alice".into()),     // username
            MockResponse::Text("glpat-xxx".into()), // token
            MockResponse::Index(2),                 // done adding
        ]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = RepoPlatformStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        match result {
            StepOutcome::Completed(StepData::RepoPlatform { accounts }) => {
                assert_eq!(accounts.len(), 2);
            }
            other => panic!("Expected Completed with 2 accounts, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn github_token_stored_in_credentials() {
        use crate::credentials::{CredentialStore, MemoryStore};

        let ui = MockUi::new(vec![
            MockResponse::Index(0),               // GitHub
            MockResponse::Text("alice".into()),   // username
            MockResponse::Text("ghp_xxx".into()), // token
            MockResponse::Index(2),               // done adding
        ]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let creds = MemoryStore::new();
        let mut ctx = make_context(&ui, &ollama, &git, &fs);
        ctx.credentials = Some(&creds);

        let step = RepoPlatformStep;
        step.execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        // Verify token was stored
        let stored = creds.get("github:github.com:alice");
        assert!(stored.is_ok(), "Token should be stored in credentials");
        assert_eq!(stored.unwrap(), "ghp_xxx");
    }

    #[tokio::test]
    async fn gitlab_token_stored_in_credentials() {
        use crate::credentials::{CredentialStore, MemoryStore};

        let ui = MockUi::new(vec![
            MockResponse::Index(1),                  // GitLab
            MockResponse::Text("gitlab.com".into()), // host
            MockResponse::Text("bob".into()),        // username
            MockResponse::Text("glpat-yyy".into()),  // token
            MockResponse::Index(2),                  // done adding
        ]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let creds = MemoryStore::new();
        let mut ctx = make_context(&ui, &ollama, &git, &fs);
        ctx.credentials = Some(&creds);

        let step = RepoPlatformStep;
        step.execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        let stored = creds.get("gitlab:gitlab.com:bob");
        assert!(stored.is_ok(), "Token should be stored in credentials");
        assert_eq!(stored.unwrap(), "glpat-yyy");
    }
}

// -- Preferences Step --

mod preferences {
    use super::*;
    use crate::onboarding::steps::preferences::PreferencesStep;

    #[tokio::test]
    async fn selects_json_output_with_auto_stage() {
        let ui = MockUi::new(vec![
            MockResponse::Index(1),   // JSON
            MockResponse::Bool(true), // auto-stage
        ]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = PreferencesStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        match result {
            StepOutcome::Completed(StepData::Preferences {
                output_format,
                auto_stage,
            }) => {
                assert_eq!(output_format, OutputFormat::Json);
                assert!(auto_stage);
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn skip_uses_defaults() {
        let ui = MockUi::new(vec![MockResponse::Index(4)]); // skip
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = PreferencesStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        match result {
            StepOutcome::Completed(StepData::Preferences {
                output_format,
                auto_stage,
            }) => {
                assert_eq!(output_format, OutputFormat::Terminal);
                assert!(!auto_stage);
            }
            other => panic!("Expected Completed with defaults, got {other:?}"),
        }
    }
}

// -- Team Config Step --

mod team_config {
    use super::*;
    use crate::onboarding::steps::team_config::TeamConfigStep;

    #[tokio::test]
    async fn not_in_repo_skips() {
        let ui = MockUi::new(vec![]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = TeamConfigStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        assert!(matches!(result, StepOutcome::Skipped));
    }

    #[tokio::test]
    async fn in_repo_generates_config() {
        let ui = MockUi::new(vec![MockResponse::Bool(true)]); // confirm generate
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::in_repo(PathBuf::from("/tmp/test-repo"));
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = TeamConfigStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        match result {
            StepOutcome::Completed(StepData::TeamConfig {
                generated_path: Some(path),
            }) => {
                assert!(path.to_string_lossy().contains(".codereview.yaml"));
            }
            other => panic!("Expected Completed with path, got {other:?}"),
        }

        // Verify file was written
        let written = fs.written_files.lock().unwrap();
        assert_eq!(written.len(), 1);
        assert!(written[0].1.contains("code-review configuration"));
    }

    #[tokio::test]
    async fn existing_config_preserved() {
        let ui = MockUi::new(vec![]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::in_repo(PathBuf::from("/tmp/test-repo"));
        let fs =
            MockFileSystem::with_existing(vec![PathBuf::from("/tmp/test-repo/.codereview.yaml")]);
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = TeamConfigStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        match result {
            StepOutcome::Completed(StepData::TeamConfig {
                generated_path: Some(_),
            }) => {}
            other => panic!("Expected Completed, got {other:?}"),
        }

        // No files should have been written
        assert!(fs.written_files.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn user_declines_generation() {
        let ui = MockUi::new(vec![MockResponse::Bool(false)]); // decline
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::in_repo(PathBuf::from("/tmp/test-repo"));
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = TeamConfigStep;
        let result = step
            .execute(&ctx, &OnboardingState::default())
            .await
            .unwrap();

        assert!(matches!(result, StepOutcome::Skipped));
    }
}

// -- Done Step --

mod done {
    use super::*;
    use crate::onboarding::steps::done::DoneStep;

    #[tokio::test]
    async fn shows_summary_of_completed_steps() {
        let mut state = OnboardingState::default();
        state.record(
            StepId::ModelSelection,
            StepStatus::Completed,
            Some(StepData::ModelSelection {
                selected_model: "gemma4".to_string(),
                pulled_new: false,
            }),
        );
        state.record(
            StepId::Preferences,
            StepStatus::Completed,
            Some(StepData::Preferences {
                output_format: OutputFormat::Json,
                auto_stage: true,
            }),
        );
        state.record(StepId::RepoPlatform, StepStatus::Skipped, None);

        let ui = MockUi::new(vec![]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let step = DoneStep;
        let result = step.execute(&ctx, &state).await.unwrap();

        assert!(matches!(result, StepOutcome::Completed(StepData::Done)));

        let output = ui.output.lock().unwrap();
        let combined = output.join("\n");
        assert!(combined.contains("gemma4"));
        assert!(combined.contains("JSON"));
        assert!(combined.contains("Skipped"));
        assert!(combined.contains("/review"));
    }
}
