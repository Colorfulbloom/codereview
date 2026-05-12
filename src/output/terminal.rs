//! Terminal output formatter — colored, grouped by file.

use console::Style;

use super::OutputFormatter;
use crate::review::models::{ReviewResult, Severity};

pub struct TerminalFormatter;

impl TerminalFormatter {
    fn severity_icon(severity: &Severity) -> &'static str {
        match severity {
            Severity::Error => "E",
            Severity::Warning => "W",
            Severity::Info => "I",
        }
    }

    fn severity_style(severity: &Severity) -> Style {
        match severity {
            Severity::Error => Style::new().red().bold(),
            Severity::Warning => Style::new().yellow().bold(),
            Severity::Info => Style::new().cyan(),
        }
    }
}

impl OutputFormatter for TerminalFormatter {
    fn format(&self, result: &ReviewResult) -> String {
        let mut out = String::new();

        if result.findings.is_empty() {
            if result.languages_detected.is_empty() && result.agents_ran.is_empty() {
                out.push_str("No supported languages detected in the changed files.\n");
                out.push_str("Supported: PHP, Drupal, JavaScript, CSS, HTML, YAML.\n");
                out.push_str(
                    "Tip: Use custom agents in .codereview.yaml to review other languages.\n",
                );
                return out;
            }
            out.push_str("No issues found.\n\n");
        }

        // Summary line
        let errors = result.count_by_severity(Severity::Error);
        let warnings = result.count_by_severity(Severity::Warning);
        let infos = result.count_by_severity(Severity::Info);

        let summary_style = Style::new().bold();
        out.push_str(&format!(
            "{}\n",
            summary_style.apply_to(format!(
                "Found {} issue(s): {} error(s), {} warning(s), {} info",
                result.total_findings(),
                errors,
                warnings,
                infos
            ))
        ));
        out.push('\n');

        // Group by file
        let by_file = result.findings_by_file();

        for (file_path, findings) in &by_file {
            let file_style = Style::new().bold().underlined();
            out.push_str(&format!("{}\n", file_style.apply_to(file_path)));

            for finding in findings {
                let style = Self::severity_style(&finding.severity);
                let icon = Self::severity_icon(&finding.severity);

                out.push_str(&format!(
                    "  {} line {}: {}\n",
                    style.apply_to(format!("[{icon}]")),
                    finding.line_number,
                    style.apply_to(&finding.title),
                ));
                out.push_str(&format!("    {}\n", finding.description));
                if !finding.suggestion.is_empty() {
                    let hint_style = Style::new().dim();
                    out.push_str(&format!(
                        "    {} {}\n",
                        hint_style.apply_to("Fix:"),
                        finding.suggestion
                    ));
                }
            }
            out.push('\n');
        }

        // Footer with duration and rule count
        let dim = Style::new().dim();

        let rule_info = if result.rules_applied > 0 {
            let langs = result.languages_detected.join(", ");
            format!(" ({} rules: {})", result.rules_applied, langs)
        } else {
            String::new()
        };

        out.push_str(&format!(
            "{}\n",
            dim.apply_to(format!(
                "Reviewed {} file(s) in {:.1}s using {}{}",
                result.files_reviewed,
                result.duration.as_secs_f64(),
                result.model_used,
                rule_info
            ))
        ));

        // Show which agents ran
        if !result.agents_ran.is_empty() {
            out.push_str(&format!(
                "{}\n",
                dim.apply_to(format!("Agents: {}", result.agents_ran.join(", ")))
            ));
        }

        // Hint about /rules when custom config is present
        if result.has_custom_config {
            out.push_str(&format!(
                "{}\n",
                dim.apply_to("Tip: Run /rules to see which rules were active for this review.")
            ));
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::models::{Category, ReviewFinding, ReviewResult, Severity};
    use std::time::Duration;

    fn sample_finding(severity: Severity, file: &str, line: usize) -> ReviewFinding {
        ReviewFinding {
            file_path: file.to_string(),
            line_number: line,
            end_line: None,
            severity,
            category: Category::Bug,
            title: "Test issue".to_string(),
            description: "This is a test issue".to_string(),
            suggestion: "Fix it".to_string(),
        }
    }

    fn sample_result(findings: Vec<ReviewFinding>) -> ReviewResult {
        ReviewResult {
            files_reviewed: 2,
            model_used: "test-model".to_string(),
            duration: Duration::from_millis(1500),
            rules_applied: 0,
            languages_detected: vec![],
            has_custom_config: false,
            agents_ran: vec![],
            findings,
        }
    }

    #[test]
    fn empty_findings_shows_no_issues() {
        let formatter = TerminalFormatter;
        let mut result = sample_result(vec![]);
        // Simulate a real review that ran agents but found nothing
        result.agents_ran = vec!["Security".to_string()];
        result.languages_detected = vec!["PHP".to_string()];
        let output = formatter.format(&result);
        assert!(output.contains("No issues found"));
    }

    #[test]
    fn summary_line_shows_counts() {
        let formatter = TerminalFormatter;
        let result = sample_result(vec![
            sample_finding(Severity::Error, "a.rs", 1),
            sample_finding(Severity::Warning, "a.rs", 2),
            sample_finding(Severity::Info, "b.rs", 3),
        ]);
        let output = formatter.format(&result);
        assert!(output.contains("3 issue(s)"));
        assert!(output.contains("1 error(s)"));
        assert!(output.contains("1 warning(s)"));
        assert!(output.contains("1 info"));
    }

    #[test]
    fn findings_grouped_by_file() {
        let formatter = TerminalFormatter;
        let result = sample_result(vec![
            sample_finding(Severity::Error, "src/a.rs", 10),
            sample_finding(Severity::Warning, "src/b.rs", 20),
            sample_finding(Severity::Info, "src/a.rs", 15),
        ]);
        let output = formatter.format(&result);
        // Both a.rs findings should appear under a.rs header
        assert!(output.contains("src/a.rs"));
        assert!(output.contains("src/b.rs"));
    }

    #[test]
    fn finding_shows_line_number() {
        let formatter = TerminalFormatter;
        let result = sample_result(vec![sample_finding(Severity::Error, "main.rs", 42)]);
        let output = formatter.format(&result);
        assert!(output.contains("line 42"));
    }

    #[test]
    fn finding_shows_title_and_description() {
        let formatter = TerminalFormatter;
        let result = sample_result(vec![sample_finding(Severity::Warning, "lib.rs", 5)]);
        let output = formatter.format(&result);
        assert!(output.contains("Test issue"));
        assert!(output.contains("This is a test issue"));
    }

    #[test]
    fn finding_shows_suggestion() {
        let formatter = TerminalFormatter;
        let result = sample_result(vec![sample_finding(Severity::Info, "lib.rs", 5)]);
        let output = formatter.format(&result);
        assert!(output.contains("Fix:"));
        assert!(output.contains("Fix it"));
    }

    #[test]
    fn footer_shows_duration_and_model() {
        let formatter = TerminalFormatter;
        let result = sample_result(vec![sample_finding(Severity::Error, "a.rs", 1)]);
        let output = formatter.format(&result);
        assert!(output.contains("1.5s"));
        assert!(output.contains("test-model"));
    }

    #[test]
    fn severity_icons() {
        assert_eq!(TerminalFormatter::severity_icon(&Severity::Error), "E");
        assert_eq!(TerminalFormatter::severity_icon(&Severity::Warning), "W");
        assert_eq!(TerminalFormatter::severity_icon(&Severity::Info), "I");
    }

    #[test]
    fn empty_suggestion_not_shown() {
        let formatter = TerminalFormatter;
        let mut finding = sample_finding(Severity::Error, "a.rs", 1);
        finding.suggestion = String::new();
        let result = sample_result(vec![finding]);
        let output = formatter.format(&result);
        assert!(!output.contains("Fix:"));
    }

    #[test]
    fn footer_shows_rule_count_when_rules_applied() {
        let formatter = TerminalFormatter;
        let mut result = sample_result(vec![sample_finding(Severity::Error, "a.rs", 1)]);
        result.rules_applied = 14;
        result.languages_detected = vec!["PHP".to_string(), "JavaScript".to_string()];
        let output = formatter.format(&result);
        assert!(output.contains("(14 rules: PHP, JavaScript)"));
    }

    #[test]
    fn footer_no_rule_info_when_zero_rules() {
        let formatter = TerminalFormatter;
        let result = sample_result(vec![sample_finding(Severity::Error, "a.rs", 1)]);
        let output = formatter.format(&result);
        assert!(!output.contains("rules:"));
    }

    #[test]
    fn shows_config_hint_when_custom_config() {
        let formatter = TerminalFormatter;
        let mut result = sample_result(vec![sample_finding(Severity::Error, "a.rs", 1)]);
        result.has_custom_config = true;
        let output = formatter.format(&result);
        assert!(output.contains("Tip: Run /rules"));
    }

    #[test]
    fn no_config_hint_when_default_config() {
        let formatter = TerminalFormatter;
        let result = sample_result(vec![sample_finding(Severity::Error, "a.rs", 1)]);
        let output = formatter.format(&result);
        assert!(!output.contains("Tip:"));
    }

    #[test]
    fn shows_agents_that_ran() {
        let formatter = TerminalFormatter;
        let mut result = sample_result(vec![sample_finding(Severity::Error, "a.rs", 1)]);
        result.agents_ran = vec![
            "Security".to_string(),
            "Bug Detection".to_string(),
            "Style (PHP)".to_string(),
        ];
        let output = formatter.format(&result);
        assert!(output.contains("Agents: Security, Bug Detection, Style (PHP)"));
    }

    #[test]
    fn no_agents_line_when_empty() {
        let formatter = TerminalFormatter;
        let result = sample_result(vec![sample_finding(Severity::Error, "a.rs", 1)]);
        let output = formatter.format(&result);
        assert!(!output.contains("Agents:"));
    }

    #[test]
    fn no_findings_still_shows_footer() {
        let formatter = TerminalFormatter;
        let mut result = sample_result(vec![]);
        result.agents_ran = vec!["Security".to_string(), "Style (PHP)".to_string()];
        result.rules_applied = 10;
        result.languages_detected = vec!["PHP".to_string()];
        let output = formatter.format(&result);
        assert!(output.contains("No issues found"));
        assert!(output.contains("Agents: Security, Style (PHP)"));
        assert!(output.contains("10 rules"));
    }

    #[test]
    fn no_languages_detected_shows_warning() {
        let formatter = TerminalFormatter;
        let result = sample_result(vec![]);
        // No languages, no agents, no rules — unsupported file types
        let output = formatter.format(&result);
        assert!(output.contains("No supported languages detected"));
    }
}
