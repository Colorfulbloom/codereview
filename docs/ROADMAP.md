# Roadmap — Future Features

Ideas that have been designed (at least roughly) but deliberately not built yet.
Each entry records the motivation so future work doesn't have to rediscover it.

---

## Tier 4: `--verify` — LLM second-pass finding verification

**Status:** designed, not started. Deferred by choice on 2026-06-12.

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

**Why deferred:** it roughly doubles wall-clock time (one extra LLM call per
finding or per file). On the reference hardware (MacBook Air M5 16GB, ~20 min
for a 16-file module) that's too expensive to be default-on. Revisit once the
Tier 1–3 precision numbers are known from real usage.

**Prerequisite worth keeping:** the verdict prompt should reuse the evidence
field that Tier 1 introduced — a finding that survived deterministic
verification already carries its quoted line.

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

## Adding an entry

Keep the format: **Status**, the motivating context (what happened that made
this worth writing down), the rough design if one exists, and why it isn't
built yet.
