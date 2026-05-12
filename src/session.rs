//! Session state — tracks what's been reviewed within a REPL session.

use std::collections::HashMap;
use std::time::Instant;

use crate::review::models::ReviewResult;

/// Tracks review state within a single REPL session.
///
/// This is in-memory only — not persisted to SQLite.
/// A new session starts fresh each time the REPL launches.
#[derive(Debug, Default)]
pub struct SessionState {
    /// Files that have been reviewed, keyed by path.
    /// Value is a hash of the diff content at review time.
    reviewed_files: HashMap<String, u64>,

    /// The most recent review result.
    last_review: Option<ReviewResult>,

    /// When the last review was run.
    last_review_at: Option<Instant>,
}

impl SessionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that files were reviewed with the given diff hashes.
    pub fn record_review(&mut self, result: ReviewResult, diff_hashes: HashMap<String, u64>) {
        for (path, hash) in diff_hashes {
            self.reviewed_files.insert(path, hash);
        }
        self.last_review = Some(result);
        self.last_review_at = Some(Instant::now());
    }

    /// Check if a file has been reviewed with the current diff content.
    /// Returns true if the file was reviewed and the diff hash matches.
    pub fn is_current(&self, path: &str, current_hash: u64) -> bool {
        self.reviewed_files
            .get(path)
            .is_some_and(|&h| h == current_hash)
    }

    /// Get paths of files that have changed since last review.
    /// Returns all paths if nothing has been reviewed yet.
    pub fn changed_since_review<'a>(
        &self,
        current_hashes: &'a HashMap<String, u64>,
    ) -> Vec<&'a str> {
        current_hashes
            .iter()
            .filter(|(path, hash)| !self.is_current(path, **hash))
            .map(|(path, _)| path.as_str())
            .collect()
    }

    /// Number of files reviewed in this session.
    pub fn reviewed_count(&self) -> usize {
        self.reviewed_files.len()
    }

    /// The most recent review result.
    pub fn last_review(&self) -> Option<&ReviewResult> {
        self.last_review.as_ref()
    }

    /// Whether any review has been run in this session.
    pub fn has_reviewed(&self) -> bool {
        self.last_review.is_some()
    }

    /// Clear all session state (for re-review from scratch).
    pub fn clear(&mut self) {
        self.reviewed_files.clear();
        self.last_review = None;
        self.last_review_at = None;
    }
}

/// Active output format for the REPL session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormatChoice {
    Terminal,
    Json,
    Markdown,
    Annotations,
}

impl std::fmt::Display for OutputFormatChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormatChoice::Terminal => write!(f, "terminal"),
            OutputFormatChoice::Json => write!(f, "json"),
            OutputFormatChoice::Markdown => write!(f, "markdown"),
            OutputFormatChoice::Annotations => write!(f, "annotations"),
        }
    }
}

impl OutputFormatChoice {
    pub fn all() -> &'static [OutputFormatChoice] {
        &[
            OutputFormatChoice::Terminal,
            OutputFormatChoice::Json,
            OutputFormatChoice::Markdown,
            OutputFormatChoice::Annotations,
        ]
    }
}

