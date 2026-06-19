# Roadmap — Future Features

Ideas that have been designed (at least roughly) but deliberately not built yet.
Each entry records the motivation so future work doesn't have to rediscover it.

---

## Architecture assessment — keep the core, trim the edges (2026-06-17)

**Status:** observed during a full-codebase review; no work started.

A deep read of the codebase against the AI-code-review landscape (CodeRabbit,
Copilot review, Qodo, Semgrep/SonarQube) found the **core review pipeline is
lean and on-trend** — ~11K production LOC, 526 fully-mocked tests, and the
"deterministic linters own the mechanical rules / the LLM owns the semantics"
design is the same hybrid pattern the cloud leaders converged on. The DI traits
(`GitAgent`, `OllamaClient`, `PhpcsRunner`, `LinterRunner`, `FindingCache`) and
the hallucination gates are *justified*: the traits are what make the offline
526-test suite possible, and each gate traces to a real triaged false positive.
Don't "simplify" those away — that would be removing the test seams and the
empirically-earned guardrails.

The weight sits at the **edges**, orthogonal to local code review:

1. **`src/agent/` — agentic mode, built but unwired (~650 LOC).** See its own
   entry below. Cut or finish; today it is pure carrying cost.
2. **Onboarding + platform + credentials (~2.5K LOC + `oauth2`, `octocrab`,
   `keyring-core`, `reqwest`).** This whole cluster exists for the onboarding
   wizard's GitHub/GitLab PAT collection, token verification, and keyring
   storage — the only network surface in a "no code leaves the machine" tool,
   and orthogonal to review itself. The payoff feature it implies (auto-posting
   reviews as PR comments via octocrab) is **not wired up**; the annotations
   formatter only emits text. Decide whether PR-platform integration is a real
   roadmap item: if yes, finish it; if no, this is the single biggest de-bloat
   available — drop the four deps and the platform onboarding steps.
3. **Two config generators.** `src/init.rs` (`code-review init`) and onboarding's
   `team_config.rs` both write `.codereview.yaml`. Collapse to one path.

**One nuance inside the gate stack:** `js-no-var` and `promoted-constructor` are
patches for *one specific 9B model's specific misfires*. The durable gates —
evidence, existence, linter supersession — generalize; the pattern-specific ones
are a maintenance treadmill as models/rules change. Watch, don't cut.

**Strategic read:** most gate complexity is a *tax on the weak local model.* The
deterministic-linter layer is valuable regardless of model tier and is the real
differentiator. The highest-leverage direction is a stronger local model (e.g.
qwen2.5-coder-32b class) + leaning harder on the deterministic layer, not more
bespoke gates.

---

## Tier 4: `--verify` — LLM second-pass finding verification

**Status:** ✅ IMPLEMENTED 2026-06-15 (`src/review/verify.rs`). Opt-in via
`--verify` / `verify: true`; `verify_model:` overrides the judge. Scoped to
bug/security findings, per-finding, keep-on-uncertainty. The two observed
examples below ship as regression fixtures. Design record kept for context.

**Original status:** designed, not started. Deferred by choice on 2026-06-12.

**Context:** A real-world review of a Drupal module (qwen3.5:9b-mlx, 100
findings) measured ~44% precision — ~41 findings were hallucinated. Tiers 1–3
(shipped) attack this with *deterministic* checks: line-numbered prompts, a
required `evidence` quote verified against the actual source, no-op-fix
detection, negative guidance in rule descriptions, and accuracy guardrails in
every system prompt. The **existence gate** (shipped, `src/review/claims.rs`)
adds one more deterministic check: a finding claiming an API "does not exist →
fatal error" is dropped when that symbol is found defined in the project /
framework source on disk. Together these catch fabricated code, self-identical
fixes, and false not-found claims — but they cannot catch a finding that quotes
real code while *misjudging* it (e.g. "SQL injection" against a correctly
parameterized query builder line). That residual interpretation class is what
Tier 4 is for.

**Design:** an opt-in second pass (CLI `--verify`, config `verify: true`):

