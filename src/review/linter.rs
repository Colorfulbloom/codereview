//! ESLint + Stylelint as deterministic finding sources for JS/CSS — the JS/CSS
//! analog of [`phpcs`](super::phpcs).
//!
//! JS/CSS review was the last category with no deterministic backstop: it was
//! 100% LLM, and a local 9B under-reports (it missed 62 `var` declarations and
//! 8 `innerHTML` assignments in a real module). ESLint and Stylelint are to
//! JS/CSS what phpcs is to PHP — they cannot miss a `var` or a `!important` the
//! way a model does. So the mechanical JS/CSS rules are sourced from the
//! linters, and the LLM is left to the semantic findings (XSS logic, error
//! handling) a linter can't make.
//!
//! Both tools share a shape: run a binary, get a JSON array of per-file
//! results, map to [`ReviewFinding`]s. The differences (binary name, output
//! schema, which LLM rules they supersede, config-file names) are captured by
//! [`LinterKind`] so the runner/parse/merge plumbing is written once.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::models::{Category, ReviewFinding, Severity};
use crate::git::FileDiff;

/// Which linter — selects the binary, output parser, file extensions, config
/// names, and superseded LLM rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinterKind {
    Eslint,
    Stylelint,
}

/// LLM rule IDs ESLint supersedes — mechanical JS rules a linter enforces
/// deterministically. The model keeps the semantic ones (`js-xss-prevention`,
/// `js-error-handling`).
pub const SUPERSEDED_BY_ESLINT: &[&str] = &[
    "js-no-var",
    "js-strict-equality",
    "js-no-console-log",
    "js-no-unused-vars",
];

/// LLM rule IDs Stylelint supersedes — the stylistic CSS rules.
pub const SUPERSEDED_BY_STYLELINT: &[&str] = &[
    "css-color-format",
    "css-no-duplicate-selectors",
    "css-no-important",
    "css-max-nesting",
];

impl LinterKind {
    /// Human label shown as review progress (like an agent name).
    pub fn label(&self) -> &'static str {
        match self {
            LinterKind::Eslint => "ESLint",
            LinterKind::Stylelint => "Stylelint",
        }
    }

    /// Default binary name (used when no command override and no local install).
    pub fn binary(&self) -> &'static str {
        match self {
            LinterKind::Eslint => "eslint",
            LinterKind::Stylelint => "stylelint",
        }
    }

    /// File extensions this linter handles.
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            LinterKind::Eslint => &["js", "jsx", "mjs", "cjs"],
            LinterKind::Stylelint => &["css", "scss", "less"],
        }
    }

    /// LLM rule IDs this linter supersedes.
    pub fn superseded(&self) -> &'static [&'static str] {
        match self {
            LinterKind::Eslint => SUPERSEDED_BY_ESLINT,
            LinterKind::Stylelint => SUPERSEDED_BY_STYLELINT,
        }
    }
}

/// Whether a rule id is superseded by ESLint or Stylelint (dropped from the LLM
/// when the owning linter is active).
pub fn is_superseded(rule_id: &str) -> bool {
    SUPERSEDED_BY_ESLINT.contains(&rule_id) || SUPERSEDED_BY_STYLELINT.contains(&rule_id)
}

/// Runs a linter. A trait so tests use a mock and never need node installed.
pub trait LinterRunner {
    /// Run the linter on `files`; return raw JSON stdout, or `None` if it
    /// isn't available (the caller then falls back to the LLM).
    fn run(&self, files: &[PathBuf]) -> Option<String>;

    /// Whether the linter is installed AND a project config resolves. Gates
    /// whether the LLM's superseded rules are dropped — if the linter can't
    /// actually run, the LLM keeps checking the category.
    fn available(&self) -> bool;

    /// Which linter this is (selects the output parser + superseded rules).
    fn kind(&self) -> LinterKind;
}

// ── ESLint `--format json` shape (only the fields we use) ──
#[derive(Deserialize)]
struct EslintFile {
    #[serde(rename = "filePath", default)]
    file_path: String,
    #[serde(default)]
    messages: Vec<EslintMessage>,
}

#[derive(Deserialize)]
struct EslintMessage {
    #[serde(rename = "ruleId", default)]
    rule_id: Option<String>,
    #[serde(default)]
    severity: u8,
    #[serde(default)]
    message: String,
    #[serde(default)]
    line: usize,
}

