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

mod runtime;

const DEFAULT_MODEL: &str = "gemma4:latest";

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Resolve project root once (git repo root, or cwd)
    let project_root = db::find_project_root()?;
    let conn = db::init_at(&project_root)?;

    let rt = tokio::runtime::Runtime::new()?;

    match cli.command {
        Some(Command::Onboard { reset }) => {
            let ollama = runtime::LiveOllamaClient;
            code_review::onboarding::run::run_onboarding_interactive(&conn, &ollama, reset, &rt)
        }
        Some(Command::Init) => code_review::init::run_init(),
        None if cli.diff.is_some() => run_noninteractive(&cli, &conn, &project_root, &rt),
        None => {
            // Default: check onboarding, then start REPL
            let persistence = SqliteOnboardingStore::new(&conn);
            if onboarding::needs_onboarding(&persistence)? {
                let ollama = runtime::LiveOllamaClient;
                onboarding::run::run_onboarding_interactive(&conn, &ollama, false, &rt)?;
            }

            let model = resolve_model(&persistence, cli.model.as_deref());
            let git = LiveGitAgent::new(project_root.clone());
            let ollama = runtime::LiveOllamaClient;

            let config_path = project_root.join(".codereview.yaml");
            let config = if config_path.exists() {
                Config::load_from_file(&config_path).unwrap_or_default()
            } else {
                Config::default()
            };

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
    let ollama = runtime::LiveOllamaClient;

    let config_path = project_root.join(".codereview.yaml");
    let config = if config_path.exists() {
        Config::load_from_file(&config_path).unwrap_or_default()
    } else {
        Config::default()
    };

    let formatter: Box<dyn OutputFormatter> = match cli.format {
        OutputFormat::Terminal => Box::new(TerminalFormatter),
        OutputFormat::Json => Box::new(JsonFormatter),
        OutputFormat::Markdown => Box::new(MarkdownFormatter),
        OutputFormat::Annotations => Box::new(AnnotationFormatter),
    };

    let (output, _result) = rt.block_on(engine::run_review(
        &git,
        &ollama,
        formatter.as_ref(),
        &model,
        &config,
        cli.diff.as_deref(),
    ))?;

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
