# vtcode-core
[Root AGENTS.md](../AGENTS.md) | Agent loop, tools, LLM, safety, UI.

## Module map

- `core/agent/`: runtime, event recording, runner, progress, harness artifacts; `request_envelope` freezes each segment's prompt and ordered catalog.
- `tools/`: handlers, registry, policy, execution; `read_file/batch.rs` owns batch admission, bounded fan-out, and ordered assembly while `read_file.rs` owns single-file semantics.
- `llm/`: re-export facade; providers live in `vtcode-llm`. `exec/events.rs` re-exports canonical `vtcode-exec-events::ThreadEvent`; `prompts/` owns cached static profiles, compiled runtime guidance, and bounded resource caches.

## Rules

- Re-export public APIs from `lib.rs`; consumers must not reach into submodules, and keep constants in `config::constants` rather than inline.
- `exec_policy` and `command_safety` are separate policy layers.
- Session event sinks are authoritative: preserve ordering, bounded backpressure, and await the drain handle before reporting task success. Recovery/final assistant messages must use the canonical `ThreadEvent` path rather than history-only writes.
- Trajectory logging is best effort: retain line/byte bounds and expose drops/failures through diagnostics.

## Workflows
- Add tools under `tools/`, register/classify them, wire them into `core/agent/`; approved-plan task trackers deduplicate normalized step descriptions while preserving first-seen order, and approval must fail closed if the tracker cannot be persisted. Add providers with `adding-llm-providers` and update model enumeration, presets, and the `llm/providers` facade.

## Gotchas

- Import `RetryPolicyCoreExt` for domain retry methods; budget decisions belong to `llm/usage_cost.rs` and `BudgetStatus::classify`, not call sites.
- Compaction preserves conversation and starts a new immutable request segment before replacing history; `context_reset.rs` intentionally discards it. Keep boundary reasons on the shared transition path.
- `AgentSessionState.messages` is `Arc<Vec<Message>>`; use `messages_mut()`/`Arc::make_mut` for mutations so request and continuation histories stay shared without deep copies.
- Async plugin, skill, file-tool, planning, persistent-memory, durable-scheduler, and prompt-resource cache-miss paths must use Tokio filesystem APIs or `spawn_blocking` for recursive/synchronous scans, record loading, claim files, persistence, or Git work.
- `TerminalAppLauncher::launch_editor_target_non_waiting` is for existing-file GUI opens; preserve adapter-specific reuse/location flags and keep temporary-file `/edit` flows on the waiting API.
- Token/catalog deferral thresholds are correct behavior; warn only when deferral is disabled but beneficial. Shell safety must preserve raw command text when classifying dynamic syntax; keep optional SQLite on rusqlite 0.39 until stable cfg_select support is available.
- `web_fetch` accepts remote HTTP(S) URLs only; local workspace reads must use `read_file`/`unified_file`, and its structured error should direct the model to that fallback. `AnsiRenderer` owns the session-local tool-summary display mode; compact batches flush before non-success summaries and never change the `ThreadEvent` contract.
- Planning snapshots may retain normal-mode definitions for provider cache stability, but active names stay read-only (`exec_command`, `code_search`, and enabled `request_user_input`); direct `write_stdin`/`apply_patch` calls remain blocked while internal plan persistence stays available.
