//! Prompt construction for LLM-based code review.

use crate::git::FileDiff;
use crate::language::rules::Rule;

/// Build system and user prompts for a diff-mode code review.
///
/// If `rules` is empty, uses a generic review prompt.
/// If `rules` is provided, includes specific instructions for what to check.
pub fn build_review_prompts(diffs: &[FileDiff], rules: &[Rule]) -> (String, String) {
    let system = build_system_prompt(rules);
    let user = build_user_prompt(diffs);
    (system, user)
}

fn build_system_prompt(rules: &[Rule]) -> String {
    let mut prompt = String::from(
        "You are an expert code reviewer. Analyze the provided code diff and identify issues.\n\n",
    );
    prompt.push_str(crate::review::agents::JSON_SCHEMA);

    if !rules.is_empty() {
        prompt.push_str("\n\n## Rules to check\n\n");
        for rule in rules {
            prompt.push_str(&format!(
                "- [{}] {}: {}\n",
                rule.severity, rule.id, rule.description
            ));
        }
    }

    prompt
}

pub(crate) fn build_user_prompt(diffs: &[FileDiff]) -> String {
    let mut prompt = String::new();

    prompt.push_str(&format!(
        "Review the following changes across {} file(s):\n\n",
        diffs.len()
    ));

    for diff in diffs {
        prompt.push_str(&format!("=== {} ({}) ===\n", diff.path, diff.status));
        for hunk in &diff.hunks {
            prompt.push_str(&format!(
                "--- lines {}-{} → {}-{} ---\n",
                hunk.old_start,
                hunk.old_start + hunk.old_lines,
                hunk.new_start,
                hunk.new_start + hunk.new_lines
            ));
            // Number every new-file line so the model cites real locations
            // instead of estimating (deletions get no number — they don't
            // exist in the new file).
            let mut n = hunk.new_start.max(1);
            for line in hunk.content.lines() {
                if line.starts_with('-') {
                    prompt.push_str(&format!("    | {line}\n"));
                } else {
                    prompt.push_str(&format!("{n:>4}| {line}\n"));
                    n += 1;
                }
            }
        }
        prompt.push('\n');
    }

    prompt.push_str("Analyze the changes and output issues as a JSON array.");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{DiffHunk, FileStatus};
    use crate::language::Language;
    use crate::review::models::Severity;

    fn sample_diff() -> FileDiff {
        FileDiff {
            path: "src/main.rs".to_string(),
            status: FileStatus::Modified,
            hunks: vec![DiffHunk {
                old_start: 10,
                old_lines: 3,
                new_start: 10,
                new_lines: 5,
                content:
                    " fn main() {\n-    old_code();\n+    new_code();\n+    more_code();\n }\n"
                        .to_string(),
            }],
        }
    }

    fn sample_rules() -> Vec<Rule> {
        vec![
            Rule {
                id: "php-no-eval".into(),
                language: Language::Php,
                severity: Severity::Error,
                description: "Never use eval()".into(),
                enabled: true,
            },
            Rule {
                id: "php-type-declarations".into(),
                language: Language::Php,
                severity: Severity::Warning,
                description: "Use type declarations".into(),
                enabled: true,
            },
        ]
    }

    #[test]
    fn system_prompt_requests_json() {
        let (system, _) = build_review_prompts(&[sample_diff()], &[]);
        assert!(system.contains("JSON"));
        assert!(system.contains("severity"));
        assert!(system.contains("file_path"));
        assert!(system.contains("line_number"));
        assert!(system.contains("suggestion"));
    }

    #[test]
    fn system_prompt_defines_severities() {
        let (system, _) = build_review_prompts(&[sample_diff()], &[]);
        assert!(system.contains("\"error\""));
        assert!(system.contains("\"warning\""));
        assert!(system.contains("\"info\""));
    }

    #[test]
    fn system_prompt_defines_categories() {
        let (system, _) = build_review_prompts(&[sample_diff()], &[]);
        assert!(system.contains("\"bug\""));
        assert!(system.contains("\"security\""));
        assert!(system.contains("\"performance\""));
    }

    #[test]
    fn system_prompt_includes_rules_when_provided() {
        let (system, _) = build_review_prompts(&[sample_diff()], &sample_rules());
        assert!(system.contains("Rules to check"));
        assert!(system.contains("php-no-eval"));
        assert!(system.contains("Never use eval()"));
        assert!(system.contains("php-type-declarations"));
    }

    #[test]
    fn system_prompt_no_rules_section_when_empty() {
        let (system, _) = build_review_prompts(&[sample_diff()], &[]);
        assert!(!system.contains("Rules to check"));
    }

    #[test]
    fn rules_include_severity_prefix() {
        let (system, _) = build_review_prompts(&[sample_diff()], &sample_rules());
        assert!(system.contains("[error] php-no-eval"));
        assert!(system.contains("[warning] php-type-declarations"));
    }

    #[test]
    fn user_prompt_numbers_new_file_lines() {
        let (_, user) = build_review_prompts(&[sample_diff()], &[]);
        // Context line carries the hunk's starting new-file number.
        assert!(user.contains("  10|  fn main() {"), "user:\n{user}");
        // Additions advance the counter (10 is the context line above).
        assert!(user.contains("  11| +    new_code();"));
        assert!(user.contains("  12| +    more_code();"));
        // Deletions don't exist in the new file — no number.
        assert!(user.contains("    | -    old_code();"));
    }

    #[test]
    fn system_prompt_requires_evidence() {
        let (system, _) = build_review_prompts(&[sample_diff()], &[]);
        assert!(system.contains("\"evidence\""));
        assert!(system.contains("Accuracy requirements"));
    }

    #[test]
    fn user_prompt_includes_file_path() {
        let (_, user) = build_review_prompts(&[sample_diff()], &[]);
        assert!(user.contains("src/main.rs"));
    }

    #[test]
    fn user_prompt_includes_diff_content() {
        let (_, user) = build_review_prompts(&[sample_diff()], &[]);
        assert!(user.contains("-    old_code();"));
        assert!(user.contains("+    new_code();"));
    }

    #[test]
    fn user_prompt_includes_file_count() {
        let diffs = vec![sample_diff(), sample_diff()];
        let (_, user) = build_review_prompts(&diffs, &[]);
        assert!(user.contains("2 file(s)"));
    }

    #[test]
    fn user_prompt_includes_line_ranges() {
        let (_, user) = build_review_prompts(&[sample_diff()], &[]);
        assert!(user.contains("lines 10-13"));
        assert!(user.contains("10-15"));
    }

    #[test]
    fn user_prompt_includes_file_status() {
        let (_, user) = build_review_prompts(&[sample_diff()], &[]);
        assert!(user.contains("modified"));
    }

    #[test]
    fn empty_diffs_produces_valid_prompt() {
        let (system, user) = build_review_prompts(&[], &[]);
        assert!(!system.is_empty());
        assert!(user.contains("0 file(s)"));
    }

    #[test]
    fn multiple_hunks_in_single_file() {
        let diff = FileDiff {
            path: "lib.rs".to_string(),
            status: FileStatus::Modified,
            hunks: vec![
                DiffHunk {
                    old_start: 1,
                    old_lines: 2,
                    new_start: 1,
                    new_lines: 3,
                    content: "+use std::io;\n".to_string(),
                },
                DiffHunk {
                    old_start: 50,
                    old_lines: 1,
                    new_start: 51,
                    new_lines: 4,
                    content: "+fn helper() {\n+    todo!()\n+}\n".to_string(),
                },
            ],
        };

        let (_, user) = build_review_prompts(&[diff], &[]);
        assert!(user.contains("lines 1-"));
        assert!(user.contains("lines 50-"));
        assert!(user.contains("+use std::io;"));
        assert!(user.contains("+fn helper()"));
    }

    #[test]
    fn added_file_shows_correct_status() {
        let diff = FileDiff {
            path: "new_file.rs".to_string(),
            status: FileStatus::Added,
            hunks: vec![DiffHunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                content: "+fn new() {}\n".to_string(),
            }],
        };

        let (_, user) = build_review_prompts(&[diff], &[]);
        assert!(user.contains("added"));
    }
}
