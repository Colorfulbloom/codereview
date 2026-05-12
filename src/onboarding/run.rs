//! Public onboarding runner — can be called from both main.rs and the REPL.

use anyhow::Result;
use dialoguer::{Confirm, Input, MultiSelect, Password, Select};
use indicatif::{ProgressBar, ProgressStyle};
use rusqlite::Connection;

use super::OnboardingOrchestrator;
use super::progress::OnboardingPersistence;
use super::progress::SqliteOnboardingStore;
use super::steps::{
    AppInfo, FileSystem, GitContext, OllamaClient, SpinnerHandle, StepContext, TerminalUi,
};

/// Run the onboarding wizard interactively.
///
/// Can be called from `main.rs` or from the REPL via `/onboard`.
pub fn run_onboarding_interactive(
    conn: &Connection,
    ollama: &dyn OllamaClient,
    reset: bool,
    rt: &tokio::runtime::Runtime,
) -> Result<()> {
    let persistence = SqliteOnboardingStore::new(conn);

    if reset {
        persistence.clear_state()?;
    }

    let orchestrator = OnboardingOrchestrator::new(&persistence);

    let ui = SimpleConsoleUi;
    let git = SimpleGitContext;
    let fs = SimpleFileSystem;
    let app_info = AppInfo {
        name: "code-review",
        version: env!("CARGO_PKG_VERSION"),
    };

    // Use the FallbackStore for credential storage (keyring → env vars)
    let cred_store = crate::credentials::FallbackStore::new();

    let ctx = StepContext {
        ui: &ui,
        ollama,
        git: &git,
        fs: &fs,
        app_info: &app_info,
        credentials: Some(&cred_store),
    };

    rt.block_on(orchestrator.run(&ctx))?;
    Ok(())
}

// Simple implementations of the traits for onboarding.
// These mirror runtime.rs but live in the library so the REPL can use them.

struct SimpleConsoleUi;

impl TerminalUi for SimpleConsoleUi {
    fn print(&self, message: &str) {
        println!("{message}");
    }

    fn print_header(&self, title: &str) {
        use console::Style;
        let style = Style::new().bold().cyan();
        println!("\n{}", style.apply_to(format!("--- {title} ---")));
    }

    fn prompt(&self, message: &str) -> Option<String> {
        Input::new().with_prompt(message).interact_text().ok()
    }

    fn prompt_with_default(&self, message: &str, default: &str) -> Option<String> {
        Input::new()
            .with_prompt(message)
            .default(default.to_string())
            .interact_text()
            .ok()
    }

    fn confirm(&self, message: &str, default: bool) -> Option<bool> {
        Confirm::new()
            .with_prompt(message)
            .default(default)
            .interact()
            .ok()
    }

    fn select(&self, message: &str, items: &[&str]) -> Option<usize> {
        Select::new()
            .with_prompt(message)
            .items(items)
            .interact()
            .ok()
    }

    fn multi_select(&self, message: &str, items: &[&str]) -> Option<Vec<usize>> {
        MultiSelect::new()
            .with_prompt(message)
            .items(items)
            .interact()
            .ok()
    }

    fn password(&self, message: &str) -> Option<String> {
        Password::new().with_prompt(message).interact().ok()
    }

    fn start_spinner(&self, message: &str) -> Box<dyn SpinnerHandle> {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        pb.set_message(message.to_string());
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        Box::new(SimpleSpinner(pb))
    }
}

struct SimpleSpinner(ProgressBar);

impl SpinnerHandle for SimpleSpinner {
    fn finish(&self, message: &str) {
        self.0.finish_with_message(message.to_string());
    }
}

struct SimpleGitContext;

impl GitContext for SimpleGitContext {
    fn is_repo(&self) -> bool {
        git2::Repository::discover(".").is_ok()
    }

    fn repo_root(&self) -> Option<std::path::PathBuf> {
        git2::Repository::discover(".")
            .ok()
            .and_then(|repo| repo.workdir().map(|p| p.to_path_buf()))
    }
}

struct SimpleFileSystem;

impl FileSystem for SimpleFileSystem {
    fn exists(&self, path: &std::path::Path) -> bool {
        path.exists()
    }

    fn write(&self, path: &std::path::Path, content: &str) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)
    }
}
