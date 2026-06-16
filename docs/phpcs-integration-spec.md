# Spec: PHPCS (Drupal coding standards) as a deterministic finding source

Status: **IMPLEMENTED** (review-speedups branch). Decisions taken: `auto` default,
`Drupal,DrupalPractice` standards, supersede overlapping LLM rules, PHP/Drupal only.
TDD throughout — see `src/review/phpcs.rs` (16 tests), the orchestrator supersede test,
and the config test. This document is retained as the design reference.

## 1. Goal

Use `phpcs` with the `Drupal` + `DrupalPractice` standards (from `drupal/coder`) — the
tool Drupal core's own CI uses — as a **deterministic source of truth** for the
rule-based Drupal/PHP findings the LLM currently hallucinates (static `\Drupal::`
calls / DI, PSR-12 style, naming, deprecations). The LLM stops producing those
categories; it keeps the semantic ones (logic bugs, exploitability, design) that a
linter can't judge.

Non-goal (this phase): JS/CSS/YAML linters. The seam is designed so they can plug in
later, but only PHPCS ships now.

## 2. Where it fits in the pipeline

```
run_review (engine.rs)
  collect_review_diffs            (existing)
  detect_review_languages         (existing)
  ── NEW: phpcs::collect_findings(runner, files, config)   ← deterministic, no LLM
  run_agents (orchestrator)       (existing, with superseded rules disabled)
  claims gate                     (existing — only applies to LLM findings)
  merge(phpcs_findings, llm_findings) → dedup → ReviewResult
```

PHPCS runs **before** the LLM agents, on the PHP/Drupal files in the review set. Its
findings are first-class `ReviewFinding`s and flow through the existing formatters,
cache, and output unchanged.

## 3. New module: `src/review/phpcs.rs`

### 3a. DI seam (mirrors `GitAgent` / `OllamaClient`)
```rust
/// Runs PHP_CodeSniffer. Trait so tests use a mock and never need PHP installed.
pub trait PhpcsRunner {
    /// Run phpcs on `files` with `standard`; return raw JSON stdout, or None if
    /// phpcs/the standard isn't available (caller falls back to the LLM).
    fn run(&self, files: &[PathBuf], standard: &str) -> Option<String>;
}

pub struct LivePhpcsRunner { project_root: PathBuf, phpcs_path: PathBuf }
// + MockPhpcsRunner (in #[cfg(test)]) returning canned JSON.
```

### 3b. Pure functions (fully unit-testable, no PHP, no FS)
```rust
pub fn build_args(files: &[PathBuf], standard: &str) -> Vec<String>;
pub fn parse_report(json: &str) -> Vec<ReviewFinding>;
fn category_for(sniff_source: &str) -> Category;   // DrupalPractice.* → BestPractice, Drupal.* → Style, *.Security.* → Security
fn severity_for(phpcs_type: &str) -> Severity;      // "ERROR" → Error, "WARNING" → Warning
```

### 3c. Detection (thin FS layer)
`locate_phpcs(project_root) -> Option<PathBuf>`: prefer `<root>/vendor/bin/phpcs`
(the common Drupal case), then `phpcs` on `PATH`. Returns `None` if neither exists →
phpcs is skipped, a one-line hint is logged (`composer require --dev drupal/coder`),
and the review proceeds LLM-only.

## 4. Invocation details
- Command: `phpcs --standard=Drupal,DrupalPractice --report=json --extensions=php,module,install,theme,profile,inc <files>`
- **Exit code is NOT failure.** phpcs exits non-zero when it finds violations — that's
  the normal, expected case. We read stdout regardless of exit code; only a missing
  binary / empty-or-unparseable stdout is a "skip".
- Files: the PHP/Drupal files already in the review set (`.php/.module/.install/.theme/.profile/.inc`).

