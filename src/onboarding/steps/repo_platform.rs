use async_trait::async_trait;

use super::{OnboardingStep, StepContext, StepId, StepOutcome};
use crate::credentials::credential_key;
use crate::onboarding::error::OnboardingError;
use crate::onboarding::state::{OnboardingState, Platform, PlatformAccount, StepData};

pub struct RepoPlatformStep;

#[async_trait]
impl OnboardingStep for RepoPlatformStep {
    fn id(&self) -> StepId {
        StepId::RepoPlatform
    }

    fn title(&self) -> &'static str {
        "Repository Platform"
    }

    async fn execute(
        &self,
        ctx: &StepContext<'_>,
        _prior_state: &OnboardingState,
    ) -> Result<StepOutcome, OnboardingError> {
        ctx.ui
            .print("Link your GitHub or GitLab account(s) to enable PR integration.");
        ctx.ui.print(
            "You can link multiple accounts (useful for submodules on different platforms).",
        );
        ctx.ui.print("");

        let mut accounts: Vec<PlatformAccount> = Vec::new();

        loop {
            let action = if accounts.is_empty() {
                "Add an account"
            } else {
                "Add another account"
            };

            let items = if accounts.is_empty() {
                vec!["GitHub", "GitLab", "Skip for now"]
            } else {
                vec!["GitHub", "GitLab", "Done adding accounts"]
            };

            ctx.ui.print("");
            let selection = match ctx.ui.select(action, &items) {
                Some(idx) => idx,
                None => return Ok(StepOutcome::Interrupted),
            };

            match selection {
                0 => match add_github_account(ctx).await? {
                    Some(account) => {
                        ctx.ui.print(&format!(
                            "Linked GitHub account: {} on {}",
                            account.username, account.host
                        ));
                        accounts.push(account);
                    }
                    None => return Ok(StepOutcome::Interrupted),
                },
                1 => match add_gitlab_account(ctx).await? {
                    Some(account) => {
                        ctx.ui.print(&format!(
                            "Linked GitLab account: {} on {}",
                            account.username, account.host
                        ));
                        accounts.push(account);
                    }
                    None => return Ok(StepOutcome::Interrupted),
                },
                2 => break,
                _ => break,
            }
        }

        if accounts.is_empty() {
            ctx.ui
                .print("No accounts linked. You can add them later with /onboard.");
            return Ok(StepOutcome::Skipped);
        }

        ctx.ui
            .print(&format!("{} account(s) linked.", accounts.len()));
        Ok(StepOutcome::Completed(StepData::RepoPlatform { accounts }))
    }
}

async fn add_github_account(
    ctx: &StepContext<'_>,
) -> Result<Option<PlatformAccount>, OnboardingError> {
    let host = "github.com".to_string();

    ctx.ui.print("");
    ctx.ui
        .print("GitHub authentication via Personal Access Token:");
    ctx.ui
        .print("Create a Fine-Grained PAT at: https://github.com/settings/tokens?type=beta");
    ctx.ui.print(
        "Required permissions: contents (read), pull_requests (read+write), metadata (read)",
    );
    ctx.ui.print("");

    let username = match ctx.ui.prompt("GitHub username:") {
        Some(u) => u,
        None => return Ok(None),
    };

    let token = match ctx.ui.password("Paste your token:") {
        Some(t) => t,
        None => return Ok(None),
    };

    let token_ref = credential_key("github", &host, &username);

    // Verify token with GitHub API
    match verify_github_token(&token).await {
        Ok(verified_user) => {
            ctx.ui
                .print(&format!("Verified: authenticated as {verified_user}"));
        }
        Err(e) => {
            ctx.ui
                .print(&format!("Warning: could not verify token: {e}"));
            ctx.ui
                .print("The token will be stored anyway — you can re-verify later.");
        }
    }

    // Store token in credential store
    if let Some(creds) = ctx.credentials {
        match creds.store(&token_ref, &token) {
            Ok(()) => {
                ctx.ui.print("Token stored securely.");
            }
            Err(e) => {
                ctx.ui
                    .print(&format!("Warning: could not store token in keyring: {e}"));
                ctx.ui.print("You may need to re-enter it later.");
            }
        }
    } else {
        ctx.ui
            .print("Note: No credential store available. Token will not be persisted.");
    }

    Ok(Some(PlatformAccount {
        platform: Platform::GitHub,
        host,
        username,
        token_ref,
    }))
}

async fn add_gitlab_account(
    ctx: &StepContext<'_>,
) -> Result<Option<PlatformAccount>, OnboardingError> {
    ctx.ui.print("");

    let host = match ctx
        .ui
        .prompt_with_default("GitLab instance URL:", "gitlab.com")
    {
        Some(h) => h,
        None => return Ok(None),
    };

    ctx.ui.print(&format!("Authenticating with {host}"));
    ctx.ui.print("");

    let settings_url = if host == "gitlab.com" {
        "https://gitlab.com/-/user_settings/personal_access_tokens".to_string()
    } else {
        format!("https://{host}/-/user_settings/personal_access_tokens")
    };
    ctx.ui
        .print("GitLab authentication via Personal Access Token:");
    ctx.ui.print(&format!("Create a PAT at: {settings_url}"));
    ctx.ui
        .print("Required scopes: api (for posting review comments), read_api (for reading)");
    ctx.ui.print("");

    let username = match ctx.ui.prompt("GitLab username:") {
        Some(u) => u,
        None => return Ok(None),
    };

    let token = match ctx.ui.password("Paste your token:") {
        Some(t) => t,
        None => return Ok(None),
    };

    let token_ref = credential_key("gitlab", &host, &username);

    // Verify token with GitLab API
    match verify_gitlab_token(&host, &token).await {
        Ok(verified_user) => {
            ctx.ui
                .print(&format!("Verified: authenticated as {verified_user}"));
        }
        Err(e) => {
            ctx.ui
                .print(&format!("Warning: could not verify token: {e}"));
            ctx.ui
                .print("The token will be stored anyway — you can re-verify later.");
        }
    }

    // Store token in credential store
    if let Some(creds) = ctx.credentials {
        match creds.store(&token_ref, &token) {
            Ok(()) => {
                ctx.ui.print("Token stored securely.");
            }
            Err(e) => {
                ctx.ui
                    .print(&format!("Warning: could not store token in keyring: {e}"));
                ctx.ui.print("You may need to re-enter it later.");
            }
        }
    } else {
        ctx.ui
            .print("Note: No credential store available. Token will not be persisted.");
    }

    Ok(Some(PlatformAccount {
        platform: Platform::GitLab,
        host,
        username,
        token_ref,
    }))
}

/// Verify a GitHub token by calling GET /user.
async fn verify_github_token(token: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "code-review-cli")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("HTTP error: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("API returned {}", resp.status()));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    Ok(json["login"].as_str().unwrap_or("unknown").to_string())
}

/// Verify a GitLab token by calling GET /api/v4/user.
async fn verify_gitlab_token(host: &str, token: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let url = format!("https://{host}/api/v4/user");
    let resp = client
        .get(&url)
        .header("PRIVATE-TOKEN", token)
        .send()
        .await
        .map_err(|e| format!("HTTP error: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("API returned {}", resp.status()));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    Ok(json["username"].as_str().unwrap_or("unknown").to_string())
}
