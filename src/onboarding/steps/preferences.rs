use async_trait::async_trait;

use super::{OnboardingStep, StepContext, StepId, StepOutcome};
use crate::onboarding::error::OnboardingError;
use crate::onboarding::state::{OnboardingState, OutputFormat, StepData};

pub struct PreferencesStep;

#[async_trait]
impl OnboardingStep for PreferencesStep {
    fn id(&self) -> StepId {
        StepId::Preferences
    }

    fn title(&self) -> &'static str {
        "Preferences"
    }

    async fn execute(
        &self,
        ctx: &StepContext<'_>,
        _prior_state: &OnboardingState,
    ) -> Result<StepOutcome, OnboardingError> {
        ctx.ui
            .print("Set your default preferences (you can change these later).");
        ctx.ui.print("");

        // Output format
        let format_options = vec![
            "Terminal (colored text output)",
            "JSON (structured data)",
            "PR Annotations (inline comments)",
            "Report (file-based report)",
            "Skip for now (use defaults)",
        ];

        let format_idx = match ctx.ui.select("Default output format:", &format_options) {
            Some(idx) => idx,
            None => return Ok(StepOutcome::Interrupted),
        };

        if format_idx == 4 {
            ctx.ui
                .print("Using defaults (terminal output, manual staging).");
            return Ok(StepOutcome::Completed(StepData::Preferences {
                output_format: OutputFormat::Terminal,
                auto_stage: false,
            }));
        }

        let output_format = match format_idx {
            0 => OutputFormat::Terminal,
            1 => OutputFormat::Json,
            2 => OutputFormat::Annotations,
            3 => OutputFormat::Report,
            _ => OutputFormat::Terminal,
        };

        // Auto-stage
        ctx.ui.print("");
        let auto_stage = match ctx.ui.confirm(
            "Auto-stage all reviewed files when committing? (you can still override per commit)",
            false,
        ) {
            Some(v) => v,
            None => return Ok(StepOutcome::Interrupted),
        };

        ctx.ui.print("");
        ctx.ui.print(&format!("Output format: {output_format}"));
        ctx.ui.print(&format!(
            "Staging:  {}",
            if auto_stage { "auto" } else { "manual" }
        ));

        Ok(StepOutcome::Completed(StepData::Preferences {
            output_format,
            auto_stage,
        }))
    }
}
