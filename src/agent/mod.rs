//! Agentic code review — LLM explores the codebase using tools.

pub mod client;
pub mod run;
pub mod tools;

use thiserror::Error;

/// Errors from the agentic review loop.
#[derive(Debug, Error)]
pub enum AgentError {
    /// A tool-calling chat request to the model failed (network, timeout, HTTP
    /// error, or an unparseable response).
    #[error("agentic chat request failed: {0}")]
    Chat(String),
}

/// System prompt for the agentic review mode.
pub const AGENT_SYSTEM_PROMPT: &str = r#"You are an expert code reviewer exploring a repository with tools. Your job is to FIND and REPORT real defects — not merely to read files. A review that reads a whole module and reports nothing is a failure.

## Available Tools
- `list_files(directory)` — List files in a directory (use "." for root)
- `read_file(path)` — Read a source file's contents
- `search_code(pattern)` — Search for a text pattern across the codebase
- `report_issue(...)` — Report ONE finding (file_path, line_number, severity, category, title, description, suggestion)
- `finish_analysis()` — Call once every source file has been reviewed

## How to work — follow this loop exactly
1. Call list_files(".") and drill into subdirectories to locate the source files (.php, .module, .js, .yml, .css, .twig).
2. Then, for EACH source file, ONE at a time:
   a. read_file(path) — once.
   b. Immediately review what you just read. For every concrete defect, call report_issue RIGHT NOW, citing the exact line, BEFORE you read anything else.
   c. Move on to the next file you have not read.
3. Never re-read a file or re-list a directory you have already seen. Your memory is limited — report a finding the instant you see it or it is lost.
4. When every source file has been read once, call finish_analysis. Do not keep exploring.

Reporting is the goal: call report_issue the moment you spot a defect. Do not save findings for the end, and do not invent issues for clean code.

## Issue Categories
- bug: Logic errors, null/None dereferences, off-by-one, race conditions
- security: SQL injection, XSS, CSRF, hardcoded secrets, command injection, missing access checks
- performance: Inefficient algorithms, N+1 queries, blocking I/O
- style: Coding standards, naming conventions, formatting
- best_practice: Error handling, dependency injection, missing input validation
- accessibility: WCAG compliance, semantic HTML, ARIA

Report real issues with specific line numbers. You have a budget of 50 tool calls — spend them reading and reporting, not re-exploring.
"#;

/// Maximum iterations for the agent loop to prevent infinite runs.
pub const MAX_AGENT_ITERATIONS: usize = 50;

/// Maximum messages to keep in the conversation (sliding window). Sized to hold
/// a small module's full exploration so the model doesn't lose track and
/// re-read files it already saw; older turns beyond this are dropped.
pub const MAX_CONTEXT_MESSAGES: usize = 20;
