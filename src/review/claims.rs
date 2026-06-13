//! Deterministic suppression of "this API does not exist" hallucinations.
//!
//! Local models confidently assert that a framework method/property is missing
//! and that the code "will fatal error" — the single most dangerous review
//! output, because it pushes a developer to revert correct work. These claims
//! are *checkable*: the symbol either exists in the project/framework source on
//! disk or it doesn't. This module extracts the claimed symbol, looks it up in
//! a [`SourceIndex`] built from the real source, and drops the finding (with a
//! logged proof) when the symbol plainly exists.
//!
//! Born from the bcutd_heatmap triage: a 9B model claimed `setLoggerFactory()`,
//! `$configFactory`, `logger()`, and `$httpClient` were undeclared — every one
//! is defined in Drupal core or the module itself.
//!
//! Deliberately narrow: it only acts on existence claims, and only ever drops
//! on *positive* proof of existence. An unreadable or capped source tree means
//! "unknown" → the finding is kept. Suppression can never be a false drop.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::models::ReviewFinding;

/// What kind of symbol an existence claim is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// A method or free function (`foo()`).
    Method,
    /// An object property / field (`$foo`).
    Property,
}

/// A parsed "symbol X does not exist" claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistenceClaim {
    pub symbol: String,
    pub kind: SymbolKind,
}

/// Phrases that signal a claim of non-existence. Kept tight on purpose —
/// precision over recall, since a false match here risks dropping a real
/// finding.
const NEGATION_PHRASES: &[&str] = &[
    "does not have",
    "does not exist",
    "doesn't exist",
    "not found",
    "undefined method",
    "undefined property",
    "never declared",
    "non-existent",
    "nonexistent",
    "does not declare",
    "not declared",
];

const VISIBILITY_KEYWORDS: &[&str] = &["public", "protected", "private", "var", "readonly"];

/// Detect a non-existence claim in a finding's prose and extract the symbol.
///
/// Returns `None` for anything that is not an existence claim (SQL injection,
/// style nits, etc.) — those are out of scope for this gate.
pub fn extract_existence_claim(description: &str, _suggestion: &str) -> Option<ExistenceClaim> {
    let lower = description.to_lowercase();
    if !NEGATION_PHRASES.iter().any(|p| lower.contains(p)) {
        return None;
    }

    let tokens = backtick_tokens(description);
    let methods: Vec<String> = tokens.iter().filter_map(|t| as_method(t)).collect();
    let properties: Vec<(bool, String)> = tokens.iter().filter_map(|t| as_property(t)).collect();

    // Prefer the property whose access is `$this->X` (the field), not a local
    // `$x` param that happens to be quoted alongside it.
    let pick_property = || {
        properties
            .iter()
            .find(|(is_this, _)| *is_this)
            .or_else(|| properties.first())
            .map(|(_, name)| name.clone())
    };

    let wants_property = lower.contains("property") || lower.contains("declare");

    if wants_property && let Some(symbol) = pick_property() {
        return Some(ExistenceClaim { symbol, kind: SymbolKind::Property });
    }
    if let Some(symbol) = methods.first() {
        return Some(ExistenceClaim { symbol: symbol.clone(), kind: SymbolKind::Method });
    }
    if let Some(symbol) = pick_property() {
        return Some(ExistenceClaim { symbol, kind: SymbolKind::Property });
    }
    None
}

/// Text segments between backticks.
fn backtick_tokens(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn is_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !s.chars().next().unwrap().is_ascii_digit()
}

/// A method token like `setLoggerFactory()` or `Foo::bar()` → bare name.
fn as_method(token: &str) -> Option<String> {
    let before_paren = token.split('(').next()?;
    let name = before_paren.rsplit("->").next()?.rsplit("::").next()?.trim_start_matches('$');
    (token.contains('(') && is_identifier(name)).then(|| name.to_string())
}

/// A property token like `$this->configFactory` or `$httpClient` → (is_this, name).
fn as_property(token: &str) -> Option<(bool, String)> {
    if !token.starts_with('$') || token.contains('(') {
        return None;
    }
    let is_this = token.starts_with("$this->");
    let name = token.rsplit("->").next()?.trim_start_matches('$');
    is_identifier(name).then(|| (is_this, name.to_string()))
}

