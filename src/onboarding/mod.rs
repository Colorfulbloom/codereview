pub mod error;
pub mod progress;
pub mod run;
#[cfg(test)]
mod run_tests;
pub mod state;
#[cfg(test)]
mod step_tests;
pub mod steps;
#[cfg(test)]
pub mod testutil;

use error::OnboardingError;
use progress::OnboardingPersistence;
use state::{OnboardingState, StepStatus};
use steps::{OnboardingStep, StepContext, StepOutcome};

use steps::done::DoneStep;
use steps::model_selection::ModelSelectionStep;
use steps::ollama_check::OllamaCheckStep;
use steps::preferences::PreferencesStep;
use steps::repo_platform::RepoPlatformStep;
use steps::team_config::TeamConfigStep;
use steps::welcome::WelcomeStep;

/// Top-level API for the onboarding flow.
pub struct OnboardingOrchestrator<'a> {
    steps: Vec<Box<dyn OnboardingStep>>,
    persistence: &'a dyn OnboardingPersistence,
}

impl<'a> OnboardingOrchestrator<'a> {
    pub fn new(persistence: &'a dyn OnboardingPersistence) -> Self {
        let steps: Vec<Box<dyn OnboardingStep>> = vec![
            Box::new(WelcomeStep),
            Box::new(OllamaCheckStep),
            Box::new(ModelSelectionStep),
            Box::new(RepoPlatformStep),
            Box::new(PreferencesStep),
            Box::new(TeamConfigStep),
            Box::new(DoneStep),
        ];

        Self { steps, persistence }
    }

    /// Run onboarding. Resumes from the first incomplete step if prior progress exists.
    pub async fn run(&self, ctx: &StepContext<'_>) -> Result<OnboardingState, OnboardingError> {
        let mut state = self.persistence.load_state()?.unwrap_or_default();

        for step in &self.steps {
            let step_id = step.id();

            // Skip already-completed or skipped steps
            if let Some(entry) = state.entries.get(&step_id)
                && matches!(entry.status, StepStatus::Completed | StepStatus::Skipped)
            {
                continue;
            }

            // Show progress (print_header already adds a leading newline)
            ctx.ui.print_header(&format!(
                "Step {}/{}: {}",
                step_id.number(),
                steps::StepId::total(),
                step.title()
            ));

            match step.execute(ctx, &state).await? {
                StepOutcome::Completed(data) => {
                    state.record(step_id, StepStatus::Completed, Some(data));
                    self.persistence.save_state(&state)?;
                }
                StepOutcome::Skipped => {
                    state.record(step_id, StepStatus::Skipped, None);
                    self.persistence.save_state(&state)?;
                }
                StepOutcome::Interrupted => {
                    // Save progress so far, then exit cleanly
                    self.persistence.save_state(&state)?;
                    ctx.ui.print("");
                    ctx.ui
                        .print("Onboarding paused. Progress saved — it will resume next time.");
                    return Ok(state);
                }
            }
        }

        Ok(state)
    }

    /// Reset all progress for re-running from scratch.
    pub fn reset(&self) -> Result<(), OnboardingError> {
        self.persistence.clear_state()
    }
}

/// Check if onboarding needs to run.
pub fn needs_onboarding(persistence: &dyn OnboardingPersistence) -> Result<bool, OnboardingError> {
    match persistence.load_state()? {
        None => Ok(true),
        Some(state) if !state.is_complete() => Ok(true),
        Some(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testutil::*;

    #[test]
    fn needs_onboarding_true_when_empty() {
        let persistence = MockPersistence::empty();
        assert!(needs_onboarding(&persistence).unwrap());
    }

    #[test]
    fn needs_onboarding_true_when_partial() {
        let mut state = OnboardingState::default();
        state.record(steps::StepId::Welcome, StepStatus::Completed, None);
        let persistence = MockPersistence::with_state(state);
        assert!(needs_onboarding(&persistence).unwrap());
    }

    #[test]
    fn needs_onboarding_false_when_complete() {
        let mut state = OnboardingState::default();
        for step in steps::StepId::all() {
            state.record(*step, StepStatus::Completed, None);
        }
        let persistence = MockPersistence::with_state(state);
        assert!(!needs_onboarding(&persistence).unwrap());
    }

    #[tokio::test]
    async fn orchestrator_runs_all_steps_on_fresh_start() {
        let persistence = MockPersistence::empty();
        let orchestrator = OnboardingOrchestrator::new(&persistence);

        // Script UI responses for all 7 steps:
        // Welcome: confirm yes
        // OllamaCheck: ollama is running, no prompts needed
        // ModelSelection: select first model
        // RepoPlatform: skip
        // Preferences: select terminal output, decline auto-stage
        // TeamConfig: not in repo, auto-skipped
        // Done: no prompts
        let ui = MockUi::new(vec![
            MockResponse::Bool(true),  // Welcome: ready?
            MockResponse::Index(0),    // ModelSelection: pick first model
            MockResponse::Index(2),    // RepoPlatform: skip
            MockResponse::Index(0),    // Preferences: terminal output
            MockResponse::Bool(false), // Preferences: auto-stage no
        ]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let state = orchestrator.run(&ctx).await.unwrap();

        // All steps should be completed or skipped
        assert!(state.is_complete());
        // Persistence should have been called for each step
        assert!(*persistence.save_count.lock().unwrap() >= 7);
    }

    #[tokio::test]
    async fn orchestrator_resumes_from_partial_state() {
        // Pre-populate first 2 steps as completed
        let mut prior = OnboardingState::default();
        prior.record(
            steps::StepId::Welcome,
            StepStatus::Completed,
            Some(state::StepData::Welcome),
        );
        prior.record(
            steps::StepId::OllamaCheck,
            StepStatus::Completed,
            Some(state::StepData::OllamaCheck {
                ollama_version: "0.5.0".to_string(),
                was_already_running: true,
            }),
        );

        let persistence = MockPersistence::with_state(prior);
        let orchestrator = OnboardingOrchestrator::new(&persistence);

        // Only need responses for steps 3-7
        let ui = MockUi::new(vec![
            MockResponse::Index(0),    // ModelSelection: pick first model
            MockResponse::Index(2),    // RepoPlatform: skip
            MockResponse::Index(0),    // Preferences: terminal
            MockResponse::Bool(false), // Preferences: auto-stage
        ]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let state = orchestrator.run(&ctx).await.unwrap();
        assert!(state.is_complete());
    }

    #[tokio::test]
    async fn orchestrator_handles_interruption() {
        let persistence = MockPersistence::empty();
        let orchestrator = OnboardingOrchestrator::new(&persistence);

        // Interrupt at welcome step
        let ui = MockUi::new(vec![MockResponse::Interrupt]);
        let ollama = MockOllamaClient::running();
        let git = MockGitContext::not_in_repo();
        let fs = MockFileSystem::empty();
        let ctx = make_context(&ui, &ollama, &git, &fs);

        let state = orchestrator.run(&ctx).await.unwrap();

        // Should not be complete
        assert!(!state.is_complete());
        // Should have saved the interrupted state
        assert!(*persistence.save_count.lock().unwrap() >= 1);
    }

    #[test]
    fn orchestrator_reset_clears_state() {
        let mut state = OnboardingState::default();
        state.record(steps::StepId::Welcome, StepStatus::Completed, None);
        let persistence = MockPersistence::with_state(state);
        let orchestrator = OnboardingOrchestrator::new(&persistence);

        orchestrator.reset().unwrap();
        assert!(persistence.state.lock().unwrap().is_none());
    }
}
