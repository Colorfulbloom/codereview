//! Splitting diffs into context-window-sized chunks.
//!
//! The model has a finite context window. Sending every diff in one request
//! either errors (when the prompt exceeds the model's architectural maximum) or
//! is silently truncated (when `num_ctx` is small). To avoid both, a review
//! packs files into chunks that fit a token budget and issues one request per
//! chunk, merging the findings.

use crate::git::{DiffHunk, FileDiff};

/// Conservative chars-per-token ratio. Real tokenizers average ~3.5–4 chars per
/// token for code; dividing by a smaller number *overestimates* the token count,
/// which keeps requests safely under the model's limit.
const CHARS_PER_TOKEN: usize = 3;

/// Tokens reserved within the context window for the model's response. Also
/// used as the `num_predict` generation cap on review calls: without a cap, a
/// model that slips into a repetition loop generates until the whole context
/// window fills — observed as a 20-minute single-call hang on real hardware.
pub const RESPONSE_HEADROOM_TOKENS: usize = 4096;

/// Default context window (tokens) used when the model's maximum can't be
/// detected and no override is configured.
pub const DEFAULT_CONTEXT_TOKENS: usize = 32_768;

/// Floor on the per-request input budget, so an unusually large system prompt
/// can't drive the available space to zero.
const MIN_INPUT_TOKENS: usize = 512;

/// Resolved context sizing for a review run.
#[derive(Debug, Clone, Copy)]
pub struct ContextBudget {
    /// `num_ctx` to request from Ollama — covers the prompt *and* the generated
    /// response. Kept stable across all requests in a run so Ollama doesn't
    /// reload the model between calls.
    pub num_ctx: usize,

    /// The `think` option for chat requests. `Some(false)` on thinking-capable
    /// models (review calls want JSON, not minutes of reasoning tokens);
    /// `None` for everything else — Ollama rejects the option on models
    /// without the capability.
    pub think: Option<bool>,
}

impl ContextBudget {
    /// A budget large enough that chunking never splits. Used in tests so a
    /// single mocked response satisfies one agent call.
    pub const fn unlimited() -> Self {
        Self {
            num_ctx: usize::MAX,
            think: None,
        }
    }

    /// Token budget available for the *user prompt* of one request, after
    /// reserving room for the system prompt and the model's response.
    pub fn input_token_budget(&self, system_prompt: &str) -> usize {
        self.num_ctx
            .saturating_sub(RESPONSE_HEADROOM_TOKENS)
            .saturating_sub(estimate_tokens(system_prompt))
            .max(MIN_INPUT_TOKENS)
    }
}

/// Estimate the token count of a string (a deliberate overestimate).
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(CHARS_PER_TOKEN)
}

/// Estimated token cost of rendering one file into the review prompt, including
/// a small allowance for the `=== path ===` and hunk headers.
fn file_diff_tokens(diff: &FileDiff) -> usize {
    let mut chars = diff.path.len() + 16;
    for hunk in &diff.hunks {
        chars += hunk.content.len() + 32;
    }
    chars.div_ceil(CHARS_PER_TOKEN)
}

/// Pack `diffs` into chunks whose estimated token cost stays within
/// `input_token_budget`. Files that individually exceed the budget are split by
/// line via [`split_file_diff`]. Order is preserved.
pub fn chunk_diffs(diffs: &[FileDiff], input_token_budget: usize) -> Vec<Vec<FileDiff>> {
    let budget = input_token_budget.max(MIN_INPUT_TOKENS);
    let mut chunks: Vec<Vec<FileDiff>> = Vec::new();
    let mut current: Vec<FileDiff> = Vec::new();
    let mut current_tokens = 0usize;

    for diff in diffs {
        let cost = file_diff_tokens(diff);

        if cost > budget {
            // Too big to ever fit alongside others — flush, then split it.
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
                current_tokens = 0;
            }
            for piece in split_file_diff(diff, budget) {
                chunks.push(vec![piece]);
            }
            continue;
        }

        if current_tokens + cost > budget && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current_tokens = 0;
        }

        current.push(diff.clone());
        current_tokens += cost;
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

