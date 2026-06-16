//! Tier 4 — interpretation-hallucination gate via an LLM second pass.
//!
//! Tiers 1–3 are deterministic: they catch fabricated code, self-identical
//! fixes, and false "API does not exist" claims by checking the model's output
//! against the source on disk. What they cannot catch is a finding that quotes
//! *real* code while *misjudging* it — a "missing null check" that sits on the
//! next line, a "missing try/catch" that is actually present, a `||` guard read
//! out of evaluation order. This pass re-asks the model one focused question per
//! finding ("is this specific defect really present in this code?") and drops
//! only the findings it positively judges invalid.
//!
//! Opt-in (`--verify` / `verify: true`) because it adds one LLM call per
//! in-scope finding. Scoped to bug/security findings — interpretation
//! hallucinations cluster there; style/linter findings rarely misfire this way.
//! Like every other gate it only ever drops on positive proof: an errored,
//! timed-out, or unparseable verdict keeps the finding.

use crate::git::FileDiff;
use crate::onboarding::steps::OllamaClient;
use crate::review::chunking::ContextBudget;
use crate::review::models::{Category, ReviewFinding};

/// A judge's verdict on a single finding.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// `true` = the finding correctly identifies a real problem in this code;
    /// `false` = it misreads the code or the defect is not actually present.
    pub valid: bool,
    /// One-sentence justification from the judge (may be empty).
    pub reason: String,
}

/// System prompt for the verify pass. One job: judge whether the specific
/// defect a finding claims is really PRESENT in the code shown — not whether
/// it's worth fixing. Drops are reserved for genuine misreads, so the pass
/// can't delete a real-but-minor finding (importance is not the judge's call).
const VERDICT_SYSTEM_PROMPT: &str = "You are verifying ONE code-review finding for correctness against the actual code shown. \
Your ONLY job is to decide whether the specific defect the finding describes is really PRESENT \
in this code. You are NOT judging whether the fix is worthwhile, idiomatic, or important, and \
NOT whether the framework makes it low-risk — only whether the problem actually exists. Many \
findings misread correct code: a \"missing\" null check that exists on the next line, a \
\"missing\" try/catch that is actually present a few lines down, a `||` or `&&` guard read out \
of evaluation order. Read the cited line together with the surrounding lines before judging.\n\n\
Respond with ONLY a JSON object and nothing else:\n\
{\"valid\": true, \"reason\": \"one short sentence\"}\n\
- valid=true: the defect the finding describes is really present — EVEN IF it is minor, \
low-risk, or the framework partly mitigates it. Importance is not your call; presence is.\n\
- valid=false: ONLY when the code already does what the finding says is missing, or the finding \
otherwise misreads the code. If you are unsure, answer true.";

/// Shape of the model's JSON verdict, before validation.
#[derive(serde::Deserialize)]
struct RawVerdict {
    valid: bool,
    #[serde(default)]
    reason: String,
}

/// Parse a `{"valid": bool, "reason": string}` verdict, tolerant of any prose
/// or `<think>` banner the model wraps around it. Returns `None` when no usable
/// verdict can be read — callers treat that as "uncertain → keep the finding".
pub fn parse_verdict(response: &str) -> Option<Verdict> {
    let trimmed = response.trim();

    // Fast path: the whole response is the JSON object.
    if let Ok(v) = serde_json::from_str::<RawVerdict>(trimmed) {
        return Some(Verdict {
            valid: v.valid,
            reason: v.reason,
        });
    }

    // Tolerant path: pull the outermost {...} out of surrounding prose.
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str::<RawVerdict>(&trimmed[start..=end])
        .ok()
        .map(|v| Verdict {
            valid: v.valid,
            reason: v.reason,
        })
}

/// Whether a finding is in scope for the verify pass. Interpretation
/// hallucinations cluster in bug/security; style and linter findings rarely
/// misfire this way, so verifying them only burns LLM calls.
pub fn is_verifiable(finding: &ReviewFinding) -> bool {
    matches!(finding.category, Category::Bug | Category::Security)
}