// ── Stylelint `--formatter json` shape ──
#[derive(Deserialize)]
struct StylelintFile {
    #[serde(default)]
    source: String,
    #[serde(default)]
    warnings: Vec<StylelintWarning>,
}

#[derive(Deserialize)]
struct StylelintWarning {
    #[serde(default)]
    line: usize,
    #[serde(default)]
    rule: String,
    #[serde(default)]
    severity: String,
    #[serde(default)]
    text: String,
}

/// Config file names that mean "ESLint can lint this project".
const ESLINT_CONFIGS: &[&str] = &[
    "eslint.config.js",
    "eslint.config.mjs",
    "eslint.config.cjs",
    ".eslintrc",
    ".eslintrc.js",
    ".eslintrc.cjs",
    ".eslintrc.json",
    ".eslintrc.yml",
    ".eslintrc.yaml",
];

/// Config file names that mean "Stylelint can lint this project".
const STYLELINT_CONFIGS: &[&str] = &[
    ".stylelintrc",
    ".stylelintrc.json",
    ".stylelintrc.js",
    ".stylelintrc.cjs",
    ".stylelintrc.yml",
    ".stylelintrc.yaml",
    "stylelint.config.js",
    "stylelint.config.cjs",
];

/// The review files a given linter should lint (filtered by extension).
pub fn review_files(diffs: &[FileDiff], kind: LinterKind) -> Vec<PathBuf> {
    diffs
        .iter()
        .filter(|d| has_extension(&d.path, kind))
        .map(|d| PathBuf::from(&d.path))
        .collect()
}

fn has_extension(path: &str, kind: LinterKind) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    kind.extensions().contains(&ext)
}

/// Parse a linter's JSON output into findings, dispatching on `kind`. Sorted by
/// file then line so output is stable.
pub fn parse_report(kind: LinterKind, json: &str) -> Vec<ReviewFinding> {
    let mut findings = match kind {
        LinterKind::Eslint => parse_eslint(json),
        LinterKind::Stylelint => parse_stylelint(json),
    };
    findings.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line_number.cmp(&b.line_number))
    });
    findings
}

fn parse_eslint(json: &str) -> Vec<ReviewFinding> {
    let Some(files) = parse_json_array::<EslintFile>(json) else {
        return vec![];
    };
    files
        .into_iter()
        .flat_map(|file| {
            let path = file.file_path;
            file.messages.into_iter().map(move |m| {
                let rule = m.rule_id.unwrap_or_default();
                ReviewFinding {
                    file_path: path.clone(),
                    line_number: m.line.max(1),
                    end_line: None,
                    severity: eslint_severity(m.severity),
                    category: Category::Style,
                    title: linter_label(&rule),
                    description: m.message,
                    suggestion: rule_suggestion("ESLint", &rule),
                }
            })
        })
        .collect()
}

fn parse_stylelint(json: &str) -> Vec<ReviewFinding> {
    let Some(files) = parse_json_array::<StylelintFile>(json) else {
        return vec![];
    };
    files
        .into_iter()
        .flat_map(|file| {
            let path = file.source;
            file.warnings.into_iter().map(move |w| ReviewFinding {
                file_path: path.clone(),
                line_number: w.line.max(1),
                end_line: None,
                severity: stylelint_severity(&w.severity),
                category: Category::Style,
                title: linter_label(&w.rule),
                description: w.text,
                suggestion: rule_suggestion("Stylelint", &w.rule),
            })
        })
        .collect()
}

/// Deserialize a top-level JSON array, tolerating leading/trailing noise that
/// container wrappers (lando/ddev/docker) can print around it.
fn parse_json_array<T: serde::de::DeserializeOwned>(s: &str) -> Option<Vec<T>> {
    if let Ok(v) = serde_json::from_str::<Vec<T>>(s) {
        return Some(v);
    }
    let start = s.find('[')?;
    let end = s.rfind(']')?;
    serde_json::from_str::<Vec<T>>(&s[start..=end]).ok()
}

fn eslint_severity(severity: u8) -> Severity {
    match severity {
        2 => Severity::Error,
        1 => Severity::Warning,
        _ => Severity::Info,
    }
}

fn stylelint_severity(severity: &str) -> Severity {
    match severity.to_ascii_lowercase().as_str() {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        _ => Severity::Info,
    }
}

