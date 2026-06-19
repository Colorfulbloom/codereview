//! Tool definitions and executor for agentic code review.
//!
//! The LLM uses these tools to explore a repository autonomously.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::review::models::{Category, ReviewFinding, Severity};

/// A tool definition for the Ollama tool-calling API.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// A tool call from the LLM.
///
/// Derives `Serialize` too (not just `Deserialize`) because the agent loop
/// echoes the assistant's `tool_calls` back into the next request's message
/// history, which is what Ollama expects before a `tool` result message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: Value,
}

/// Result of executing a tool.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub output: String,
    pub is_finished: bool,
}

/// Executes tools on behalf of the LLM agent.
///
/// All file paths are sandboxed to the repository root.
pub struct ToolExecutor {
    repo_root: PathBuf,
    findings: Vec<ReviewFinding>,
}

impl ToolExecutor {
    pub fn new(repo_root: PathBuf) -> Self {
        Self {
            repo_root,
            findings: Vec::new(),
        }
    }

    pub fn findings(&self) -> &[ReviewFinding] {
        &self.findings
    }

    /// Execute a tool call and return the result.
    pub fn execute(&mut self, call: &ToolCall) -> ToolResult {
        match call.function.name.as_str() {
            "list_files" => self.list_files(&call.function.arguments),
            "read_file" => self.read_file(&call.function.arguments),
            "search_code" => self.search_code(&call.function.arguments),
            "report_issue" => self.report_issue(&call.function.arguments),
            "finish_analysis" => ToolResult {
                output: "Analysis complete.".to_string(),
                is_finished: true,
            },
            name => ToolResult {
                output: format!("Unknown tool: {name}"),
                is_finished: false,
            },
        }
    }

    /// Validate that a path stays within the repo root (security sandboxing).
    fn safe_path(&self, relative: &str) -> Option<PathBuf> {
        let full = self.repo_root.join(relative);
        // Canonicalize to resolve ../ and symlinks
        let canonical = full.canonicalize().ok()?;
        let root_canonical = self.repo_root.canonicalize().ok()?;
        if canonical.starts_with(&root_canonical) {
            Some(canonical)
        } else {
            None
        }
    }

    fn list_files(&self, args: &Value) -> ToolResult {
        let dir = args
            .get("directory")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        let full_path = match self.safe_path(dir) {
            Some(p) => p,
            None => {
                return ToolResult {
                    output: "Access denied: path outside repository".to_string(),
                    is_finished: false,
                };
            }
        };

        if !full_path.is_dir() {
            return ToolResult {
                output: format!("Not a directory: {dir}"),
                is_finished: false,
            };
        }

        let mut entries = Vec::new();
        if let Ok(dir_entries) = std::fs::read_dir(&full_path) {
            for entry in dir_entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.')
                    || name == "node_modules"
                    || name == "target"
                    || name == "vendor"
                {
                    continue;
                }
                let suffix = if entry.path().is_dir() { "/" } else { "" };
                entries.push(format!("{name}{suffix}"));
            }
        }
        entries.sort();
        entries.truncate(200); // Cap to prevent context overflow

