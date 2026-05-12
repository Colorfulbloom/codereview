//! JSON output formatter.

use super::OutputFormatter;
use crate::review::models::ReviewResult;

pub struct JsonFormatter;

impl OutputFormatter for JsonFormatter {
    fn format(&self, result: &ReviewResult) -> String {
        serde_json::to_string_pretty(result)
            .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::models::{Category, ReviewFinding, ReviewResult, Severity};
    use std::time::Duration;

    fn sample_result() -> ReviewResult {
        ReviewResult {
            findings: vec![ReviewFinding {
                file_path: "src/main.rs".to_string(),
                line_number: 42,
                end_line: None,
                severity: Severity::Error,
                category: Category::Bug,
                title: "Null deref".to_string(),
                description: "Possible null dereference".to_string(),
                suggestion: "Add null check".to_string(),
            }],
            files_reviewed: 1,
            model_used: "test-model".to_string(),
            duration: Duration::from_secs(2),
            rules_applied: 0,
            languages_detected: vec![],
            has_custom_config: false,
            agents_ran: vec![],
        }
    }

    #[test]
    fn json_output_is_valid_json() {
        let formatter = JsonFormatter;
        let output = formatter.format(&sample_result());
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_object());
    }

    #[test]
    fn json_output_contains_findings() {
        let formatter = JsonFormatter;
        let output = formatter.format(&sample_result());
        assert!(output.contains("Null deref"));
        assert!(output.contains("src/main.rs"));
        assert!(output.contains("42"));
    }

    #[test]
    fn json_output_contains_metadata() {
        let formatter = JsonFormatter;
        let output = formatter.format(&sample_result());
        assert!(output.contains("test-model"));
        assert!(output.contains("files_reviewed"));
    }

    #[test]
    fn json_empty_findings() {
        let formatter = JsonFormatter;
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
        let output = formatter.format(&result);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["findings"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn json_output_deserializes_back() {
        let formatter = JsonFormatter;
        let result = sample_result();
        let output = formatter.format(&result);
        let restored: ReviewResult = serde_json::from_str(&output).unwrap();
        assert_eq!(restored.total_findings(), 1);
        assert_eq!(restored.model_used, "test-model");
    }
}
