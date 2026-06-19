//! The agentic review loop: the model drives, we execute its tool calls.
//!
//! Unlike the shipped pipeline (chunk the diff, send N one-shot prompts), this
//! lets the model navigate the repository itself: it calls `list_files` /
//! `read_file` / `search_code`, reports issues, and stops via `finish_analysis`.
//! Whether a small local model can actually follow this protocol is the open
//! question this spike exists to answer.

use std::path::PathBuf;
use std::time::Instant;

use super::client::{AgentChatClient, AgentMessage};
use super::tools::{ToolCall, ToolExecutor, get_tool_definitions};
use super::{AGENT_SYSTEM_PROMPT, AgentError, MAX_AGENT_ITERATIONS, MAX_CONTEXT_MESSAGES};
use crate::review::models::ReviewResult;

/// The opening instruction that kicks the model into the explore loop.
const SEED_USER_PROMPT: &str = "Review this module now. Call list_files(\".\") first, then read each source file ONE at a time and call report_issue immediately for every defect you find in it before moving on to the next file. When every source file has been read once, call finish_analysis. Reporting findings is the goal — do not just read.";

/// Run an agentic review: drive `client` through the tool loop until it finishes
/// (or the iteration budget runs out), then collect whatever it reported into a
/// [`ReviewResult`].
pub async fn run_agentic_review<C: AgentChatClient + ?Sized>(
    repo_root: PathBuf,
    client: &C,
    model: &str,
    think: Option<bool>,
    on_event: &dyn Fn(&str),
) -> Result<ReviewResult, AgentError> {
    let start = Instant::now();
    let mut executor = ToolExecutor::new(repo_root);
    let tools = get_tool_definitions();
    let mut messages = vec![
        AgentMessage::system(AGENT_SYSTEM_PROMPT),
        AgentMessage::user(SEED_USER_PROMPT),
    ];

    for _ in 0..MAX_AGENT_ITERATIONS {
        // A chat failure mid-exploration (timeout, or the qwen tool-call parser
        // 500-ing on a corrupted generation) must not throw away everything
        // found so far. Stop and return the partial result, like the rest of
        // the pipeline keeps findings on uncertainty.
        let turn = match client.chat_turn(model, &messages, &tools, think).await {
            Ok(t) => t,
            Err(e) => {
                on_event(&format!("chat request failed — stopping with partial results: {e}"));
                crate::logging::warn(format!("agentic loop stopped on chat error: {e}"));
                break;
            }
        };

        // No tool calls means the model produced a final answer (or, on a small
        // model, gave up / replied in prose). Surface what it said and stop.
        if turn.tool_calls.is_empty() {
            let said = turn.content.trim();
            if said.is_empty() {
                on_event("model returned no tool call and no text — stopping");
            } else {
                on_event(&format!("model stopped with prose (no tool call): {}", preview(said)));
            }
            break;
        }

        // Echo the assistant turn (with its tool_calls) before the tool results,
        // which is the message order Ollama expects.
        messages.push(AgentMessage::assistant(
            turn.content.clone(),
            turn.tool_calls.clone(),
        ));

        let mut finished = false;
        for call in &turn.tool_calls {
            on_event(&describe_call(call));
            let result = executor.execute(call);
            messages.push(AgentMessage::tool(result.output));
            finished |= result.is_finished;
        }
        if finished {
            break;
        }

        trim_messages(&mut messages, MAX_CONTEXT_MESSAGES);
    }

    Ok(ReviewResult {
        findings: executor.findings().to_vec(),
        files_reviewed: 0,
        model_used: model.to_string(),
        duration: start.elapsed(),
        rules_applied: 0,
        languages_detected: Vec::new(),
        has_custom_config: false,
        agents_ran: vec!["agentic".to_string()],
    })
}

/// A short human-readable label for a tool call, e.g. `read_file(src/Foo.php)`
/// or `report_issue("SQL injection")`. Used for progress output.
fn describe_call(call: &ToolCall) -> String {
    let f = &call.function;
    let detail = ["directory", "path", "pattern", "title"]
        .iter()
        .find_map(|k| f.arguments.get(*k).and_then(|v| v.as_str()))
        .unwrap_or("");
    if detail.is_empty() {
        format!("{}()", f.name)
    } else {
        format!("{}({detail})", f.name)
    }
}

/// First line of `s`, truncated for one-line progress display.
fn preview(s: &str) -> String {
    let line = s.lines().next().unwrap_or("");
    if line.len() > 100 {
        format!("{}…", &line[..100])
    } else {
        line.to_string()
    }
}

