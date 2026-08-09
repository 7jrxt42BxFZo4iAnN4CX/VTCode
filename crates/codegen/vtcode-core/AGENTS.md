# vtcode-core
[Root AGENTS.md](../AGENTS.md) | Agent loop, tools, LLM, safety, UI.
## Module map
- `core/agent/`: runtime, event recording, runner, progress, harness artifacts; `request_envelope` freezes each segment's prompt and ordered catalog.
- `tools/`: handlers, registry, policy, execution; `read_file/batch.rs` owns batch admission while `read_file.rs` owns single-file semantics; `registry/executors/` is split into `exec_command.rs`, `exec_output.rs`, `exec_sessions.rs`, `exec_support.rs`; `grep_file.rs` delegates to `grep_backend.rs`.
- `llm/`: re-export facade; providers live in `vtcode-llm`. `exec/events.rs` re-exports canonical `vtcode-exec-events::ThreadEvent`; `prompts/` owns cached static profiles, compiled runtime guidance, and bounded resource caches.
- `mcp/plugin_providers.rs`: discovers Agent Plugin MCP servers and maps them to `McpProviderConfig`. `./`-prefixed stdio `command`/`cwd` are canonicalized at discovery so symlink escapes cannot bypass the plugin-root sandbox.
- `skills/loader.rs`: loads `SKILL.md` manifests from plugin `skills/*/` directories into the session skill catalog.
## Rules
- Re-export public APIs from `lib.rs`; consumers must not reach into submodules, and keep constants in `config::constants` rather than inline.
- `exec_policy` and `command_safety` are separate policy layers.
- Session event sinks are authoritative: preserve ordering, bounded backpressure, and await the drain handle before reporting task success. Recovery/final assistant messages must use the canonical `ThreadEvent` path rather than history-only writes.
- Trajectory logging is best effort: retain line/byte bounds and expose drops/failures through diagnostics.
## Workflows
- Add tools under `tools/`, register/classify them, wire them into `core/agent/`; approved-plan task trackers deduplicate normalized step descriptions while preserving first-seen order, approval must fail closed if the tracker cannot be persisted, and plan artifacts must validate concrete targets and verification markers before writing. Add providers with `adding-llm-providers` and update model enumeration, presets, and the `llm/providers` facade. NVIDIA-style first-class providers also need startup defaults, the secret enum, lightweight routing, and generated docs metadata.
- Custom provider registration stays in the factory and delegates protocol selection to the profile-aware router; do not add arbitrary custom names to the finite `Provider` enum.

## Gotchas

- Import `RetryPolicyCoreExt` for domain retry methods; budget decisions belong to `llm/usage_cost.rs` and `BudgetStatus::classify`, not call sites.
- Plugin MCP `command`/`cwd` starting with `./` are canonicalized and containment-checked by `vtcode_agent_plugins::validate_plugin_relative` before spawn. Never re-resolve them relative to the working directory at spawn time — that defeats the containment check.
- Compaction preserves conversation and starts a new immutable request segment before replacing history; `context_reset.rs` intentionally discards it. Keep boundary reasons on the shared transition path.
- `AgentSessionState.messages` is `Arc<Vec<Message>>`; use `messages_mut()`/`Arc::make_mut` for mutations so request and continuation histories stay shared without deep copies.
- Async plugin, skill, file-tool, planning, persistent-memory, durable-scheduler, prompt-resource cache-miss, context-reset manifest, and trajectory setup paths must use Tokio filesystem APIs or `spawn_blocking` for recursive/synchronous scans, record loading, claim files, persistence, or Git work; registry workspace settings, including persistent-memory config, are startup snapshots.
- Evaluator `generalization_notes` are bounded, validated, task-scoped evidence; replans must preserve scopes and add falsifiers to the tracker without promoting notes to global memory. `SessionProgressSink` coalesces snapshots through a bounded writer, so progress mutations must not perform synchronous filesystem I/O.
- `SessionToolCatalog` projection caches are private and lazy per entry/documentation mode; rebuild the catalog when registrations change. Basic directory-list caches are keyed by canonical workspace and response shape.
- `TerminalAppLauncher::launch_editor_target_non_waiting` is for existing-file GUI opens; preserve adapter-specific reuse/location flags and keep temporary-file `/edit` flows on the waiting API.
- Token/catalog deferral thresholds are correct behavior; warn only when deferral is disabled but beneficial. Shell safety must preserve raw command text when classifying dynamic syntax; keep optional SQLite on rusqlite 0.39 until stable cfg_select support is available.
- `web_fetch` accepts remote HTTP(S) URLs only; local reads must use `read_file`/`unified_file`. `AnsiRenderer` owns the session-local tool-summary display mode; compact batches flush before non-success summaries and never change the `ThreadEvent` contract. Active names stay read-only; `write_stdin`/`apply_patch` remain blocked while internal plan persistence is available. Pipe/PTy and MCP stdio children inherit filtered environment policy; use the sandbox-aware launch APIs rather than bypassing them.
