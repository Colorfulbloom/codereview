//! PHP_CodeSniffer (Drupal coding standards) as a deterministic finding source.
//!
//! `phpcs --standard=Drupal,DrupalPractice` is the tool Drupal core's own CI uses to
//! enforce coding standards — including the static-`\Drupal::`-vs-dependency-injection
//! check (`DrupalPractice.Objects.GlobalDrupal`). It is deterministic: it cannot invent
//! service IDs, miscount, or misread promoted constructor properties the way a local LLM
//! does. So the rule-based Drupal/PHP categories are sourced from phpcs, and the LLM is
//! left to the semantic findings a linter can't make.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::models::{Category, ReviewFinding, Severity};
use crate::git::FileDiff;

/// Extensions phpcs should lint (PHP + Drupal file types).
const PHP_EXTENSIONS: &str = "php,module,install,theme,profile,inc";

// ── phpcs --report=json shape (only the fields we use) ──
#[derive(Deserialize)]
struct PhpcsReport {
    #[serde(default)]
    files: std::collections::HashMap<String, PhpcsFile>,
}

#[derive(Deserialize)]
struct PhpcsFile {
    #[serde(default)]
    messages: Vec<PhpcsMessage>,
}

#[derive(Deserialize)]
struct PhpcsMessage {
    message: String,
    #[serde(default)]
    source: String,
    #[serde(rename = "type", default)]
    msg_type: String,
    #[serde(default)]
    line: usize,
}

/// LLM rule IDs that phpcs supersedes — the orchestrator drops these from the
/// Drupal/PHP agents when phpcs is active, so the model no longer (mis)checks them.
pub const SUPERSEDED_BY_PHPCS: &[&str] = &[
    "drupal-dependency-injection",
    "php-psr12-style",
    "drupal-psr12-style",
    "drupal-coding-standards",
    "php-type-declarations",
    "drupal-type-declarations",
];

/// Runs PHP_CodeSniffer. A trait so tests use a mock and never need PHP installed.
pub trait PhpcsRunner {
    /// Run phpcs on `files` with `standard`; return raw JSON stdout, or `None` if
    /// phpcs/the standard isn't available (the caller then falls back to the LLM).
    fn run(&self, files: &[PathBuf], standard: &str) -> Option<String>;

    /// Whether phpcs is installed and usable. Gates whether the LLM's superseded
    /// rules are dropped — if phpcs can't run, the LLM keeps checking them so the
    /// category is never left unchecked.
    fn available(&self) -> bool;
}

/// The PHP/Drupal files from a review set that phpcs should lint.
pub fn php_review_files(diffs: &[FileDiff]) -> Vec<PathBuf> {
    diffs
        .iter()
        .filter(|d| is_php_path(&d.path))
        .map(|d| PathBuf::from(&d.path))
        .collect()
}

fn is_php_path(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    PHP_EXTENSIONS.split(',').any(|e| e == ext)
}

/// Whether a rule id is superseded by phpcs (and so should be dropped from the LLM).
pub fn is_superseded(rule_id: &str) -> bool {
    SUPERSEDED_BY_PHPCS.contains(&rule_id)
}

/// Build the phpcs CLI args for a JSON report over `files`.
pub fn build_args(files: &[PathBuf], standard: &str) -> Vec<String> {
    let mut args = vec![
        format!("--standard={standard}"),
        "--report=json".to_string(),
        format!("--extensions={PHP_EXTENSIONS}"),
    ];
    args.extend(files.iter().map(|f| f.to_string_lossy().into_owned()));
    args
}

/// Split a configured phpcs command into program + args (whitespace-separated),
/// so `ddev exec phpcs` becomes `["ddev", "exec", "phpcs"]`.
fn split_command(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().map(str::to_string).collect()
}

/// Whether `phpcs -i` output confirms the requested standard is actually
/// installed and runnable. Catches both "php missing" (empty output) and
/// "standard not registered" — in either case phpcs must NOT be treated as
/// active, or the LLM's superseded rules would be dropped with nothing covering
/// them.
fn probe_confirms_standard(probe_stdout: &str, standard: &str) -> bool {
    let Some(first) = standard.split(',').next().map(str::trim) else {
        return false;
    };
    !first.is_empty() && probe_stdout.to_lowercase().contains(&first.to_lowercase())
}

