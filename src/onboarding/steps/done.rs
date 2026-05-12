use async_trait::async_trait;

use super::{OnboardingStep, StepContext, StepId, StepOutcome};
use crate::onboarding::error::OnboardingError;
use crate::onboarding::state::{OnboardingState, StepData, StepStatus};

pub struct DoneStep;

#[async_trait]
impl OnboardingStep for DoneStep {
    fn id(&self) -> StepId {
        StepId::Done
    }

    fn title(&self) -> &'static str {
        "Done"
    }

    async fn execute(
        &self,
        ctx: &StepContext<'_>,
        prior_state: &OnboardingState,
    ) -> Result<StepOutcome, OnboardingError> {
        ctx.ui.print_header("Setup Complete");
        ctx.ui.print("");

        // Summarize what was configured
        if let Some(StepData::ModelSelection { selected_model, .. }) =
            prior_state.get_data(StepId::ModelSelection)
        {
            ctx.ui.print(&format!("  Model:    {selected_model}"));
        }

        if let Some(StepData::RepoPlatform { accounts }) =
            prior_state.get_data(StepId::RepoPlatform)
        {
            for account in accounts {
                ctx.ui.print(&format!(
                    "  Account:  {} ({})",
                    account.username, account.host
                ));
            }
        }

        if let Some(StepData::Preferences {
            output_format,
            auto_stage,
        }) = prior_state.get_data(StepId::Preferences)
        {
            ctx.ui.print(&format!("  Output:   {output_format}"));
            ctx.ui.print(&format!(
                "  Staging:  {}",
                if *auto_stage { "auto" } else { "manual" }
            ));
        }

        if let Some(StepData::TeamConfig {
            generated_path: Some(path),
        }) = prior_state.get_data(StepId::TeamConfig)
        {
            ctx.ui.print(&format!("  Config:   {}", path.display()));
        }

        // Show skipped steps
        let skipped: Vec<&str> = [
            (StepId::OllamaCheck, "Ollama setup"),
            (StepId::ModelSelection, "Model selection"),
            (StepId::RepoPlatform, "GitHub/GitLab linking"),
            (StepId::Preferences, "Preferences"),
            (StepId::TeamConfig, "Team configuration"),
        ]
        .iter()
        .filter(|(id, _)| prior_state.step_status(*id) == Some(StepStatus::Skipped))
        .map(|(_, label)| *label)
        .collect();

        if !skipped.is_empty() {
            ctx.ui.print("");
            ctx.ui.print(&format!("  Skipped:  {}", skipped.join(", ")));
            ctx.ui.print("  Run /onboard to complete these later.");
        }

        ctx.ui.print("");
        ctx.ui.print("Get started:");
        ctx.ui
            .print("  /review   — run a code review on your changes");
        ctx.ui.print("  /diff     — view your current diff");
        ctx.ui.print("  /rules    — see active review rules");
        ctx.ui.print("  /help     — list all commands");
        ctx.ui.print("");

        Ok(StepOutcome::Completed(StepData::Done))
    }
}