        ToolResult {
            output: entries.join("\n"),
            is_finished: false,
        }
    }

    fn read_file(&self, args: &Value) -> ToolResult {
        let path = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return ToolResult {
                    output: "Missing required parameter: path".to_string(),
                    is_finished: false,
                };
            }
        };

        let full_path = match self.safe_path(path) {
            Some(p) => p,
            None => {
                return ToolResult {
                    output: "Access denied: path outside repository".to_string(),
                    is_finished: false,
                };
            }
        };

        if !full_path.is_file() {
            return ToolResult {
                output: format!("Not a file: {path}"),
                is_finished: false,
            };
        }

        // Limit file size to 100KB
        if let Ok(metadata) = std::fs::metadata(&full_path)
            && metadata.len() > 100 * 1024
        {
            return ToolResult {
                output: "File too large (>100KB)".to_string(),
                is_finished: false,
            };
        }

        match std::fs::read_to_string(&full_path) {
            Ok(content) => ToolResult {
                output: content,
                is_finished: false,
            },
            Err(e) => ToolResult {
                output: format!("Failed to read file: {e}"),
                is_finished: false,
            },
        }
    }

    fn search_code(&self, args: &Value) -> ToolResult {
        let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return ToolResult {
                    output: "Missing required parameter: pattern".to_string(),
                    is_finished: false,
                };
            }
        };

        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        let mut results = Vec::new();
        self.search_in_dir(&self.repo_root, pattern, &mut results, max_results, 0);

        ToolResult {
            output: if results.is_empty() {
                "No matches found".to_string()
            } else {
                results.join("\n")
            },
            is_finished: false,
        }
    }

    /// Max directory depth for search to prevent stack overflow.
    const MAX_SEARCH_DEPTH: usize = 20;

    /// Text file extensions for search (skip binaries).
    const TEXT_EXTENSIONS: &'static [&'static str] = &[
        "rs", "py", "js", "ts", "jsx", "tsx", "go", "java", "c", "cpp", "h", "hpp", "rb", "php",
        "css", "scss", "html", "htm", "twig", "yml", "yaml", "json", "toml", "md", "txt", "sh",
        "module", "install", "theme", "inc", "vue", "svelte",
    ];

    fn search_in_dir(
        &self,
        dir: &Path,
        pattern: &str,
        results: &mut Vec<String>,
        max: usize,
        depth: usize,
    ) {
        if results.len() >= max || depth > Self::MAX_SEARCH_DEPTH {
            return;
        }

        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            if results.len() >= max {
                break;
            }

            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "vendor"
            {
                continue;
            }

            if path.is_dir() {
                self.search_in_dir(&path, pattern, results, max, depth + 1);
            } else if path.is_file() {
                // Only search text files
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !Self::TEXT_EXTENSIONS.contains(&ext) {
                    continue;
                }

                if let Ok(content) = std::fs::read_to_string(&path) {
                    for (line_num, line) in content.lines().enumerate() {
                        if line.contains(pattern) {
                            let rel_path = path.strip_prefix(&self.repo_root).unwrap_or(&path);
                            results.push(format!("{}:{}", rel_path.display(), line_num + 1));
                            if results.len() >= max {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    fn report_issue(&mut self, args: &Value) -> ToolResult {
        let severity = match args
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("warning")
        {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            _ => Severity::Info,
        };

        let category = match args
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("bug")
        {
            "bug" => Category::Bug,
            "security" => Category::Security,
            "performance" => Category::Performance,
            "style" => Category::Style,
            "best_practice" => Category::BestPractice,
            "accessibility" => Category::Accessibility,
            other => Category::Other(other.to_string()),
        };

        let finding = ReviewFinding {
            file_path: args
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            line_number: args
                .get("line_number")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize,
            end_line: args
                .get("end_line")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize),
            severity,
            category,
            title: args
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Issue")
                .to_string(),
            description: args
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            suggestion: args
                .get("suggestion")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        };

        self.findings.push(finding);

        ToolResult {
            output: "Issue reported.".to_string(),
            is_finished: false,
        }
    }
}

/// Get the tool definitions to send to Ollama.
pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "list_files".to_string(),
                description: "List files and directories. Use '.' for root.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "directory": { "type": "string", "description": "Directory path relative to repo root" }
                    },
                    "required": []
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "read_file".to_string(),
                description: "Read a source file's contents.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path relative to repo root" }
                    },
                    "required": ["path"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "search_code".to_string(),
                description: "Search for a text pattern in the codebase.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "Text to search for" },
                        "max_results": { "type": "integer", "description": "Max results (default 10)" }
                    },
                    "required": ["pattern"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "report_issue".to_string(),
                description: "Report a code issue found during analysis.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "file_path": { "type": "string" },
                        "line_number": { "type": "integer" },
                        "severity": { "type": "string", "enum": ["error", "warning", "info"] },
                        "category": { "type": "string", "enum": ["bug", "security", "performance", "style", "best_practice", "accessibility"] },
                        "title": { "type": "string" },
                        "description": { "type": "string" },
                        "suggestion": { "type": "string" }
                    },
                    "required": ["file_path", "line_number", "severity", "category", "title", "description"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "finish_analysis".to_string(),
                description: "Call when done analyzing the repository.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_repo() -> (TempDir, ToolExecutor) {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("main.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
        )
        .unwrap();

        let executor = ToolExecutor::new(dir.path().to_path_buf());
        (dir, executor)
    }

    #[test]
    fn list_files_root() {
        let (_dir, mut executor) = setup_test_repo();
        let result = executor.execute(&ToolCall {
            function: FunctionCall {
                name: "list_files".into(),
                arguments: json!({"directory": "."}),
            },
        });
        assert!(result.output.contains("main.rs"));
        assert!(result.output.contains("src/"));
        assert!(!result.is_finished);
    }

    #[test]
    fn list_files_subdirectory() {
        let (_dir, mut executor) = setup_test_repo();
        let result = executor.execute(&ToolCall {
            function: FunctionCall {
                name: "list_files".into(),
                arguments: json!({"directory": "src"}),
            },
        });
        assert!(result.output.contains("lib.rs"));
    }

    #[test]
    fn read_file_success() {
        let (_dir, mut executor) = setup_test_repo();
        let result = executor.execute(&ToolCall {
            function: FunctionCall {
                name: "read_file".into(),
                arguments: json!({"path": "main.rs"}),
            },
        });
        assert!(result.output.contains("fn main()"));
    }

    #[test]
    fn read_file_path_traversal_blocked() {
        let (_dir, mut executor) = setup_test_repo();
        let result = executor.execute(&ToolCall {
            function: FunctionCall {
                name: "read_file".into(),
                arguments: json!({"path": "../../../etc/passwd"}),
            },
        });
        assert!(result.output.contains("Access denied"));
    }

    #[test]
    fn search_code_finds_pattern() {
        let (_dir, mut executor) = setup_test_repo();
        let result = executor.execute(&ToolCall {
            function: FunctionCall {
                name: "search_code".into(),
                arguments: json!({"pattern": "println"}),
            },
        });
        assert!(result.output.contains("main.rs:2"));
    }

    #[test]
    fn search_code_no_results() {
        let (_dir, mut executor) = setup_test_repo();
        let result = executor.execute(&ToolCall {
            function: FunctionCall {
                name: "search_code".into(),
                arguments: json!({"pattern": "nonexistent_pattern_xyz"}),
            },
        });
        assert!(result.output.contains("No matches"));
    }

    #[test]
    fn report_issue_collects_finding() {
        let (_dir, mut executor) = setup_test_repo();
        let result = executor.execute(&ToolCall {
            function: FunctionCall {
                name: "report_issue".into(),
                arguments: json!({
                    "file_path": "main.rs",
                    "line_number": 2,
                    "severity": "warning",
                    "category": "style",
                    "title": "Debug output",
                    "description": "println in production code",
                    "suggestion": "Use logging framework"
                }),
            },
        });
        assert!(result.output.contains("reported"));
        assert_eq!(executor.findings().len(), 1);
        assert_eq!(executor.findings()[0].title, "Debug output");
    }

    #[test]
    fn finish_analysis_sets_finished() {
        let (_dir, mut executor) = setup_test_repo();
        let result = executor.execute(&ToolCall {
            function: FunctionCall {
                name: "finish_analysis".into(),
                arguments: json!({}),
            },
        });
        assert!(result.is_finished);
    }

    #[test]
    fn unknown_tool_returns_error() {
        let (_dir, mut executor) = setup_test_repo();
        let result = executor.execute(&ToolCall {
            function: FunctionCall {
                name: "nonexistent_tool".into(),
                arguments: json!({}),
            },
        });
        assert!(result.output.contains("Unknown tool"));
    }

    #[test]
    fn tool_definitions_count() {
        let tools = get_tool_definitions();
        assert_eq!(tools.len(), 5);
        let names: Vec<&str> = tools.iter().map(|t| t.function.name.as_str()).collect();
        assert!(names.contains(&"list_files"));
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"search_code"));
        assert!(names.contains(&"report_issue"));
        assert!(names.contains(&"finish_analysis"));
    }

    #[test]
    fn multiple_issues_collected() {
        let (_dir, mut executor) = setup_test_repo();
        for i in 0..3 {
            executor.execute(&ToolCall {
                function: FunctionCall {
                    name: "report_issue".into(),
                    arguments: json!({
                        "file_path": "main.rs",
                        "line_number": i,
                        "severity": "info",
                        "category": "style",
                        "title": format!("Issue {i}"),
                        "description": "test",
                    }),
                },
            });
        }
        assert_eq!(executor.findings().len(), 3);
    }
}
