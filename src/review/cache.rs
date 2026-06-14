//! Per-file finding cache: skip re-reviewing files that haven't changed.
//!
//! The dev loop is review → fix one file → re-review, but a naive review re-sends
//! every file through every agent each time. This cache stores the findings for
//! one file under one agent, keyed by the file's content plus the exact review
//! parameters (agent, rules, model, prompt version). On re-review an unchanged
//! file is served from the cache and never re-sent to the LLM — including the
//! common case of a *clean* file, whose empty result is cached as a hit so it
//! costs zero on the next pass.
//!
//! Only ever a speed optimisation: a miss (or a disabled cache) falls straight
//! through to a normal LLM review, so findings are never fabricated by the cache.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use rusqlite::Connection;

use super::models::ReviewFinding;
use crate::language::rules::Rule;

/// Bump this whenever an agent's prompt or the finding schema changes — the
/// model would now produce different output, so every cached entry must be
/// considered stale. (Rules and model are already part of the key.)
const PROMPT_VERSION: u32 = 1;

/// A cache of per-file review findings.
pub trait FindingCache {
    /// Findings for this key: `Some` (possibly empty) on a hit, `None` on miss.
    /// An empty `Vec` is a real hit — a file that reviewed clean.
    fn get(&self, key: &str) -> Option<Vec<ReviewFinding>>;

    /// Store the findings for one file under its key (empty = clean file).
    fn put(&self, key: &str, findings: &[ReviewFinding]);
}

/// The content of a file diff as sent to the LLM (all hunks concatenated).
pub fn diff_content(hunks: &[String]) -> String {
    hunks.join("\n")
}

/// Cache key for one file under one agent. Any change to the file content, the
/// agent, its rule set (id+severity, order-independent), the model, or the
/// prompt version produces a different key, so stale entries are never served.
pub fn cache_key(agent_name: &str, rules: &[Rule], model: &str, content: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    PROMPT_VERSION.hash(&mut hasher);
    agent_name.hash(&mut hasher);
    model.hash(&mut hasher);

    // Rule identity is id + severity, sorted so ordering doesn't matter.
    let mut rule_sig: Vec<String> = rules
        .iter()
        .map(|r| format!("{}={}", r.id, r.severity))
        .collect();
    rule_sig.sort();
    for r in &rule_sig {
        r.hash(&mut hasher);
    }

    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// In-memory cache for tests.
#[derive(Default)]
pub struct MemoryCache {
    store: RefCell<HashMap<String, Vec<ReviewFinding>>>,
}

impl FindingCache for MemoryCache {
    fn get(&self, key: &str) -> Option<Vec<ReviewFinding>> {
        self.store.borrow().get(key).cloned()
    }

    fn put(&self, key: &str, findings: &[ReviewFinding]) {
        self.store
            .borrow_mut()
            .insert(key.to_string(), findings.to_vec());
    }
}

/// SQLite-backed cache living in the project's `state.db`.
pub struct SqliteCache<'a> {
    conn: &'a Connection,
}

impl<'a> SqliteCache<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }
}

impl FindingCache for SqliteCache<'_> {
    fn get(&self, key: &str) -> Option<Vec<ReviewFinding>> {
        let json: String = self
            .conn
            .query_row(
                "SELECT findings_json FROM file_review_cache WHERE cache_key = ?1",
                [key],
                |row| row.get(0),
            )
            .ok()?;
        // A present row that fails to parse is treated as a miss (safe).
        serde_json::from_str(&json).ok()
    }

    fn put(&self, key: &str, findings: &[ReviewFinding]) {
        if let Ok(json) = serde_json::to_string(findings) {
            // Best-effort: a cache write must never fail a review.
            let _ = self.conn.execute(
                "INSERT OR REPLACE INTO file_review_cache (cache_key, findings_json) VALUES (?1, ?2)",
                rusqlite::params![key, json],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::Language;
    use crate::review::models::{Category, Severity};

    fn rule(id: &str, sev: Severity) -> Rule {
        Rule {
            id: id.into(),
            language: Language::Php,
            severity: sev,
            description: "d".into(),
            enabled: true,
        }
    }

    fn finding(title: &str) -> ReviewFinding {
        ReviewFinding {
            file_path: "a.php".into(),
            line_number: 1,
            end_line: None,
            severity: Severity::Warning,
            category: Category::Style,
            title: title.into(),
            description: "d".into(),
            suggestion: "s".into(),
        }
    }

    #[test]
    fn key_is_stable_for_same_inputs() {
        let rules = [rule("php-no-eval", Severity::Error)];
        let a = cache_key("Security", &rules, "m", "code");
        let b = cache_key("Security", &rules, "m", "code");
        assert_eq!(a, b);
    }

    #[test]
    fn key_changes_when_any_input_changes() {
        let rules = [rule("php-no-eval", Severity::Error)];
        let base = cache_key("Security", &rules, "m", "code");
        assert_ne!(base, cache_key("Bug Detection", &rules, "m", "code")); // agent
        assert_ne!(base, cache_key("Security", &rules, "other", "code")); // model
        assert_ne!(base, cache_key("Security", &rules, "m", "changed")); // content
        let rules2 = [rule("php-no-eval", Severity::Warning)];
        assert_ne!(base, cache_key("Security", &rules2, "m", "code")); // rule severity
    }

    #[test]
    fn key_is_rule_order_independent() {
        let a = [rule("a", Severity::Error), rule("b", Severity::Warning)];
        let b = [rule("b", Severity::Warning), rule("a", Severity::Error)];
        assert_eq!(
            cache_key("S", &a, "m", "code"),
            cache_key("S", &b, "m", "code")
        );
    }

    #[test]
    fn memory_cache_miss_then_hit() {
        let cache = MemoryCache::default();
        assert!(cache.get("k").is_none()); // miss

        cache.put("k", &[finding("X")]);
        let hit = cache.get("k").expect("hit");
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].title, "X");
    }

    #[test]
    fn empty_findings_is_a_hit_not_a_miss() {
        // A clean file caches as present-but-empty so it costs zero on re-review.
        let cache = MemoryCache::default();
        cache.put("clean", &[]);
        let hit = cache.get("clean");
        assert!(hit.is_some(), "clean file must be a hit");
        assert!(hit.unwrap().is_empty());
    }

    #[test]
    fn sqlite_cache_round_trips_including_empty() {
        let conn = crate::db::init_in_memory().unwrap();
        let cache = SqliteCache::new(&conn);

        assert!(cache.get("k").is_none()); // miss before write
        cache.put("k", &[finding("Y")]);
        assert_eq!(cache.get("k").unwrap()[0].title, "Y");

        cache.put("clean", &[]);
        assert!(cache.get("clean").is_some()); // empty is still a hit
        assert!(cache.get("clean").unwrap().is_empty());
    }
}