/// Build the focused single-claim verdict prompt: the file's numbered code
/// (identical to what the original agent saw) plus the one finding to judge.
pub fn build_verdict_prompt(finding: &ReviewFinding, diff: &FileDiff) -> (String, String) {
    let mut user = String::from("Here is the code under review, with new-file line numbers:\n\n");
    user.push_str(&crate::review::prompt::numbered_diff_block(diff));
    user.push_str("\n=== Finding to verify ===\n");
    user.push_str(&format!("Line: {}\n", finding.line_number));
    user.push_str(&format!("Severity: {}\n", finding.severity));
    user.push_str(&format!("Category: {}\n", finding.category));
    user.push_str(&format!("Title: {}\n", finding.title));
    user.push_str(&format!("Description: {}\n", finding.description));
    user.push_str(&format!("Suggested fix: {}\n", finding.suggestion));
    user.push_str(
        "\nIs this finding correct for the code above? Respond with the JSON verdict only.",
    );

    (VERDICT_SYSTEM_PROMPT.to_string(), user)
}

/// Run the verify pass over `findings`, dropping only those an in-scope verdict
/// positively rejects. Out-of-scope findings, findings whose file isn't in the
/// reviewed diffs, and any errored/unparseable verdict are kept untouched.
pub async fn verify_findings(
    findings: Vec<ReviewFinding>,
    diffs: &[FileDiff],
    ollama: &dyn OllamaClient,
    model: &str,
    budget: ContextBudget,
) -> Vec<ReviewFinding> {
    let mut kept = Vec::with_capacity(findings.len());

    for finding in findings {
        // Out of scope (style/linter/etc.) — never verified.
        if !is_verifiable(&finding) {
            kept.push(finding);
            continue;
        }

        // Can't show the judge the code this finding refers to — keep it rather
        // than judge blind.
        let Some(diff) = diffs.iter().find(|d| d.path == finding.file_path) else {
            kept.push(finding);
            continue;
        };

        let (system, user) = build_verdict_prompt(&finding, diff);
        match ollama
            .chat_sized(model, &system, &user, budget.num_ctx, budget.think)
            .await
        {
            // The only case that drops: a parseable verdict that says invalid.
            Ok(resp) => match parse_verdict(&resp) {
                Some(v) if !v.valid => {
                    crate::logging::info(format!(
                        "verify pass dropped finding '{}' ({}:{}): {}",
                        finding.title, finding.file_path, finding.line_number, v.reason
                    ));
                }
                // valid=true, or no usable verdict — keep on uncertainty.
                _ => kept.push(finding),
            },
            Err(e) => {
                crate::logging::warn(format!(
                    "verify pass kept finding '{}' (verifier error: {e})",
                    finding.title
                ));
                kept.push(finding);
            }
        }
    }

    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::FileStatus;
    use crate::git::testutil::make_file_diff;
    use crate::review::models::Severity;
    use crate::review::testutil::MockOllama;

    fn finding(category: Category, line: usize, title: &str) -> ReviewFinding {
        ReviewFinding {
            file_path: "src/Foo.php".into(),
            line_number: line,
            end_line: None,
            severity: Severity::Error,
            category,
            title: title.into(),
            description: "desc".into(),
            suggestion: "fix".into(),
        }
    }

    #[test]
    fn parse_verdict_reads_invalid() {
        let v = parse_verdict(r#"{"valid": false, "reason": "the null check is on the next line"}"#)
            .unwrap();
        assert!(!v.valid);
        assert!(v.reason.contains("next line"));
    }

    #[test]
    fn parse_verdict_reads_valid() {
        let v = parse_verdict(r#"{"valid": true, "reason": "real bug"}"#).unwrap();
        assert!(v.valid);
    }

    #[test]
    fn parse_verdict_tolerates_surrounding_prose() {
        let v = parse_verdict(
            "Sure! Here is my verdict:\n{\"valid\": false, \"reason\": \"x\"}\nHope that helps.",
        )
        .unwrap();
        assert!(!v.valid);
    }

    #[test]
    fn parse_verdict_missing_reason_is_ok() {
        let v = parse_verdict(r#"{"valid": false}"#).unwrap();
        assert!(!v.valid);
        assert_eq!(v.reason, "");
    }

    #[test]
    fn parse_verdict_garbage_is_none() {
        assert!(parse_verdict("not json at all").is_none());
        assert!(parse_verdict("").is_none());
    }

    #[test]
    fn is_verifiable_bug_and_security_only() {
        assert!(is_verifiable(&finding(Category::Bug, 1, "t")));
        assert!(is_verifiable(&finding(Category::Security, 1, "t")));
        assert!(!is_verifiable(&finding(Category::Style, 1, "t")));
        assert!(!is_verifiable(&finding(Category::BestPractice, 1, "t")));
        assert!(!is_verifiable(&finding(Category::Accessibility, 1, "t")));
        assert!(!is_verifiable(&finding(Category::Performance, 1, "t")));
        assert!(!is_verifiable(&finding(Category::Other("x".into()), 1, "t")));
    }

    #[test]
    fn build_verdict_prompt_includes_finding_and_numbered_code() {
        let diff = make_file_diff(
            "src/Foo.php",
            FileStatus::Added,
            "+$data = json_decode($body, TRUE);",
        );
        let f = finding(Category::Bug, 1, "Missing null check");
        let (system, user) = build_verdict_prompt(&f, &diff);
        assert!(system.contains("JSON"), "system asks for a JSON verdict");
        assert!(system.to_lowercase().contains("valid"));
        assert!(user.contains("Missing null check"), "finding title present");
        assert!(user.contains("json_decode"), "code present");
        assert!(user.contains("desc"), "description present");
    }

    #[test]
    fn verdict_prompt_judges_presence_not_importance() {
        // The pass must only drop genuine misreads, never "real but minor"
        // findings — so the system prompt has to say so explicitly.
        let diff = make_file_diff("src/Foo.php", FileStatus::Added, "+$x = 1;");
        let f = finding(Category::Bug, 1, "t");
        let (system, _) = build_verdict_prompt(&f, &diff);
        let s = system.to_lowercase();
        // valid=false is reserved for the code already doing what's "missing".
        assert!(s.contains("already does what the finding says is missing"));
        // importance is explicitly not grounds to drop.
        assert!(s.contains("importance is not your call"));
        // and the tie-breaker is keep.
        assert!(s.contains("if you are unsure, answer true"));
    }

    #[tokio::test]
    async fn invalid_finding_is_dropped() {
        let diff = make_file_diff(
            "src/Foo.php",
            FileStatus::Added,
            "+$data = json_decode($body, TRUE);",
        );
        let f = finding(Category::Bug, 1, "Missing null check");
        let ollama = MockOllama::with_response(r#"{"valid": false, "reason": "checked next line"}"#);
        let kept = verify_findings(vec![f], &[diff], &ollama, "m", ContextBudget::unlimited()).await;
        assert!(kept.is_empty(), "finding judged invalid must be dropped");
    }

    #[tokio::test]
    async fn valid_finding_is_kept() {
        let diff = make_file_diff("src/Foo.php", FileStatus::Added, "+eval($x);");
        let f = finding(Category::Security, 1, "Eval usage");
        let ollama = MockOllama::with_response(r#"{"valid": true, "reason": "genuine"}"#);
        let kept = verify_findings(vec![f], &[diff], &ollama, "m", ContextBudget::unlimited()).await;
        assert_eq!(kept.len(), 1);
    }

    #[tokio::test]
    async fn out_of_scope_finding_is_not_verified() {
        // A style finding must be kept WITHOUT an LLM call.
        let diff = make_file_diff("src/Foo.php", FileStatus::Added, "+$x=1;");
        let f = finding(Category::Style, 1, "Spacing");
        let ollama = MockOllama::with_response(r#"{"valid": false}"#);
        let kept = verify_findings(vec![f], &[diff], &ollama, "m", ContextBudget::unlimited()).await;
        assert_eq!(kept.len(), 1, "style finding kept");
        assert_eq!(ollama.call_count(), 0, "out-of-scope finding must not hit the LLM");
    }

    #[tokio::test]
    async fn unparseable_verdict_keeps_finding() {
        let diff = make_file_diff("src/Foo.php", FileStatus::Added, "+eval($x);");
        let f = finding(Category::Bug, 1, "Maybe bug");
        let ollama = MockOllama::with_response("I think this is fine, no JSON here");
        let kept = verify_findings(vec![f], &[diff], &ollama, "m", ContextBudget::unlimited()).await;
        assert_eq!(
            kept.len(),
            1,
            "unparseable verdict must not drop (keep on uncertainty)"
        );
    }

    #[tokio::test]
    async fn missing_diff_keeps_finding() {
        // No diff for the finding's file -> can't show the judge the code -> keep.
        let other = make_file_diff("src/Other.php", FileStatus::Added, "+$x=1;");
        let f = finding(Category::Bug, 1, "Bug in Foo");
        let ollama = MockOllama::with_response(r#"{"valid": false}"#);
        let kept = verify_findings(vec![f], &[other], &ollama, "m", ContextBudget::unlimited()).await;
        assert_eq!(kept.len(), 1);
        assert_eq!(ollama.call_count(), 0);
    }
}
