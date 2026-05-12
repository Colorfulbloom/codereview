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

If the file has invalid YAML syntax, the tool silently falls back to defaults. Run `/config` in the REPL to verify your file was loaded.

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

**Valid values:** `php`, `drupal`, `javascript`, `css`, `html`

When specified, only these languages are reviewed — even if other file types appear in the diff. This is useful for projects where you want to focus reviews on specific languages.

When omitted, the tool auto-detects languages from changed file extensions. If Drupal project markers are found (`.info.yml`, `.module`, `core/lib/Drupal`), PHP files are automatically promoted to Drupal.

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
