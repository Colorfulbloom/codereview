//! Data types for the review pipeline.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Severity of a review finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Info => write!(f, "info"),
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
        }
    }
}

/// Category of a review finding.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Bug,
    Security,
    Performance,
    Style,
    BestPractice,
    Accessibility,
    #[serde(untagged)]
    Other(String),
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Category::Bug => write!(f, "bug"),
            Category::Security => write!(f, "security"),
            Category::Performance => write!(f, "performance"),
            Category::Style => write!(f, "style"),
            Category::BestPractice => write!(f, "best_practice"),
            Category::Accessibility => write!(f, "accessibility"),
            Category::Other(s) => write!(f, "{s}"),
        }
    }
}

/// A single finding from a code review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFinding {
    pub file_path: String,
    pub line_number: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    pub severity: Severity,
    pub category: Category,
    pub title: String,
    pub description: String,
    pub suggestion: String,
}

/// The complete result of a review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewResult {
    pub findings: Vec<ReviewFinding>,
    pub files_reviewed: usize,
    pub model_used: String,
    pub duration: Duration,
    /// Number of rules applied during this review.
    #[serde(default)]
    pub rules_applied: usize,
    /// Languages detected in the reviewed files.
    #[serde(default)]
    pub languages_detected: Vec<String>,
    /// Whether the config has custom overrides (for hint display).
    #[serde(default)]
    pub has_custom_config: bool,
    /// Names of agents that ran during this review.
    #[serde(default)]
    pub agents_ran: Vec<String>,
}

impl ReviewResult {
    /// Count findings by severity.
    pub fn count_by_severity(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .count()
    }

    /// Total number of findings.
    pub fn total_findings(&self) -> usize {
        self.findings.len()
    }

    /// Get findings grouped by file path.
    pub fn findings_by_file(&self) -> std::collections::BTreeMap<&str, Vec<&ReviewFinding>> {
        let mut map: std::collections::BTreeMap<&str, Vec<&ReviewFinding>> =
            std::collections::BTreeMap::new();
        for finding in &self.findings {
            map.entry(finding.file_path.as_str())
                .or_default()
                .push(finding);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_finding(severity: Severity) -> ReviewFinding {
        ReviewFinding {
            file_path: "src/main.rs".to_string(),
            line_number: 42,
            end_line: None,
            severity,
            category: Category::Bug,
            title: "Potential null dereference".to_string(),
            description: "This unwrap() could panic if the value is None".to_string(),
            suggestion: "Use if let or match instead of unwrap()".to_string(),
        }
    }

    #[test]
    fn severity_ordering() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn severity_display() {
        assert_eq!(Severity::Info.to_string(), "info");
        assert_eq!(Severity::Warning.to_string(), "warning");
        assert_eq!(Severity::Error.to_string(), "error");
    }

    #[test]
    fn category_display() {
        assert_eq!(Category::Bug.to_string(), "bug");
        assert_eq!(Category::Security.to_string(), "security");
        assert_eq!(Category::BestPractice.to_string(), "best_practice");
        assert_eq!(Category::Other("custom".into()).to_string(), "custom");
    }

    #[test]
    fn finding_serializes_to_json() {
        let finding = sample_finding(Severity::Error);
        let json = serde_json::to_string(&finding).unwrap();

        assert!(json.contains("\"severity\":\"error\""));
        assert!(json.contains("\"line_number\":42"));
        assert!(json.contains("\"category\":\"bug\""));
        // end_line should be omitted when None
        assert!(!json.contains("end_line"));
    }

    #[test]
    fn finding_deserializes_from_json() {
        let json = r#"{
            "file_path": "lib.rs",
            "line_number": 10,
            "severity": "warning",
            "category": "security",
            "title": "Hardcoded secret",
            "description": "API key is hardcoded",
            "suggestion": "Use environment variable"
        }"#;

        let finding: ReviewFinding = serde_json::from_str(json).unwrap();
        assert_eq!(finding.file_path, "lib.rs");
        assert_eq!(finding.line_number, 10);
        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(finding.category, Category::Security);
        assert!(finding.end_line.is_none());
    }

    #[test]
    fn finding_with_end_line() {
        let mut finding = sample_finding(Severity::Info);
        finding.end_line = Some(50);

        let json = serde_json::to_string(&finding).unwrap();
        assert!(json.contains("\"end_line\":50"));

        let restored: ReviewFinding = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.end_line, Some(50));
    }

