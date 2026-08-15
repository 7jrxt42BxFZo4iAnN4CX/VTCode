# Tools Configuration

This document describes the tools-related configuration in `vtcode.toml`.

- max_tool_loops: Maximum number of inner tool-call loops per user turn. The default is `40`; set to `0` to disable the limit and rely on the other turn safeguards. Planning raises smaller nonzero values to its planning floor, while prompt-approved extensions remain bounded by hard caps.
  - Configuration: `[tools].max_tool_loops` in `vtcode.toml`
  - Code default: `vtcode_config::constants::tool_limits::DEFAULT_MAX_TOOL_LOOPS`, consumed by `crates/codegen/vtcode-config/src/core/tools.rs`
  - Default: `40`

Example:

```toml
[tools]
default_policy = "prompt"
max_tool_loops = 40
```

## Blocked-call limits and blocked handoffs

`max_consecutive_blocked_tool_calls_per_turn` controls the consecutive blocked-call cap. The runtime also applies a bounded total fuse: normal mode allows two times the configured cap, Plan Mode allows four times the cap, and recovery mode retains the tighter configured cap. These limits apply consistently to policy/preflight denials and blocked execution failures; the consecutive streak resets after an allowed call, while the total count remains per turn.

When the fuse stops a turn, VT Code forces a session-history checkpoint before creating the blocked handoff. A persisted archive produces a verified `vtcode --resume <archive-id>` command. Disabled history or a failed checkpoint produces a handoff without a resume command and states why resume is unavailable. Interactive sessions remain available for the next user input.

Runner paths without session-archive support likewise omit the resume command instead of treating a runtime session ID as an archive identifier.


Tool outputs are rendered with ANSI styles in the chat interface. Tools should return plain text.