/// Split one over-budget file into several single-file diffs, each within
/// budget. Splits happen on line boundaries, and each piece's `new_start` is set
/// to the new-file line number where it begins so findings keep accurate line
/// numbers.
fn split_file_diff(diff: &FileDiff, input_token_budget: usize) -> Vec<FileDiff> {
    // Leave room for the headers that wrap each piece's content.
    let char_budget = input_token_budget
        .saturating_mul(CHARS_PER_TOKEN)
        .saturating_sub(diff.path.len() + 64)
        .max(256);

    let mut pieces: Vec<FileDiff> = Vec::new();

    for hunk in &diff.hunks {
        // The new-file line number of the next line we emit.
        let mut new_line = hunk.new_start.max(1);
        let mut buf = String::new();
        let mut buf_start = new_line;
        let mut buf_new_lines = 0u32;

        for line in hunk.content.split_inclusive('\n') {
            // Deletions don't advance the new-file line counter; additions and
            // context lines do.
            let advances = !line.starts_with('-');

            if !buf.is_empty() && buf.len() + line.len() > char_budget {
                pieces.push(make_piece(diff, buf_start, buf_new_lines, std::mem::take(&mut buf)));
                buf_start = new_line;
                buf_new_lines = 0;
            }

            buf.push_str(line);
            if advances {
                buf_new_lines += 1;
                new_line += 1;
            }
        }

        if !buf.is_empty() {
            pieces.push(make_piece(diff, buf_start, buf_new_lines, buf));
        }
    }

    // A file with empty hunks still yields one (empty) piece so it isn't dropped.
    if pieces.is_empty() {
        pieces.push(diff.clone());
    }

    pieces
}

fn make_piece(diff: &FileDiff, new_start: u32, new_lines: u32, content: String) -> FileDiff {
    FileDiff {
        path: diff.path.clone(),
        status: diff.status,
        hunks: vec![DiffHunk {
            old_start: 0,
            old_lines: 0,
            new_start: new_start.max(1),
            new_lines,
            content,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::FileStatus;

    fn diff_with_lines(path: &str, n: usize) -> FileDiff {
        let mut content = String::new();
        for i in 1..=n {
            content.push_str(&format!("+line {i}\n"));
        }
        FileDiff {
            path: path.into(),
            status: FileStatus::Added,
            hunks: vec![DiffHunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: n as u32,
                content,
            }],
        }
    }

    #[test]
    fn estimate_is_conservative() {
        // 12 chars / 3 = 4 tokens (a real tokenizer would say ~3).
        assert_eq!(estimate_tokens("abcdefghijkl"), 4);
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn small_diffs_stay_in_one_chunk() {
        let diffs = vec![
            diff_with_lines("a.php", 3),
            diff_with_lines("b.php", 3),
            diff_with_lines("c.php", 3),
        ];
        let chunks = chunk_diffs(&diffs, 100_000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 3);
    }

    #[test]
    fn files_split_across_chunks_when_over_budget() {
        // Each file fits the (floored) budget on its own, but all three together
        // exceed it — so packing produces multiple chunks without splitting any
        // individual file.
        let diffs = vec![
            diff_with_lines("a.php", 80),
            diff_with_lines("b.php", 80),
            diff_with_lines("c.php", 80),
        ];
        // Sanity: a single file must be under the floored budget, or this would
        // exercise the split path instead of the packing path.
        assert!(file_diff_tokens(&diffs[0]) < MIN_INPUT_TOKENS);

        let chunks = chunk_diffs(&diffs, MIN_INPUT_TOKENS);
        assert!(
            chunks.len() > 1,
            "expected multiple chunks, got {}",
            chunks.len()
        );
        // Every original file appears exactly once across all chunks (no splitting).
        let total: usize = chunks.iter().map(|c| c.len()).sum();
        assert_eq!(total, 3);
    }

    #[test]
    fn oversized_single_file_is_split_by_line() {
        // One file far larger than the budget must be broken into several pieces.
        let big = diff_with_lines("huge.php", 5000);
        let chunks = chunk_diffs(&[big], MIN_INPUT_TOKENS);
        assert!(chunks.len() > 1, "oversized file should split");
        // Each chunk holds exactly one piece of the file.
        assert!(chunks.iter().all(|c| c.len() == 1));
        // Pieces carry ascending, non-overlapping new_start line numbers.
        let starts: Vec<u32> = chunks
            .iter()
            .map(|c| c[0].hunks[0].new_start)
            .collect();
        assert_eq!(starts[0], 1);
        for w in starts.windows(2) {
            assert!(w[1] > w[0], "line numbers must advance: {starts:?}");
        }
    }

    #[test]
    fn unlimited_budget_has_no_think_override() {
        assert!(ContextBudget::unlimited().think.is_none());
    }

    #[test]
    fn unlimited_budget_never_splits() {
        let budget = ContextBudget::unlimited();
        let input = budget.input_token_budget("a system prompt");
        let chunks = chunk_diffs(&[diff_with_lines("x.php", 100_000)], input);
        assert_eq!(chunks.len(), 1);
    }
}