/// A short title from a lint rule id, with a fallback for ruleId:null (ESLint
/// emits that for parse errors).
fn linter_label(rule: &str) -> String {
    if rule.is_empty() {
        "lint error".to_string()
    } else {
        rule.to_string()
    }
}

fn rule_suggestion(tool: &str, rule: &str) -> String {
    if rule.is_empty() {
        String::new()
    } else {
        format!("See {tool} rule: {rule}")
    }
}

/// Whether a resolvable project config exists for `kind` under `root` — without
/// one, ESLint/Stylelint lint nothing, so the linter must not be treated active.
pub fn has_config(kind: LinterKind, root: &Path) -> bool {
    let (configs, pkg_key) = match kind {
        LinterKind::Eslint => (ESLINT_CONFIGS, "eslintConfig"),
        LinterKind::Stylelint => (STYLELINT_CONFIGS, "stylelint"),
    };
    configs.iter().any(|c| root.join(c).exists()) || package_json_has_key(root, pkg_key)
}

/// Whether `package.json` at `root` has a top-level `key` (e.g. `eslintConfig`).
/// A malformed package.json is treated as "no config" (safe — keeps the LLM).
fn package_json_has_key(root: &Path, key: &str) -> bool {
    let Ok(content) = std::fs::read_to_string(root.join("package.json")) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&content)
        .ok()
        .and_then(|v| v.get(key).cloned())
        .is_some()
}

/// Run `runner` and map its output to findings. Empty file list or an
/// unavailable runner yields no findings (the LLM still runs).
pub fn collect_findings(runner: &dyn LinterRunner, files: &[PathBuf]) -> Vec<ReviewFinding> {
    if files.is_empty() {
        return vec![];
    }
    match runner.run(files) {
        Some(json) => {
            let mut findings = parse_report(runner.kind(), &json);
            normalize_finding_paths(&mut findings, files);
            findings
        }
        None => vec![],
    }
}

/// Map linter-reported paths (often container-absolute, e.g. `/app/js/x.js`)
/// back to the repo-relative path the review uses, by suffix-matching against
/// the files we sent — so a file isn't reported twice (linter path vs LLM path).
fn normalize_finding_paths(findings: &mut [ReviewFinding], review_files: &[PathBuf]) {
    for f in findings.iter_mut() {
        if let Some(rel) = review_files.iter().find_map(|rf| {
            let rf = rf.to_string_lossy();
            (f.file_path != rf && f.file_path.ends_with(rf.as_ref())).then_some(rf)
        }) {
            f.file_path = rel.into_owned();
        }
    }
}

/// Build the linter CLI args for a JSON report over `files`.
pub fn build_args(kind: LinterKind, files: &[PathBuf]) -> Vec<String> {
    let mut args: Vec<String> = match kind {
        LinterKind::Eslint => vec!["--format".into(), "json".into()],
        LinterKind::Stylelint => vec!["--formatter".into(), "json".into()],
    };
    args.extend(files.iter().map(|f| f.to_string_lossy().into_owned()));
    args
}

/// Split a configured command into program + args (whitespace-separated), so
/// `lando eslint` becomes `["lando", "eslint"]`.
fn split_command(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().map(str::to_string).collect()
}

/// Pick the stream carrying the linter's JSON report. ESLint writes it to
/// stdout, Stylelint to stderr — so prefer stdout, fall back to stderr when
/// stdout is empty. `None` when neither stream has anything.
fn select_linter_output(stdout: &str, stderr: &str) -> Option<String> {
    if !stdout.trim().is_empty() {
        Some(stdout.to_string())
    } else if !stderr.trim().is_empty() {
        Some(stderr.to_string())
    } else {
        None
    }
}

/// Locate the linter binary in the project's `node_modules/.bin`, if present.
fn node_bin(root: &Path, binary: &str) -> Option<PathBuf> {
    let candidate = root.join("node_modules/.bin").join(binary);
    candidate.exists().then_some(candidate)
}

/// Locate the linter: project `node_modules/.bin/<tool>` first, then `PATH`.
pub fn locate_linter(root: &Path, kind: LinterKind) -> Option<PathBuf> {
    node_bin(root, kind.binary()).or_else(|| path_binary(kind.binary()))
}

/// A binary resolved on `PATH` (best-effort; not unit-tested).
fn path_binary(binary: &str) -> Option<PathBuf> {
    let cmd = if cfg!(target_os = "windows") { "where" } else { "which" };
    let out = std::process::Command::new(cmd).arg(binary).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout);
    let first = path.lines().next()?.trim();
    (!first.is_empty()).then(|| PathBuf::from(first))
}

