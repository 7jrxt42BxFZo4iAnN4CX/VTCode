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


Tool outputs are rendered with ANSI styles in the chat interface. Tools should return plain text.
