//! Platform integration — GitHub and GitLab API clients.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::review::models::ReviewFinding;

/// Error type for platform operations.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("Not authenticated for {0}")]
    NotAuthenticated(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("{0}")]
    Other(String),
}

/// A file changed in a PR/MR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrFile {
    pub path: String,
    pub patch: Option<String>,
}

/// Trait for platform API operations.
#[async_trait]
pub trait PlatformClient: Send + Sync {
    /// Get the diff/changed files for a PR or MR.
    async fn get_pr_files(&self, pr_number: u64) -> Result<Vec<PrFile>, PlatformError>;

    /// Post review findings as inline comments on a PR/MR.
    async fn post_review_comments(
        &self,
        pr_number: u64,
        findings: &[ReviewFinding],
    ) -> Result<usize, PlatformError>;
}

/// GitHub API client.
pub struct GitHubClient {
    token: String,
    owner: String,
    repo: String,
    client: reqwest::Client,
}

impl GitHubClient {
    pub fn new(token: String, owner: String, repo: String) -> Self {
        Self {
            token,
            owner,
            repo,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl PlatformClient for GitHubClient {
    async fn get_pr_files(&self, pr_number: u64) -> Result<Vec<PrFile>, PlatformError> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/pulls/{pr_number}/files",
            self.owner, self.repo
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "code-review-cli")
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(PlatformError::Api(format!("GitHub API error: {body}")));
        }

        let files: Vec<serde_json::Value> = resp.json().await?;
        Ok(files
            .iter()
            .map(|f| PrFile {
                path: f["filename"].as_str().unwrap_or("").to_string(),
                patch: f["patch"].as_str().map(String::from),
            })
            .collect())
    }

    async fn post_review_comments(
        &self,
        pr_number: u64,
        findings: &[ReviewFinding],
    ) -> Result<usize, PlatformError> {
        if findings.is_empty() {
            return Ok(0);
        }

        let url = format!(
            "https://api.github.com/repos/{}/{}/pulls/{pr_number}/reviews",
            self.owner, self.repo
        );

        let comments: Vec<serde_json::Value> = findings
            .iter()
            .map(|f| {
                serde_json::json!({
                    "path": f.file_path,
                    "line": f.line_number,
                    "body": format!("**[{}] {}**\n\n{}\n\n*Suggestion:* {}", f.severity, f.title, f.description, f.suggestion),
                })
            })
            .collect();

        let body = serde_json::json!({
            "event": "COMMENT",
            "body": format!("Code Review: Found {} issue(s)", findings.len()),
            "comments": comments,
        });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "code-review-cli")
            .header("Accept", "application/vnd.github+json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(PlatformError::Api(format!("GitHub API error: {body}")));
        }

        Ok(findings.len())
    }
}

/// GitLab API client.
pub struct GitLabClient {
    token: String,
    host: String,
    project_id: String,
    client: reqwest::Client,
}

impl GitLabClient {
    pub fn new(token: String, host: String, project_id: String) -> Self {
        Self {
            token,
            host,
            project_id,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl PlatformClient for GitLabClient {
    async fn get_pr_files(&self, mr_iid: u64) -> Result<Vec<PrFile>, PlatformError> {
        let url = format!(
            "https://{}/api/v4/projects/{}/merge_requests/{mr_iid}/changes",
            self.host, self.project_id
        );

        let resp = self
            .client
            .get(&url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(PlatformError::Api(format!("GitLab API error: {body}")));
        }

        let json: serde_json::Value = resp.json().await?;
        let changes = json["changes"].as_array().cloned().unwrap_or_default();

        Ok(changes
            .iter()
            .map(|c| PrFile {
                path: c["new_path"].as_str().unwrap_or("").to_string(),
                patch: c["diff"].as_str().map(String::from),
            })
            .collect())
    }

    async fn post_review_comments(
        &self,
        mr_iid: u64,
        findings: &[ReviewFinding],
    ) -> Result<usize, PlatformError> {
        let mut posted = 0;

        for finding in findings {
            let url = format!(
                "https://{}/api/v4/projects/{}/merge_requests/{mr_iid}/discussions",
                self.host, self.project_id
            );

            let body = serde_json::json!({
                "body": format!(
                    "**[{}] {}**\n\n{}\n\n*Suggestion:* {}",
                    finding.severity, finding.title, finding.description, finding.suggestion
                ),
                "position": {
                    "position_type": "text",
                    "new_path": finding.file_path,
                    "new_line": finding.line_number,
                    "base_sha": "",
                    "head_sha": "",
                    "start_sha": "",
                }
            });

            let resp = self
                .client
                .post(&url)
                .header("PRIVATE-TOKEN", &self.token)
                .json(&body)
                .send()
                .await?;

            if resp.status().is_success() {
                posted += 1;
            }
        }

        Ok(posted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Mock platform client for testing.
    pub struct MockPlatformClient {
        pub files: Vec<PrFile>,
        pub post_count: Mutex<usize>,
    }

    impl MockPlatformClient {
        pub fn new(files: Vec<PrFile>) -> Self {
            Self {
                files,
                post_count: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl PlatformClient for MockPlatformClient {
        async fn get_pr_files(&self, _pr_number: u64) -> Result<Vec<PrFile>, PlatformError> {
            Ok(self.files.clone())
        }

        async fn post_review_comments(
            &self,
            _pr_number: u64,
            findings: &[ReviewFinding],
        ) -> Result<usize, PlatformError> {
            *self.post_count.lock().unwrap() = findings.len();
            Ok(findings.len())
        }
    }

    #[test]
    fn pr_file_serializes() {
        let file = PrFile {
            path: "src/main.rs".to_string(),
            patch: Some("+fn new() {}".to_string()),
        };
        let json = serde_json::to_string(&file).unwrap();
        assert!(json.contains("src/main.rs"));
    }

    #[tokio::test]
    async fn mock_client_returns_files() {
        let client = MockPlatformClient::new(vec![PrFile {
            path: "a.rs".to_string(),
            patch: None,
        }]);

        let files = client.get_pr_files(1).await.unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "a.rs");
    }

    #[tokio::test]
    async fn mock_client_posts_comments() {
        let client = MockPlatformClient::new(vec![]);

        let findings = vec![ReviewFinding {
            file_path: "a.rs".to_string(),
            line_number: 1,
            end_line: None,
            severity: crate::review::models::Severity::Error,
            category: crate::review::models::Category::Bug,
            title: "Bug".to_string(),
            description: "A bug".to_string(),
            suggestion: "Fix".to_string(),
        }];

        let count = client.post_review_comments(1, &findings).await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(*client.post_count.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn mock_client_empty_findings_posts_zero() {
        let client = MockPlatformClient::new(vec![]);
        let count = client.post_review_comments(1, &[]).await.unwrap();
        assert_eq!(count, 0);
    }
}
