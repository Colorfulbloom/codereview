pub mod commands;

use anyhow::Result;
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Editor, Helper};

use commands::CommandRegistry;

use crate::config::Config;
use crate::git::GitAgent;
use crate::language;
use crate::language::rules::builtin_rules;
use crate::onboarding;
use crate::onboarding::steps::OllamaClient;
use crate::output::terminal::TerminalFormatter;
use crate::review::engine;
use crate::session::{OutputFormatChoice, SessionState};

/// Dependencies available to the REPL session.
pub struct SessionContext<'a> {
    pub git: &'a dyn GitAgent,
    pub ollama: &'a dyn OllamaClient,
    pub model: String,
    pub config: Config,
    pub rt: &'a tokio::runtime::Runtime,
    pub session: std::cell::RefCell<SessionState>,
    pub output_format: std::cell::RefCell<OutputFormatChoice>,
    pub db: &'a rusqlite::Connection,
}

/// Tab-completion helper for slash commands.
struct ReplHelper {
    commands: Vec<&'static str>,
}

impl Helper for ReplHelper {}
impl Highlighter for ReplHelper {}
impl Hinter for ReplHelper {
    type Hint = String;
}
impl Validator for ReplHelper {}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        if !line.starts_with('/') {
            return Ok((0, vec![]));
        }

        let prefix = &line[..pos];
        let matches: Vec<Pair> = self
            .commands
            .iter()
            .filter(|cmd| cmd.starts_with(prefix))
            .map(|cmd| Pair {
                display: cmd.to_string(),
                replacement: cmd.to_string(),
            })
            .collect();

        Ok((0, matches))
    }
}

