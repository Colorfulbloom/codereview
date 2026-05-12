# Code Review

AI-powered local code review using [Ollama](https://ollama.com). Your code never leaves your machine.

Built for development teams that cannot use paid AI platforms (Claude, ChatGPT, etc.) due to security policies. Reviews PHP, Drupal, JavaScript, CSS, and HTML codebases using specialized sub-agents that produce focused, accurate results from local LLMs.

---

## Features

- **Specialized sub-agents** -- Security, Bug Detection, Style, Accessibility, and Custom agents each make focused LLM calls for higher accuracy
- **Local AI** -- uses Ollama with any model that fits your hardware. No data leaves your machine
- **Language-aware** -- [43 built-in rules](#built-in-rules) for PHP, Drupal, JavaScript, CSS, HTML/Twig, and YAML
- **Interactive REPL** -- explore diffs, review code, commit, all from one session
- **CI/CD ready** -- non-interactive mode with JSON, Markdown, and GitHub Actions annotation output
- **Team configurable** -- shared `.codereview.yaml` for custom rules, custom agents, and rule overrides
- **Git integration** -- diffs, staging, and AI-generated commit messages
- **Cross-platform** -- macOS, Windows, Linux
- **Hardware-aware** -- detects your system RAM and recommends models that fit

---

## Installation

### Prerequisites

1. [Rust](https://rustup.rs/) (stable)
2. [Ollama](https://ollama.com) installed and running
3. A model pulled (the `init` wizard helps you choose one)

### Recommended Models

| Model               | RAM Needed | Best For                              |
| ------------------- | ---------- | ------------------------------------- |
| `gemma4`            | ~6GB       | Fast reviews, smaller edits           |
| `qwen2.5-coder:14b` | ~10GB      | Strong code specialist                |
| `devstral:24b`      | ~15GB      | Deep agentic analysis                 |
| `qwen3-coder:30b`   | ~20GB      | Best overall quality, 256K context    |
| `qwen2.5-coder:32b` | ~22GB      | Highest accuracy                      |
| `llama3.3:70b`      | ~48GB      | Excellent all-rounder (64GB+ systems) |

```bash
ollama pull gemma4
```

### Build and Install

```bash
git clone <repo-url>
cd code-review
cargo build --release

# Install system-wide (optional)
cargo install --path .
```

---

## Quick Start

### First Run and Onboarding

```bash
cd /path/to/your/project
code-review
```

The onboarding wizard runs automatically on first launch:

1. **Ollama Check** -- verifies Ollama is installed and running
2. **Model Selection** -- pick a model (or pull one). Detects your hardware and recommends models that fit
3. **Platform Linking** -- connect GitHub/GitLab accounts (optional)
4. **Preferences** -- output format, auto-stage setting
5. **Team Config** -- generate a `.codereview.yaml` starter file

Every step is skippable. If interrupted, progress is saved and resumes next time.

```bash
code-review onboard          # resume from where you left off
code-review onboard --reset  # start fresh
```

### Generate a Config File

```bash
code-review init    # interactive wizard for .codereview.yaml
```

Or from the REPL: `/init`

The wizard detects your hardware, queries Ollama for installed models, and walks you through model selection, language configuration, rule overrides, custom rules, and custom agents.

### Your First Review

```bash
code-review
```

```
cr> /status    # see branch + changed files
cr> /diff      # preview the diff (colored)
cr> /review    # run the AI review
```

---

## How Reviews Work

When you run `/review`, the tool doesn't send one massive prompt to the LLM. Instead, it uses **specialized sub-agents**, each focused on a specific domain.

### The Sub-Agent Pipeline

```
/review
  1. Git Agent          -- gets your diff (unstaged, staged, or branch)
  2. Language Agent      -- detects PHP, Drupal, JS, CSS, HTML
  3. SecurityAgent       -- SQL injection, XSS, secrets, eval
  4. BugDetectionAgent   -- error handling, type safety, unused code
  5. LanguageStyleAgent  -- per-language: PSR-12, Drupal DI, no-var, CSS rules
  6. AccessibilityAgent  -- WCAG, alt text, form labels (HTML/CSS only)
  7. CustomRulesAgent    -- your team's rules from .codereview.yaml
  8. Config Agents       -- your custom agents from .codereview.yaml
  9. Output Agent        -- merge, deduplicate, format
```

### Why Multiple Agents?

Local LLMs (especially 6-12GB models) produce significantly better results with focused, shorter prompts. Instead of one prompt with 43 rules asking "check everything," each agent gets 4-10 rules and a clear instruction like "Your ONLY job is to find security vulnerabilities." The LLM can focus and produce more accurate findings.

### What Gets Skipped

- **AccessibilityAgent** -- only when HTML or CSS files are in the diff
- **CustomRulesAgent** -- only when you have `custom_rules` in `.codereview.yaml`
- **Custom Agents** -- only when you define `agents` in `.codereview.yaml`
- **LanguageStyleAgent** -- one per detected language (only languages in your diff)
- Any agent with zero applicable rules is skipped entirely

---

## REPL Commands

Launch the REPL with `code-review`. The prompt is `cr>`. Tab completion is supported.

### Review

| Command   | Description                                                   |
| --------- | ------------------------------------------------------------- |
| `/review` | Run a code review on your current changes                     |
| `/diff`   | View the current diff (colored: green = added, red = removed) |
| `/rules`  | Show active review rules per detected language                |
| `/commit` | Stage files and commit with an AI-generated message           |

### Configuration

| Command    | Description                                       |
| ---------- | ------------------------------------------------- |
| `/config`  | View current configuration                        |
| `/output`  | Set output format                                 |
| `/models`  | List available Ollama models (shows active model) |
| `/init`    | Generate a `.codereview.yaml` for your project    |
| `/onboard` | Re-run the onboarding wizard                      |

### Session

| Command   | Description                                  |
| --------- | -------------------------------------------- |
| `/status` | Show branch, changed files, and active model |
| `/help`   | Show all available commands                  |
| `/quit`   | Exit the REPL                                |

---

## Non-Interactive Mode

Run a review without entering the REPL. Designed for CI/CD and scripting.

```bash
# Review changes vs a branch
code-review --diff main

# Output as JSON
code-review --diff main --format json

# Write a markdown report to file
code-review --diff main --format markdown -o review-report.md

# GitHub Actions annotations
code-review --diff main --format annotations

# Override the model
code-review --diff main -m qwen3-coder:30b --format json
```

### CLI Flags

| Flag              | Short | Description                                   | Default           |
| ----------------- | ----- | --------------------------------------------- | ----------------- |
| `--diff <REF>`    |       | Branch, commit, or ref to diff against        | (enters REPL)     |
| `--format <FMT>`  |       | `terminal`, `json`, `markdown`, `annotations` | `terminal`        |
| `--model <NAME>`  | `-m`  | Override the Ollama model                     | (from onboarding) |
| `--output <PATH>` | `-o`  | Write output to file                          | (stdout)          |

### Subcommands

| Command               | Description                                           |
| --------------------- | ----------------------------------------------------- |
| `code-review onboard` | Run the onboarding wizard (`--reset` for fresh start) |
| `code-review init`    | Generate `.codereview.yaml` interactively             |

---

## Output Formats

### Terminal (default)

```
Found 3 issue(s): 1 error(s), 1 warning(s), 1 info

src/Controller.php
  [E] line 42: SQL injection risk
    Raw SQL concatenation with user input
    Fix: Use parameterized queries

  [W] line 15: Missing type declaration
    Parameter $name has no type hint
    Fix: Add string type: function getName(string $name)

Reviewed 3 file(s) in 2.1s using gemma4 (14 rules: PHP, JavaScript)
```

### JSON

Full structured output with `findings`, `files_reviewed`, `model_used`, `duration`, `rules_applied`, and `languages_detected`. Pipe into other tools or store for analysis.

### Markdown

Report with summary table, per-file issue tables, and metadata. Attach to PRs or share with team leads.

### Annotations

GitHub Actions workflow command format:

```
::error file=src/Controller.php,line=42::SQL injection risk: Raw SQL concatenation
::warning file=src/Controller.php,line=15::Missing type: No type hint
```

---

## Configuration

Create a `.codereview.yaml` in your project root. Check it into version control so your team shares the same settings.

### Quick Example

```yaml
model: qwen3-coder:30b

exclude:
  - .lando.yml
  - .gitignore
  - "*.log"
  - vendor/
  - node_modules/

rules:
  php:
    php-psr12-style:
      enabled: false
  javascript:
    js-no-console-log:
      enabled: false

custom_rules:
  - id: no-debug-code
    description: "No dd(), var_dump(), or console.log() in production"
    languages: [php, javascript]
    severity: error

agents:
  - name: "Laravel Conventions"
    prompt: |
      You are a Laravel expert. Check for Form Requests,
      Eloquent relationships, and route model binding.
    languages: [php]
```

### Built-in Rules

| Language    | Rules | Examples                                                                      |
| ----------- | ----- | ----------------------------------------------------------------------------- |
| PHP         | 6     | PSR-12 style, type declarations, SQL injection, eval, secrets                 |
| Drupal      | 10    | All PHP rules + dependency injection, hook attributes, coding standards       |
| JavaScript  | 6     | no-var, strict equality, error handling, XSS prevention                       |
| CSS         | 4     | No !important, nesting depth, duplicate selectors                             |
| HTML + Twig | 12    | Alt text, semantic elements, WCAG 2.2, Twig undefined vars, raw filter, trans |
| YAML        | 5     | Valid syntax, indentation, duplicate keys, special values, no secrets         |

See [all 43 rule IDs with severities](docs/CONFIGURATION.md#all-built-in-rule-ids) in the configuration reference.

### What You Can Configure

- **Exclude files and directories** -- skip `.lando.yml`, `vendor/`, `*.log`, etc.
- **Override built-in rules** -- disable or change severity
- **Add custom rules** -- team-specific checks (the LLM follows your instructions)
- **Create custom agents** -- specialized reviewers with their own system prompts (PCI-DSS, Laravel, performance, etc.)
- **Set model and output format** -- per-project defaults

Full reference with all 43 rule IDs, custom agent schema, and real-world examples: **[docs/CONFIGURATION.md](docs/CONFIGURATION.md)**

---

## CI/CD Integration

### GitHub Actions

```yaml
name: Code Review
on: [pull_request]

jobs:
  review:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Install Ollama
        run: curl -fsSL https://ollama.com/install.sh | sh

      - name: Pull model
        run: ollama pull gemma4

      - name: Install code-review
        run: cargo install --path .

      - name: Run review
        run: code-review --diff origin/main --format annotations
```

### GitLab CI

```yaml
code-review:
  stage: test
  script:
    - curl -fsSL https://ollama.com/install.sh | sh
    - ollama pull gemma4 &
    - cargo install --path .
    - wait
    - code-review --diff origin/main --format json -o review.json
  artifacts:
    paths:
      - review.json
```

---

## Git Workflow

### Reviewing and Committing

```
cr> /review    # review changes
cr> /commit    # stage, write message, commit
```

The `/commit` workflow:

1. Shows all changed files (staged + unstaged, deduplicated)
2. Multi-select which files to stage (handles deleted files)
3. Generates a commit message from review findings
4. Lets you edit or replace the message
5. Confirms before committing
6. No AI marker is ever added to the commit

### Status

```
cr> /status

  Branch:  feature/my-branch
  Model:   gemma4:latest
  Staged:
    added new_file.php
  Unstaged:
    modified src/Controller.php
```

---

## Language Detection

Languages are auto-detected from file extensions in your diff.

| Language   | Extensions                                               |
| ---------- | -------------------------------------------------------- |
| PHP        | `.php`, `.inc`                                           |
| Drupal     | `.module`, `.install`, `.theme`, `.profile`, `.info.yml` |
| JavaScript | `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.tsx`             |
| CSS        | `.css`, `.scss`, `.sass`, `.less`                        |
| HTML       | `.html`, `.htm`, `.twig`                                 |

**Drupal auto-detection:** If your project contains `.info.yml` files, `.module` files, or `core/lib/Drupal`, all `.php` files are automatically promoted to Drupal and get Drupal-specific rules.

---

## Platform Linking

Connect GitHub or GitLab accounts during onboarding for future PR integration.

**GitHub:** Create a [Fine-Grained PAT](https://github.com/settings/tokens?type=beta) with permissions: `contents` (read), `pull_requests` (read+write), `metadata` (read).

**GitLab:** Create a PAT at User Settings > Access Tokens with scopes: `api`, `read_api`. Supports custom instance URLs for self-hosted GitLab.

Multiple accounts are supported (useful for submodules across platforms).

---

## Data Storage

Onboarding state is stored in SQLite:

| Platform | Path                                                       |
| -------- | ---------------------------------------------------------- |
| macOS    | `~/Library/Application Support/code-review/code-review.db` |
| Linux    | `~/.local/share/code-review/code-review.db`                |
| Windows  | `%APPDATA%\code-review\code-review.db`                     |

**Reset:**

```bash
code-review onboard --reset     # reset onboarding only
rm <path-above>                 # full reset (delete database)
```

---

## Troubleshooting

### "Cannot connect to Ollama"

```bash
curl http://127.0.0.1:11434    # check if running
ollama serve                    # start it
```

### "No changes to review"

The tool reviews unstaged changes first, then falls back to staged. Make sure you have uncommitted changes in a git repository.

### "No models available"

```bash
ollama pull gemma4
```

### Review takes too long

- Use a smaller model: `code-review -m gemma4`
- The timeout is 300 seconds per agent call
- Fewer files = faster (each agent processes the diff)

### Wrong language detected

Run `/rules` to see detected languages. If PHP shows as Drupal, it's because Drupal markers (`.info.yml`, `.module`) were found in your project.

### Custom rules not appearing

1. Verify `.codereview.yaml` is in the project root
2. Run `/config` to confirm it loaded
3. Run `/rules` to see custom rules tagged with `(custom)`
4. Check YAML syntax -- invalid YAML falls back to defaults silently

---

## License

MIT
