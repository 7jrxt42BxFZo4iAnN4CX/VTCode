# vtcode (binary)

[Root AGENTS.md](../AGENTS.md) | CLI entrypoint, session bootstrap, and agent runloop wiring.

Detailed runloop recovery and allocator notes live in the [vtcode binary gotchas guide](../docs/development/vtcode-binary-gotchas.md).

## Modules

`main.rs` binary entry | `agent/` runloop + subagent dispatch | `cli/` handlers | `startup/` onboarding | `updater/` downloads and self-replacement | `codex_app_server/` bridge | `main_helpers/` tracing and runtime init

## Rules

- Keep the binary thin; runtime logic belongs in `vtcode-core`.
- `mimalloc` is the default allocator; `allocator-jemalloc` opts into `tikv-jemalloc`. Measure with `vtcode bench-allocator` before changing it; see the [allocator guide](../docs/development/ALLOCATOR_MEMORY.md).
- Install `vtcode_ui::tui::panic_hook` before producing output.
- `agent/runloop/` is the single-agent loop; `unified/turn/session_loop_runner/mod.rs` is its facade and `orchestration.rs` owns the loop body.
- Keep transcript/modal editor opens on the bounded runtime coordinator, not queued `/edit` submissions.
- Keep `/secret` provider/key validation and storage selection behind `turn/session/slash_commands/secrets/storage.rs` and the central resolver.
- Route batched tool metrics through the shared execution helper so every executed status is recorded.
- Keep PTY status handoff during stream shutdown separate from output rendering.
- `agent/runloop/unified/turn/compaction/` delegates to `vtcode-core::compaction`; reserve segment boundaries with the shared transition helper.
- Updates own asset selection, checksum verification, safe extraction, and `self_replace`; `main_helpers` owns relaunch context and runtime initialization.
- Centralize provider-noise sanitization in `turn::provider_noise` and `stream_sanitization::StreamSanitizer`.
- Preserve prompt-section ordering and wire-tool shaping invariants; see the detailed guide before changing request assembly.
- Preserve planning recovery, approval, interview, and budget-synthesis invariants; use the shared `ThreadEvent` contract for telemetry.
- Completed turns must publish a non-empty final response through both renderer and harness paths; blocked recovery remains visible.
## Gotchas

The detailed maintainer notes are in [vtcode-binary-gotchas.md](../docs/development/vtcode-binary-gotchas.md); keep this file focused and under 30 lines.