/// Start the interactive REPL session.
pub fn start(ctx: SessionContext) -> Result<()> {
    let registry = CommandRegistry::new();
    let helper = ReplHelper {
        commands: registry.command_names(),
    };

    {
        use console::Style;
        let title = Style::new().bold().cyan();
        let hint = Style::new().dim();
        println!(
            "{} {}",
            title.apply_to("code-review REPL ready."),
            hint.apply_to("Type /help for available commands.")
        );
        println!();
    }

    let mut rl = Editor::new()?;
    rl.set_helper(Some(helper));

    loop {
        let readline = rl.readline("cr> ");

        match readline {
            Ok(line) => {
                let input = line.trim();

                if input.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(input);

                match input {
                    "/help" => registry.print_help(),
                    "/quit" | "/exit" => {
                        println!("Goodbye.");
                        break;
                    }
                    "/review" => handle_review(&ctx),
                    "/debug" => handle_debug(&ctx),
                    "/diff" => handle_diff(&ctx),
                    "/status" => handle_status(&ctx),
                    "/rules" => handle_rules(&ctx),
                    "/config" => handle_config(&ctx),
                    "/commit" => handle_commit(&ctx),
                    "/models" => handle_models(&ctx),
                    "/output" => handle_output(&ctx),
                    "/init" => {
                        if let Err(e) = crate::init::run_init() {
                            eprintln!("Init failed: {e}");
                        }
                    }
                    "/onboard" => {
                        handle_onboard(&ctx);
                    }
                    cmd if cmd.starts_with('/') => {
                        println!("Unknown command: {cmd}. Type /help for available commands.");
                    }
                    _ => {
                        println!("Type a slash command to get started. Try /help.");
                    }
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                println!("Ctrl-C pressed. Type /quit to exit.");
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                println!("Goodbye.");
                break;
            }
            Err(err) => {
                eprintln!("Error: {err}");
                break;
            }
        }
    }

    Ok(())
}

fn handle_review(ctx: &SessionContext) {
    use console::Style;
    use indicatif::{ProgressBar, ProgressStyle};

    let dim = Style::new().dim();

    // Pre-flight: check Ollama is reachable
    let ollama_ok = ctx.rt.block_on(ctx.ollama.is_running());
    if !ollama_ok {
        println!("Cannot connect to Ollama at http://127.0.0.1:11434");
        println!("Start it with: ollama serve");
        return;
    }

    // Show what we're about to review (use diff_all for accurate count)
    let all_diffs = ctx.git.diff_all().unwrap_or_default();
    // Apply exclusions
    let all_diffs: Vec<_> = all_diffs
        .into_iter()
        .filter(|d| !ctx.config.is_excluded(&d.path))
        .collect();
    let total_changes = all_diffs.len();

    if total_changes > 0 {
        let all_paths: Vec<&str> = all_diffs.iter().map(|d| d.path.as_str()).collect();
        let languages = crate::language::detect_languages(&all_paths);

        if languages.is_empty() {
            println!(
                "No supported languages detected in {} changed file(s).",
                total_changes
            );
            println!("Supported: PHP, Drupal, JavaScript, CSS, HTML, YAML.");
            println!(
                "{}",
                dim.apply_to("Tip: Use custom agents in .codereview.yaml for other languages.")
            );
            return;
        }

        let lang_list: Vec<String> = languages.iter().map(|l| l.to_string()).collect();
        println!(
            "{}",
            dim.apply_to(format!(
                "Detected {} file(s): {}",
                total_changes,
                lang_list.join(", ")
            ))
        );
    }

    use crate::output::annotations::AnnotationFormatter;
    use crate::output::json::JsonFormatter;
    use crate::output::markdown::MarkdownFormatter;

    let format = *ctx.output_format.borrow();
    let formatter: Box<dyn crate::output::OutputFormatter> = match format {
        OutputFormatChoice::Terminal => Box::new(TerminalFormatter),
        OutputFormatChoice::Json => Box::new(JsonFormatter),
        OutputFormatChoice::Markdown => Box::new(MarkdownFormatter),
        OutputFormatChoice::Annotations => Box::new(AnnotationFormatter),
    };

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.set_message(format!("Reviewing with {}...", ctx.model));
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    let result = ctx.rt.block_on(engine::run_review(
        ctx.git,
        ctx.ollama,
        formatter.as_ref(),
        &ctx.model,
        &ctx.config,
        None,
    ));

    spinner.finish_and_clear();

    match result {
        Ok((output, _)) => {
            print!("{output}");
        }
        Err(engine::ReviewError::NotARepo) => {
            println!("Not inside a git repository. Navigate to a repo and try again.");
        }
        Err(engine::ReviewError::NoChanges) => {
            println!("No changes to review.");
            println!(
                "{}",
                dim.apply_to(
                    "The tool reviews unstaged or staged changes. If you already committed, use: /review with --diff <branch>"
                )
            );
        }
        Err(e) => {
            eprintln!("Review failed: {e}");
        }
    }
}

fn handle_diff(ctx: &SessionContext) {
    use console::Style;

    if !ctx.git.is_repo() {
        println!("Not inside a git repository.");
        return;
    }

    let diffs = match ctx.git.diff_all() {
        Ok(d) if !d.is_empty() => d,
        _ => {
            println!("No changes to show.");
            return;
        }
    };

    let file_style = Style::new().bold().white();
    let status_style = Style::new().cyan();
    let add_style = Style::new().green();
    let del_style = Style::new().red();
    let hunk_style = Style::new().cyan().dim();
    let context_style = Style::new().white().dim();

    for diff in &diffs {
        println!(
            "\n{} {}",
            file_style.apply_to(&diff.path),
            status_style.apply_to(format!("({})", diff.status))
        );
        for hunk in &diff.hunks {
            println!(
                "{}",
                hunk_style.apply_to(format!(
                    "@@ -{},{} +{},{} @@",
                    hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
                ))
            );
            for line in hunk.content.lines() {
                if line.starts_with('+') {
                    println!("{}", add_style.apply_to(line));
                } else if line.starts_with('-') {
                    println!("{}", del_style.apply_to(line));
                } else {
                    println!("{}", context_style.apply_to(line));
                }
            }
        }
    }
    println!();
}

fn handle_status(ctx: &SessionContext) {
    use console::Style;

    let label_style = Style::new().cyan().bold();
    let value_style = Style::new().white().bold();
    let section_style = Style::new().yellow().bold();
    let added_style = Style::new().green();
    let modified_style = Style::new().yellow();
    let deleted_style = Style::new().red();
    let path_style = Style::new().white();

    if !ctx.git.is_repo() {
        println!("Not inside a git repository.");
        return;
    }

    println!();

    // Branch
    if let Ok(Some(branch)) = ctx.git.current_branch() {
        println!(
            "  {} {}",
            label_style.apply_to("Branch:"),
            value_style.apply_to(branch)
        );
    }

    // Model
    println!(
        "  {} {}",
        label_style.apply_to("Model:"),
        value_style.apply_to(&ctx.model)
    );

    // Output format
    println!(
        "  {} {}",
        label_style.apply_to("Output:"),
        value_style.apply_to(ctx.output_format.borrow().to_string())
    );

    // Changed files
    let unstaged = ctx.git.changed_files_unstaged().unwrap_or_default();
    let staged = ctx.git.changed_files_staged().unwrap_or_default();

    if unstaged.is_empty() && staged.is_empty() {
        println!(
            "  {} {}",
            label_style.apply_to("Changes:"),
            Style::new().dim().apply_to("none")
        );
    } else {
        if !staged.is_empty() {
            println!("  {}:", section_style.apply_to("Staged"));
            for f in &staged {
                let status_style = match f.status {
                    crate::git::FileStatus::Added => &added_style,
                    crate::git::FileStatus::Modified => &modified_style,
                    crate::git::FileStatus::Deleted => &deleted_style,
                    _ => &modified_style,
                };
                println!(
                    "    {} {}",
                    status_style.apply_to(format!("{:<10}", f.status)),
                    path_style.apply_to(&f.path)
                );
            }
        }
        if !unstaged.is_empty() {
            println!("  {}:", section_style.apply_to("Unstaged"));
            for f in &unstaged {
                let status_style = match f.status {
                    crate::git::FileStatus::Added => &added_style,
                    crate::git::FileStatus::Modified => &modified_style,
                    crate::git::FileStatus::Deleted => &deleted_style,
                    _ => &modified_style,
                };
                println!(
                    "    {} {}",
                    status_style.apply_to(format!("{:<10}", f.status)),
                    path_style.apply_to(&f.path)
                );
            }
        }
    }
    println!();
}

fn handle_rules(ctx: &SessionContext) {
    use console::Style;

    if !ctx.git.is_repo() {
        println!("Not inside a git repository.");
        return;
    }

    // Detect languages from changed files
    let unstaged = ctx.git.changed_files_unstaged().unwrap_or_default();
    let staged = ctx.git.changed_files_staged().unwrap_or_default();
    let all_paths: Vec<&str> = unstaged
        .iter()
        .chain(staged.iter())
        .map(|f| f.path.as_str())
        .collect();

    let languages = language::detect_languages(&all_paths);

    if languages.is_empty() && all_paths.is_empty() {
        println!("No changes detected. Make some changes to see active rules.");
        return;
    }

    let header_style = Style::new().bold().cyan();
    let enabled_style = Style::new().green();
    let disabled_style = Style::new().red().dim();
    let is_drupal = language::is_drupal_project(&all_paths);

    for lang in &languages {
        let file_count = all_paths
            .iter()
            .filter(|p| {
                let mut detected = language::detect_language(p);
                // Apply same promotion as detect_languages
                if is_drupal && detected == Some(language::Language::Php) {
                    detected = Some(language::Language::Drupal);
                }
                detected == Some(*lang)
            })
            .count();

        println!(
            "\n{} ({} file(s) detected)",
            header_style.apply_to(lang),
            file_count
        );

        let all_rules = builtin_rules(*lang);
        let effective = ctx.config.effective_rules(*lang);
        let effective_ids: std::collections::HashSet<&str> =
            effective.iter().map(|r| r.id.as_str()).collect();

        let builtin_ids: std::collections::HashSet<&str> =
            all_rules.iter().map(|r| r.id.as_str()).collect();

        for rule in &all_rules {
            if effective_ids.contains(rule.id.as_str()) {
                println!(
                    "  {} [{}] {}",
                    enabled_style.apply_to("✓"),
                    rule.severity,
                    rule.description
                );
            } else {
                println!(
                    "  {} [{}] {} (disabled)",
                    disabled_style.apply_to("✗"),
                    rule.severity,
                    rule.description
                );
            }
        }

        // Show custom rules not in builtins
        let custom_style = Style::new().magenta();
        for rule in &effective {
            if !builtin_ids.contains(rule.id.as_str()) {
                println!(
                    "  {} [{}] {} {}",
                    enabled_style.apply_to("✓"),
                    rule.severity,
                    rule.description,
                    custom_style.apply_to("(custom)")
                );
            }
        }
    }
    println!();
}

fn handle_config(ctx: &SessionContext) {
    use console::Style;

    let title_style = Style::new().bold().cyan();
    let label_style = Style::new().cyan();
    let value_style = Style::new().white().bold();
    let default_style = Style::new().dim();
    let override_style = Style::new().yellow();

    println!("\n{}:\n", title_style.apply_to("Current configuration"));

    if let Some(ref model) = ctx.config.model {
        println!(
            "  {} {}",
            label_style.apply_to("model:"),
            value_style.apply_to(model)
        );
    } else {
        println!(
            "  {} {} {}",
            label_style.apply_to("model:"),
            value_style.apply_to(&ctx.model),
            default_style.apply_to("(default)")
        );
    }

    if let Some(ref fmt) = ctx.config.output_format {
        println!(
            "  {} {}",
            label_style.apply_to("output_format:"),
            value_style.apply_to(fmt)
        );
    } else {
        println!(
            "  {} {} {}",
            label_style.apply_to("output_format:"),
            value_style.apply_to("terminal"),
            default_style.apply_to("(default)")
        );
    }

    if let Some(ref langs) = ctx.config.languages {
        println!(
            "  {} {}",
            label_style.apply_to("languages:"),
            value_style.apply_to(langs.join(", "))
        );
    } else {
        println!(
            "  {} {}",
            label_style.apply_to("languages:"),
            default_style.apply_to("auto-detect")
        );
    }

    if !ctx.config.rules.is_empty() {
        println!("  {}:", override_style.apply_to("rule overrides"));
        for (lang, overrides) in &ctx.config.rules {
            for (rule_id, ov) in overrides {
                let mut parts = Vec::new();
                if let Some(enabled) = ov.enabled {
                    parts.push(format!("enabled={enabled}"));
                }
                if let Some(severity) = ov.severity {
                    parts.push(format!("severity={severity}"));
                }
                println!(
                    "    {} {}",
                    override_style.apply_to(format!("{lang}/{rule_id}:")),
                    parts.join(", ")
                );
            }
        }
    }

    if !ctx.config.custom_rules.is_empty() {
        println!("  {}:", override_style.apply_to("custom rules"));
        for rule in &ctx.config.custom_rules {
            println!(
                "    {} {} {}",
                label_style.apply_to(&rule.id),
                default_style.apply_to("—"),
                rule.description
            );
        }
    }

    println!("\n  Edit .codereview.yaml to change configuration.\n");
}

fn handle_commit(ctx: &SessionContext) {
    use console::Style;
    use dialoguer::{Confirm, Input, MultiSelect};

    if !ctx.git.is_repo() {
        println!("Not inside a git repository.");
        return;
    }

    // Get all changed files (staged + unstaged)
    let unstaged = ctx.git.changed_files_unstaged().unwrap_or_default();
    let staged = ctx.git.changed_files_staged().unwrap_or_default();

    if unstaged.is_empty() && staged.is_empty() {
        println!("No changes to commit.");
        return;
    }

    // Deduplicate files (a file can appear in both staged and unstaged)
    let mut seen = std::collections::HashSet::new();
    let mut deduped: Vec<&crate::git::ChangedFile> = Vec::new();
    for f in unstaged.iter().chain(staged.iter()) {
        if seen.insert(&f.path) {
            deduped.push(f);
        }
    }

    // Show current state
    let bold = Style::new().bold();
    println!("\n{}", bold.apply_to("Files with changes:"));

    let all_files: Vec<String> = deduped
        .iter()
        .map(|f| format!("{} {}", f.status, f.path))
        .collect();

    // Let user select files to stage
    let selections = match MultiSelect::new()
        .with_prompt("Select files to stage")
        .items(&all_files)
        .interact()
    {
        Ok(s) => s,
        Err(_) => return,
    };

    if selections.is_empty() {
        println!("No files selected. Commit cancelled.");
        return;
    }

    // Stage selected files
    let to_stage: Vec<&str> = selections
        .iter()
        .map(|&i| deduped[i].path.as_str())
        .collect();

    if let Err(e) = ctx.git.stage_files(&to_stage) {
        eprintln!("Failed to stage files: {e}");
        return;
    }

    println!("Staged {} file(s).", to_stage.len());

    // Generate commit message via Ollama
    let default_msg = if let Some(review) = ctx.session.borrow().last_review() {
        let summary: Vec<String> = review
            .findings
            .iter()
            .take(5)
            .map(|f| format!("{}: {}", f.severity, f.title))
            .collect();
        if summary.is_empty() {
            "Update code (reviewed, no issues found)".to_string()
        } else {
            format!("Fix review findings: {}", summary.join("; "))
        }
    } else {
        "Update code".to_string()
    };

    // Let user edit the message
    let message = match Input::<String>::new()
        .with_prompt("Commit message")
        .default(default_msg)
        .interact_text()
    {
        Ok(m) => m,
        Err(_) => return,
    };

    // Confirm
    let confirmed = Confirm::new()
        .with_prompt(format!("Commit with message: \"{}\"?", message))
        .default(true)
        .interact()
        .unwrap_or(false);

    if !confirmed {
        println!("Commit cancelled.");
        return;
    }

    // Commit
    match ctx.git.commit(&message) {
        Ok(oid) => {
            let short_oid = &oid[..8.min(oid.len())];
            println!("Committed: {short_oid} {message}");
        }
        Err(e) => {
            eprintln!("Commit failed: {e}");
        }
    }
}

fn handle_onboard(ctx: &SessionContext) {
    use dialoguer::Confirm;

    let reset = Confirm::new()
        .with_prompt("Start onboarding from scratch? (No = resume from where you left off)")
        .default(false)
        .interact()
        .unwrap_or(false);

    match onboarding::run::run_onboarding_interactive(ctx.db, ctx.ollama, reset, ctx.rt) {
        Ok(()) => {
            println!("Onboarding complete. Changes take effect on next /review.");
        }
        Err(e) => {
            eprintln!("Onboarding failed: {e}");
        }
    }
}

fn handle_debug(ctx: &SessionContext) {
    use console::Style;

    let title_style = Style::new().bold().magenta();
    let label_style = Style::new().cyan();
    let value_style = Style::new().white().bold();
    let ok_style = Style::new().green().bold();
    let warn_style = Style::new().yellow();
    let err_style = Style::new().red();
    let dim = Style::new().dim();
    let path_style = Style::new().white();

    println!("\n{}\n", title_style.apply_to("=== Debug Info ==="));

    // Git
    let is_repo = ctx.git.is_repo();
    println!(
        "{} {}",
        label_style.apply_to("Git repo:"),
        if is_repo {
            ok_style.apply_to("true".to_string())
        } else {
            err_style.apply_to("false".to_string())
        }
    );
    if let Ok(root) = ctx.git.repo_root() {
        println!(
            "{} {}",
            label_style.apply_to("Repo root:"),
            path_style.apply_to(root.display().to_string())
        );
    }
    if let Ok(Some(branch)) = ctx.git.current_branch() {
        println!(
            "{} {}",
            label_style.apply_to("Branch:"),
            value_style.apply_to(branch)
        );
    }

    // Unstaged changes
    let unstaged = ctx.git.changed_files_unstaged().unwrap_or_default();
    println!(
        "\n{} {}",
        label_style.apply_to("Unstaged changes:"),
        value_style.apply_to(unstaged.len().to_string())
    );
    for f in &unstaged {
        let lang = crate::language::detect_language(&f.path);
        let lang_str = lang
            .map(|l| ok_style.apply_to(l.to_string()).to_string())
            .unwrap_or_else(|| dim.apply_to("(unsupported)").to_string());
        println!(
            "  {} {} [{}]",
            warn_style.apply_to(f.status.to_string()),
            path_style.apply_to(&f.path),
            lang_str
        );
    }

    // Staged changes
    let staged = ctx.git.changed_files_staged().unwrap_or_default();
    println!(
        "\n{} {}",
        label_style.apply_to("Staged changes:"),
        value_style.apply_to(staged.len().to_string())
    );
    for f in &staged {
        let lang = crate::language::detect_language(&f.path);
        let lang_str = lang
            .map(|l| ok_style.apply_to(l.to_string()).to_string())
            .unwrap_or_else(|| dim.apply_to("(unsupported)").to_string());
        println!(
            "  {} {} [{}]",
            warn_style.apply_to(f.status.to_string()),
            path_style.apply_to(&f.path),
            lang_str
        );
    }

    // Language detection
    let all_paths: Vec<&str> = unstaged
        .iter()
        .chain(staged.iter())
        .map(|f| f.path.as_str())
        .collect();
    let languages = crate::language::detect_languages(&all_paths);
    let lang_list: Vec<String> = languages.iter().map(|l| l.to_string()).collect();
    println!(
        "\n{} {}",
        label_style.apply_to("Detected languages:"),
        if lang_list.is_empty() {
            dim.apply_to("none".to_string())
        } else {
            ok_style.apply_to(lang_list.join(", "))
        }
    );
    println!(
        "{} {}",
        label_style.apply_to("Is Drupal project:"),
        if crate::language::is_drupal_project(&all_paths) {
            ok_style.apply_to("true".to_string())
        } else {
            dim.apply_to("false".to_string())
        }
    );

    // Diffs
    let all_diffs = ctx.git.diff_all().unwrap_or_default();
    println!(
        "\n{} {}",
        label_style.apply_to("Total diffs (HEAD → working tree):"),
        value_style.apply_to(all_diffs.len().to_string())
    );
    for d in &all_diffs {
        let total_lines: usize = d.hunks.iter().map(|h| h.content.lines().count()).sum();
        println!(
            "  {} {} {}",
            path_style.apply_to(&d.path),
            dim.apply_to(format!("{} hunks", d.hunks.len())),
            dim.apply_to(format!("~{} lines", total_lines))
        );
    }

    // Ollama
    let ollama_running = ctx.rt.block_on(ctx.ollama.is_running());
    println!(
        "\n{} {}",
        label_style.apply_to("Ollama running:"),
        if ollama_running {
            ok_style.apply_to("true".to_string())
        } else {
            err_style.apply_to("false".to_string())
        }
    );
    println!(
        "{} {}",
        label_style.apply_to("Active model:"),
        value_style.apply_to(&ctx.model)
    );

    if ollama_running {
        let models = ctx
            .rt
            .block_on(ctx.ollama.list_models())
            .unwrap_or_default();
        println!(
            "{} {}",
            label_style.apply_to("Available models:"),
            models.join(", ")
        );
        let model_available = models.iter().any(|m| m.starts_with(&ctx.model));
        println!(
            "{} {}",
            label_style.apply_to("Selected model available:"),
            if model_available {
                ok_style.apply_to("true".to_string())
            } else {
                err_style.apply_to("false".to_string())
            }
        );
    }

    // Config
    let has_config = !ctx.config.rules.is_empty()
        || !ctx.config.custom_rules.is_empty()
        || ctx.config.model.is_some();
    println!(
        "\n{} {}",
        label_style.apply_to("Config loaded:"),
        if has_config {
            ok_style.apply_to("true".to_string())
        } else {
            dim.apply_to("false".to_string())
        }
    );
    println!(
        "{} {}",
        label_style.apply_to("Custom rules:"),
        value_style.apply_to(ctx.config.custom_rules.len().to_string())
    );
    println!(
        "{} {}",
        label_style.apply_to("Custom agents:"),
        value_style.apply_to(ctx.config.agents.len().to_string())
    );

    println!("\n{}\n", title_style.apply_to("=== End Debug ==="));
}

fn handle_output(ctx: &SessionContext) {
    use dialoguer::Select;

    let current = *ctx.output_format.borrow();
    println!("\nCurrent output format: {current}\n");

    let options = vec![
        "terminal — colored text in terminal",
        "json — structured JSON (pipe to other tools)",
        "markdown — report with tables",
        "annotations — GitHub Actions format",
    ];

    let current_idx = match current {
        OutputFormatChoice::Terminal => 0,
        OutputFormatChoice::Json => 1,
        OutputFormatChoice::Markdown => 2,
        OutputFormatChoice::Annotations => 3,
    };

    let selection = match Select::new()
        .with_prompt("Select output format")
        .items(&options)
        .default(current_idx)
        .interact()
    {
        Ok(idx) => idx,
        Err(_) => return,
    };

    let new_format = match selection {
        0 => OutputFormatChoice::Terminal,
        1 => OutputFormatChoice::Json,
        2 => OutputFormatChoice::Markdown,
        3 => OutputFormatChoice::Annotations,
        _ => return,
    };

    *ctx.output_format.borrow_mut() = new_format;
    println!("Output format set to: {new_format}");
    println!();
}

fn handle_models(ctx: &SessionContext) {
    use console::Style;

    let label_style = Style::new().cyan().bold();
    let model_style = Style::new().white().bold();
    let active_style = Style::new().green().bold();
    let dim = Style::new().dim();

    println!(
        "\n{} {}\n",
        label_style.apply_to("Active model:"),
        model_style.apply_to(&ctx.model)
    );

    let models = ctx.rt.block_on(ctx.ollama.list_models());
    match models {
        Ok(models) => {
            if models.is_empty() {
                println!(
                    "{}",
                    dim.apply_to("No models available. Pull one with `ollama pull <model>`.")
                );
            } else {
                println!("{}:", label_style.apply_to("Available models"));
                for m in &models {
                    if *m == ctx.model {
                        println!(
                            "  {} {}",
                            active_style.apply_to(&m),
                            active_style.apply_to("(active)")
                        );
                    } else {
                        println!("  {}", model_style.apply_to(&m));
                    }
                }
            }
        }
        Err(e) => {
            eprintln!(
                "{} {e}",
                Style::new().red().apply_to("Failed to list models:")
            );
        }
    }
    println!();
}