/// Parse a phpcs `--report=json` payload into findings (sorted by file then line).
pub fn parse_report(json: &str) -> Vec<ReviewFinding> {
    let Some(report) = parse_phpcs_json(json) else {
        return vec![]; // unparseable output → no findings (graceful)
    };

    let mut findings: Vec<ReviewFinding> = report
        .files
        .into_iter()
        .flat_map(|(path, file)| {
            file.messages.into_iter().map(move |m| ReviewFinding {
                file_path: path.clone(),
                line_number: m.line.max(1),
                end_line: None,
                severity: severity_for(&m.msg_type),
                category: category_for(&m.source),
                title: sniff_label(&m.source),
                description: m.message,
                suggestion: if m.source.is_empty() {
                    String::new()
                } else {
                    format!("See Drupal standard: {}", m.source)
                },
            })
        })
        .collect();

    // HashMap order is nondeterministic — sort for stable output.
    findings.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line_number.cmp(&b.line_number))
    });
    findings
}

/// Map a sniff source (e.g. `DrupalPractice.Objects.GlobalDrupal`) to a category.
/// Deserialize a phpcs report, tolerating leading/trailing noise that container
/// wrappers (lando/ddev/docker) can print around the JSON object.
fn parse_phpcs_json(s: &str) -> Option<PhpcsReport> {
    if let Ok(report) = serde_json::from_str::<PhpcsReport>(s) {
        return Some(report);
    }
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    serde_json::from_str::<PhpcsReport>(&s[start..=end]).ok()
}

fn category_for(source: &str) -> Category {
    if source.contains("Security") {
        Category::Security
    } else if source.starts_with("DrupalPractice.") {
        Category::BestPractice
    } else {
        Category::Style
    }
}

/// Map a phpcs message `type` ("ERROR"/"WARNING") to a [`Severity`].
fn severity_for(phpcs_type: &str) -> Severity {
    match phpcs_type.to_ascii_uppercase().as_str() {
        "ERROR" => Severity::Error,
        "WARNING" => Severity::Warning,
        _ => Severity::Info,
    }
}

/// A short title from a sniff source (its leaf name).
fn sniff_label(source: &str) -> String {
    match source.rsplit('.').next() {
        Some(leaf) if !leaf.is_empty() => leaf.to_string(),
        _ => "Coding standard".to_string(),
    }
}

/// Locate `vendor/bin/phpcs` under the project root, if present.
fn vendor_phpcs(project_root: &Path) -> Option<PathBuf> {
    let candidate = project_root.join("vendor/bin/phpcs");
    candidate.exists().then_some(candidate)
}

/// Locate phpcs: project `vendor/bin/phpcs` first, then `phpcs` on `PATH`.
pub fn locate_phpcs(project_root: &Path) -> Option<PathBuf> {
    vendor_phpcs(project_root).or_else(path_phpcs)
}

/// `phpcs` resolved on `PATH` (best-effort; not unit-tested).
fn path_phpcs() -> Option<PathBuf> {
    let cmd = if cfg!(target_os = "windows") { "where" } else { "which" };
    let out = std::process::Command::new(cmd).arg("phpcs").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout);
    let first = path.lines().next()?.trim();
    (!first.is_empty()).then(|| PathBuf::from(first))
}

/// Run phpcs via `runner` and map its output to findings. Empty file list or an
/// unavailable runner yields no findings (the LLM still runs).
pub fn collect_findings(
    runner: &dyn PhpcsRunner,
    files: &[PathBuf],
    standard: &str,
) -> Vec<ReviewFinding> {
    if files.is_empty() {
        return vec![];
    }
    match runner.run(files, standard) {
        Some(json) => {
            let mut findings = parse_report(&json);
            normalize_finding_paths(&mut findings, files);
            findings
        }
        None => vec![],
    }
}

/// Map phpcs's reported paths (often container-absolute, e.g. `/app/docroot/...`)
/// back to the repo-relative path the review uses, by suffix-matching against the
/// files we sent — so a file isn't reported twice (phpcs path vs LLM path).
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

/// Live runner: invokes the configured (or located) phpcs command from the
/// project root so reported paths match the review's (repo-relative) paths.
///
/// `command` is the program + leading args — `["vendor/bin/phpcs"]`, or a
/// container wrapper like `["ddev", "exec", "phpcs"]`. Empty = phpcs not found.
pub struct LivePhpcsRunner {
    project_root: PathBuf,
    command: Vec<String>,
    standard: String,
    /// Cached availability probe — running `phpcs -i` once is enough.
    available: std::sync::OnceLock<bool>,
}

