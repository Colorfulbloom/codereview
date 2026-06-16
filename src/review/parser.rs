//! Parse LLM responses into structured ReviewFindings and verify them
//! against the code that was actually sent.

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::models::ReviewFinding;
use crate::git::FileDiff;

/// A finding as the LLM emits it: the public fields plus the transient
/// `evidence` quote, which exists only to be checked against the source and
/// is discarded afterwards.
#[derive(Debug, Deserialize)]
struct RawFinding {
    #[serde(flatten)]
    finding: ReviewFinding,
    #[serde(default)]
    evidence: Option<String>,
}

/// Parse an LLM response string into review findings (no verification).
pub fn parse_review_response(response: &str) -> Vec<ReviewFinding> {
    parse_response_as::<ReviewFinding>(response)
}

/// Parse an LLM response and verify each finding against the diffs that were
/// sent in the prompt. Local models fabricate findings with confidence; these
/// checks are deterministic and cannot be hallucinated past:
///
/// - a finding must quote its offending line (`evidence`), and that quote must
///   actually appear in the file it names — otherwise it is dropped;
/// - the evidence's real location overrides the model's claimed line number;
/// - a "fix" that merely reproduces existing code is a no-op, not a finding.
pub fn parse_and_verify_response(response: &str, diffs: &[FileDiff]) -> Vec<ReviewFinding> {
    parse_response_as::<RawFinding>(response)
        .into_iter()
        .filter_map(|raw| match verify_finding(raw, diffs) {
            Ok(finding) => Some(finding),
            Err(discarded) => {
                crate::logging::warn(format!("discarded finding: {discarded}"));
                None
            }
        })
        .collect()
}