1. After the normal agent pipeline produces findings, group them per file.
2. For each finding, send a focused prompt: the finding (title, description,
   evidence, suggestion) plus the surrounding code context, asking one
   question: *"Is this finding correct for this code? Answer with a JSON
   verdict: {\"valid\": bool, \"reason\": string}."*
3. Drop findings judged invalid; optionally annotate survivors with the
   confirmation.
4. Use the same model by default; allow `-m`-style override so a larger model
   can judge a smaller model's findings.

**As shipped (where it diverged from the design above):** verification is
**per-finding**, not grouped per file (point 1) — each finding is judged alone
for maximum focus. It is **scoped to bug/security findings** (interpretation
hallucinations cluster there; style and phpcs findings are skipped). The verdict
prompt judges **presence, not importance** — `valid:false` is reserved for a
genuine misread, so a real-but-minor finding is kept (this guardrail was added
after a live run dropped a real "missing try/catch" on a relevance basis). The
model override is the `verify_model` config key (point 4). Keep-on-uncertainty
throughout: an errored, timed-out, or unparseable verdict keeps the finding.

**Why opt-in, not default:** it roughly doubles wall-clock time (one extra LLM
call per in-scope finding). On the reference hardware (MacBook Air M5 16GB)
that's too expensive to run on every iteration, so it ships behind `--verify` /
`verify: true`. The pass runs *after* the per-file cache, so an unchanged
re-review still serves agent findings from cache and only spends new calls on
the verify step (≈74s warm on the 16-file reference module vs. ≈20 min cold).

**Prerequisite worth keeping:** the verdict prompt should reuse the evidence
field that Tier 1 introduced — a finding that survived deterministic
verification already carries its quoted line.

**Observed examples (Tier-4 regression fixtures, captured 2026-06-15):** a
`qwen3.5:9b` review of the `bcutd_heatmap` module (phpcs active, all other gates
shipped) produced exactly two false positives, and both are the residual
interpretation class above — each quoted a real line, so every deterministic
gate passed, yet each misjudged correct code:

- *"Missing null check after JSON decode"* on `HeatmapTrackController::track`.
  The model claimed `$data['events']` is dereferenced before any null check.
  The very next line is `if (!is_array($data) || empty($data['events']) || ...)`
  — `||` short-circuits on `!is_array($data)` for a `null` decode, so the
  "offending" access never runs. The model misread boolean evaluation order.
- *"Missing error handling for external API call"* on
  `HeatmapDataController::ga4`. The finding's own prose admits the method "HAS a
  try-catch block (lines 79-86)", then asserts the inputs are unvalidated — but
  the lines above already do
  `if (empty($property_id) || empty($credentials_json)) { return ...400; }`.
  Both premises are false; it describes no real defect.

Both are *self-contained misreads of correct code* — no cross-file context would
fix them, which is why a focused per-finding second pass (the design above) is
the right shape. Use these two as the regression fixtures when Tier 4 is built.

---

## Agentic verification — model-requested reference lookups

**Status:** designed, deferred 2026-06-12. Complement to the shipped existence
gate, not a replacement.

**Idea:** instead of the app verifying the model's claims after the fact, let
the model *ask*: the prompt instructs it that before asserting any API does or
doesn't exist it must emit a `VERIFY <symbol>` request; the app greps the
source (reusing `claims::SourceIndex`) and feeds the answer back; the model then
finalizes without the false claim. The same retrieval primitive the gate
already builds, consumed as a tool instead of a post-filter.

**Why it does NOT replace the deterministic gate:** the harmful hallucinations
are *confident* ("this will cause a fatal error") — the model doesn't feel
uncertain, so a "verify when unsure" trigger misses exactly the findings that
matter. A mandatory "always verify this claim type" rule is better but relies
on a 9B model following a fine-grained procedural instruction deep into a long
generation, requires a multi-turn tool-call loop the current one-shot pipeline
doesn't have, and multiplies the already-painful review time. The grep gate
enforces the *same policy* deterministically, for free, after generation — so
it is the floor; agentic verification is an optional layer on top.

**When it becomes worthwhile:** with a larger, tool-reliable model (e.g.
`qwen3.5:27b`, already on the reference machine) and the multi-turn chat
plumbing. It generalizes beyond existence claims — the model could request any
reference — which is its real advantage over the narrow gate.

---

## Agentic review mode — `src/agent/` (built but unwired)

**Status:** primitive built + tested (origin predates the current Tier-1–4
work); the driving loop was never written. Decision pending: finish or cut.

**What exists (`src/agent/`, ~650 LOC, 12 passing tests):** a complete
tool-using foundation for an *alternative* review strategy where the model
navigates the repo itself instead of being fed a chunked diff —

- `AGENT_SYSTEM_PROMPT` + `MAX_AGENT_ITERATIONS = 50` + `MAX_CONTEXT_MESSAGES =
  10` (`mod.rs`);
- `ToolExecutor` with five tools — `list_files`, `read_file`, `search_code`,
  `report_issue`, `finish_analysis` — each path-sandboxed to the repo root
  (canonicalize + `starts_with`, `../` traversal blocked and tested), with size/
  depth/result caps, collecting findings into `ReviewFinding`s (`tools.rs`);
- `get_tool_definitions()` — the Ollama tool-calling schema for all five.

**What is missing (the glue):** the agent loop itself — send the system prompt +
tool definitions to Ollama `/api/chat` with `tools`, receive a `ToolCall`,
dispatch via `ToolExecutor::execute`, feed the `ToolResult` back under the
`MAX_CONTEXT_MESSAGES` sliding window, repeat up to `MAX_AGENT_ITERATIONS` until
`is_finished`, then return `executor.findings()` as a `ReviewResult`. Nothing in
`main`/`repl`/`cli` invokes any of it; `pub mod agent` is the only reference, so
it compiles but never runs.

**Why it matters / the fork:** this is a *different review shape* than the
shipped pipeline — whole-repo, cross-file, model-directed exploration vs. the
current diff-scoped, app-directed chunking. It is also the concrete tooling the
"Agentic verification" entry above assumes (its `VERIFY <symbol>` retrieval is
just `search_code` + `read_file`). Both share the same prerequisite: a larger,
tool-reliable local model (qwen2.5-coder-32b / qwen3.5:27b), which the one-shot
chunked pipeline does not need. Until that model is the default, the loop adds
review time and tool-following risk on a 9B for no clear win.

**Decision:** finish it (wire the loop, gate behind `--agentic`, validate against
the same Tier-4 fixtures) **only** if pursuing the stronger-model direction;
otherwise delete `src/agent/` and reclaim the carrying cost. Either way the
shipped chunked agents in `src/review/agents/` are unaffected — different thing
entirely (see naming note).

**Naming note:** `src/review/agents/` ("security", "bugs", "style", …) are *not*
agentic — they are specialized one-shot prompt templates run in parallel. Only
`src/agent/` is an "agent" in the tool-using sense. Worth disambiguating in the
docs so the two aren't conflated.

---

## Idle-based streaming timeout

**Status:** idea.

The per-request timeout (`llm_timeout_seconds`) is wall-clock: a
slow-but-progressing call and a genuinely hung one look identical until the
deadline. Switching the Ollama calls to `"stream": true` would let the client
reset a (shorter) idle timer on every received token — slow hardware finishes
unbounded work while a stalled server still fails fast. Replaces the "raise
the timeout until it works" guesswork.

---

## Surface skipped files in path mode

**Status:** idea.

Path mode silently skips unsupported, binary, empty, and >256KB files
(`src/review/source.rs`). A one-line stderr summary ("skipped 3 file(s): 2
unsupported, 1 over 256KB") would prevent "why wasn't my file reviewed?"
confusion.

---

## Verify recommended model names against the Ollama registry

**Status:** open question.

README and `init` recommendations include model names (e.g. `gemma4`) that
have not been checked against what Ollama's registry actually serves. A wrong
name makes the quick-start fail on copy-paste.

---

## Selective / per-language cache clearing

**Status:** considered, deferred 2026-06-18. The full `clear-cache` command
(CLI `code-review clear-cache` + REPL `/clear-cache`) shipped that day; a
*selective* form (`clear-cache javascript css`) was scoped and skipped.

**Idea:** let `clear-cache` take language (or path) filters so you can drop just
the JS/CSS entries without losing the PHP cache.

**Why it's non-trivial:** the `file_review_cache` table stores only
`(cache_key, findings_json)`, where `cache_key` is an **opaque hash** of
`(PROMPT_VERSION, agent, model, rules, file_content)` (`review/cache.rs`). The
language isn't a queryable column, so there's no `WHERE language = ?`. Doing it
properly means: add a `language` column (+ an `ALTER TABLE` migration for
existing `.codereview/state.db`), thread the language through
`FindingCache::put` (currently `put(key, findings)` — touches the trait, both
impls, the `MemoryCache` mock, and the orchestrator call sites), add
`clear_languages(&[Language])`, and parse the args in CLI/REPL. A few hours of
TDD — moderate, not free.

**Why it's probably not worth it (the real reason deferred):** the cache already
self-invalidates at language granularity. The **rule set is part of the cache
key**, so changing a language's rules (enable/disable, severity) misses that
language's entries automatically; changing the model or bumping `PROMPT_VERSION`
busts everything. And the deterministic linters (ESLint/Stylelint/phpcs) are
**not cached at all** — they re-run every review. So the scenarios where a manual
per-language clear does something the system doesn't already do are very narrow.

**Cheaper alternative if the need ever arises:** `clear-cache` then review only
the target path (`--path js/`) — the cache rebuilds lazily for just what's
touched. Revisit only with a concrete workflow the rule-aware auto-busting
doesn't cover.

---

## Adding an entry

Keep the format: **Status**, the motivating context (what happened that made
this worth writing down), the rough design if one exists, and why it isn't
built yet.