/// All `function <name>` definitions in a source blob, with 1-based line.
fn defined_functions(source: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (i, line) in source.lines().enumerate() {
        for name in keyword_followed_by_identifier(line, "function") {
            out.push((name, i + 1));
        }
    }
    out
}

/// All declared properties (`protected $x`, promoted `private T $x`) in a
/// source blob, with 1-based line.
fn defined_properties(source: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (i, line) in source.lines().enumerate() {
        for kw in VISIBILITY_KEYWORDS {
            for start in word_positions(line, kw) {
                // First `$identifier` after the visibility keyword on this line.
                if let Some(dollar) = line[start..].find('$') {
                    let rest = &line[start + dollar + 1..];
                    let name: String =
                        rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                    if is_identifier(&name) {
                        out.push((name, i + 1));
                    }
                }
            }
        }
    }
    out
}

/// Identifiers appearing immediately after a whole-word `keyword` on a line
/// (e.g. `function foo` → `foo`).
fn keyword_followed_by_identifier(line: &str, keyword: &str) -> Vec<String> {
    let mut names = Vec::new();
    for start in word_positions(line, keyword) {
        let after = line[start + keyword.len()..].trim_start();
        let name: String =
            after.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if is_identifier(&name) {
            names.push(name);
        }
    }
    names
}

/// Byte offsets where `word` appears as a whole word in `line`.
fn word_positions(line: &str, word: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut from = 0;
    while let Some(rel) = line[from..].find(word) {
        let at = from + rel;
        let before_ok = at == 0 || !line.as_bytes()[at - 1].is_ascii_alphanumeric();
        let after = at + word.len();
        let after_ok =
            after >= line.len() || !line.as_bytes()[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            positions.push(at);
        }
        from = at + word.len();
    }
    positions
}

/// Whether any finding makes an existence claim — a cheap gate so the engine
/// only pays for a source scan when it can actually help.
pub fn any_existence_claim(findings: &[ReviewFinding]) -> bool {
    findings
        .iter()
        .any(|f| extract_existence_claim(&f.description, &f.suggestion).is_some())
}

/// An index of symbols defined across a set of source files, mapping each name
/// to a human-readable `path:line` proof of where it was defined.
pub struct SourceIndex {
    functions: HashMap<String, String>,
    properties: HashMap<String, String>,
}

impl SourceIndex {
    /// Build an index from in-memory `(path, content)` pairs. Used by tests and
    /// by [`SourceIndex::build`] after it reads files.
    pub fn from_sources(sources: &[(&str, &str)]) -> Self {
        let mut functions = HashMap::new();
        let mut properties = HashMap::new();
        for (path, content) in sources {
            for (name, line) in defined_functions(content) {
                functions.entry(name).or_insert_with(|| format!("{path}:{line}"));
            }
            for (name, line) in defined_properties(content) {
                properties.entry(name).or_insert_with(|| format!("{path}:{line}"));
            }
        }
        Self { functions, properties }
    }

    /// Proof location if the claimed symbol is defined, else `None`.
    fn proof(&self, claim: &ExistenceClaim) -> Option<&str> {
        match claim.kind {
            SymbolKind::Method => self.functions.get(&claim.symbol).map(String::as_str),
            // A promoted constructor property is both a param and a field, so a
            // property claim is satisfied by either table.
            SymbolKind::Property => self
                .properties
                .get(&claim.symbol)
                .or_else(|| self.functions.get(&claim.symbol))
                .map(String::as_str),
        }
    }
}

/// Drop findings that claim a symbol is missing when that symbol is in fact
/// defined in the indexed source. Each suppression is logged with its proof.
pub fn verify_existence_claims(
    findings: Vec<ReviewFinding>,
    index: &SourceIndex,
) -> Vec<ReviewFinding> {
    findings
        .into_iter()
        .filter(|finding| {
            let Some(claim) = extract_existence_claim(&finding.description, &finding.suggestion)
            else {
                return true;
            };
            match index.proof(&claim) {
                Some(location) => {
                    crate::logging::warn(format!(
                        "discarded hallucinated finding \"{}\" ({}): claims {:?} `{}` is missing, but it is defined at {}",
                        finding.title, finding.file_path, claim.kind, claim.symbol, location
                    ));
                    false
                }
                None => true,
            }
        })
        .collect()
}