fn verify_finding(raw: RawFinding, diffs: &[FileDiff]) -> Result<ReviewFinding, String> {
    let mut finding = raw.finding;
    let discard =
        |finding: &ReviewFinding, reason: &str| {
            Err(format!(
                "\"{}\" ({}:{}) — {reason}",
                finding.title, finding.file_path, finding.line_number
            ))
        };

    let Some(evidence) = raw.evidence else {
        return discard(&finding, "no evidence quoted");
    };
    let needle = normalize(&evidence);
    if needle.is_empty() {
        return discard(&finding, "no evidence quoted");
    }

    // js-no-var precision gate: the rule targets the legacy `var` keyword. A
    // var→let/const recommendation whose quoted line contains no `var` token
    // is a misfire — round-3 saw it fire on `let` loop counters and even
    // suggest an impossible `const` (the counter is reassigned). Drop it,
    // while genuine `var` usages (evidence contains `var`) sail through.
    if is_var_modernization_claim(&finding.title, &finding.description, &finding.suggestion)
        && !line_has_var_keyword(&evidence)
    {
        return discard(&finding, "var-modernization finding, but the quoted line has no `var` keyword");
    }

    // A "fix" identical to the flagged code changes nothing.
    let suggestion = normalize(&finding.suggestion);
    if suggestion == needle {
        return discard(&finding, "suggestion repeats the existing code");
    }

    // Resolve the file: exact path first, then a unique suffix match (models
    // sometimes echo a shortened path).
    let Some(diff) = diffs.iter().find(|d| d.path == finding.file_path).or_else(|| {
        let suffix_matches: Vec<&FileDiff> = diffs
            .iter()
            .filter(|d| d.path.ends_with(&finding.file_path))
            .collect();
        (suffix_matches.len() == 1).then(|| suffix_matches[0])
    }) else {
        return discard(&finding, "names a file that was not in this review");
    };
    finding.file_path = diff.path.clone();

    let lines: Vec<(usize, String)> = new_file_lines(diff).collect();

    // Promoted-constructor gate: a "property mismatch / never defined" claim
    // about a class that uses PHP 8 constructor property promotion is the model
    // misreading promoted params as undefined properties — drop it. Only drops
    // on positive proof of a promoted constructor in the reviewed file.
    if is_constructor_property_claim(&finding.title, &finding.description) {
        let content: String = lines
            .iter()
            .map(|(_, c)| c.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if has_promoted_constructor(&content) {
            return discard(
                &finding,
                "constructor-property claim, but the class uses promoted constructor properties",
            );
        }
    }

    let whole_file: String = lines.iter().map(|(_, c)| normalize(c)).collect();

    // A longer "fix" that already exists verbatim in the file is equally a
    // no-op (the YAML-finding signature). The length floor keeps short generic
    // advice from matching by coincidence.
    if suggestion.len() >= 20 && whole_file.contains(&suggestion) {
        return discard(&finding, "suggestion already exists verbatim in the file");
    }

    // The quoted line must exist in the file that was sent.
    let matches: Vec<usize> = lines
        .iter()
        .filter(|(_, content)| normalize(content).contains(&needle))
        .map(|(n, _)| *n)
        .collect();

    match matches.as_slice() {
        [] => {
            // Not on any single line — accept multi-line evidence if the file
            // as a whole contains it (no line correction possible then).
            if whole_file.contains(&needle) {
                Ok(finding)
            } else {
                discard(&finding, "evidence not found in the code")
            }
        }
        found => {
            // Real location wins over the claimed number; on repeats, the
            // closest to the claim.
            finding.line_number = *found
                .iter()
                .min_by_key(|n| n.abs_diff(finding.line_number))
                .expect("non-empty");
            Ok(finding)
        }
    }
}

/// Whitespace-insensitive form for comparisons, so indentation drift between
/// the model's quote and the source doesn't cause false drops.
fn normalize(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Whether a finding is a `js-no-var` style recommendation (replace the legacy
/// `var` keyword with `let`/`const`). Detected from prose because findings
/// don't carry their originating rule id; the `let`/`const` requirement keeps
/// it effectively JS-scoped.
fn is_var_modernization_claim(title: &str, description: &str, suggestion: &str) -> bool {
    let text = format!("{title} {description} {suggestion}").to_lowercase();
    contains_word(&text, "var") && (contains_word(&text, "let") || contains_word(&text, "const"))
}

/// Whether a line of code contains `var` as a standalone keyword, not part of
/// an identifier like `myvar` or `variance`.
fn line_has_var_keyword(line: &str) -> bool {
    contains_word(line, "var")
}

/// Whether a finding claims a constructor "property mismatch" / "property never
/// defined". Local models misread PHP 8 *promoted* constructor properties (e.g.
/// `private readonly Connection $database`) as undefined and invent a "PHP
/// error" — see [`has_promoted_constructor`].
fn is_constructor_property_claim(title: &str, description: &str) -> bool {
    let text = format!("{title} {description}").to_lowercase();
    text.contains("property")
        && (text.contains("mismatch")
            || text.contains("never defined")
            || text.contains("never declared")
            || text.contains("not defined")
            || text.contains("not declared"))
}

/// Whether `code` defines a constructor using PHP 8 property promotion — i.e. a
/// visibility keyword appears in the `__construct` *parameter list* (before the
/// body `{`). A plain constructor's params have no visibility keyword.
fn has_promoted_constructor(code: &str) -> bool {
    let Some(start) = code.find("function __construct") else {
        return false;
    };
    let after = &code[start..];
    let signature = &after[..after.find('{').unwrap_or(after.len())];
    ["private", "protected", "public"]
        .iter()
        .any(|kw| contains_word(signature, kw))
}

/// Case-sensitive whole-word search: `word` must be delimited by a
/// non-identifier character (or a string boundary) on each side.
fn contains_word(haystack: &str, word: &str) -> bool {
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    haystack.match_indices(word).any(|(i, _)| {
        let before_ok = haystack[..i].chars().next_back().is_none_or(|c| !is_ident(c));
        let after_ok = haystack[i + word.len()..]
            .chars()
            .next()
            .is_none_or(|c| !is_ident(c));
        before_ok && after_ok
    })
}

/// Iterate the new-file lines of a diff with their line numbers, skipping
/// deletions and stripping the diff prefix.
fn new_file_lines(diff: &FileDiff) -> impl Iterator<Item = (usize, String)> + '_ {
    diff.hunks.iter().flat_map(|hunk| {
        let mut n = hunk.new_start.max(1) as usize;
        hunk.content
            .lines()
            .filter(|line| !line.starts_with('-'))
            .map(move |line| {
                let content = line.strip_prefix('+').unwrap_or(line);
                let numbered = (n, content.to_string());
                n += 1;
                numbered
            })
    })
}

fn parse_response_as<T: DeserializeOwned>(response: &str) -> Vec<T> {
    // Try as a JSON array first
    if let Ok(findings) = serde_json::from_str::<Vec<T>>(response) {
        return findings;
    }

    // Try extracting JSON from ```json blocks specifically
    let json_blocks = extract_json_blocks(response);
    for block in &json_blocks {
        if let Ok(findings) = serde_json::from_str::<Vec<T>>(block) {
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
        if let Ok(findings) = serde_json::from_str::<Vec<T>>(block) {
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
fn parse_json_lines<T: DeserializeOwned>(text: &str) -> Vec<T> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with('{'))
        .filter_map(|line| serde_json::from_str::<T>(line).ok())
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

    // ── Evidence verification (built from the bcutd_heatmap triage findings) ──

    use crate::git::{DiffHunk, FileDiff, FileStatus};

    fn js_chunk() -> Vec<FileDiff> {
        vec![FileDiff {
            path: "js/overlay.js".into(),
            status: FileStatus::Added,
            hunks: vec![DiffHunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 4,
                content: "+(function () {\n+  e.preventDefault();\n+  var overlay = null;\n+})();\n"
                    .into(),
            }],
        }]
    }

    fn finding_json(line: usize, evidence: &str, suggestion: &str) -> String {
        format!(
            r#"[{{"file_path":"js/overlay.js","line_number":{line},"severity":"error","category":"style","title":"T","description":"D","suggestion":"{suggestion}","evidence":"{evidence}"}}]"#
        )
    }

    #[test]
    fn verified_finding_gets_line_corrected_from_evidence() {
        // Triage signature #3: the model claimed synthetic line numbers.
        // Evidence locates the real line; the claimed number is overridden.
        let response = finding_json(35, "var overlay = null;", "Use const");
        let findings = parse_and_verify_response(&response, &js_chunk());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line_number, 3);
    }

    #[test]
    fn fabricated_evidence_is_dropped() {
        // Triage #44: console.log flagged where none exists in the module.
        let response = finding_json(35, "console.log('debug');", "Remove it");
        let findings = parse_and_verify_response(&response, &js_chunk());
        assert!(findings.is_empty());
    }

    #[test]
    fn missing_evidence_is_dropped() {
        let response = r#"[{"file_path":"js/overlay.js","line_number":2,"severity":"error","category":"style","title":"T","description":"D","suggestion":"S"}]"#;
        let findings = parse_and_verify_response(response, &js_chunk());
        assert!(findings.is_empty());
    }

    #[test]
    fn suggestion_identical_to_evidence_is_dropped() {
        // Triage signature #1: the "fix" is character-for-character the
        // existing code (all 12 YAML findings).
        let response = finding_json(2, "e.preventDefault();", "e.preventDefault();");
        let findings = parse_and_verify_response(&response, &js_chunk());
        assert!(findings.is_empty());
    }

    #[test]
    fn long_suggestion_already_present_in_file_is_dropped() {
        // A multi-line "fix" that reproduces existing file content verbatim is
        // a no-op, even when it isn't byte-identical to the one-line evidence.
        let response = finding_json(
            1,
            "(function () {",
            r"(function () {\n  e.preventDefault();\n  var overlay = null;",
        );
        let findings = parse_and_verify_response(&response, &js_chunk());
        assert!(findings.is_empty());
    }

    #[test]
    fn whitespace_differences_in_evidence_are_tolerated() {
        let response = finding_json(3, "var overlay=null;", "Use const");
        let findings = parse_and_verify_response(&response, &js_chunk());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line_number, 3);
    }

    // ── js-no-var precision gate (round-3 triage) ──

    #[test]
    fn detects_var_modernization_claims() {
        // Real js-no-var findings reference both `var` and `let`/`const`.
        assert!(is_var_modernization_claim(
            "Use of var-like loop variable",
            "The style guide prefers const/let over var.",
            "Consider using const.",
        ));
        assert!(is_var_modernization_claim("T", "replace var with let", "S"));
        // Unrelated findings must not activate the gate.
        assert!(!is_var_modernization_claim(
            "SQL injection",
            "raw query built from user input",
            "use a parameterized query",
        ));
        // "constructor"/"variance" must not count as const/var via substring.
        assert!(!is_var_modernization_claim(
            "Unused constructor argument",
            "the constructor parameter is never used",
            "remove it",
        ));
    }

    #[test]
    fn var_keyword_is_word_bounded() {
        assert!(line_has_var_keyword("var x = 1;"));
        assert!(line_has_var_keyword("  var overlay = null;"));
        assert!(!line_has_var_keyword("for (let p = 0; p < n; p += 4) {"));
        assert!(!line_has_var_keyword("let myvar = 1;")); // identifier, not keyword
        assert!(!line_has_var_keyword("const variance = 2;")); // 'var' inside a word
    }

    #[test]
    fn var_finding_without_var_keyword_is_dropped() {
        // The exact round-3 misfire: js-no-var fired on a `let` counter and
        // even suggested `const`, impossible since `p += 4` reassigns it.
        let response = r#"[{"file_path":"js/overlay.js","line_number":347,"severity":"warning","category":"style","title":"Use of var-like loop variable","description":"The style guide prefers const/let over var.","suggestion":"Consider using const if p is not reassigned.","evidence":"for (let p = 0; p < src.length; p += 4) {"}]"#;
        let findings = parse_and_verify_response(response, &js_chunk());
        assert!(findings.is_empty());
    }

    #[test]
    fn var_finding_with_real_var_keyword_is_kept() {
        // A genuine `var` usage must still be flagged — the gate only drops
        // misfires, never real var findings.
        let response = r#"[{"file_path":"js/overlay.js","line_number":3,"severity":"warning","category":"style","title":"Use let or const instead of var","description":"Avoid the legacy var keyword.","suggestion":"Replace var with const.","evidence":"var overlay = null;"}]"#;
        let findings = parse_and_verify_response(response, &js_chunk());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line_number, 3);
    }

    // ── Promoted-constructor gate (round-6 triage) ──

    #[test]
    fn detects_constructor_property_claims() {
        assert!(is_constructor_property_claim(
            "Property name mismatch in constructor assignment",
            "The property is declared as `$database` but the code assigns to `$this->configFactory`. configFactory was never defined.",
        ));
        // Unrelated findings must not match.
        assert!(!is_constructor_property_claim("SQL injection", "raw query from input"));
        assert!(!is_constructor_property_claim("Missing doc comment", "add a docblock"));
    }

    #[test]
    fn detects_promoted_constructor() {
        let promoted = "class X {\n  public function __construct(\n    private readonly Connection $database,\n    ConfigFactoryInterface $config_factory,\n  ) {\n    $this->configFactory = $config_factory;\n  }\n}";
        assert!(has_promoted_constructor(promoted));

        let plain = "class X {\n  public function __construct(Connection $database) {\n    $this->database = $database;\n  }\n}";
        assert!(!has_promoted_constructor(plain));

        assert!(!has_promoted_constructor("class X {\n  public function foo() {}\n}"));
    }

    #[test]
    fn promoted_constructor_property_mismatch_is_dropped() {
        // The exact round-6 hallucination: "property mismatch" invented on a
        // class that uses PHP 8 constructor property promotion.
        let diff = FileDiff {
            path: "src/Foo.php".into(),
            status: FileStatus::Added,
            hunks: vec![DiffHunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 6,
                content: "+<?php\n+class Foo {\n+  public function __construct(private readonly Connection $database, ConfigFactoryInterface $config_factory) {\n+    $this->configFactory = $config_factory;\n+  }\n+}".into(),
            }],
        };
        let response = r#"[{"file_path":"src/Foo.php","line_number":4,"severity":"error","category":"bug","title":"Property name mismatch in constructor assignment","description":"The property is declared as `$database` but the code assigns to `$this->configFactory`. configFactory was never defined.","suggestion":"assign to config_factory instead","evidence":"$this->configFactory = $config_factory;"}]"#;
        let findings = parse_and_verify_response(response, &[diff]);
        assert!(findings.is_empty(), "promoted-ctor property mismatch must be dropped");
    }

    #[test]
    fn property_mismatch_kept_on_plain_constructor() {
        // A non-promoted constructor keeps the finding — it could be a real bug.
        let diff = FileDiff {
            path: "src/Bar.php".into(),
            status: FileStatus::Added,
            hunks: vec![DiffHunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 6,
                content: "+<?php\n+class Bar {\n+  public function __construct(Connection $db) {\n+    $this->databse = $db;\n+  }\n+}".into(),
            }],
        };
        let response = r#"[{"file_path":"src/Bar.php","line_number":4,"severity":"error","category":"bug","title":"Property name mismatch","description":"property never defined","suggestion":"fix the property name","evidence":"$this->databse = $db;"}]"#;
        let findings = parse_and_verify_response(response, &[diff]);
        assert_eq!(findings.len(), 1, "plain-constructor mismatch is kept");
    }

    #[test]
    fn evidence_in_wrong_file_is_dropped() {
        let response = r#"[{"file_path":"other.js","line_number":3,"severity":"error","category":"style","title":"T","description":"D","suggestion":"Use const","evidence":"var overlay = null;"}]"#;
        let findings = parse_and_verify_response(response, &js_chunk());
        assert!(findings.is_empty());
    }

    #[test]
    fn closest_match_wins_when_evidence_repeats() {
        let chunk = vec![FileDiff {
            path: "a.js".into(),
            status: FileStatus::Added,
            hunks: vec![DiffHunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 5,
                content: "+var x = 1;\n+f();\n+g();\n+h();\n+var x = 1;\n".into(),
            }],
        }];
        let response = r#"[{"file_path":"a.js","line_number":4,"severity":"error","category":"style","title":"T","description":"D","suggestion":"Use const","evidence":"var x = 1;"}]"#;
        let findings = parse_and_verify_response(response, &chunk);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line_number, 5);
    }
}
