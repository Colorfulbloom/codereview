# Code Review

AI-powered local code review using [Ollama](https://ollama.com). Your code never leaves your machine.

Built for development teams that cannot use paid AI platforms (Claude, ChatGPT, etc.) due to security policies. Reviews PHP, Drupal, JavaScript, CSS, and HTML codebases using specialized sub-agents that produce focused, accurate results from local LLMs.

---

## Features

- **Specialized sub-agents** -- Security, Bug Detection, Style, Accessibility, and Custom agents each make focused LLM calls for higher accuracy
- **Local AI** -- uses Ollama with any model that fits your hardware. No data leaves your machine
- **Deterministic + AI** -- when a linter is installed, the mechanical rules come from it (exact, no hallucination) and the LLM is reserved for the semantic findings a linter can't make: PHP/Drupal from `phpcs` (dependency injection, coding standards), JS from **ESLint** and CSS from **Stylelint** (`var`, `===`, `!important`, ...). See [linter config](docs/CONFIGURATION.md#eslint--stylelint).
- **Language-aware** -- [43 built-in rules](#built-in-rules) for PHP, Drupal, JavaScript, CSS, HTML/Twig, and YAML
- **Review anything** -- your uncommitted changes, your branch's commits before a PR (`--diff` / `/review --diff <ref>`), or any module, theme, or file as-is (`--path` / `/review <path>`)
- **Reviews uncommitted *and* untracked code** -- brand-new files don't need to be `git add`ed first
- **Context-aware chunking** -- auto-detects the model's context window and splits large reviews to fit, so nothing is silently truncated or rejected
- **Interactive REPL** -- explore diffs, review code, commit, all from one session
- **CI/CD ready** -- non-interactive mode with JSON, Markdown, and GitHub Actions annotation output
- **Team configurable** -- shared `.codereview.yaml` for custom rules, custom agents, and rule overrides
- **Git integration** -- diffs, staging, and AI-generated commit messages
- **Cross-platform** -- macOS, Windows, Linux
- **Hardware-aware** -- detects your system RAM and recommends models that fit
- **Hallucination-resistant** -- every finding must quote the offending line; quotes are verified against the actual source and unverifiable findings are dropped before you see them

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

### Build

```bash
git clone <repo-url>
cd code-review
cargo build --release
```

This produces an optimized binary at `target/release/code-review`.

### Install for the current user

```bash
cargo install --path .
```

Installs to `~/.cargo/bin/code-review` (make sure `~/.cargo/bin` is on your `PATH`).

### Install globally for all users

To make `code-review` available to **every** user on the machine, build as your
normal user and copy the release binary into a system directory:

```bash
cargo build --release
sudo install -m 755 target/release/code-review /usr/local/bin/code-review
```

Verify:

```bash
which code-review     # -> /usr/local/bin/code-review
code-review --help
```

> **Why not `sudo cargo install`?** Running the whole install as root recompiles
> the project as root and fails, because rustup's toolchain (`cargo`,
> `RUSTUP_HOME`, `CARGO_HOME`) lives under *your* home directory, not root's.
> Build as yourself and elevate only the final copy. If you must use
> `cargo install` directly, preserve your environment:
>
> ```bash
> sudo env "PATH=$PATH" CARGO_HOME="$HOME/.cargo" RUSTUP_HOME="$HOME/.rustup" \
>   cargo install --path . --root /usr/local
> ```

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

The wizard detects your hardware, queries Ollama for installed models, and walks you through model selection, performance settings (per-request LLM timeout), language configuration, rule overrides, custom rules, and custom agents.

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
  1. Source Agent        -- gets your diff (unstaged, staged, untracked, branch) or a --path target
  2. Language Agent      -- detects PHP, Drupal, JS, CSS, HTML
  3. SecurityAgent       -- SQL injection, XSS, secrets, eval
  4. BugDetectionAgent   -- error handling, type safety, unused code
  5. LanguageStyleAgent  -- per-language: PSR-12, Drupal DI, no-var, CSS rules
  6. AccessibilityAgent  -- WCAG, alt text, form labels (HTML/CSS only)
  7. TwigAgent           -- |raw filter, autoescape, undefined vars (.twig files only)
  8. CustomRulesAgent    -- your team's rules from .codereview.yaml
  9. Config Agents       -- your custom agents from .codereview.yaml
  10. Output Agent       -- merge, deduplicate, format
```

### Why Multiple Agents?

Local LLMs (especially 6-12GB models) produce significantly better results with focused, shorter prompts. Instead of one prompt with 43 rules asking "check everything," each agent gets 4-10 rules and a clear instruction like "Your ONLY job is to find security vulnerabilities." The LLM can focus and produce more accurate findings.

### What Gets Skipped

- **AccessibilityAgent** -- only when HTML or CSS files are in the diff
- **TwigAgent** -- only when `.twig` files are in the diff
- **CustomRulesAgent** -- only when you have `custom_rules` in `.codereview.yaml`
- **Custom Agents** -- only when you define `agents` in `.codereview.yaml`
- **LanguageStyleAgent** -- one per detected language (only languages in your diff)
- Any agent with zero applicable rules is skipped entirely

### Accuracy: how hallucinated findings are filtered

Local models sometimes report issues with confidence that aren't real. Several
deterministic checks run *after* the model and before you see the output -- no
extra LLM calls:

- **Evidence check** -- every finding must quote the offending line, and that
  quote must actually appear in the reviewed code, or it's dropped.
- **No-op fix check** -- a "fix" identical to the existing code is discarded.
- **Existence gate** -- a finding claiming an API "does not exist / will fatal
  error" is dropped when that method or property is actually defined in your
  project or framework source (e.g. `vendor/`, Drupal `core/`). This kills the
  most damaging false positives -- the ones that push you to revert correct
  code.
- **Promoted-constructor gate** -- a "property never defined / mismatch" finding
  is dropped when the constructor uses PHP 8 property promotion (the model
  misreading promoted params as undefined).

Each discarded finding is recorded in the log with the reason (and, for the
existence gate, the `file:line` proof).

Those checks are deterministic and free, but they can't catch a finding that
quotes *real* code while *misreading* it -- a "missing" null check that sits on
the next line, a "missing" try/catch that's actually present, a `||` guard read
out of evaluation order. For those, add **`--verify`** (or `verify: true` in
config): an opt-in LLM second pass that re-checks each bug/security finding
against its code and drops the ones it judges to misread correct code. It costs
one extra LLM call per in-scope finding, so it's off by default; point it at a
larger judge model with `verify_model:` if you like. See
[verify config](docs/CONFIGURATION.md#verify).

---

## What Gets Reviewed

By default a review looks at your **git changes**, and "changes" now includes
brand-new files you haven't staged yet:

- **Unstaged** edits to tracked files
- **Staged** changes
- **Untracked** files (new files not yet `git add`ed) -- gitignored files are still skipped

You can also review **existing code as-is**, independent of git, by pointing the
tool at a path. This is how you review an already-committed module, a theme, or
any loose file -- there's no diff required.

```bash
cd /path/to/your/project

# Review an entire custom module, regardless of git state
code-review --path docroot/modules/custom/my_module

# A single file
code-review --path docroot/modules/custom/my_module/src/Controller/MyController.php

# Same review, machine-readable / saved to a file
code-review --path docroot/modules/custom/my_module --format json
code-review --path docroot/modules/custom/my_module --format markdown -o review.md

# With a specific model
code-review --path docroot/modules/custom/my_module -m qwen3-coder:30b
```

Or from the REPL:

```
cr> /review                       # your current changes (default)
cr> /review path/to/module        # review everything under a path, as-is
```

In path mode, every supported file under the path is reviewed. Unsupported,
binary, and very large files (>256KB, typically generated/minified assets) are
skipped.

### Pre-PR Review (commits vs a base branch)

Before pushing a branch for a PR, review **what the PR will actually contain**
-- your branch's commits, diffed against the base branch:

```bash
code-review --diff origin/main          # or main, a tag, a SHA, HEAD~3 ...
```

Or from the REPL:

```
cr> /review --diff origin/main
```

Two things to know:

- The diff is taken from the **merge base** (like a GitHub PR), so it covers
  only *your* commits -- never changes the base branch gained after you
  branched off.
- It reviews **committed work only**. If you have uncommitted edits, the tool
  prints a note reminding you they're not included -- commit them or run a
  plain `/review`.

If you pass a bare argument (`/review main`), the tool resolves it
filesystem-first: an existing file or directory is reviewed as a path, anything
else is tried as a git ref, and a note tells you which interpretation was used.

---

## Context Window & Large Reviews

The tool sizes the model's context window for you so large reviews don't fail or
get silently truncated:

1. It **auto-detects** the model's maximum context length from Ollama.
2. It requests an appropriate `num_ctx` (default cap **32768** tokens, capped by
   the model's max) so the model actually reads the whole prompt.
3. It **splits** the diff into chunks that fit the budget, reviews each chunk,
   and merges the findings. Oversized single files are split by line.

Override the budget in `.codereview.yaml` with [`max_context_tokens`](#quick-example)
-- raise it for fewer, larger requests (more RAM) or lower it to cap memory use.

---

## REPL Commands

Launch the REPL with `code-review`. The prompt is `cr>`. Tab completion is supported.

### Review

| Command                | Description                                                          |
| ---------------------- | ------------------------------------------------------------------- |
| `/review`              | Run a code review on your current changes (staged, unstaged, untracked) |
| `/review <path>`       | Review a file or directory as-is, regardless of git state           |
| `/review --diff <ref>` | Review your branch's commits vs a base (pre-PR review)              |
| `/review --verify`     | Add the anti-hallucination second pass (combine with any of the above) |
| `/diff`           | View the current diff (colored: green = added, red = removed)       |
| `/rules`          | Show active review rules per detected language                      |
| `/commit`         | Stage files and commit with an AI-generated message                 |

### Configuration

| Command    | Description                                       |
| ---------- | ------------------------------------------------- |
| `/config`  | View current configuration                        |
| `/output`  | Set output format                                 |
| `/models`  | List available Ollama models (shows active model) |
| `/init`    | Generate a `.codereview.yaml` for your project    |
| `/onboard` | Re-run the onboarding wizard                      |
| `/clear-cache` | Clear the per-file review cache (forces a clean re-review) |

### Session

| Command   | Description                                            |
| --------- | ------------------------------------------------------ |
| `/status` | Show branch, changed files, and active model           |
| `/debug`  | Show diagnostic info (git, Ollama, config, languages)  |
| `/help`   | Show all available commands                            |
| `/quit`   | Exit the REPL (alias: `/exit`)                         |

---

## Non-Interactive Mode

Run a review without entering the REPL. Designed for CI/CD and scripting.

```bash
# Review your branch's commits vs a base (pre-PR review, merge-base semantics)
code-review --diff main

# Review uncommitted changes (pre-commit hooks, scripting)
code-review --uncommitted --format json

# Review an existing module/theme/file as-is (no git diff needed)
code-review --path docroot/modules/custom/my_module
code-review --path src/Controller.php --format json

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

| Flag              | Short | Description                                          | Default           |
| ----------------- | ----- | ---------------------------------------------------- | ----------------- |
| `--diff <REF>`    |       | Base to diff commits against (branch, tag, SHA, `HEAD~N`); uses the merge base, like a PR | (enters REPL)     |
| `--path <PATH>`   |       | Review a file/directory as-is (takes precedence over `--diff`) | (enters REPL)     |
| `--uncommitted`   |       | Review uncommitted changes (staged, unstaged, untracked) | (enters REPL)     |
| `--verify`        |       | LLM second pass: re-check each bug/security finding, drop interpretation hallucinations (one extra call per in-scope finding) | off |
| `--format <FMT>`  |       | `terminal`, `json`, `markdown`, `annotations`        | `terminal`        |
| `--model <NAME>`  | `-m`  | Override the Ollama model                            | (from onboarding) |
| `--output <PATH>` | `-o`  | Write output to file                                 | (stdout)          |

Any of `--diff`, `--path`, or `--uncommitted` triggers non-interactive mode; with none of them, `code-review` opens the REPL.

Progress (what's being reviewed, which agent is running) prints to **stderr**,
so stdout stays purely the report -- piping to `jq` or writing with `-o` is
unaffected. On a terminal you get a live spinner; in CI logs, plain lines.

### Subcommands

| Command               | Description                                           |
| --------------------- | ----------------------------------------------------- |
| `code-review onboard` | Run the onboarding wizard (`--reset` for fresh start) |
| `code-review init`    | Generate `.codereview.yaml` interactively             |
| `code-review clear-cache` | Clear the per-file review cache for this project  |

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

# Optional: default output format (terminal, json, markdown, annotations)
output_format: terminal

# Optional: limit the review to specific languages (auto-detected if omitted)
# languages: [php, drupal, javascript]

# Optional: cap the context window (tokens) requested from the model.
# Auto-detected from the model and capped at its max; defaults to 32768.
max_context_tokens: 32768

# Optional: per-LLM-request timeout in seconds (default 300).
# Raise on slow hardware; 0 = no timeout (review hangs if Ollama stalls).
llm_timeout_seconds: 300

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

> **Note:** Drupal's 10 rules are the 6 PHP rules -- inherited as distinct rules
> under `drupal-`-prefixed IDs (e.g. `drupal-sql-injection`), so they can be
> overridden independently of their PHP counterparts -- plus 4 Drupal-specific rules.

See [all 43 rule IDs with severities](docs/CONFIGURATION.md#all-built-in-rule-ids) in the configuration reference.

### What You Can Configure

- **Exclude files and directories** -- skip `.lando.yml`, `vendor/`, `*.log`, etc.
- **Override built-in rules** -- disable or change severity
- **Add custom rules** -- team-specific checks (the LLM follows your instructions)
- **Create custom agents** -- specialized reviewers with their own system prompts (PCI-DSS, Laravel, performance, etc.)
- **Set model and output format** -- per-project defaults
- **Cap the context window** -- `max_context_tokens` to control memory use and chunk size

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

Languages are auto-detected from file extensions in your diff (or under your `--path` target).

| Language   | Extensions                                               |
| ---------- | -------------------------------------------------------- |
| PHP        | `.php`, `.inc`                                           |
| Drupal     | `.module`, `.install`, `.theme`, `.profile`, `.info.yml` |
| JavaScript | `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.tsx`             |
| CSS        | `.css`, `.scss`, `.sass`, `.less`                        |
| HTML       | `.html`, `.htm`, `.twig`                                 |
| YAML       | `.yml`, `.yaml` (`.info.yml` is treated as Drupal)       |

**Drupal auto-detection:** If your project contains `.info.yml` files, `.module` files, or `core/lib/Drupal`, all `.php` files are automatically promoted to Drupal and get Drupal-specific rules.

---

## Platform Linking

Connect GitHub or GitLab accounts during onboarding for future PR integration.

**GitHub:** Create a [Fine-Grained PAT](https://github.com/settings/tokens?type=beta) with permissions: `contents` (read), `pull_requests` (read+write), `metadata` (read).

**GitLab:** Create a PAT at User Settings > Access Tokens with scopes: `api`, `read_api`. Supports custom instance URLs for self-hosted GitLab.

Multiple accounts are supported (useful for submodules across platforms).

---

## Data Storage

Onboarding state is stored in a **per-project** SQLite database, and a
persistent log of errors, warnings, and review runs is written alongside it:

```
<project root>/.codereview/state.db
<project root>/.codereview/logs/code-review.log
```

The log records every review run (target, model, file count, duration,
findings), every error and warning the app prints, LLM request failures, and
each finding discarded by evidence verification — check it when something
behaved unexpectedly after the terminal has scrolled away. It rotates at 5MB
(one `.log.1` generation kept). `/debug` in the REPL shows the active path.

The project root is the git repository root (or the current directory when not
in a git repository). The tool automatically appends `.codereview/` to an
existing `.gitignore` so the database is never committed -- if your project has
no `.gitignore`, add the entry yourself.

**Reset:**

```bash
code-review onboard --reset     # reset onboarding only
rm -rf .codereview/             # full reset (delete the project's database)
```

---

## Troubleshooting

### "Cannot connect to Ollama"

```bash
curl http://127.0.0.1:11434    # check if running
ollama serve                    # start it
```

### "No changes to review"

The tool reviews your uncommitted work -- unstaged, staged, and untracked (new)
files. Make sure you have uncommitted changes in a git repository.

If you **already committed** your work, review the branch's commits against the
base instead:

```bash
code-review --diff main               # or, in the REPL: /review --diff main
```

To review existing code that hasn't changed at all (a module, a theme), use
path mode -- it doesn't need a diff:

```bash
code-review --path path/to/module     # or, in the REPL: /review path/to/module
```

### "No models available"

```bash
ollama pull gemma4
```

### Review takes too long

- Use a smaller model: `code-review -m gemma4`
- The timeout is 300 seconds per LLM call by default -- raise it with `llm_timeout_seconds` in `.codereview.yaml` if a slow machine legitimately needs longer
- Reasoning models (qwen3.5, deepseek-r1, etc.) have thinking disabled automatically during reviews, so they answer directly instead of deliberating first
- Fewer files = faster (each agent processes the diff)
- Large reviews are split into context-sized chunks, so a big diff means more
  LLM calls. Lower `max_context_tokens` for less RAM, or narrow the scope with
  `--path` or `exclude` patterns.

### "input length ... exceeds the model's maximum context length"

A single request was larger than the model can accept. This is handled
automatically now (the diff is chunked to fit), but if you still hit it, lower
`max_context_tokens` in `.codereview.yaml` or exclude large generated files
(minified CSS/JS, lockfiles) via `exclude`.

### Wrong language detected

Run `/rules` to see detected languages. If PHP shows as Drupal, it's because Drupal markers (`.info.yml`, `.module`) were found in your project.

### Custom rules not appearing

1. Verify `.codereview.yaml` is in the project root
2. Run `/config` to confirm it loaded
3. Run `/rules` to see custom rules tagged with `(custom)`
4. Check YAML syntax -- an invalid file falls back to defaults and prints a warning at startup naming the parse error

---

## License

MIT
