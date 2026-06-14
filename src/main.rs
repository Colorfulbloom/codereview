use std::path::Path;

use anyhow::Result;
use clap::Parser;

use code_review::cli::{Cli, Command, OutputFormat};
use code_review::config::Config;
use code_review::db;
use code_review::git::LiveGitAgent;
use code_review::onboarding;
use code_review::onboarding::progress::{OnboardingPersistence, SqliteOnboardingStore};
use code_review::onboarding::state::StepData;
use code_review::output::OutputFormatter;
use code_review::output::annotations::AnnotationFormatter;
use code_review::output::json::JsonFormatter;
use code_review::output::markdown::MarkdownFormatter;
use code_review::output::terminal::TerminalFormatter;
use code_review::repl;
use code_review::review::engine;
use code_review::review::source::ReviewTarget;

mod runtime;

const DEFAULT_MODEL: &str = "gemma4:latest";

fn main() -> Result<()> {
    let result = run();
    if let Err(ref e) = result {
        // Top-level Display only — the thiserror chains here are transparent
        // ("{0}" + #[from]), so the alternate format would print every
        // message twice.
        code_review::logging::error(format!("fatal: {e}"));
    }
    result
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // Resolve project root once (git repo root, or cwd)
    let project_root = db::find_project_root()?;
    let conn = db::init_at(&project_root)?;

    // Persistent log lives next to the per-project state db. Failure to open
    // it must not block the app — reviews still work, just without history.
    let log_path = project_root.join(".codereview").join("logs").join("code-review.log");
    if let Err(e) = code_review::logging::init(&log_path) {
        eprintln!("Warning: could not open log file {}: {e}", log_path.display());
    }
    code_review::logging::info(format!(
        "started: {}",
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    ));

    let rt = tokio::runtime::Runtime::new()?;

    match cli.command {
        Some(Command::Onboard { reset }) => {
            let ollama = runtime::LiveOllamaClient::default();
            code_review::onboarding::run::run_onboarding_interactive(&conn, &ollama, reset, &rt)
        }
        Some(Command::Init) => code_review::init::run_init(),
        None if cli.diff.is_some() || cli.path.is_some() || cli.uncommitted => {
            run_noninteractive(&cli, &conn, &project_root, &rt)
        }
        None => {
            // Default: check onboarding, then start REPL
            let persistence = SqliteOnboardingStore::new(&conn);
            if onboarding::needs_onboarding(&persistence)? {
                let ollama = runtime::LiveOllamaClient::default();
                onboarding::run::run_onboarding_interactive(&conn, &ollama, false, &rt)?;
            }

            let model = resolve_model(&persistence, cli.model.as_deref());
            let git = LiveGitAgent::new(project_root.clone());

            let config_path = project_root.join(".codereview.yaml");
            let (config, warning) = Config::load_lenient(&config_path);
            if let Some(warning) = warning {
                eprintln!("{warning}");
            }
            let ollama = runtime::LiveOllamaClient::with_timeout(config.llm_timeout());

            let ctx = repl::SessionContext {
                git: &git,
                ollama: &ollama,
                model,
                config,
                rt: &rt,
                session: std::cell::RefCell::new(code_review::session::SessionState::new()),
                output_format: std::cell::RefCell::new(
                    code_review::session::OutputFormatChoice::Terminal,
                ),
                db: &conn,
            };

            repl::start(ctx)
        }
    }
}

/// Non-interactive mode: review and output to stdout or file.
fn run_noninteractive(
    cli: &Cli,
    conn: &rusqlite::Connection,
    project_root: &Path,
    rt: &tokio::runtime::Runtime,
) -> Result<()> {
    let persistence = SqliteOnboardingStore::new(conn);
    let model = resolve_model(&persistence, cli.model.as_deref());
    let git = LiveGitAgent::new(project_root.to_path_buf());

    let config_path = project_root.join(".codereview.yaml");
    let (config, warning) = Config::load_lenient(&config_path);
    if let Some(warning) = warning {
        eprintln!("{warning}");
    }
    let ollama = runtime::LiveOllamaClient::with_timeout(config.llm_timeout());

    let formatter: Box<dyn OutputFormatter> = match cli.format {
        OutputFormat::Terminal => Box::new(TerminalFormatter),
        OutputFormat::Json => Box::new(JsonFormatter),
        OutputFormat::Markdown => Box::new(MarkdownFormatter),
        OutputFormat::Annotations => Box::new(AnnotationFormatter),
    };

    // --path wins over --diff; --uncommitted reviews the working tree.
    let target = if let Some(path) = cli.path.as_deref() {
        ReviewTarget::Path(Path::new(path))
    } else if let Some(base) = cli.diff.as_deref() {
        ReviewTarget::Ref(base)
    } else {
        debug_assert!(cli.uncommitted);
        ReviewTarget::WorkingTree
    };

    // Pre-flight summary so a long review never starts in dead silence. All
    // progress goes to stderr — stdout stays purely the report so pipes and
    // -o files are unaffected. Pre-flight errors fall through to run_review,
    // which produces the canonical message.
    if let Ok(diffs) = engine::collect_review_diffs(&git, &target, &config)
        && !diffs.is_empty()
    {
        let languages = engine::detect_review_languages(&git, &target, &diffs);
        let lang_list: Vec<String> = languages.iter().map(|l| l.to_string()).collect();
        let scope = match target {
            ReviewTarget::Path(p) => format!("under {}", p.display()),
            ReviewTarget::Ref(base) => format!("changed vs {base} (committed changes only)"),
            ReviewTarget::WorkingTree => "with uncommitted changes".to_string(),
        };
        eprintln!(
            "Reviewing {} file(s) {scope}: {}",
            diffs.len(),
            lang_list.join(", ")
        );
        eprintln!("Model: {model}");
    }

    // Per-agent progress: a live spinner on a terminal, plain lines otherwise
    // (CI logs still show which agent is running).
    let stderr_is_tty = console::Term::stderr().is_term();
    let spinner = if stderr_is_tty {
        let pb = indicatif::ProgressBar::new_spinner()
            .with_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg} ({elapsed})")
                    .expect("static spinner template is valid"),
            );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        Some(pb)
    } else {
        None
    };

    let cache = code_review::review::cache::SqliteCache::new(conn);
    let result = rt.block_on(engine::run_review(
        &git,
        &ollama,
        formatter.as_ref(),
        &model,
        &config,
        target,
        |agent| match &spinner {
            Some(pb) => pb.set_message(format!("{agent}...")),
            None => eprintln!("  {agent}..."),
        },
        Some(&cache),
    ));

    if let Some(pb) = &spinner {
        pb.finish_and_clear();
    }
    let (output, _result) = result?;

    if let Some(ref output_path) = cli.output {
        std::fs::write(output_path, &output)?;
        eprintln!("Report written to: {output_path}");
    } else {
        print!("{output}");
    }

    Ok(())
}

/// Resolve which model to use: CLI flag > onboarding state > default.
fn resolve_model(persistence: &SqliteOnboardingStore, cli_model: Option<&str>) -> String {
    if let Some(m) = cli_model {
        return m.to_string();
    }

    persistence
        .load_state()
        .ok()
        .flatten()
        .and_then(|s| {
            if let Some(StepData::ModelSelection { selected_model, .. }) =
                s.get_data(code_review::onboarding::steps::StepId::ModelSelection)
            {
                Some(selected_model.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}
