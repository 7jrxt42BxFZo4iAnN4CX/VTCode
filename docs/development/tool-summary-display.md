# Tool Summary Display

VT Code has two independent tool-display settings:

- `ui.tool_output_mode` controls how command and tool result bodies are truncated or spooled.
- `ui.tool_display_mode` controls the transition summaries shown before those bodies. It defaults to `expanded`; `compact` groups adjacent successful summaries only when their tool, semantic action, and stable arguments match.

```toml
[ui]
tool_display_mode = "compact"
```

Compact groups retain shared parameters and list pagination-only differences such as `max_results`, `limit`, `offset`, `page`, and `cursor`. A group is rendered only for two or more compatible calls and uses a bounded `+N more` suffix for long distinct-value lists. Failures, warnings, cancellations, PTY blocks, diffs, and result bodies remain individually visible.

The runtime mode can be changed for the current session with `Alt+T`. This action is rebindable through the existing keybinding configuration. Use `/config` to cycle `ui.tool_display_mode` and persist the choice to `vtcode.toml`.

Expanded mode is the compatibility default and preserves the existing per-call summary layout.
