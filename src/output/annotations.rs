//! PR annotation output formatter.
//!
//! Produces GitHub Actions workflow commands or GitLab CI report format.

use super::OutputFormatter;
use crate::review::models::{ReviewResult, Severity};

pub struct AnnotationFormatter;

impl AnnotationFormatter {
    /// Escape special characters for GitHub Actions workflow commands.
    fn escape_gh(s: &str) -> String {
        s.replace('%', "%25")
            .replace('\n', "%0A")
            .replace('\r', "%0D")
    }

    fn severity_to_gh_level(severity: &Severity) -> &'static str {
        match severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "notice",
        }
    }
}

impl OutputFormatter for AnnotationFormatter {
    fn format(&self, result: &ReviewResult) -> String {
        let mut out = String::new();

        for finding in &result.findings {
            // GitHub Actions workflow command format:
            // ::error file={name},line={line}[,endLine={end}]::{message}
            let level = Self::severity_to_gh_level(&finding.severity);
            let end_line = finding
                .end_line
                .map(|e| format!(",endLine={e}"))
                .unwrap_or_default();
            let title = Self::escape_gh(&finding.title);
            let desc = Self::escape_gh(&finding.description);
            out.push_str(&format!(
                "::{level} file={},line={}{end_line}::{title}: {desc}\n",
                finding.file_path, finding.line_number,
            ));
        }

        if result.findings.is_empty() {
            out.push_str("::notice::Code review passed with no issues\n");
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
                    file_path: "src/main.rs".to_string(),
                    line_number: 42,
                    end_line: None,
                    severity: Severity::Error,
                    category: Category::Bug,
                    title: "Bug".to_string(),
                    description: "A bug".to_string(),
                    suggestion: "Fix".to_string(),
                },
                ReviewFinding {
                    file_path: "src/lib.rs".to_string(),
                    line_number: 10,
                    end_line: None,
                    severity: Severity::Warning,
                    category: Category::Style,
                    title: "Style".to_string(),
                    description: "Bad style".to_string(),
                    suggestion: "Fix".to_string(),
                },
            ],
            files_reviewed: 2,
            model_used: "test".to_string(),
            duration: Duration::from_secs(1),
            rules_applied: 0,
            languages_detected: vec![],
            has_custom_config: false,
            agents_ran: vec![],
        }
    }

    #[test]
    fn annotation_format_error() {
        let formatter = AnnotationFormatter;
        let output = formatter.format(&sample_result());
        assert!(output.contains("::error file=src/main.rs,line=42::Bug: A bug"));
    }

    #[test]
    fn annotation_format_warning() {
        let formatter = AnnotationFormatter;
        let output = formatter.format(&sample_result());
        assert!(output.contains("::warning file=src/lib.rs,line=10::Style: Bad style"));
    }

    #[test]
    fn annotation_info_uses_notice() {
        let formatter = AnnotationFormatter;
        let result = ReviewResult {
            findings: vec![ReviewFinding {
                file_path: "a.rs".to_string(),
                line_number: 1,
                end_line: None,
                severity: Severity::Info,
                category: Category::Style,
                title: "Note".to_string(),
                description: "FYI".to_string(),
                suggestion: "".to_string(),
            }],
            files_reviewed: 1,
            model_used: "test".to_string(),
            duration: Duration::from_secs(0),
            rules_applied: 0,
            languages_detected: vec![],
            has_custom_config: false,
            agents_ran: vec![],
        };
        let output = formatter.format(&result);
        assert!(output.contains("::notice file=a.rs,line=1::Note: FYI"));
    }

    #[test]
    fn annotation_no_issues() {
        let formatter = AnnotationFormatter;
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
        assert!(output.contains("::notice::Code review passed with no issues"));
    }

    #[test]
    fn annotation_one_per_line() {
        let formatter = AnnotationFormatter;
        let output = formatter.format(&sample_result());
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);
    }
}
