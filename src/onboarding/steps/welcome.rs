use async_trait::async_trait;

use super::{OnboardingStep, StepContext, StepId, StepOutcome};
use crate::onboarding::error::OnboardingError;
use crate::onboarding::state::{OnboardingState, StepData};

pub struct WelcomeStep;

#[async_trait]
impl OnboardingStep for WelcomeStep {
    fn id(&self) -> StepId {
        StepId::Welcome
    }

    fn title(&self) -> &'static str {
        "Welcome"
    }

    async fn execute(
        &self,
        ctx: &StepContext<'_>,
        _prior_state: &OnboardingState,
    ) -> Result<StepOutcome, OnboardingError> {
        ctx.ui
            .print_header(&format!("{} v{}", ctx.app_info.name, ctx.app_info.version));
        ctx.ui.print("");
        ctx.ui.print("AI-powered local code review using Ollama.");
        ctx.ui.print("Your code never leaves your machine.");
        ctx.ui.print("");
        ctx.ui.print("This wizard will help you set up:");
        ctx.ui.print("  - Ollama (local LLM engine)");
        ctx.ui.print("  - A language model for code review");
        ctx.ui.print("  - GitHub/GitLab account linking");
        ctx.ui.print("  - Your review preferences");
        ctx.ui.print("  - Team configuration");
        ctx.ui.print("");
        ctx.ui
            .print("You can re-run this at any time with /onboard or `code-review onboard`.");
        ctx.ui
            .print("Press Ctrl-C at any prompt to pause and save your progress.");
        ctx.ui.print("");

        match ctx.ui.confirm("Ready to get started?", true) {
            Some(true) => Ok(StepOutcome::Completed(StepData::Welcome)),
            Some(false) => Ok(StepOutcome::Skipped),
            None => Ok(StepOutcome::Interrupted),
        }
    }
}
