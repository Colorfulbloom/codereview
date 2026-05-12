//! Parse LLM responses into structured ReviewFindings.

use super::models::ReviewFinding;

/// Parse an LLM response string into review findings.
///
/// Handles multiple formats:
/// - JSON array of findings
/// - One JSON object per line (JSON Lines)
/// - JSON embedded in markdown code blocks
/// - Mixed text and JSON
pub fn parse_review_response(response: &str) -> Vec<ReviewFinding> {
    // Try as a JSON array first
    if let Ok(findings) = serde_json::from_str::<Vec<ReviewFinding>>(response) {
        return findings;
    }

    // Try extracting JSON from ```json blocks specifically
    let json_blocks = extract_json_blocks(response);
    for block in &json_blocks {
        if let Ok(findings) = serde_json::from_str::<Vec<ReviewFinding>>(block) {
            return findings;
        }
    }

    // Try JSON lines within ```json blocks
    if !json_blocks.is_empty() {
        let combined = json_blocks.join("\n");
        let findings = parse_json_lines(&combined);
        if !findings.is_empty() {
            return findings;
        }
    }

    // Try any ``` code block as fallback
    let all_blocks = extract_all_code_blocks(response);
    for block in &all_blocks {
        if let Ok(findings) = serde_json::from_str::<Vec<ReviewFinding>>(block) {
            return findings;
        }
    }
    if !all_blocks.is_empty() {
        let combined = all_blocks.join("\n");
        let findings = parse_json_lines(&combined);
        if !findings.is_empty() {
            return findings;
        }
    }

    // Final fallback: parse JSON lines from the raw response
    parse_json_lines(response)
}

/// Extract content from ```json code blocks only.
fn extract_json_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current_block = String::new();
    let mut in_json_block = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```json") && !in_json_block {
            in_json_block = true;
            continue;
        }
        if trimmed == "```" && in_json_block {
            in_json_block = false;
            if !current_block.is_empty() {
                blocks.push(std::mem::take(&mut current_block));
            }
            continue;
        }
        if in_json_block {
            current_block.push_str(line);
            current_block.push('\n');
        }
    }

    blocks
}

/// Extract content from any ``` code block (fallback).
fn extract_all_code_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current_block = String::new();
    let mut in_block = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") && !in_block {
            in_block = true;
            continue;
        }
        if trimmed == "```" && in_block {
            in_block = false;
            if !current_block.is_empty() {
                blocks.push(std::mem::take(&mut current_block));
            }
            continue;
        }
        if in_block {
            current_block.push_str(line);
            current_block.push('\n');
        }
    }

    blocks
}

/// Parse JSON objects from individual lines.
fn parse_json_lines(text: &str) -> Vec<ReviewFinding> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with('{'))
        .filter_map(|line| serde_json::from_str::<ReviewFinding>(line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::models::{Category, Severity};

    const SAMPLE_FINDING_JSON: &str = r#"{"file_path":"src/main.rs","line_number":42,"severity":"error","category":"bug","title":"Unwrap on None","description":"This unwrap could panic","suggestion":"Use ? operator"}"#;

    #[test]
    fn parse_json_array() {
        let input = format!("[{SAMPLE_FINDING_JSON}]");
        let findings = parse_review_response(&input);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file_path, "src/main.rs");
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn parse_json_lines_format() {
        let input = format!("{SAMPLE_FINDING_JSON}\n{SAMPLE_FINDING_JSON}");
        let findings = parse_review_response(&input);
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn parse_json_in_markdown_block() {
        let input = format!("Here are the issues:\n\n```json\n[{SAMPLE_FINDING_JSON}]\n```\n");
        let findings = parse_review_response(&input);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, Category::Bug);
    }

    #[test]
    fn parse_json_lines_in_markdown_block() {
        let input =
            format!("Issues found:\n\n```\n{SAMPLE_FINDING_JSON}\n{SAMPLE_FINDING_JSON}\n```\n");
        let findings = parse_review_response(&input);
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn parse_mixed_text_and_json() {
        let input = format!(
            "I found the following issues:\n\n{SAMPLE_FINDING_JSON}\n\nThat's all I found."
        );
        let findings = parse_review_response(&input);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn parse_empty_response() {
        let findings = parse_review_response("");
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_no_json_response() {
        let findings = parse_review_response("The code looks great! No issues found.");
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_malformed_json_skipped() {
        let input = format!("{SAMPLE_FINDING_JSON}\n{{\"bad\": \"json\"}}\n{SAMPLE_FINDING_JSON}");
        let findings = parse_review_response(&input);
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn parse_multiple_findings_different_severities() {
        let input = r#"[
            {"file_path":"a.rs","line_number":1,"severity":"error","category":"bug","title":"Bug","description":"Desc","suggestion":"Fix"},
            {"file_path":"b.rs","line_number":2,"severity":"warning","category":"security","title":"Warn","description":"Desc","suggestion":"Fix"},
            {"file_path":"c.rs","line_number":3,"severity":"info","category":"style","title":"Info","description":"Desc","suggestion":"Fix"}
        ]"#;
        let findings = parse_review_response(input);
        assert_eq!(findings.len(), 3);
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(findings[1].severity, Severity::Warning);
        assert_eq!(findings[2].severity, Severity::Info);
    }

    #[test]
    fn parse_finding_with_end_line() {
        let input = r#"[{"file_path":"a.rs","line_number":10,"end_line":15,"severity":"warning","category":"performance","title":"Slow","description":"N+1","suggestion":"Batch"}]"#;
        let findings = parse_review_response(input);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].end_line, Some(15));
    }

    // C2: json blocks extracted separately from non-json blocks
    #[test]
    fn json_block_extracted_separately_from_code_block() {
        let input = format!(
            "Issues:\n\n```json\n[{SAMPLE_FINDING_JSON}]\n```\n\nExample:\n\n```rust\nfn main() {{}}\n```"
        );
        let findings = parse_review_response(&input);
        assert_eq!(findings.len(), 1);
    }

    // W1: unclosed code block doesn't consume rest of text
    #[test]
    fn unclosed_code_block_handled_gracefully() {
        let input = format!(
            "Some text\n\n```json\n{SAMPLE_FINDING_JSON}\n\nMore text with no closing fence"
        );
        // Should still find the JSON line via fallback
        let findings = parse_review_response(&input);
        assert_eq!(findings.len(), 1);
    }

    // W5: Category::Other deserialization
    #[test]
    fn parse_unknown_category_as_other() {
        let input = r#"[{"file_path":"a.rs","line_number":1,"severity":"info","category":"maintainability","title":"T","description":"D","suggestion":"S"}]"#;
        let findings = parse_review_response(input);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].category, Category::Other(ref s) if s == "maintainability"));
    }
}