/// Live runner: invokes the configured (or located) linter from the project
/// root so reported paths match the review's (repo-relative) paths.
///
/// `command` is the program + leading args — `["eslint"]`, or a container
/// wrapper like `["lando", "eslint"]`. Empty = the linter wasn't found.
pub struct LiveLinter {
    project_root: PathBuf,
    command: Vec<String>,
    kind: LinterKind,
    /// Cached availability probe — running `<tool> --version` once is enough.
    available: std::sync::OnceLock<bool>,
}

impl LiveLinter {
    /// Build from config: use the linter's `command` if set, else locate it.
    pub fn from_config(
        project_root: PathBuf,
        kind: LinterKind,
        config: &crate::config::Config,
    ) -> Self {
        let override_cmd = match kind {
            LinterKind::Eslint => config.eslint_command(),
            LinterKind::Stylelint => config.stylelint_command(),
        };
        let command = match override_cmd {
            Some(c) => split_command(c),
            None => locate_linter(&project_root, kind)
                .map(|p| vec![p.to_string_lossy().into_owned()])
                .unwrap_or_default(),
        };
        Self {
            project_root,
            command,
            kind,
            available: std::sync::OnceLock::new(),
        }
    }

    /// A `Command` of `self.command` + `extra`, run from the project root.
    fn command_with(&self, extra: &[String]) -> Option<std::process::Command> {
        let (prog, lead) = self.command.split_first()?;
        let mut cmd = std::process::Command::new(prog);
        cmd.args(lead).args(extra).current_dir(&self.project_root);
        Some(cmd)
    }
}

impl LinterRunner for LiveLinter {
    fn available(&self) -> bool {
        *self.available.get_or_init(|| {
            // Real probe: the binary must actually execute (catches the
            // host-vs-container failure) AND a project config must resolve —
            // without a config the linter lints nothing, so dropping the LLM's
            // rules would leave the category uncovered.
            let Some(mut cmd) = self.command_with(&["--version".to_string()]) else {
                return false;
            };
            let runs = matches!(
                cmd.output(),
                Ok(out) if out.status.success()
                    && !String::from_utf8_lossy(&out.stdout).trim().is_empty()
            );
            runs && has_config(self.kind, &self.project_root)
        })
    }

    fn run(&self, files: &[PathBuf]) -> Option<String> {
        let mut cmd = self.command_with(&build_args(self.kind, files))?;
        let out = cmd.output().ok()?;
        // Linters exit NON-ZERO when they find problems — normal, not a failure.
        // ESLint writes its JSON report to stdout, Stylelint to stderr; take
        // whichever stream actually carries it.
        select_linter_output(
            &String::from_utf8_lossy(&out.stdout),
            &String::from_utf8_lossy(&out.stderr),
        )
    }

    fn kind(&self) -> LinterKind {
        self.kind
    }
}

#[cfg(test)]
pub struct MockLinterRunner {
    pub json: Option<String>,
    pub kind: LinterKind,
}

#[cfg(test)]
impl LinterRunner for MockLinterRunner {
    fn run(&self, _files: &[PathBuf]) -> Option<String> {
        self.json.clone()
    }
    fn available(&self) -> bool {
        self.json.is_some()
    }
    fn kind(&self) -> LinterKind {
        self.kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::FileStatus;

    const ESLINT_JSON: &str = r#"[
      {"filePath":"/app/js/tracker.js","messages":[
        {"ruleId":"no-var","severity":2,"message":"Unexpected var, use let or const instead.","line":13,"column":3},
        {"ruleId":"eqeqeq","severity":1,"message":"Expected === and instead saw ==.","line":40,"column":7}
      ]}
    ]"#;

