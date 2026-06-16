# .codereview.yaml Configuration Reference

> For installation, quick start, and usage, see the [README](../README.md).

This document covers everything you can do with `.codereview.yaml`. Place this file in your project root and check it into version control so your entire team uses the same review settings.

Generate a starter file with `code-review init` or from the REPL with `/init`.

---

## Table of Contents

- [File Location](#file-location)
- [Complete Schema](#complete-schema)
- [Top-Level Fields](#top-level-fields)
  - [model](#model)
  - [output_format](#output_format)
  - [languages](#languages)
  - [max_context_tokens](#max_context_tokens)
  - [llm_timeout_seconds](#llm_timeout_seconds)
  - [phpcs](#phpcs)
  - [eslint / stylelint](#eslint--stylelint)
  - [verify](#verify)
- [Rule Overrides](#rule-overrides)
  - [Disabling a Rule](#disabling-a-rule)
  - [Changing Severity](#changing-severity)
  - [Override Multiple Rules](#override-multiple-rules)
  - [Language Keys](#language-keys)
- [Custom Rules](#custom-rules)
  - [Basic Custom Rule](#basic-custom-rule)
  - [Multi-Language Rule](#multi-language-rule)
  - [Global Rule (All Languages)](#global-rule-all-languages)
  - [Custom Rule Fields](#custom-rule-fields)
- [Excluding Files](#excluding-files)
- [How Rules Are Merged](#how-rules-are-merged)
- [How Rules Work](#how-rules-work)
- [All Built-in Rule IDs](#all-built-in-rule-ids)
- [Verifying Your Configuration](#verifying-your-configuration)
- [Examples](#examples)
  - [Drupal Project](#drupal-project)
  - [JavaScript-Heavy Project](#javascript-heavy-project)
  - [Strict Security Team](#strict-security-team)
  - [Minimal Config](#minimal-config)

---

## File Location

The file must be named `.codereview.yaml` and placed in the root of your git repository:

```
your-project/
  .codereview.yaml    <-- here
  src/
  composer.json
  ...
```

The tool loads this file automatically when you run `code-review` from within the project directory.

If the file does not exist, all built-in rules are used with default settings.

If the file has invalid YAML syntax, the tool falls back to defaults and prints a warning at startup naming the parse error (the file never half-applies). Run `/config` in the REPL to verify your file was loaded.

---

## Complete Schema

Every field is optional. An empty file is valid and uses all defaults.

```yaml
# ──────────────────────────────────────────
# Top-Level Settings
# ──────────────────────────────────────────

# Override the Ollama model used for reviews.
# This takes precedence over the model selected during onboarding.
# Can also be overridden per-run with: code-review -m <model>
model: qwen3-coder:30b

# Default output format for reviews.
# Values: terminal, json, markdown, annotations
output_format: terminal

# Restrict which languages are reviewed.
# If omitted, languages are auto-detected from changed files.
# Values: php, drupal, javascript, css, html, yaml
languages:
  - php
  - drupal
  - javascript
  - css
  - html
  - yaml

# Maximum context window (in tokens) to request from the model.
# The tool auto-detects the model's maximum and uses the smaller of the two,
# then splits large reviews into chunks that fit. Lower this to cap memory use;
# raise it for fewer, larger requests on a big-RAM machine.
# Default: 32768
max_context_tokens: 32768

# Per-LLM-request timeout in seconds. Default: 300 when unset.
# Raise it on slow hardware; 0 disables it (a stalled Ollama then hangs).
llm_timeout_seconds: 300

# Anti-hallucination LLM second pass (opt-in). Re-checks each bug/security
# finding against its code and drops the ones that misread correct code.
# Adds one LLM call per in-scope finding, so it's off by default.
# CLI/REPL: code-review --verify  /  /review --verify
verify: false
# verify_model: "qwen3.5:27b"   # optional: a larger model to judge findings

# PHP_CodeSniffer (Drupal coding standards) as the deterministic source of
# truth for rule-based Drupal/PHP checks (dependency injection, coding
# standards). The tool verifies phpcs actually runs the Drupal standard before
# relying on it; if it can't, the LLM keeps checking those rules. Omit this
# block for "auto" (run when phpcs is found). When PHP runs in a container,
# invoke phpcs through it.
phpcs:
  command: "lando phpcs"     # or "ddev exec phpcs"; omit when phpcs is on PATH

# ESLint + Stylelint: the deterministic source of truth for the mechanical
# JS/CSS rules (var, ===, console.log, !important, ...) — the JS/CSS analog of
# phpcs. Auto-run when the binary is installed AND a project config resolves
# (eslint.config.js / .eslintrc* / .stylelintrc* / package.json); if neither,
# the LLM keeps checking JS/CSS. Point `command` at a container if node is in one.
eslint:
  command: "lando eslint"      # or "ddev exec eslint"; omit when on PATH
stylelint:
  command: "lando stylelint"   # omit when stylelint is on PATH

# Files and directories to exclude from review.
# Supports exact paths, glob patterns, and directory prefixes.
exclude:
  - .lando.yml             # exact file
  - .gitignore             # exact file
  - "*.log"                # glob: all .log files
  - "*.min.js"             # glob: minified JS
  - "*.min.css"            # glob: minified CSS
  - vendor/                # directory: all files under vendor/
  - node_modules/          # directory: all files under node_modules/
  - "*/test/*"             # path pattern: any test directory

# ──────────────────────────────────────────
# Rule Overrides
# ──────────────────────────────────────────

# Override built-in rules per language.
# You can disable rules or change their severity.
rules:
  # Language key (lowercase): php, drupal, javascript, css, html
  php:
    # Rule ID (see "All Built-in Rule IDs" section below)
    php-psr12-style:
      enabled: false          # disable this rule entirely
    php-error-handling:
      severity: warning       # change from error to warning

  javascript:
    js-no-console-log:
      enabled: false          # allow console.log

  css:
    css-max-nesting:
      severity: warning       # upgrade from info to warning

# ──────────────────────────────────────────
# Custom Rules
# ──────────────────────────────────────────

# Define your own review rules. These are instructions to the LLM
# about additional things to check for in your code.
custom_rules:
  - id: no-debug-code
    description: "No debug statements (var_dump, dd, dump, console.log, debugger) in production code"
    languages: [php, drupal, javascript]
    severity: error

  - id: require-docblocks
    description: "All public methods must have a docblock with @param and @return annotations"
    languages: [php, drupal]
    severity: warning

  - id: max-function-length
    description: "Functions should not exceed 50 lines of code"
    severity: info
```

---

## Top-Level Fields

### model

```yaml
model: gemma4
```

**Type:** string (optional)

**Default:** The model selected during onboarding, or `gemma4:latest` if onboarding was skipped.

**Priority chain:** CLI flag (`-m`) > `.codereview.yaml` > onboarding selection > default

**Example values:**
- `gemma4`
- `qwen3-coder:30b`
- `qwen2.5-coder:32b`
- `deepseek-coder-v2:16b`
- `devstral:24b`

The model must be available in Ollama. Run `ollama list` to see installed models.

---

### output_format

```yaml
output_format: terminal
```

**Type:** string (optional)

**Default:** `terminal`

**Valid values:**

| Value | Description |
|-------|-------------|
| `terminal` | Colored text output with severity icons |
| `json` | Structured JSON with all findings and metadata |
| `markdown` | Markdown report with summary tables |
| `annotations` | GitHub Actions workflow command format |

---

### languages

```yaml
languages:
  - php
  - javascript
```

**Type:** list of strings (optional)

**Default:** Auto-detected from the file extensions in your diff.

**Valid values:** `php`, `drupal`, `javascript`, `css`, `html`, `yaml`

When specified, only these languages are reviewed — even if other file types appear in the diff. This is useful for projects where you want to focus reviews on specific languages.

When omitted, the tool auto-detects languages from changed file extensions. If Drupal project markers are found (`.info.yml`, `.module`, `core/lib/Drupal`), PHP files are automatically promoted to Drupal.

---

### max_context_tokens

```yaml
max_context_tokens: 32768
```

**Type:** integer (optional)

**Default:** `32768`, capped by the model's detected maximum.

Controls the context window (`num_ctx`) the tool requests from Ollama, which determines how much code can go into each review request. The tool:

1. Auto-detects the model's maximum context length from Ollama.
2. Requests the smaller of `max_context_tokens` and that maximum, so the model actually reads the whole prompt instead of silently truncating it.
3. Splits the diff into chunks that fit the budget, reviews each chunk, and merges the findings. A single file larger than the budget is split by line.

**When to change it:**

| Goal | Setting |
|------|---------|
| Reduce memory use (smaller KV cache) | Lower it, e.g. `16384` |
| Fewer, larger requests on a big-RAM machine | Raise it, e.g. `65536` |
| Default balanced behavior | Omit it (uses `32768`) |

A larger context window fits more code per request but uses more RAM for the model's KV cache. The value is always capped at what the model actually supports, so setting it above the model's maximum has no effect.

---

### llm_timeout_seconds

```yaml
llm_timeout_seconds: 600
```

**Type:** integer (optional)

**Default:** `300`

Timeout in seconds for each LLM request. A review makes one request per agent per chunk; if a single request exceeds this limit, the review fails with a timeout error naming this setting.

Raise it on slow hardware (a fanless laptop running a large model) where a single call legitimately needs more time. Before raising it, consider whether a smaller model or a lower `max_context_tokens` solves the problem faster — a review that needs 10-minute calls is painful to iterate with.

**`0` disables the timeout entirely.** Use with care: if Ollama ever stalls (model crash, memory deadlock), the review hangs forever instead of failing with a message. A generous finite value (`900`) is usually the better choice.

Note: on reasoning-capable models (qwen3.5, deepseek-r1, etc.) the tool automatically disables thinking mode for review calls, which is usually the difference between a 5-minute call and a 1-minute one. Models without the capability are unaffected.

---

### phpcs

```yaml
phpcs:
  enabled: true              # omit = auto (run when installed); false to disable
  standard: "Drupal,DrupalPractice"
```

**Type:** block (optional)

When PHP_CodeSniffer and the Drupal coding standards (`drupal/coder`) are installed,
the tool runs `phpcs --standard=Drupal,DrupalPractice` as a **deterministic source of
truth** for the rule-based Drupal/PHP checks (dependency injection, PSR-12, naming,
coding standards) — the same tool Drupal core's own CI uses. Those findings are exact:
no invented service IDs, no miscounts, no misread constructors.

When phpcs is active, the LLM **stops checking the rules phpcs covers**
(`drupal-dependency-injection`, `php-psr12-style`, `drupal-coding-standards`,
`*-type-declarations`), so it can't hallucinate them — the LLM is left to the semantic
findings a linter can't make (logic bugs, exploitability). The LLM keeps those rules if
phpcs isn't installed, so the category is never left unchecked.

- **`enabled`** — omit for *auto* (run when phpcs is found and works); `false` disables it;
  `true` forces it (still only runs if it works).
- **`standard`** — phpcs standard(s); defaults to `Drupal,DrupalPractice`.
- **`command`** — how to invoke phpcs (space-separated). Unset = locate `vendor/bin/phpcs`
  or `phpcs` on `PATH`. Set this when PHP runs in a container (see below).

phpcs is only treated as active if it **actually runs and lists the configured standard**
(verified by a `phpcs -i` probe). If php is missing, the standard isn't installed, or phpcs
lives only in a container, the tool falls back to letting the LLM check those rules — it
never drops a category and leaves it uncovered.

### Setting up phpcs + the Drupal standard

```bash
# Install Coder (the Drupal standard) and the auto-registration plugin:
composer require --dev drupal/coder dealerdirect/phpcodesniffer-composer-installer

# Verify the Drupal standard is registered:
vendor/bin/phpcs -i        # should list "Drupal, DrupalPractice"
```

If `phpcs -i` does **not** list `Drupal`, register it manually:
```bash
vendor/bin/phpcs --config-set installed_paths vendor/drupal/coder/coder_sniffer
```

### Running phpcs when PHP is in a container (DDEV / Lando / Docker)

If your project runs PHP inside a container, the host has no `php` to execute
`vendor/bin/phpcs` — point `command` at the container instead:

```yaml
# DDEV
phpcs:
  command: "ddev exec phpcs"

# Lando (with a "phpcs" tooling command, or via SSH)
phpcs:
  command: "lando phpcs"

# Docker Compose (service name "php"; -T disables TTY allocation)
phpcs:
  command: "docker compose exec -T php vendor/bin/phpcs"
```

The command runs from the project root, and file paths are passed repo-relative, so the
container must mount the project at its working directory (DDEV/Lando do this by default).
Make sure `drupal/coder` is installed **inside the container** (run the `composer require`
above through `ddev composer` / `lando composer` so it lands in the container's vendor/).

PHP/Drupal uses phpcs; **JS and CSS use ESLint and Stylelint** the same way (see
below). YAML is still reviewed by the LLM.

---

### eslint / stylelint

```yaml
eslint:
  enabled: true              # omit = auto (run when installed + configured); false disables
  command: "lando eslint"    # optional — how to invoke it
stylelint:
  command: "lando stylelint"
```

**Type:** block (optional), one per linter

The JS/CSS analog of [phpcs](#phpcs). JS/CSS review was previously 100% LLM, and a
local model under-reports mechanical issues (it missed 62 `var` declarations and 8
`innerHTML` uses in one real module). **ESLint** and **Stylelint** are
deterministic — they cannot miss a `var` or a `!important` — so the mechanical
JS/CSS rules are sourced from them, and the LLM is left to the semantic findings a
linter can't make (XSS logic, error handling).

When ESLint is active the LLM stops checking `js-no-var`, `js-strict-equality`,
`js-no-console-log`, and `js-no-unused-vars`. When Stylelint is active it stops
checking `css-color-format`, `css-no-duplicate-selectors`, `css-no-important`, and
`css-max-nesting`. The LLM keeps those rules when the linter isn't available, so
the category is never left unchecked.

- **`enabled`** — omit for *auto*; `false` disables; `true` forces it (still only
  runs if it works).
- **`command`** — how to invoke the linter (space-separated). Unset = locate
  `node_modules/.bin/<tool>` or `<tool>` on `PATH`. Set this when node runs in a
  container.

A linter is only treated as active if it **actually executes** (a `--version`
probe) **AND a project config resolves** (`eslint.config.js` / `.eslintrc*` /
`.stylelintrc*` / a `package.json` key). Without a config the linter lints
nothing, so the tool falls back to the LLM rather than dropping the category —
the same "never leave a category uncovered" rule as phpcs.

### Setting up ESLint / Stylelint

```bash
# Dev dependencies:
npm install --save-dev eslint stylelint stylelint-config-standard

# Minimal configs in the project root:
#   eslint.config.js     (flat config) or .eslintrc.json
#   .stylelintrc.json  ->  { "extends": "stylelint-config-standard" }
```

In a container, invoke through it — e.g. `eslint: { command: "ddev exec eslint" }`.

---

### verify

```yaml
verify: true                 # off when omitted
verify_model: "qwen3.5:27b"  # optional — defaults to the review model
```

**Type:** boolean (optional) + string (optional)

**Default:** `false`

An opt-in **LLM second pass** that re-checks findings the deterministic gates
can't judge. The evidence, existence, and promoted-constructor gates catch
*fabricated* code, but not a finding that quotes **real** code while
**misreading** it — a "missing" null check that sits on the next line, a
"missing" try/catch that's actually present a few lines down, a `||` guard read
out of evaluation order.

When enabled, each **bug** and **security** finding is sent back to the model on
its own — with the file's numbered code and one question: *is this specific
defect really present in this code?* A finding the judge rejects is dropped; a
finding it confirms, an errored/timed-out call, or an unparseable verdict is
**kept** (the pass only ever drops on a clear "invalid", like every other gate).

- Scoped to bug/security findings — interpretation hallucinations cluster there;
  style and phpcs findings are never sent to it.
- Costs **one extra LLM call per in-scope finding**, so it's off by default and
  roughly doubles wall-clock on a finding-heavy review. Turn it on for a final
  pre-PR pass, not every iteration.
- **`verify_model`** lets a larger, more reliable judge vet a smaller model's
  findings (e.g. review with `qwen3.5:9b`, verify with `qwen3.5:27b`). Defaults
  to the review model.
- CLI/REPL equivalents: `code-review --verify …` and `/review --verify`.

Each dropped finding is recorded in the log with the judge's reason.

---

## Rule Overrides

Override built-in rules in the `rules` section. Each language has its own block, and each rule is referenced by its ID.

### Disabling a Rule

```yaml
rules:
  php:
    php-psr12-style:
      enabled: false
```

The rule will not be sent to the LLM and will show as disabled in `/rules`.

### Changing Severity

```yaml
rules:
  javascript:
    js-no-var:
      severity: info        # downgrade from error to info
```

**Valid severity values:** `error`, `warning`, `info`

This does not change what the LLM finds — it changes the severity label that appears in the review output for findings triggered by this rule.

### Override Multiple Rules

```yaml
rules:
  php:
    php-psr12-style:
      enabled: false
    php-type-declarations:
      severity: info
    php-error-handling:
      severity: warning

  javascript:
    js-no-console-log:
      enabled: false
    js-strict-equality:
      severity: info

  css:
    css-max-nesting:
      severity: warning
    css-color-format:
      enabled: false
```

### Language Keys

The key under `rules:` must match the language name in lowercase:

| Language | Key |
|----------|-----|
| PHP | `php` |
| Drupal | `drupal` |
| JavaScript | `javascript` |
| CSS | `css` |
| HTML | `html` |

Drupal rules have IDs prefixed with `drupal-` (not `php-`). To override a PHP rule that Drupal inherits, use the `drupal-` prefixed ID under the `drupal` key:

```yaml
rules:
  drupal:
    drupal-psr12-style:      # NOT php-psr12-style
      enabled: false
    drupal-dependency-injection:
      severity: warning
```

---

## Custom Rules

Custom rules are additional instructions sent to the LLM. They tell the LLM what extra things to check for beyond the built-in rules.

### Basic Custom Rule

```yaml
custom_rules:
  - id: no-debug-code
    description: "No debug statements (var_dump, dd, console.log) in production code"
    languages: [php, javascript]
    severity: error
```

### Multi-Language Rule

```yaml
custom_rules:
  - id: no-todo-comments
    description: "No TODO or FIXME comments — create tickets instead"
    languages: [php, drupal, javascript, css, html]
    severity: warning
```

### Global Rule (All Languages)

Omit `languages` entirely to apply a rule to every language:

```yaml
custom_rules:
  - id: max-file-length
    description: "Files should not exceed 500 lines of code"
    severity: info
```

This rule will be included in the review prompt for PHP, Drupal, JavaScript, CSS, and HTML files.

### Custom Rule Fields

| Field | Required | Type | Default | Description |
|-------|----------|------|---------|-------------|
| `id` | Yes | string | — | Unique identifier for the rule. Use lowercase with hyphens. |
| `description` | Yes | string | — | What the LLM should check for. Be specific and actionable. |
| `languages` | No | list of strings | (all languages) | Which languages this rule applies to. |
| `severity` | No | string | `warning` | Severity level: `error`, `warning`, or `info`. |

### Tips for Writing Good Custom Rules

**Be specific.** The `description` is sent directly to the LLM as a review instruction. Vague descriptions produce vague results.

Bad:
```yaml
- id: code-quality
  description: "Check for code quality issues"
```

Good:
```yaml
- id: no-magic-numbers
  description: "Numbers other than 0 and 1 should be extracted to named constants. Example: use MAX_RETRIES = 3 instead of hardcoding 3"
```

**Include examples** when the rule has nuance:

```yaml
- id: drupal-service-injection
  description: "Controllers must inject services via __construct, not use \\Drupal::service(). Bad: $node_storage = \\Drupal::entityTypeManager()->getStorage('node'). Good: public function __construct(EntityTypeManagerInterface $etm) { $this->storage = $etm->getStorage('node'); }"
```

**Use the right severity:**

| Severity | When to use |
|----------|-------------|
| `error` | Must fix. Blocks merge. Security issues, hard bugs, broken patterns. |
| `warning` | Should fix. Code quality, maintainability, team conventions. |
| `info` | Nice to fix. Style preferences, minor suggestions. |

---

## Excluding Files

Use the `exclude` field to skip files and directories from review. Excluded files are filtered out before any agents run — they won't be sent to the LLM.

```yaml
exclude:
  - .lando.yml
  - .gitignore
  - "*.log"
  - vendor/
  - node_modules/
```

### Supported Patterns

| Pattern | Matches | Example |
|---------|---------|---------|
| `filename` | Exact file name | `.lando.yml` matches `.lando.yml` only |
| `*.ext` | Files with extension | `*.log` matches `error.log`, `debug.log` |
| `*.suffix` | Files ending with | `*.min.js` matches `app.min.js` |
| `dir/` | Directory and all contents | `vendor/` matches `vendor/autoload.php` |
| `*/name/*` | Path containing directory | `*/test/*` matches `src/test/helper.php` |

### Common Excludes for Drupal Projects

```yaml
exclude:
  - .lando.yml
  - .gitignore
  - "*.log"
  - vendor/
  - node_modules/
  - core/                  # Drupal core (if you don't want to review it)
  - "*.min.js"
  - "*.min.css"
```

---

## How Rules Are Merged

When you run `/review`, rules are assembled in this order:

```
1. Built-in rules for each detected language (all enabled by default)
        ↓
2. Rule overrides from .codereview.yaml applied:
   - enabled: false  → rule is removed
   - severity: X     → rule severity is changed
        ↓
3. Custom rules from .codereview.yaml added on top
        ↓
4. Only enabled rules are sent to the LLM
        ↓
5. Rules are distributed to specialized sub-agents:
   - Security rules → SecurityAgent
   - Bug/error rules → BugDetectionAgent
   - Style/convention rules → LanguageStyleAgent (per language)
   - HTML accessibility rules → AccessibilityAgent
   - Custom rules → CustomRulesAgent
```

The final set of active rules is:
- All built-in rules that are still enabled (after overrides)
- Plus all custom rules whose `languages` match (or that have no `languages` filter)

Each sub-agent receives only its domain-relevant rules and makes a focused LLM call. This produces better results than sending all rules in one prompt.

You can see the final merged result by running `/rules` in the REPL.

### Which Agent Gets Which Rules?

| Agent | Rule IDs (suffix match) |
|-------|------------------------|
| SecurityAgent | `*-sql-injection`, `*-no-eval`, `*-no-hardcoded-secrets`, `*-xss-prevention` |
| BugDetectionAgent | `*-error-handling`, `*-type-declarations`, `*-no-unused-vars` |
| LanguageStyleAgent | Everything else for that language (PSR-12, DI, no-var, CSS rules) |
| AccessibilityAgent | `html-alt-text`, `html-semantic-elements`, `html-heading-hierarchy`, `html-form-labels`, `html-link-text`, `html-contrast` |
| CustomRulesAgent | All rules from `custom_rules` in your config |

Disabling a rule in `.codereview.yaml` removes it from its agent. If all rules for an agent are disabled, that agent is skipped entirely.

---

## How Rules Work

Rules in `code-review` are **not static analysis**. They are instructions embedded in the LLM's system prompt. When you run `/review`, the tool:

1. Detects languages from your changed files
2. Collects all active rules for those languages
3. Builds a system prompt that includes each rule as a check item
4. Sends your diff + the system prompt to Ollama
5. The LLM reads the diff and applies the rules as it reviews

This means:
- Rules are advisory — the LLM uses its judgment to apply them
- Rules work best when they describe patterns the LLM can recognize in code
- The LLM may find issues not covered by any rule (it's a general code reviewer too)
- Very specific patterns (like regex matches) are better handled by static analysis tools

---

## All Built-in Rule IDs

Use these IDs in the `rules:` section to override built-in rules.

### PHP

| Rule ID | Default Severity |
|---------|-----------------|
| `php-psr12-style` | warning |
| `php-type-declarations` | warning |
| `php-error-handling` | error |
| `php-sql-injection` | error |
| `php-no-eval` | error |
| `php-no-hardcoded-secrets` | error |

### Drupal

Inherits all PHP rules with `drupal-` prefix, plus:

| Rule ID | Default Severity |
|---------|-----------------|
| `drupal-psr12-style` | warning |
| `drupal-type-declarations` | warning |
| `drupal-error-handling` | error |
| `drupal-sql-injection` | error |
| `drupal-no-eval` | error |
| `drupal-no-hardcoded-secrets` | error |
| `drupal-dependency-injection` | error |
| `drupal-hook-attributes` | warning |
| `drupal-coding-standards` | warning |
| `drupal-no-direct-db` | warning |

### JavaScript

| Rule ID | Default Severity |
|---------|-----------------|
| `js-no-var` | error |
| `js-strict-equality` | warning |
| `js-no-unused-vars` | warning |
| `js-error-handling` | error |
| `js-no-console-log` | warning |
| `js-xss-prevention` | error |

### CSS

| Rule ID | Default Severity |
|---------|-----------------|
| `css-no-important` | warning |
| `css-no-duplicate-selectors` | warning |
| `css-max-nesting` | info |
| `css-color-format` | info |

### HTML

| Rule ID | Default Severity |
|---------|-----------------|
| `html-alt-text` | error |
| `html-semantic-elements` | warning |
| `html-heading-hierarchy` | warning |
| `html-form-labels` | error |
| `html-link-text` | warning |
| `html-contrast` | warning |

### Twig (reviewed as HTML)

| Rule ID | Default Severity |
|---------|-----------------|
| `twig-no-raw` | error |
| `twig-autoescape` | warning |
| `twig-trans` | warning |
| `twig-no-php` | error |
| `twig-attach-library` | warning |
| `twig-undefined-vars` | error |

### YAML

| Rule ID | Default Severity |
|---------|-----------------|
| `yaml-valid-syntax` | error |
| `yaml-consistent-indent` | warning |
| `yaml-no-duplicate-keys` | error |
| `yaml-quote-special-values` | warning |
| `yaml-no-hardcoded-secrets` | error |

---

## Verifying Your Configuration

### Check if the file was loaded

```
cr> /config

Current configuration:

  model: qwen3-coder:30b
  output_format: terminal
  languages: auto-detect
  rule overrides:
    php/php-psr12-style: enabled=false
  custom rules:
    no-debug-code — No debug statements in production code

  Edit .codereview.yaml to change configuration.
```

If your overrides and custom rules appear here, the file was loaded correctly. If it shows only defaults, check your YAML syntax.

### Check which rules are active

```
cr> /rules

PHP (3 file(s) detected)
  ✓ [warning] Use type declarations for function parameters...
  ✓ [error] No empty catch blocks...
  ✓ [error] No raw SQL concatenation...
  ✓ [error] Never use eval()...
  ✓ [error] No hardcoded passwords...
  ✗ [warning] PSR-12 coding style... (disabled)
  ✓ [error] No debug statements in production code (custom)

JavaScript (2 file(s) detected)
  ✓ [error] Use const or let instead of var...
  ✓ [warning] Use === and !==...
  ✓ [error] No debug statements in production code (custom)
```

- `✓` = enabled and will be checked
- `✗` = disabled via override
- `(custom)` = defined in `.codereview.yaml`, not built-in

### Verify during review

After running `/review`, the footer shows how many rules were applied:

```
Reviewed 5 file(s) in 3.2s using qwen3-coder:30b (18 rules: PHP, JavaScript)
```

If you have custom config, a hint appears:

```
Tip: Run /rules to see which rules were active for this review.
```

---

## Custom Agents

Custom agents let you define entirely new review agents with specialized system prompts. This is for power users who want domain-specific review passes beyond what the built-in agents and custom rules provide.

**When to use custom agents vs custom rules:**
- **Custom rules** — add checks to existing agents ("also look for dd() calls")
- **Custom agents** — create a new specialist ("you are a PCI-DSS compliance reviewer")

### Schema

```yaml
agents:
  - name: string          # Required. Display name shown in review output.
    prompt: string         # Required. System prompt text. JSON output schema
                           #   is appended automatically — do NOT include it.
    languages: [string]    # Optional. Limit to specific languages.
                           #   Empty = runs on all files in the diff.
    rules:                 # Optional. Inline rules for this agent.
      - id: string         #   Required. Unique rule identifier.
        description: string #  Required. What to check for.
        severity: error | warning | info  # Optional. Default: warning.
    enabled: bool          # Optional. Default: true. Set false to disable
                           #   without removing the config.
```

### How It Works

1. The tool auto-appends the JSON output schema to your prompt (so the LLM always returns parseable results)
2. If you define `rules`, they're formatted and appended after your prompt text
3. If you set `languages`, only files matching those languages are sent to this agent
4. Your custom agents run **after** all built-in agents (Security, Bug Detection, Style, Accessibility, Custom Rules)
5. Findings are merged and deduplicated with the built-in agent findings

### Example: PCI-DSS Compliance Agent

```yaml
agents:
  - name: "PCI-DSS Compliance"
    prompt: |
      You are a PCI-DSS compliance reviewer for payment processing code.
      Your ONLY job is to find violations of PCI-DSS requirements.

      Focus areas:
      - Cardholder data must never be logged or stored in plaintext
      - Payment card numbers must be masked in all output
      - Encryption keys must not be hardcoded
      - All payment API calls must use TLS
    languages: [php, javascript]
    rules:
      - id: pci-no-pan-logging
        description: "Never log or store full payment card numbers"
        severity: error
      - id: pci-mask-output
        description: "Mask card numbers in all user-facing output"
        severity: error
      - id: pci-no-hardcoded-keys
        description: "Never hardcode encryption or API keys"
        severity: error
```

### Example: Laravel Conventions Agent

```yaml
agents:
  - name: "Laravel Conventions"
    prompt: |
      You are a Laravel framework expert reviewing code for adherence
      to Laravel conventions and best practices.

      Focus areas:
      - Use Form Requests for validation, not inline validation
      - Use Eloquent relationships instead of raw joins
      - Use route model binding instead of manual lookups
      - Use Laravel's built-in helpers over raw PHP equivalents
      - Use Laravel's service container for dependency injection
    languages: [php]
    rules:
      - id: laravel-form-requests
        description: "Use Form Request classes for validation, not $request->validate()"
        severity: warning
      - id: laravel-eloquent
        description: "Use Eloquent relationships instead of raw DB::table() joins"
        severity: warning
      - id: laravel-route-model
        description: "Use route model binding instead of Model::find($id)"
        severity: info
```

### Example: Performance Agent

```yaml
agents:
  - name: "Performance Review"
    prompt: |
      You are a performance-focused code reviewer. Your ONLY job is to
      find performance issues and inefficiencies.

      Focus areas:
      - N+1 query problems in loops
      - Missing database indexes on queried columns
      - Unnecessary memory allocations
      - Blocking I/O in async contexts
      - Missing caching for expensive operations
    rules:
      - id: perf-n-plus-1
        description: "Database queries inside loops (N+1 problem)"
        severity: error
      - id: perf-missing-cache
        description: "Expensive operations without caching"
        severity: warning
```

### Example: Adding an Unsupported Language

```yaml
agents:
  - name: "Python Review"
    prompt: |
      You are a Python code reviewer expert in PEP 8, type hints,
      and modern Python best practices (3.10+).

      Focus areas:
      - Use type hints on all function signatures
      - Use f-strings instead of .format() or %
      - Use pathlib instead of os.path
      - Use dataclasses or Pydantic for data structures
      - Handle exceptions specifically, never bare except
    rules:
      - id: py-type-hints
        description: "All functions must have type hints"
        severity: warning
      - id: py-fstrings
        description: "Use f-strings instead of .format() or % formatting"
        severity: info
      - id: py-no-bare-except
        description: "Never use bare except — always specify the exception type"
        severity: error
```

Note: For unsupported languages, omit the `languages` field so the agent runs on all files, or ensure your files have extensions the tool can detect.

### Disabling a Custom Agent

```yaml
agents:
  - name: "PCI-DSS Compliance"
    prompt: "..."
    enabled: false    # temporarily disabled
```

### Important Notes

- **JSON schema is automatic** — never include JSON output instructions in your prompt. The tool appends them for you. If your prompt conflicts with the output format, parsing will fail.
- **One LLM call per agent** — each custom agent makes a separate call to Ollama. Keep the number of agents reasonable (under 8 total including built-ins) to avoid slow reviews.
- **Prompts are instructions, not code** — write your prompt as if you're briefing a human reviewer. The LLM reads your prompt, then reads the diff, and reports findings.
- **Built-in agents still run** — your custom agents run in addition to the 5 built-in agents, not instead of them. Use `rules:` overrides to disable built-in rules you don't want.

---

## Examples

### Drupal Project

```yaml
model: qwen3-coder:30b

rules:
  drupal:
    drupal-hook-attributes:
      enabled: false           # still on Drupal 10, no hook attributes yet

custom_rules:
  - id: no-dd
    description: "No dd(), dump(), or dpm() debug calls in production code"
    languages: [drupal]
    severity: error

  - id: service-injection
    description: "All custom services must be defined in MODULE.services.yml with proper autowiring. No inline service creation."
    languages: [drupal]
    severity: warning

  - id: render-arrays
    description: "Use render arrays instead of inline HTML in PHP code. Never concatenate HTML strings."
    languages: [drupal]
    severity: warning

  - id: cache-tags
    description: "Custom queries and entity loads must include proper cache tags and contexts for cache invalidation"
    languages: [drupal]
    severity: warning
```

### JavaScript-Heavy Project

```yaml
model: devstral:24b

languages:
  - javascript
  - css
  - html

rules:
  javascript:
    js-no-console-log:
      enabled: false           # we use console.log for client-side logging
    js-strict-equality:
      severity: error          # upgrade to error — enforce strict equality

custom_rules:
  - id: no-any-type
    description: "Do not use the 'any' type in TypeScript. Use proper types or 'unknown' with type guards."
    languages: [javascript]
    severity: error

  - id: react-hooks-rules
    description: "Follow React hooks rules: no hooks inside conditions/loops, hooks must be called at the top level of the component"
    languages: [javascript]
    severity: error

  - id: css-bem-naming
    description: "CSS classes must follow BEM naming convention: block__element--modifier"
    languages: [css]
    severity: warning
```

### Strict Security Team

```yaml
model: qwen3-coder:30b

rules:
  php:
    php-sql-injection:
      severity: error
    php-no-hardcoded-secrets:
      severity: error
  javascript:
    js-xss-prevention:
      severity: error

custom_rules:
  - id: no-file-uploads-without-validation
    description: "All file upload handlers must validate file type, size, and content. No direct filesystem writes from user input."
    languages: [php, drupal]
    severity: error

  - id: csrf-protection
    description: "All state-changing endpoints must have CSRF token validation"
    languages: [php, drupal]
    severity: error

  - id: input-sanitization
    description: "All user input must be sanitized before use. Use Xss::filter() in Drupal, htmlspecialchars() in PHP, DOMPurify in JavaScript."
    languages: [php, drupal, javascript]
    severity: error

  - id: no-sensitive-data-in-logs
    description: "Never log passwords, tokens, API keys, PII, or credit card numbers"
    severity: error

  - id: https-only
    description: "All external URLs must use HTTPS, not HTTP"
    severity: error
```

### Minimal Config

The smallest useful config — just override the model:

```yaml
model: gemma4
```

Everything else uses defaults: all languages auto-detected, all 43 built-in rules enabled, terminal output.
