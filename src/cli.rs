use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "code-review",
    version,
    about = "AI-powered local code review using Ollama"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Run a review without entering the REPL (non-interactive mode).
    /// Specify what to diff: a branch name, HEAD~N, or a commit hash.
    #[arg(long, value_name = "REF")]
    pub diff: Option<String>,

    /// Review a specific file or directory as-is, regardless of git state.
    /// Every supported file under the path is reviewed (a module, a theme, or
    /// any loose code). Implies non-interactive mode; takes precedence over --diff.
    #[arg(long, value_name = "PATH")]
    pub path: Option<String>,

    /// Review all uncommitted changes (staged, unstaged, untracked) without
    /// entering the REPL. Useful for pre-commit hooks and scripting.
    #[arg(long)]
    pub uncommitted: bool,

    /// Output format for non-interactive mode.
    #[arg(long, value_enum, default_value = "terminal")]
    pub format: OutputFormat,

    /// Override the Ollama model.
    #[arg(long, short)]
    pub model: Option<String>,

    /// Verify findings with an LLM second pass: re-check each bug/security
    /// finding against the code and drop interpretation hallucinations (a
    /// "missing" check that exists on the next line, etc.). Adds one LLM call
    /// per in-scope finding. Equivalent to `verify: true` in .codereview.yaml.
    #[arg(long)]
    pub verify: bool,

    /// Output file path (for markdown/json formats).
    #[arg(long, short)]
    pub output: Option<String>,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    Terminal,
    Json,
    Markdown,
    Annotations,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the onboarding wizard (re-runnable at any time)
    Onboard {
        /// Start fresh, ignoring any prior progress
        #[arg(long)]
        reset: bool,
    },
    /// Generate a .codereview.yaml configuration file for your project
    Init,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_variants() {
        // Verify all variants exist and parse
        let _t: OutputFormat = OutputFormat::Terminal;
        let _j: OutputFormat = OutputFormat::Json;
        let _m: OutputFormat = OutputFormat::Markdown;
        let _a: OutputFormat = OutputFormat::Annotations;
    }

    #[test]
    fn cli_parses_no_args() {
        let cli = Cli::parse_from(["code-review"]);
        assert!(cli.command.is_none());
        assert!(cli.diff.is_none());
        assert!(cli.model.is_none());
    }

    #[test]
    fn cli_parses_diff_flag() {
        let cli = Cli::parse_from(["code-review", "--diff", "main"]);
        assert_eq!(cli.diff.as_deref(), Some("main"));
    }

    #[test]
    fn cli_parses_path_flag() {
        let cli = Cli::parse_from(["code-review", "--path", "docroot/modules/custom/foo"]);
        assert_eq!(cli.path.as_deref(), Some("docroot/modules/custom/foo"));
        assert!(cli.diff.is_none());
    }

    #[test]
    fn cli_parses_uncommitted_flag() {
        let cli = Cli::parse_from(["code-review", "--uncommitted"]);
        assert!(cli.uncommitted);
        assert!(cli.diff.is_none());
        assert!(cli.path.is_none());
    }

    #[test]
    fn cli_parses_format_flag() {
        let cli = Cli::parse_from(["code-review", "--diff", "main", "--format", "json"]);
        assert!(matches!(cli.format, OutputFormat::Json));
    }

    #[test]
    fn cli_parses_model_flag() {
        let cli = Cli::parse_from(["code-review", "-m", "gemma4"]);
        assert_eq!(cli.model.as_deref(), Some("gemma4"));
    }

    #[test]
    fn cli_parses_verify_flag() {
        let cli = Cli::parse_from(["code-review", "--uncommitted", "--verify"]);
        assert!(cli.verify);

        let cli = Cli::parse_from(["code-review"]);
        assert!(!cli.verify);
    }

    #[test]
    fn cli_parses_output_flag() {
        let cli = Cli::parse_from(["code-review", "--diff", "main", "-o", "report.md"]);
        assert_eq!(cli.output.as_deref(), Some("report.md"));
    }

    #[test]
    fn cli_parses_onboard_subcommand() {
        let cli = Cli::parse_from(["code-review", "onboard", "--reset"]);
        assert!(matches!(
            cli.command,
            Some(Command::Onboard { reset: true })
        ));
    }
}
