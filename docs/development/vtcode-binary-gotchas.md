# vtcode Binary Gotchas

This guide preserves detailed maintainer notes for the binary crate. Keep
`src/AGENTS.md` concise; use this page when changing the runloop, update path,
allocator, or request assembly.

## Startup, updates, and allocation

- Standalone updates own asset selection, checksum verification, safe archive extraction, and `self_replace`; keep managed-install guidance separate and keep inline TUI downloads quiet.
- `main_helpers` handles runtime relaunch context; do not duplicate initialization logic in `main.rs`.
- Allocator memory pinning: vtcode's bursty/sparse Tokio workload (semaphore-capped `JoinSet` fan-out followed by idle workers) can leave both `mimalloc` and `glibc` RSS flat after a burst because frees are stranded on cross-thread lists until later allocation activity. `jemalloc` returns memory while idle only when `background_thread` is active; on macOS its build reports `background_thread currently supports pthread only` and behaves like mimalloc. On Linux containers it can reclaim memory. Measure with `vtcode bench-allocator` before changing the default.
- `load_dotenv()` must run before config load so `.env` API keys are available.
- Provider noise (for example `]<]minimax[>[`) is stripped centrally in `turn::provider_noise`; stream-level harmony and MiniMax sanitization lives in `stream_sanitization::StreamSanitizer`. Do not re-implement it inline.

## Tool-budget and recovery contracts

- Wall-clock and tool-call budget exhaustion share one contract: emit the full policy message once, compact stubs for later batch calls, then one synthesis directive via `flush_budget_synthesis_directives` after the tool batch. Do not break the turn as `Blocked`, because that skips synthesis. The flush arms `switch_to_tool_free_recovery()` so the next request removes tool definitions at the API level.
- `tool_budget_exhausted_emitted` participates in the plan-mode `mark_recovery_exhausted` gates. The preflight circuit breaker in `turn/tool_outcomes/handlers/mod.rs` follows the same `Continue` plus deferred synthesis path via `preflight_circuit_recovery_pending` and `flush_preflight_circuit_recovery`; never return `Break(Blocked)` for an approved-plan build.
- Approved-plan implementation turns receive one internal `+50` loop allowance at initialization, clamped by the ordinary hard cap; `0` remains unlimited and manual extensions/tool-call budgets stay independent.
- Queue model-facing auto-permission probe warnings while a tool batch runs. Flush them after all tool results and before recovery directives; keep the UI warning immediate and deduplicated.
- During tool-free recovery, do not inject `request_user_input`; it violates the recovery contract. Plan-aware fallback must preserve interview state, while exhausted budget or recovery caps conclude with the user-facing planning notice instead of re-forcing research or emitting a final directive with no following model call.
- The interview-denied recovery fallback must distinguish "no draft produced" from "draft persisted." `PLANNING_RECOVERY_SYNTHESIS_FALLBACK_NO_INTERVIEW` promises "Review the plan below" and offers `yes`/`implement`/`no`/`edit` — those choices dead-end without a persisted plan and contradict the appended `PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT`. When `persisted_plan_ready` is false, use `PLANNING_INTERVIEW_DENIED_NO_DRAFT_NOTICE` instead (turn_902).
- Permanent `request_user_input` denial is distinct from user cancellation. Mark denial from both execution failure and permission-flow denial, suppress the tool for the rest of the session, and route text replies through the shared planning intent classifier.

## Request assembly and planning

- `turn_processing/llm_request/` uses contract-carrying `pub(super)` submodules (`snapshot`, `tool_shaping`, `context_management`, `response_chain`, `prompt_assembly`, `prompt_sections`, and `prompt_runtime`). Go through `mod.rs` exports.
- `ContextManager::normalize_history_for_request` is request-scoped: preserve the borrowed fast path for clean histories and use the core normalizer only to repair provider-facing tool-result ordering. Never mutate durable session history as part of provider retry.
- Prompt section order in `prompt_assembly::build_prompt_output` is provider-cache-sensitive. Hosted Anthropic/OpenAI payloads retain deferred tool definitions; only ClientLocal policy omits them. `metrics.rs` emits the per-request `token_budget_breakdown`; prompt-cache diagnostics remain in `SessionStats`.
- Plans stay compact/spec-like: `PLANNING_WORKFLOW_PLAN_QUALITY_LINE` requires roughly 1500 tokens with `Action -> files/symbols -> verify:` steps and file/symbol references. The truncation recovery prompt is bounded by `MAX_PLAN_SYNTHESIS_CONDENSE_ATTEMPTS`.
- Planning research scales to request complexity; the wall-clock budget is the backstop, not a replacement hard tool-count cap. `effective_max_tool_calls_for_turn` and mid-turn config reapplication preserve the shared planning floor.
- Approval phrases are classified by `detect_planning_intent` and must restore a write-capable primary agent, preserve `auto_accept`, create the tracker, and apply the approved-plan tool-call floor. Direct and queued approval paths share this boundary.
- Emit `plan.approval.requested` and `plan.approval.resolved` for every decision using the canonical `ThreadEvent` contract and preserve Open Responses parity.
- Enter/exit phrase literals live in `vtcode_core::planning` and are shared by the runloop and Codex bridge; add a phrase once, never fork consumer-specific aliases.
- Tool-summary renderers receive `ToolSummaryRenderContext` at their public render entry points; pure helpers may remain `Option<&Path>`-driven internally.
- Compact tool-summary grouping is session-local and batch-scoped; expanded mode remains the default, and failures, warnings, PTY output, diffs, and result bodies stay ungrouped.
- Completed turns publish a non-empty final assistant response through both renderer and `ThreadEvent` harness paths. Recovery fallbacks remain visible but produce a blocked outcome; approved-plan summaries retain changed files, verification, and blockers.

For prompt/runtime source boundaries, see [runtime guidance](./runtime-guidance.md).
