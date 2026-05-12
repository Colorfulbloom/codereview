//! Markdown report output formatter.

use super::OutputFormatter;
use crate::review::models::{ReviewResult, Severity};

pub struct MarkdownFormatter;

impl OutputFormatter for MarkdownFormatter {
    fn format(&self, result: &ReviewResult) -> String {
        let mut out = String::new();

        out.push_str("# Code Review Report\n\n");

        // Metadata
        out.push_str("## Summary\n\n");
        out.push_str(&format!(
            "- **Files reviewed:** {}\n",
            result.files_reviewed
        ));
        out.push_str(&format!(
            "- **Total issues:** {}\n",
            result.total_findings()
        ));
        out.push_str(&format!(
            "- **Errors:** {} | **Warnings:** {} | **Info:** {}\n",
            result.count_by_severity(Severity::Error),
            result.count_by_severity(Severity::Warning),
            result.count_by_severity(Severity::Info),
        ));
        out.push_str(&format!("- **Model:** `{}`\n", result.model_used));
        out.push_str(&format!(
            "- **Duration:** {:.1}s\n",
            result.duration.as_secs_f64()
        ));

        if result.findings.is_empty() {
            out.push_str("\nNo issues found.\n");
            return out;
        }

        // Issues by file
        out.push_str("\n## Issues by File\n");

        let by_file = result.findings_by_file();
        for (file_path, findings) in &by_file {
            out.push_str(&format!("\n### {file_path}\n\n"));
            out.push_str("| Line | Severity | Title | Description | Suggestion |\n");
            out.push_str("|------|----------|-------|-------------|------------|\n");

            for f in findings {
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} |\n",
                    f.line_number,
                    f.severity,
                    f.title.replace('|', "\\|"),
                    f.description.replace('|', "\\|"),
                    f.suggestion.replace('|', "\\|"),
                ));
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::models::{Category, ReviewFinding, ReviewResult, Severity};
    use std::time::Duration;

    fn sample_result() -> ReviewResult {
        ReviewResult {
            findings: vec![
                ReviewFinding {
                    file_path: "src/a.rs".to_string(),
                    line_number: 10,
                    end_line: None,
                    severity: Severity::Error,
                    category: Category::Bug,
                    title: "Bug found".to_string(),
                    description: "A serious bug".to_string(),
                    suggestion: "Fix it".to_string(),
                },
                ReviewFinding {
                    file_path: "src/b.rs".to_string(),
                    line_number: 20,
                    end_line: None,
                    severity: Severity::Warning,
                    category: Category::Style,
                    title: "Style issue".to_string(),
                    description: "Formatting".to_string(),
                    suggestion: "Reformat".to_string(),
                },
            ],
            files_reviewed: 2,
            model_used: "test-model".to_string(),
            duration: Duration::from_millis(1500),
            rules_applied: 0,
            languages_detected: vec![],
            has_custom_config: false,
            agents_ran: vec![],
        }
    }

    #[test]
    fn markdown_has_title() {
        let formatter = MarkdownFormatter;
        let output = formatter.format(&sample_result());
        assert!(output.starts_with("# Code Review Report"));
    }

    #[test]
    fn markdown_has_summary() {
        let formatter = MarkdownFormatter;
        let output = formatter.format(&sample_result());
        assert!(output.contains("Files reviewed:** 2"));
        assert!(output.contains("Total issues:** 2"));
        assert!(output.contains("test-model"));
    }

    #[test]
    fn markdown_has_file_sections() {
        let formatter = MarkdownFormatter;
        let output = formatter.format(&sample_result());
        assert!(output.contains("### src/a.rs"));
        assert!(output.contains("### src/b.rs"));
    }

    #[test]
    fn markdown_has_table() {
        let formatter = MarkdownFormatter;
        let output = formatter.format(&sample_result());
        assert!(output.contains("| Line | Severity |"));
        assert!(output.contains("| 10 | error |"));
    }

    #[test]
    fn markdown_empty_findings() {
        let formatter = MarkdownFormatter;
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
        assert!(output.contains("No issues found"));
        assert!(!output.contains("Issues by File"));
    }

    #[test]
    fn markdown_escapes_pipes_in_content() {
        let formatter = MarkdownFormatter;
        let result = ReviewResult {
            findings: vec![ReviewFinding {
                file_path: "a.rs".to_string(),
                line_number: 1,
                end_line: None,
                severity: Severity::Info,
                category: Category::Style,
                title: "Test".to_string(),
                description: "Use a | b instead".to_string(),
                suggestion: "a | b".to_string(),
            }],
            files_reviewed: 1,
            model_used: "test".to_string(),
            duration: Duration::from_secs(1),
            rules_applied: 0,
            languages_detected: vec![],
            has_custom_config: false,
            agents_ran: vec![],
        };
        let output = formatter.format(&result);
        assert!(output.contains("a \\| b"));
    }
}