impl LivePhpcsRunner {
    /// Build from config: use `phpcs.command` if set, else locate phpcs on disk.
    pub fn from_config(project_root: PathBuf, config: &crate::config::Config) -> Self {
        let command = match config.phpcs_command() {
            Some(c) => split_command(c),
            None => locate_phpcs(&project_root)
                .map(|p| vec![p.to_string_lossy().into_owned()])
                .unwrap_or_default(),
        };
        Self {
            project_root,
            command,
            standard: config.phpcs_standard().to_string(),
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

impl PhpcsRunner for LivePhpcsRunner {
    fn available(&self) -> bool {
        // Real probe: phpcs must actually RUN (not just exist) AND list the
        // requested standard. Otherwise we'd drop the LLM's rules and leave the
        // category uncovered (the DDEV/host-no-php failure mode).
        *self.available.get_or_init(|| {
            let Some(mut cmd) = self.command_with(&["-i".to_string()]) else {
                return false;
            };
            match cmd.output() {
                Ok(out) => probe_confirms_standard(
                    &String::from_utf8_lossy(&out.stdout),
                    &self.standard,
                ),
                Err(_) => false,
            }
        })
    }

    fn run(&self, files: &[PathBuf], standard: &str) -> Option<String> {
        let mut cmd = self.command_with(&build_args(files, standard))?;
        let out = cmd.output().ok()?;
        // phpcs exits NON-ZERO when it finds violations — that's the normal case,
        // not a failure. Read stdout regardless; only empty output is "nothing".
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        (!stdout.trim().is_empty()).then_some(stdout)
    }
}

/// Test double: returns canned JSON (or `None` to simulate phpcs being unavailable).
#[cfg(test)]
pub struct MockPhpcsRunner {
    pub json: Option<String>,
}

#[cfg(test)]
impl PhpcsRunner for MockPhpcsRunner {
    fn run(&self, _files: &[PathBuf], _standard: &str) -> Option<String> {
        self.json.clone()
    }
    fn available(&self) -> bool {
        self.json.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_ERROR: &str = r#"{"files":{"a/File.php":{"messages":[
      {"message":"Use dependency injection instead of \\Drupal::database().","source":"DrupalPractice.Objects.GlobalDrupal.GlobalDrupal","type":"ERROR","line":30,"column":19}
    ]}}}"#;