    const STYLELINT_JSON: &str = r#"[
      {"source":"/app/css/overlay.css","warnings":[
        {"line":12,"column":3,"rule":"declaration-no-important","severity":"error","text":"Unexpected !important"}
      ]}
    ]"#;

    fn mk(p: &str) -> FileDiff {
        FileDiff {
            path: p.into(),
            status: FileStatus::Modified,
            hunks: vec![],
        }
    }

    #[test]
    fn parse_eslint_maps_messages() {
        let f = parse_report(LinterKind::Eslint, ESLINT_JSON);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].file_path, "/app/js/tracker.js");
        assert_eq!(f[0].line_number, 13);
        assert_eq!(f[0].severity, Severity::Error); // eslint severity 2
        assert_eq!(f[1].severity, Severity::Warning); // eslint severity 1
        assert!(f[0].title.contains("no-var"));
        assert!(f[0].description.contains("let or const"));
    }

    #[test]
    fn parse_stylelint_maps_warnings() {
        let f = parse_report(LinterKind::Stylelint, STYLELINT_JSON);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].file_path, "/app/css/overlay.css");
        assert_eq!(f[0].line_number, 12);
        assert_eq!(f[0].severity, Severity::Error); // stylelint "error"
        assert!(f[0].title.contains("declaration-no-important"));
        assert!(f[0].description.contains("!important"));
    }

    #[test]
    fn parse_tolerates_container_banner() {
        // lando/ddev can print banner lines before the JSON array.
        let noisy = format!("Booting app...\n{ESLINT_JSON}");
        let f = parse_report(LinterKind::Eslint, &noisy);
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn parse_empty_and_malformed() {
        assert!(parse_report(LinterKind::Eslint, "[]").is_empty());
        assert!(parse_report(LinterKind::Eslint, "not json").is_empty());
        assert!(parse_report(LinterKind::Stylelint, "[]").is_empty());
    }

    #[test]
    fn parse_sorted_by_file_then_line() {
        let json = r#"[
          {"filePath":"b.js","messages":[{"ruleId":"x","severity":2,"message":"m","line":2}]},
          {"filePath":"a.js","messages":[
            {"ruleId":"y","severity":2,"message":"m","line":9},
            {"ruleId":"z","severity":1,"message":"m","line":1}
          ]}
        ]"#;
        let f = parse_report(LinterKind::Eslint, json);
        assert_eq!(f.len(), 3);
        assert_eq!((f[0].file_path.as_str(), f[0].line_number), ("a.js", 1));
        assert_eq!((f[1].file_path.as_str(), f[1].line_number), ("a.js", 9));
        assert_eq!((f[2].file_path.as_str(), f[2].line_number), ("b.js", 2));
    }

    #[test]
    fn eslint_null_rule_id_is_handled() {
        // ESLint emits ruleId:null for parse errors; must not crash, gets a label.
        let json = r#"[{"filePath":"a.js","messages":[{"ruleId":null,"severity":2,"message":"Parsing error","line":1}]}]"#;
        let f = parse_report(LinterKind::Eslint, json);
        assert_eq!(f.len(), 1);
        assert!(!f[0].title.is_empty());
    }

    #[test]
    fn superseded_rules_are_the_mechanical_ones() {
        assert!(is_superseded("js-no-var"));
        assert!(is_superseded("css-no-important"));
        // Semantic rules stay with the LLM:
        assert!(!is_superseded("js-xss-prevention"));
        assert!(!is_superseded("js-error-handling"));
        // Per-kind:
        assert!(LinterKind::Eslint.superseded().contains(&"js-no-var"));
        assert!(LinterKind::Stylelint.superseded().contains(&"css-no-important"));
        assert!(!LinterKind::Eslint.superseded().contains(&"css-no-important"));
    }

    #[test]
    fn review_files_filters_by_extension() {
        let diffs = vec![
            mk("js/a.js"),
            mk("js/b.mjs"),
            mk("css/c.css"),
            mk("src/Foo.php"),
            mk("d.yml"),
        ];
        let js = review_files(&diffs, LinterKind::Eslint);
        assert_eq!(js.len(), 2);
        assert!(js.contains(&PathBuf::from("js/a.js")));
        assert!(js.contains(&PathBuf::from("js/b.mjs")));

        let css = review_files(&diffs, LinterKind::Stylelint);
        assert_eq!(css, vec![PathBuf::from("css/c.css")]);
    }

    #[test]
    fn collect_findings_with_mock_runner() {
        let runner = MockLinterRunner {
            json: Some(ESLINT_JSON.into()),
            kind: LinterKind::Eslint,
        };
        let f = collect_findings(&runner, &[PathBuf::from("js/tracker.js")]);
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn collect_findings_normalizes_container_paths() {
        // The linter (via lando/ddev) reports container-absolute paths; they must
        // map back to the repo-relative path the review uses.
        let runner = MockLinterRunner {
            json: Some(ESLINT_JSON.into()),
            kind: LinterKind::Eslint,
        };
        let files = vec![PathBuf::from("js/tracker.js")];
        let f = collect_findings(&runner, &files);
        assert_eq!(f[0].file_path, "js/tracker.js");
    }

    #[test]
    fn collect_findings_unavailable_or_empty_is_empty() {
        let down = MockLinterRunner {
            json: None,
            kind: LinterKind::Eslint,
        };
        assert!(collect_findings(&down, &[PathBuf::from("a.js")]).is_empty());

        let up = MockLinterRunner {
            json: Some(ESLINT_JSON.into()),
            kind: LinterKind::Eslint,
        };
        assert!(collect_findings(&up, &[]).is_empty());
    }

    #[test]
    fn has_config_detects_eslint_and_stylelint_configs() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        assert!(!has_config(LinterKind::Eslint, root));
        assert!(!has_config(LinterKind::Stylelint, root));

        std::fs::write(root.join("eslint.config.js"), "export default []\n").unwrap();
        assert!(has_config(LinterKind::Eslint, root));
        assert!(!has_config(LinterKind::Stylelint, root));

        std::fs::write(root.join(".stylelintrc.json"), "{}\n").unwrap();
        assert!(has_config(LinterKind::Stylelint, root));
    }

    #[test]
    fn select_output_prefers_stdout_then_falls_back_to_stderr() {
        // ESLint writes its JSON report to stdout.
        assert_eq!(
            select_linter_output("[{\"x\":1}]", ""),
            Some("[{\"x\":1}]".to_string())
        );
        // Stylelint writes to stderr with an empty stdout — must use stderr.
        assert_eq!(
            select_linter_output("", "[{\"y\":2}]"),
            Some("[{\"y\":2}]".to_string())
        );
        // Whitespace-only stdout still falls back to stderr.
        assert_eq!(
            select_linter_output("  \n", "[{\"z\":3}]"),
            Some("[{\"z\":3}]".to_string())
        );
        // Nothing on either stream → None.
        assert_eq!(select_linter_output("", ""), None);
        assert_eq!(select_linter_output("  ", "\n"), None);
    }

    #[test]
    fn build_args_per_tool() {
        let e = build_args(LinterKind::Eslint, &[PathBuf::from("a.js")]);
        assert!(e.contains(&"--format".to_string()) && e.contains(&"json".to_string()));
        assert!(e.iter().any(|a| a == "a.js"));

        let s = build_args(LinterKind::Stylelint, &[PathBuf::from("a.css")]);
        assert!(s.contains(&"--formatter".to_string()) && s.contains(&"json".to_string()));
        assert!(s.iter().any(|a| a == "a.css"));
    }

    #[test]
    fn split_command_handles_container_wrappers() {
        assert_eq!(split_command("eslint"), vec!["eslint"]);
        assert_eq!(split_command("lando eslint"), vec!["lando", "eslint"]);
        assert_eq!(split_command("  ddev  exec  stylelint "), vec!["ddev", "exec", "stylelint"]);
        assert!(split_command("   ").is_empty());
    }

    #[test]
    fn node_bin_detection() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(node_bin(dir.path(), "eslint").is_none());

        let bin = dir.path().join("node_modules/.bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("eslint"), "#!/bin/sh\n").unwrap();
        assert_eq!(node_bin(dir.path(), "eslint"), Some(bin.join("eslint")));
    }

    #[test]
    fn from_config_uses_command_override() {
        let config =
            crate::config::Config::parse("eslint:\n  command: \"lando eslint\"\n").unwrap();
        let linter = LiveLinter::from_config(PathBuf::from("/x"), LinterKind::Eslint, &config);
        assert_eq!(linter.command, vec!["lando", "eslint"]);
        assert_eq!(linter.kind(), LinterKind::Eslint);
    }

    #[test]
    fn has_config_detects_legacy_and_package_json() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        // Legacy dotfile.
        std::fs::write(root.join(".eslintrc.json"), "{}\n").unwrap();
        assert!(has_config(LinterKind::Eslint, root));

        // package.json key for stylelint.
        let dir2 = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir2.path().join("package.json"),
            r#"{"stylelint":{"rules":{}}}"#,
        )
        .unwrap();
        assert!(has_config(LinterKind::Stylelint, dir2.path()));
    }
}