/// Directories never worth indexing. Note we deliberately do NOT skip
/// `vendor/` or `core/` — that is exactly where framework definitions live.
const SKIP_DIRS: &[&str] = &[".git", "node_modules", "dist", "build", ".ddev", ".lando"];

/// Extensions whose definitions we index (PHP + Drupal + JS).
const INDEXED_EXTS: &[&str] =
    &["php", "inc", "module", "install", "theme", "profile", "js", "mjs", "cjs"];

/// Stop reading once this many files or bytes have been scanned. Hitting the
/// cap only makes the index less complete (fewer suppressions), never wrong.
const MAX_FILES: usize = 60_000;
const MAX_BYTES: u64 = 96 * 1024 * 1024;

impl SourceIndex {
    /// Build an index by reading indexed source files under the given roots.
    /// Bounded by [`MAX_FILES`]/[`MAX_BYTES`]; best-effort and never fails.
    pub fn build(roots: &[PathBuf]) -> Self {
        let mut functions = HashMap::new();
        let mut properties = HashMap::new();
        let mut budget = Budget { files: 0, bytes: 0 };

        for root in roots {
            walk_index(root, &mut functions, &mut properties, &mut budget);
        }

        Self { functions, properties }
    }
}

struct Budget {
    files: usize,
    bytes: u64,
}

impl Budget {
    fn exhausted(&self) -> bool {
        self.files >= MAX_FILES || self.bytes >= MAX_BYTES
    }
}

fn walk_index(
    dir: &Path,
    functions: &mut HashMap<String, String>,
    properties: &mut HashMap<String, String>,
    budget: &mut Budget,
) {
    if budget.exhausted() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        if budget.exhausted() {
            return;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            walk_index(&path, functions, properties, budget);
        } else if file_type.is_file() && has_indexed_ext(&path) {
            let Ok(meta) = entry.metadata() else { continue };
            budget.files += 1;
            budget.bytes += meta.len();
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let loc = path.display().to_string();
            for (name, line) in defined_functions(&content) {
                functions.entry(name).or_insert_with(|| format!("{loc}:{line}"));
            }
            for (name, line) in defined_properties(&content) {
                properties.entry(name).or_insert_with(|| format!("{loc}:{line}"));
            }
        }
    }
}

