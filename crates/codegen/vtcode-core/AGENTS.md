# vtcode-core
[Root AGENTS.md](../AGENTS.md) | Agent loop, tools, LLM, safety, UI.

## Module map

- `core/agent/`: runtime, event recording, runner, progress, harness artifacts.
- `tools/`: handlers, registry, policy, execution; `read_file/batch.rs` owns batch admission, bounded fan-out, and ordered assembly while `read_file.rs` owns single-file semantics.
- `llm/`: re-export facade; providers live in `vtcode-llm`. `exec/events.rs` re-exports canonical `vtcode-exec-events::ThreadEvent`.

## Rules

- Re-export public APIs from `lib.rs`; consumers must not reach into submodules.
- Keep constants in `config::constants`, not inline.
- `exec_policy` and `command_safety` are separate policy layers.
- Session event sinks are authoritative: preserve ordering, bounded backpressure, and await the drain handle before reporting task success.
- Trajectory logging is best effort: retain line/byte bounds and expose drops/failures through diagnostics.

## Workflows
- Add tools under `tools/`, register/classify them, wire them into `core/agent/`; add providers with `adding-llm-providers` and update model enumeration, presets, and the `llm/providers` facade.

## Gotchas

- `RetryPolicyCoreExt` must be imported for domain retry methods.
- Budget decisions belong to `llm/usage_cost.rs` and `BudgetStatus::classify`; do not re-derive them at call sites.
- Compaction preserves conversation; `context_reset.rs` intentionally discards it.
- `AgentSessionState.messages` is `Arc<Vec<Message>>`; use `messages_mut()`/`Arc::make_mut` for mutations so request and continuation histories stay shared without deep copies.
- Async plugin, skill, file-tool, planning, persistent-memory, and durable-scheduler paths must use Tokio filesystem APIs or `spawn_blocking` for recursive/synchronous scans, record loading, claim files, persistence, or Git work.
- `TerminalAppLauncher::launch_editor_target_non_waiting` is for existing-file GUI opens; preserve adapter-specific reuse/location flags and keep temporary-file `/edit` flows on the waiting API.
- Token/catalog deferral thresholds are correct behavior; warn only when deferral is disabled but beneficial. Shell safety must preserve raw command text when classifying dynamic syntax.
- `web_fetch` accepts remote HTTP(S) URLs only; local workspace reads must use `read_file`/`unified_file`, and its structured error should direct the model to that fallback. `AnsiRenderer` owns the session-local tool-summary display mode; compact batches flush before non-success summaries and never change the `ThreadEvent` contract.