/// Bound the conversation to `max` messages, always keeping the system prompt
/// (index 0) and dropping the oldest turns first. Prevents a long exploration
/// from overflowing a small model's context window.
fn trim_messages(messages: &mut Vec<AgentMessage>, max: usize) {
    if messages.len() <= max {
        return;
    }
    let overflow = messages.len() - max;
    messages.drain(1..1 + overflow);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::AgentTurn;
    use crate::agent::tools::{FunctionCall, ToolCall, ToolDefinition};
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// Replays a fixed script of assistant turns, then returns empty turns. Also
    /// records how many messages it was sent each turn (to assert windowing).
    struct ScriptedClient {
        turns: Mutex<VecDeque<AgentTurn>>,
        message_counts: Mutex<Vec<usize>>,
    }

    impl ScriptedClient {
        fn new(turns: Vec<AgentTurn>) -> Self {
            Self {
                turns: Mutex::new(turns.into()),
                message_counts: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl AgentChatClient for ScriptedClient {
        async fn chat_turn(
            &self,
            _model: &str,
            messages: &[AgentMessage],
            _tools: &[ToolDefinition],
            _think: Option<bool>,
        ) -> Result<AgentTurn, AgentError> {
            self.message_counts.lock().unwrap().push(messages.len());
            Ok(self
                .turns
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(AgentTurn {
                    content: "done".into(),
                    tool_calls: Vec::new(),
                }))
        }
    }

    fn tool_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            function: FunctionCall {
                name: name.into(),
                arguments: args,
            },
        }
    }

    fn turn(calls: Vec<ToolCall>) -> AgentTurn {
        AgentTurn {
            content: String::new(),
            tool_calls: calls,
        }
    }

    #[tokio::test]
    async fn loop_runs_tools_collects_findings_and_stops_on_finish() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();

        let client = ScriptedClient::new(vec![
            turn(vec![tool_call("list_files", json!({"directory": "."}))]),
            turn(vec![tool_call("read_file", json!({"path": "main.rs"}))]),
            turn(vec![tool_call(
                "report_issue",
                json!({
                    "file_path": "main.rs",
                    "line_number": 1,
                    "severity": "warning",
                    "category": "bug",
                    "title": "Empty main",
                    "description": "main does nothing"
                }),
            )]),
            turn(vec![tool_call("finish_analysis", json!({}))]),
        ]);

        let result = run_agentic_review(dir.path().to_path_buf(), &client, "test-model", None, &|_| {})
            .await
            .unwrap();

        assert_eq!(result.findings.len(), 1, "the reported issue is collected");
        assert_eq!(result.findings[0].title, "Empty main");
        assert_eq!(result.model_used, "test-model");
        assert_eq!(result.agents_ran, vec!["agentic".to_string()]);
        // It stopped on finish_analysis (4 turns), not by exhausting the budget.
        assert_eq!(client.message_counts.lock().unwrap().len(), 4);
    }

    /// Reports one issue, then the next chat turn errors (simulating the live
    /// 500 we hit). The run must keep the partial finding, not abort.
    struct ErrorAfterReport {
        calls: Mutex<usize>,
    }

    #[async_trait]
    impl AgentChatClient for ErrorAfterReport {
        async fn chat_turn(
            &self,
            _model: &str,
            _messages: &[AgentMessage],
            _tools: &[ToolDefinition],
            _think: Option<bool>,
        ) -> Result<AgentTurn, AgentError> {
            let mut n = self.calls.lock().unwrap();
            *n += 1;
            if *n == 1 {
                Ok(turn(vec![tool_call(
                    "report_issue",
                    json!({
                        "file_path": "a.rs",
                        "line_number": 1,
                        "severity": "error",
                        "category": "security",
                        "title": "Found before crash",
                        "description": "x"
                    }),
                )]))
            } else {
                Err(AgentError::Chat("simulated 500".into()))
            }
        }
    }

    #[tokio::test]
    async fn loop_keeps_partial_findings_when_chat_errors() {
        let dir = TempDir::new().unwrap();
        let client = ErrorAfterReport {
            calls: Mutex::new(0),
        };

        let result = run_agentic_review(dir.path().to_path_buf(), &client, "m", None, &|_| {})
            .await
            .expect("a chat error mid-loop must not abort the whole review");

        assert_eq!(
            result.findings.len(),
            1,
            "the finding reported before the error is retained"
        );
        assert_eq!(result.findings[0].title, "Found before crash");
    }

    #[tokio::test]
    async fn loop_stops_at_iteration_budget_when_model_never_finishes() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();

        // Empty script → every turn falls through to a perpetual list_files-style
        // tool call that never finishes. Use a non-finishing tool call.
        let mut turns = Vec::new();
        for _ in 0..(MAX_AGENT_ITERATIONS + 5) {
            turns.push(turn(vec![tool_call("list_files", json!({"directory": "."}))]));
        }
        let client = ScriptedClient::new(turns);

        let result = run_agentic_review(dir.path().to_path_buf(), &client, "m", None, &|_| {})
            .await
            .unwrap();

        assert_eq!(result.findings.len(), 0);
        // Never called the model more than the budget allows.
        assert_eq!(
            client.message_counts.lock().unwrap().len(),
            MAX_AGENT_ITERATIONS
        );
    }

    #[test]
    fn trim_keeps_system_and_most_recent() {
        let mut messages = vec![AgentMessage::system("SYS")];
        for i in 0..20 {
            messages.push(AgentMessage::user(format!("m{i}")));
        }
        trim_messages(&mut messages, MAX_CONTEXT_MESSAGES);

        assert_eq!(messages.len(), MAX_CONTEXT_MESSAGES);
        assert_eq!(messages[0].content, "SYS", "system prompt is preserved");
        // Last message is the most recent one we pushed.
        assert_eq!(messages.last().unwrap().content, "m19");
    }

    #[test]
    fn trim_is_noop_below_limit() {
        let mut messages = vec![
            AgentMessage::system("SYS"),
            AgentMessage::user("a"),
            AgentMessage::user("b"),
        ];
        trim_messages(&mut messages, MAX_CONTEXT_MESSAGES);
        assert_eq!(messages.len(), 3);
    }
}