    #[test]
    fn parse_report_maps_basic_message() {
        let f = parse_report(ONE_ERROR);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].file_path, "a/File.php");
        assert_eq!(f[0].line_number, 30);
        assert_eq!(f[0].severity, Severity::Error);
        assert!(f[0].description.contains("dependency injection"));
        assert_eq!(f[0].category, Category::BestPractice);
    }

    #[test]
    fn parse_report_maps_severity() {
        let warn = r#"{"files":{"a.php":{"messages":[{"message":"m","source":"Drupal.X.Y","type":"WARNING","line":5}]}}}"#;
        assert_eq!(parse_report(warn)[0].severity, Severity::Warning);
        assert_eq!(parse_report(ONE_ERROR)[0].severity, Severity::Error);
    }

    #[test]
    fn parse_report_empty_and_malformed() {
        assert!(parse_report("{}").is_empty());
        assert!(parse_report("not json at all").is_empty());
        assert!(parse_report(r#"{"files":{"a.php":{"messages":[]}}}"#).is_empty());
    }

    #[test]
    fn parse_report_tolerates_container_wrapper_banner() {
        // lando/ddev can prepend banner lines to stdout before the JSON.
        let noisy = format!("Booting app...\nWarning: something\n{ONE_ERROR}");
        let f = parse_report(&noisy);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].line_number, 30);
    }

    #[test]
    fn parse_report_multiple_files_sorted_by_file_then_line() {
        let json = r#"{"files":{
          "b.php":{"messages":[{"message":"m1","source":"Drupal.A","type":"ERROR","line":2}]},
          "a.php":{"messages":[
            {"message":"m2","source":"Drupal.B","type":"ERROR","line":9},
            {"message":"m3","source":"Drupal.C","type":"WARNING","line":1}
          ]}
        }}"#;
        let f = parse_report(json);
        assert_eq!(f.len(), 3);
        assert_eq!((f[0].file_path.as_str(), f[0].line_number), ("a.php", 1));
        assert_eq!((f[1].file_path.as_str(), f[1].line_number), ("a.php", 9));
        assert_eq!((f[2].file_path.as_str(), f[2].line_number), ("b.php", 2));
    }

    #[test]
    fn category_for_sniff_prefix() {
        assert_eq!(
            category_for("DrupalPractice.Objects.GlobalDrupal.GlobalDrupal"),
            Category::BestPractice
        );
        assert_eq!(
            category_for("Drupal.Commenting.FunctionComment.Missing"),
            Category::Style
        );
        assert_eq!(category_for("Drupal.Security.Something"), Category::Security);
    }

    #[test]
    fn build_args_includes_standard_json_and_files() {
        let args = build_args(
            &[PathBuf::from("a.php"), PathBuf::from("b.php")],
            "Drupal,DrupalPractice",
        );
        assert!(args.contains(&"--standard=Drupal,DrupalPractice".to_string()));
        assert!(args.contains(&"--report=json".to_string()));
        assert!(args.iter().any(|a| a == "a.php"));
        assert!(args.iter().any(|a| a == "b.php"));
    }

    #[test]
    fn superseded_rules_are_the_deterministic_ones() {
        assert!(is_superseded("drupal-dependency-injection"));
        assert!(is_superseded("php-psr12-style"));
        // Semantic rules stay with the LLM:
        assert!(!is_superseded("php-sql-injection"));
        assert!(!is_superseded("js-no-var"));
    }

    #[test]
    fn vendor_phpcs_detection() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(vendor_phpcs(dir.path()).is_none());

        let bin = dir.path().join("vendor/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("phpcs"), "#!/bin/sh\n").unwrap();
        assert_eq!(vendor_phpcs(dir.path()), Some(bin.join("phpcs")));
    }

    #[test]
    fn collect_findings_with_mock_runner() {
        let runner = MockPhpcsRunner { json: Some(ONE_ERROR.into()) };
        let f = collect_findings(&runner, &[PathBuf::from("a/File.php")], "Drupal");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].category, Category::BestPractice);
    }

    #[test]
    fn collect_findings_normalizes_container_paths_to_repo_relative() {
        // phpcs (via lando/ddev) reports container-absolute paths; they must be
        // mapped back to the repo-relative path the review uses, so a file isn't
        // listed twice (once for phpcs, once for the LLM).
        let json = r#"{"files":{"/app/docroot/modules/custom/x/Foo.php":{"messages":[
          {"message":"m","source":"Drupal.A","type":"ERROR","line":3}
        ]}}}"#;
        let runner = MockPhpcsRunner { json: Some(json.into()) };
        let files = vec![PathBuf::from("docroot/modules/custom/x/Foo.php")];
        let f = collect_findings(&runner, &files, "Drupal");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].file_path, "docroot/modules/custom/x/Foo.php");
    }

    #[test]
    fn collect_findings_falls_back_when_phpcs_unavailable() {
        let runner = MockPhpcsRunner { json: None };
        let f = collect_findings(&runner, &[PathBuf::from("a.php")], "Drupal");
        assert!(f.is_empty(), "no phpcs findings; the LLM still runs separately");
    }

    #[test]
    fn collect_findings_empty_file_list_is_empty() {
        let runner = MockPhpcsRunner { json: Some(ONE_ERROR.into()) };
        assert!(collect_findings(&runner, &[], "Drupal").is_empty());
    }

    #[test]
    fn php_review_files_filters_to_php_and_drupal_extensions() {
        use crate::git::FileStatus;
        let mk = |p: &str| FileDiff {
            path: p.into(),
            status: FileStatus::Modified,
            hunks: vec![],
        };
        let diffs = vec![
            mk("src/Foo.php"),
            mk("a.module"),
            mk("b.install"),
            mk("c.js"),
            mk("d.css"),
            mk("e.yml"),
        ];
        let files = php_review_files(&diffs);
        assert_eq!(files.len(), 3);
        assert!(files.contains(&PathBuf::from("src/Foo.php")));
        assert!(files.contains(&PathBuf::from("a.module")));
        assert!(files.contains(&PathBuf::from("b.install")));
    }

    #[test]
    fn mock_available_reflects_json_presence() {
        assert!(MockPhpcsRunner { json: Some("{}".into()) }.available());
        assert!(!MockPhpcsRunner { json: None }.available());
    }

    #[test]
    fn split_command_handles_path_and_container_wrappers() {
        assert_eq!(split_command("vendor/bin/phpcs"), vec!["vendor/bin/phpcs"]);
        assert_eq!(split_command("ddev exec phpcs"), vec!["ddev", "exec", "phpcs"]);
        assert_eq!(split_command("  lando  phpcs "), vec!["lando", "phpcs"]);
        assert!(split_command("   ").is_empty());
    }

    #[test]
    fn probe_confirms_standard_checks_first_token_case_insensitive() {
        let listed = "The installed coding standards are PEAR, Drupal, DrupalPractice and PSR2";
        assert!(probe_confirms_standard(listed, "Drupal,DrupalPractice"));
        // Standard not registered → not confirmed (we must NOT drop LLM rules).
        assert!(!probe_confirms_standard("PEAR, PSR2, Squiz", "Drupal,DrupalPractice"));
        // Empty output (e.g. php missing → env error) → not confirmed.
        assert!(!probe_confirms_standard("", "Drupal"));
    }
}