/// Compute a simple hash of diff content for change detection.
pub fn hash_diff_content(content: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::models::{Category, ReviewFinding, ReviewResult, Severity};
    use std::time::Duration;

    fn sample_result() -> ReviewResult {
        ReviewResult {
            findings: vec![ReviewFinding {
                file_path: "a.rs".to_string(),
                line_number: 1,
                end_line: None,
                severity: Severity::Warning,
                category: Category::Style,
                title: "Test".to_string(),
                description: "Test".to_string(),
                suggestion: "Fix".to_string(),
            }],
            files_reviewed: 1,
            model_used: "test".to_string(),
            duration: Duration::from_secs(1),
            rules_applied: 0,
            languages_detected: vec![],
            has_custom_config: false,
            agents_ran: vec![],
        }
    }

    #[test]
    fn new_session_is_empty() {
        let session = SessionState::new();
        assert_eq!(session.reviewed_count(), 0);
        assert!(!session.has_reviewed());
        assert!(session.last_review().is_none());
    }

    #[test]
    fn record_review_tracks_files() {
        let mut session = SessionState::new();
        let hashes = HashMap::from([
            ("a.rs".to_string(), 12345u64),
            ("b.rs".to_string(), 67890u64),
        ]);

        session.record_review(sample_result(), hashes);

        assert_eq!(session.reviewed_count(), 2);
        assert!(session.has_reviewed());
        assert!(session.last_review().is_some());
    }

    #[test]
    fn is_current_matches_hash() {
        let mut session = SessionState::new();
        let hashes = HashMap::from([("a.rs".to_string(), 12345u64)]);
        session.record_review(sample_result(), hashes);

        assert!(session.is_current("a.rs", 12345));
        assert!(!session.is_current("a.rs", 99999)); // different hash
        assert!(!session.is_current("b.rs", 12345)); // different file
    }

    #[test]
    fn changed_since_review_detects_new_files() {
        let mut session = SessionState::new();
        let hashes = HashMap::from([("a.rs".to_string(), 100u64)]);
        session.record_review(sample_result(), hashes);

        let current = HashMap::from([
            ("a.rs".to_string(), 100u64), // unchanged
            ("b.rs".to_string(), 200u64), // new file
        ]);

        let changed = session.changed_since_review(&current);
        assert_eq!(changed.len(), 1);
        assert!(changed.contains(&"b.rs"));
    }

    #[test]
    fn changed_since_review_detects_modified_files() {
        let mut session = SessionState::new();
        let hashes = HashMap::from([("a.rs".to_string(), 100u64)]);
        session.record_review(sample_result(), hashes);

        let current = HashMap::from([
            ("a.rs".to_string(), 999u64), // modified since review
        ]);

        let changed = session.changed_since_review(&current);
        assert_eq!(changed.len(), 1);
        assert!(changed.contains(&"a.rs"));
    }

    #[test]
    fn changed_since_review_empty_when_nothing_changed() {
        let mut session = SessionState::new();
        let hashes = HashMap::from([("a.rs".to_string(), 100u64)]);
        session.record_review(sample_result(), hashes);

        let current = HashMap::from([("a.rs".to_string(), 100u64)]);

        let changed = session.changed_since_review(&current);
        assert!(changed.is_empty());
    }

    #[test]
    fn changed_since_review_all_when_no_prior_review() {
        let session = SessionState::new();
        let current = HashMap::from([("a.rs".to_string(), 100u64), ("b.rs".to_string(), 200u64)]);

        let changed = session.changed_since_review(&current);
        assert_eq!(changed.len(), 2);
    }

    #[test]
    fn clear_resets_state() {
        let mut session = SessionState::new();
        let hashes = HashMap::from([("a.rs".to_string(), 100u64)]);
        session.record_review(sample_result(), hashes);

        session.clear();

        assert_eq!(session.reviewed_count(), 0);
        assert!(!session.has_reviewed());
    }

    #[test]
    fn hash_diff_content_deterministic() {
        let h1 = hash_diff_content("+fn new() {}");
        let h2 = hash_diff_content("+fn new() {}");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_diff_content_different_for_different_content() {
        let h1 = hash_diff_content("+fn a() {}");
        let h2 = hash_diff_content("+fn b() {}");
        assert_ne!(h1, h2);
    }

    #[test]
    fn output_format_display() {
        assert_eq!(OutputFormatChoice::Terminal.to_string(), "terminal");
        assert_eq!(OutputFormatChoice::Json.to_string(), "json");
        assert_eq!(OutputFormatChoice::Markdown.to_string(), "markdown");
        assert_eq!(OutputFormatChoice::Annotations.to_string(), "annotations");
    }

    #[test]
    fn output_format_all_variants() {
        let all = OutputFormatChoice::all();
        assert_eq!(all.len(), 4);
    }
}
