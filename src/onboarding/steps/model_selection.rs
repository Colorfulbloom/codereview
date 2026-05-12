use async_trait::async_trait;

use super::{OnboardingStep, StepContext, StepId, StepOutcome};
use crate::onboarding::error::OnboardingError;
use crate::onboarding::state::{OnboardingState, StepData};

/// Models recommended for code review, in order of preference.
/// Format: (name, description with RAM requirement)
const RECOMMENDED_MODELS: &[(&str, &str)] = &[
    (
        "qwen3-coder:30b",
        "Best overall, 256K context, native tool use (~20GB RAM)",
    ),
    (
        "qwen2.5-coder:32b",
        "92.7% HumanEval, matches GPT-4o level (~22GB RAM)",
    ),
    (
        "devstral:24b",
        "Purpose-built for agentic coding tasks (~16GB RAM)",
    ),
    (
        "deepseek-coder-v2:16b",
        "Strong performance, smaller footprint (~11GB RAM)",
    ),
    ("gemma4", "Fast, good for smaller edits (~6GB RAM)"),
];

pub struct ModelSelectionStep;

#[async_trait]
impl OnboardingStep for ModelSelectionStep {
    fn id(&self) -> StepId {
        StepId::ModelSelection
    }

    fn title(&self) -> &'static str {
        "Model Selection"
    }

    async fn execute(
        &self,
        ctx: &StepContext<'_>,
        _prior_state: &OnboardingState,
    ) -> Result<StepOutcome, OnboardingError> {
        let models = ctx.ollama.list_models().await?;

        if models.is_empty() {
            ctx.ui.print("No models found locally.");
            ctx.ui.print("");
            ctx.ui.print("Recommended models for code review:");

            let mut items: Vec<String> = RECOMMENDED_MODELS
                .iter()
                .map(|(name, desc)| format!("{name} — {desc}"))
                .collect();
            items.push("Skip for now".to_string());
            let item_refs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();

            match ctx.ui.select("Pick a model to pull:", &item_refs) {
                Some(idx) if idx < RECOMMENDED_MODELS.len() => {
                    let (model_name, _) = RECOMMENDED_MODELS[idx];
                    let spinner = ctx
                        .ui
                        .start_spinner(&format!("Pulling {model_name}... (this may take a while)"));
                    ctx.ollama.pull_model(model_name).await?;
                    spinner.finish(&format!("{model_name} is ready."));

                    return Ok(StepOutcome::Completed(StepData::ModelSelection {
                        selected_model: model_name.to_string(),
                        pulled_new: true,
                    }));
                }
                Some(_) => {
                    ctx.ui.print("You can pull a model later with /models.");
                    return Ok(StepOutcome::Skipped);
                }
                None => return Ok(StepOutcome::Interrupted),
            }
        }

        ctx.ui
            .print(&format!("Found {} local model(s):", models.len()));

        let item_refs: Vec<&str> = models.iter().map(|s| s.as_str()).collect();

        match ctx
            .ui
            .select("Pick your default model for code reviews:", &item_refs)
        {
            Some(idx) => Ok(StepOutcome::Completed(StepData::ModelSelection {
                selected_model: models[idx].clone(),
                pulled_new: false,
            })),
            None => Ok(StepOutcome::Interrupted),
        }
    }
}