    #[test]
    fn review_result_count_by_severity() {
        let result = ReviewResult {
            findings: vec![
                sample_finding(Severity::Error),
                sample_finding(Severity::Error),
                sample_finding(Severity::Warning),
                sample_finding(Severity::Info),
            ],
            files_reviewed: 3,
            model_used: "gemma4".to_string(),
            duration: Duration::from_secs(5),
            rules_applied: 0,
            languages_detected: vec![],
            has_custom_config: false,
            agents_ran: vec![],
        };

        assert_eq!(result.count_by_severity(Severity::Error), 2);
        assert_eq!(result.count_by_severity(Severity::Warning), 1);
        assert_eq!(result.count_by_severity(Severity::Info), 1);
        assert_eq!(result.total_findings(), 4);
    }

    #[test]
    fn review_result_findings_by_file() {
        let result = ReviewResult {
            findings: vec![
                ReviewFinding {
                    file_path: "a.rs".to_string(),
                    ..sample_finding(Severity::Error)
                },
                ReviewFinding {
                    file_path: "b.rs".to_string(),
                    ..sample_finding(Severity::Warning)
                },
                ReviewFinding {
                    file_path: "a.rs".to_string(),
                    ..sample_finding(Severity::Info)
                },
            ],
            files_reviewed: 2,
            model_used: "gemma4".to_string(),
            duration: Duration::from_secs(3),
            rules_applied: 0,
            languages_detected: vec![],
            has_custom_config: false,
            agents_ran: vec![],
        };

        let by_file = result.findings_by_file();
        assert_eq!(by_file.len(), 2);
        assert_eq!(by_file["a.rs"].len(), 2);
        assert_eq!(by_file["b.rs"].len(), 1);
    }

    #[test]
    fn empty_review_result() {
        let result = ReviewResult {
            findings: vec![],
            files_reviewed: 0,
            model_used: "test".to_string(),
            duration: Duration::from_secs(0),
            rules_applied: 0,
            languages_detected: vec![],
            has_custom_config: false,
            agents_ran: vec![],
        };

        assert_eq!(result.total_findings(), 0);
        assert_eq!(result.count_by_severity(Severity::Error), 0);
        assert!(result.findings_by_file().is_empty());
    }

    #[test]
    fn review_result_serializes() {
        let result = ReviewResult {
            findings: vec![sample_finding(Severity::Error)],
            files_reviewed: 1,
            model_used: "gemma4".to_string(),
            duration: Duration::from_secs(5),
            rules_applied: 0,
            languages_detected: vec![],
            has_custom_config: false,
            agents_ran: vec![],
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("gemma4"));
        assert!(json.contains("\"files_reviewed\":1"));
    }

    // C1: Category::Other deserialization
    #[test]
    fn category_unknown_string_deserializes_as_other() {
        let json = r#"{"file_path":"a.rs","line_number":1,"severity":"info","category":"maintainability","title":"T","description":"D","suggestion":"S"}"#;
        let finding: ReviewFinding = serde_json::from_str(json).unwrap();
        assert!(matches!(finding.category, Category::Other(ref s) if s == "maintainability"));
    }

    #[test]
    fn category_known_variant_takes_precedence() {
        let json = r#"{"file_path":"a.rs","line_number":1,"severity":"info","category":"bug","title":"T","description":"D","suggestion":"S"}"#;
        let finding: ReviewFinding = serde_json::from_str(json).unwrap();
        assert_eq!(finding.category, Category::Bug);
    }
}
