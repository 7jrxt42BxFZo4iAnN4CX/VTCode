# vtcode-core

[Root AGENTS.md](../AGENTS.md) | Agent loop, tools, LLM, safety, UI.

## Module map

- `core/agent/`: runtime, event recording, runner, progress, harness artifacts.
- `tools/`: handlers, registry, policy, execution; `read_file/batch.rs` owns batch admission, bounded fan-out, and ordered assembly while `read_file.rs` owns single-file semantics.
- `llm/`: re-export facade; provider implementations live in `vtcode-llm`.
- `exec/events.rs`: re-exports the canonical `vtcode-exec-events::ThreadEvent`.

## Rules

- Re-export public APIs from `lib.rs`; consumers must not reach into submodules.
- Keep constants in `config::constants`, not inline.
- `exec_policy` and `command_safety` are separate policy layers.
- Session event sinks are authoritative: preserve ordering, bounded backpressure, and await the drain handle before reporting task success.
- Trajectory logging is best effort: retain line/byte bounds and expose drops/failures through diagnostics.

## Workflows
- Add tools under `tools/`, register them, classify them in `ToolPolicy`, then wire them into `core/agent/`.
- Add providers with `adding-llm-providers`; update model enumeration, presets, and the `llm/providers` facade.

## Gotchas

- `RetryPolicyCoreExt` must be imported for domain retry methods.
- Budget decisions belong to `llm/usage_cost.rs` and `BudgetStatus::classify`; do not re-derive them at call sites.
- Compaction preserves conversation; `context_reset.rs` intentionally discards it.
- Token/catalog deferral thresholds are correct behavior; warn only when deferral is disabled but beneficial.