fn has_indexed_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| INDEXED_EXTS.contains(&ext))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::models::{Category, ReviewFinding, Severity};

    fn finding(description: &str, suggestion: &str) -> ReviewFinding {
        ReviewFinding {
            file_path: "src/Controller/Foo.php".into(),
            line_number: 42,
            end_line: None,
            severity: Severity::Error,
            category: Category::Bug,
            title: "T".into(),
            description: description.into(),
            suggestion: suggestion.into(),
        }
    }

    // ── extract_existence_claim: the six triage phrasings ──

    #[test]
    fn extracts_method_does_not_have() {
        let c = extract_existence_claim(
            "The `ControllerBase` class does not have a `setLoggerFactory()` method.",
            "Remove the call to setLoggerFactory().",
        )
        .unwrap();
        assert_eq!(c.symbol, "setLoggerFactory");
        assert_eq!(c.kind, SymbolKind::Method);
    }

    #[test]
    fn extracts_method_not_found() {
        let c = extract_existence_claim("Method not found: `setLoggerFactory()`", "").unwrap();
        assert_eq!(c.symbol, "setLoggerFactory");
        assert_eq!(c.kind, SymbolKind::Method);
    }

    #[test]
    fn extracts_logger_method() {
        let c = extract_existence_claim(
            "The `ConfigFormBase` class does not have a `logger()` method.",
            "",
        )
        .unwrap();
        assert_eq!(c.symbol, "logger");
        assert_eq!(c.kind, SymbolKind::Method);
    }

    #[test]
    fn extracts_property_never_declared_prefers_this_access() {
        // `$config_factory` is a local param; `$this->configFactory` is the
        // property the claim is actually about. The property name must win.
        let c = extract_existence_claim(
            "The code attempts to assign `$config_factory` to `$this->configFactory`, but this property is never declared in the class.",
            "",
        )
        .unwrap();
        assert_eq!(c.symbol, "configFactory");
        assert_eq!(c.kind, SymbolKind::Property);
    }

    #[test]
    fn extracts_property_does_not_declare() {
        let c = extract_existence_claim(
            "The class uses `$this->httpClient` but does not declare it as a constructor parameter.",
            "",
        )
        .unwrap();
        assert_eq!(c.symbol, "httpClient");
        assert_eq!(c.kind, SymbolKind::Property);
    }

    #[test]
    fn non_existence_findings_return_none() {
        assert!(extract_existence_claim("SQL injection via unvalidated `$path` parameter.", "Validate it.").is_none());
        assert!(extract_existence_claim("Use const instead of var.", "const x = 1;").is_none());
        assert!(extract_existence_claim("Potential XSS in escHtml output.", "Escape quotes.").is_none());
    }

    // ── definition scanning ──

    #[test]
    fn scans_php_method_definition() {
        let src = "<?php\nclass X {\n  public function setLoggerFactory($f) {\n  }\n}";
        let fns = defined_functions(src);
        assert!(fns.iter().any(|(n, line)| n == "setLoggerFactory" && *line == 3));
    }

    #[test]
    fn scans_js_function_definition() {
        let fns = defined_functions("(function () {\n  function escHtml(str) { return str; }\n})();");
        assert!(fns.iter().any(|(n, _)| n == "escHtml"));
    }

    #[test]
    fn scans_plain_and_promoted_properties() {
        let src = "class X {\n  protected $configFactory;\n  protected Connection $database;\n  public function __construct(\n    private readonly ClientInterface $httpClient,\n  ) {}\n}";
        let props: Vec<String> = defined_properties(src).into_iter().map(|(n, _)| n).collect();
        assert!(props.contains(&"configFactory".to_string()));
        assert!(props.contains(&"database".to_string()));
        assert!(props.contains(&"httpClient".to_string()));
    }

    // ── end-to-end suppression ──

    fn core_index() -> SourceIndex {
        SourceIndex::from_sources(&[
            ("core/.../LoggerChannelTrait.php", "  public function setLoggerFactory(LoggerChannelFactoryInterface $f) {\n  }"),
            ("core/.../FormBase.php", "  protected function logger($channel) {\n  }"),
            ("core/.../ControllerBase.php", "  protected $configFactory;"),
            ("src/Service/Ga4DataService.php", "  public function __construct(\n    private readonly ClientInterface $httpClient,\n  ) {}"),
        ])
    }

    #[test]
    fn suppresses_method_claim_when_symbol_exists() {
        let findings = vec![finding(
            "The `ControllerBase` class does not have a `setLoggerFactory()` method. This will cause a fatal error.",
            "Remove the call.",
        )];
        let kept = verify_existence_claims(findings, &core_index());
        assert!(kept.is_empty(), "existing method's 'missing' claim must be dropped");
    }

    #[test]
    fn suppresses_property_claim_when_symbol_exists() {
        let findings = vec![finding(
            "Assignment to `$this->configFactory`, but this property is never declared.",
            "Declare the property.",
        )];
        assert!(verify_existence_claims(findings, &core_index()).is_empty());
    }

    #[test]
    fn keeps_existence_claim_when_symbol_genuinely_absent() {
        let findings = vec![finding(
            "The class does not have a `totallyMadeUpMethod()` method.",
            "",
        )];
        let kept = verify_existence_claims(findings, &core_index());
        assert_eq!(kept.len(), 1, "a truly-absent symbol's claim must survive");
    }

    #[test]
    fn never_touches_non_existence_findings() {
        let findings = vec![
            finding("SQL injection via `$path`.", "Validate."),
            finding("Use const instead of var.", "const x;"),
        ];
        let kept = verify_existence_claims(findings, &core_index());
        assert_eq!(kept.len(), 2, "non-existence findings are out of scope and must pass through");
    }
}
