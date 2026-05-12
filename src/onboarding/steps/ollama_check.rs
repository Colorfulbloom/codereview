use async_trait::async_trait;

use super::{OnboardingStep, StepContext, StepId, StepOutcome};
use crate::onboarding::error::OnboardingError;
use crate::onboarding::state::{OnboardingState, StepData};

pub struct OllamaCheckStep;

#[async_trait]
impl OnboardingStep for OllamaCheckStep {
    fn id(&self) -> StepId {
        StepId::OllamaCheck
    }

    fn title(&self) -> &'static str {
        "Ollama Check"
    }

    async fn execute(
        &self,
        ctx: &StepContext<'_>,
        _prior_state: &OnboardingState,
    ) -> Result<StepOutcome, OnboardingError> {
        ctx.ui.print("Checking for Ollama...");

        // First check if Ollama binary is on PATH
        if !ctx.ollama.is_installed() {
            ctx.ui
                .print("Ollama is not installed. Install it from https://ollama.com");
            ctx.ui
                .print("After installing, re-run this step with /onboard.");
            return Ok(StepOutcome::Skipped);
        }

        let was_already_running = ctx.ollama.is_running().await;

        if !was_already_running {
            ctx.ui.print("Ollama is installed but not running.");

            match ctx.ui.confirm("Would you like to start Ollama?", true) {
                Some(true) => {
                    let spinner = ctx.ui.start_spinner("Starting Ollama...");
                    ctx.ollama.start().await?;

                    // Verify it started
                    if !ctx.ollama.is_running().await {
                        spinner.finish("Failed to start Ollama.");
                        return Err(OnboardingError::OllamaUnavailable(
                            "Ollama did not respond after starting. Try running `ollama serve` manually.".into(),
                        ));
                    }
                    spinner.finish("Ollama started successfully.");
                }
                Some(false) => {
                    ctx.ui.print(
                        "Ollama is required for code reviews. Start it with `ollama serve`.",
                    );
                    return Ok(StepOutcome::Skipped);
                }
                None => return Ok(StepOutcome::Interrupted),
            }
        } else {
            ctx.ui.print("Ollama is running.");
        }

        let version = ctx
            .ollama
            .version()
            .await
            .unwrap_or_else(|_| "unknown".into());
        ctx.ui.print(&format!("Ollama version: {version}"));

        Ok(StepOutcome::Completed(StepData::OllamaCheck {
            ollama_version: version,
            was_already_running,
        }))
    }
}
