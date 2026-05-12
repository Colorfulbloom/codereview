use std::path::PathBuf;

use async_trait::async_trait;

use super::{OnboardingStep, StepContext, StepId, StepOutcome};
use crate::onboarding::error::OnboardingError;
use crate::onboarding::state::{OnboardingState, StepData};

const STARTER_CONFIG: &str = r#"# code-review configuration
# See the README for full documentation on available options.

# Default Ollama model for reviews
# model: qwen3-coder:30b

# Default output format: terminal, json, annotations, report
# output_format: terminal

# Languages to review (auto-detected if omitted)
# languages:
#   - php
#   - drupal
#   - javascript
#   - css
#   - html

# Rule overrides
# Disable a built-in rule by setting it to false.
# Customize severity: error, warning, info
# rules:
#   php:
#     psr12-line-length:
#       enabled: true
#       severity: warning
#       max_length: 120
#   drupal:
#     no-static-service-calls:
#       enabled: true
#       severity: error
#   javascript:
#     no-var:
#       enabled: true
#       severity: error

# Custom rules
# custom_rules:
#   - id: no-debug-code
#     description: "No debug statements in production code"
#     languages: [php, javascript]
#     severity: error
#     patterns:
#       - "var_dump("
#       - "console.log("
#       - "dd("
"#;

pub struct TeamConfigStep;

#[async_trait]
impl OnboardingStep for TeamConfigStep {
    fn id(&self) -> StepId {
        StepId::TeamConfig
    }

    fn title(&self) -> &'static str {
        "Team Configuration"
    }

    async fn execute(
        &self,
        ctx: &StepContext<'_>,
        _prior_state: &OnboardingState,
    ) -> Result<StepOutcome, OnboardingError> {
        if !ctx.git.is_repo() {
            ctx.ui
                .print("Not inside a git repository. Skipping team config generation.");
            ctx.ui
                .print("Run /onboard inside a repo to generate a .codereview.yaml.");
            return Ok(StepOutcome::Skipped);
        }

        let repo_root = ctx.git.repo_root().unwrap_or_else(|| PathBuf::from("."));
        let config_path = repo_root.join(".codereview.yaml");

        if ctx.fs.exists(&config_path) {
            ctx.ui
                .print(&format!("Found existing config: {}", config_path.display()));
            ctx.ui.print("Keeping existing configuration.");
            return Ok(StepOutcome::Completed(StepData::TeamConfig {
                generated_path: Some(config_path),
            }));
        }

        match ctx
            .ui
            .confirm("Generate a starter .codereview.yaml in your repo?", true)
        {
            Some(true) => {
                ctx.fs
                    .write(&config_path, STARTER_CONFIG)
                    .map_err(OnboardingError::Io)?;

                ctx.ui.print(&format!("Created: {}", config_path.display()));
                ctx.ui
                    .print("Edit this file to customize rules for your team.");

                Ok(StepOutcome::Completed(StepData::TeamConfig {
                    generated_path: Some(config_path),
                }))
            }
            Some(false) => Ok(StepOutcome::Skipped),
            None => Ok(StepOutcome::Interrupted),
        }
    }
}