## 5. PHPCS JSON → `ReviewFinding` mapping
PHPCS `--report=json` shape:
```json
{ "files": { "<path>": { "messages": [
  { "message": "...", "source": "DrupalPractice.Objects.GlobalDrupal.GlobalDrupal",
    "severity": 5, "fixable": false, "type": "ERROR", "line": 30, "column": 19 } ] } } }
```
Mapping per message:
| ReviewFinding | from |
|---|---|
| `file_path` | the file key |
| `line_number` | `message.line` |
| `end_line` | `None` |
| `severity` | `type`: ERROR→`Error`, WARNING→`Warning` |
| `category` | `category_for(source)` (heuristic by sniff prefix) |
| `title` | short label, e.g. the leaf of `source` (`GlobalDrupal`) |
| `description` | `message` (verbatim — it's authoritative) |
| `suggestion` | `"See Drupal standard: {source}"` |

## 6. Rule-overlap handling (so we don't double-report or re-hallucinate)
When phpcs runs for PHP/Drupal, the orchestrator **excludes the LLM rules phpcs
supersedes** from the Drupal/PHP agents. A static supersede-map:
```
SUPERSEDED_BY_PHPCS = [
  "drupal-dependency-injection",  // DrupalPractice GlobalDrupal/GlobalClass/GlobalFunction
  "php-psr12-style", "drupal-psr12-style",
  "drupal-coding-standards",
  "php-type-declarations", "drupal-type-declarations",
]
```
The LLM no longer emits these, so the fake-service-ID / promoted-property / miscount
hallucinations in this category disappear at the source. (Security/bug/semantic rules
stay with the LLM.)

## 7. Config (`.codereview.yaml`)
```yaml
phpcs:
  enabled: auto        # auto (default) = run if phpcs + Drupal standard found; true/false to force
  standard: "Drupal,DrupalPractice"
  path: null           # optional explicit path to phpcs
```
`auto` keeps it zero-config for Drupal projects and a no-op everywhere else.

## 8. Interactions
- **Cache:** phpcs is fast (no LLM); caching optional. If cached, key on
  `(file_content + standard + phpcs_version)`. Recommend: skip caching phpcs in v1.
- **Diff modes:** phpcs lints the **working-tree** file (it can't read a three-dot
  diff). For `Ref` mode that may differ from the ref; documented caveat. Optional
  refinement: filter phpcs findings to changed line ranges. v1 = lint whole changed file.
- **Output/metadata:** the run summary notes phpcs ran and how many findings came from
  it vs. the LLM (nice for transparency).

## 9. TDD plan (test-first, in order)
Pure (no PHP, no FS) — write these first, watch them fail:
1. `parse_report_maps_basic_message` — one ERROR message → one finding (file/line/severity/desc).
2. `parse_report_maps_severity` — ERROR→Error, WARNING→Warning.
3. `parse_report_empty_and_malformed` — `{}` and garbage → `[]` (graceful).
4. `parse_report_multiple_files_and_messages` — counts and per-file attribution.
5. `category_for_sniff_prefix` — DrupalPractice.*→BestPractice, Drupal.*→Style, *Security*→Security.
6. `build_args_includes_standard_json_and_files`.
7. `superseded_rules_filter` — given phpcs enabled, the rule list passed to the Drupal agent excludes `drupal-dependency-injection` et al.

FS / DI (mock, no PHP):
8. `locate_phpcs_prefers_vendor_bin` (tempdir with `vendor/bin/phpcs`) and returns None when absent.
9. `collect_findings_with_mock_runner` — `MockPhpcsRunner` returns canned JSON incl. a
   `GlobalDrupal` message → the engine produces that finding at the right line, merged
   into the result.
10. `phpcs_unavailable_falls_back_to_llm` — runner returns `None` → pipeline proceeds,
    no panic, LLM findings still present.
11. `nonzero_exit_with_violations_is_not_an_error` — live-runner result handling treats
    violations-found (non-zero exit + valid JSON) as success.

## 10. Files touched
- **New:** `src/review/phpcs.rs` (trait, live + mock runners, `parse_report`, `build_args`, `category_for`, `locate_phpcs`, supersede-map) + `src/review/mod.rs` (register).
- `src/review/engine.rs` — run phpcs source, merge findings, pass supersede info.
- `src/review/agents/orchestrator.rs` — drop superseded rules when phpcs active.
- `src/config/mod.rs` — `phpcs` config block.
- `src/runtime.rs` / `main.rs` / `repl/mod.rs` — construct & inject `LivePhpcsRunner`.
- `docs/` — README + CONFIGURATION (new section), CLAUDE.md gotchas.

## 11. Effort & risk
- **Effort:** Medium-to-Large. The core (parse_report + supersede filter + DI seam) is
  small and well-bounded; the wiring (engine/orchestrator/config/callers) is the bulk,
  all mechanical. ~1 focused implementation pass with tests.
- **Risk:** low-to-medium.
  - New optional dependency (phpcs + drupal/coder) — mitigated by `auto`/graceful skip.
  - `category_for` is heuristic — wrong category is cosmetic, never a dropped/added finding.
  - Diff-mode file-vs-ref mismatch — documented caveat.
  - PHP-only this phase — JS/CSS remain LLM-based until their linters are added.

## 12. Open decisions (for you)
1. **Default mode:** `auto` (run when detected) — recommended — vs. opt-in `true`.
2. **Standards:** `Drupal,DrupalPractice` (recommended — DrupalPractice has the DI sniffs) vs. `Drupal` only.
3. **Supersede vs. coexist:** disable the overlapping LLM rules (recommended — kills the hallucination class) vs. run both and dedup.
4. **Scope:** PHP/Drupal only now (recommended) vs. also stub the generic "external linter" trait so ESLint/stylelint slot in next.
```
