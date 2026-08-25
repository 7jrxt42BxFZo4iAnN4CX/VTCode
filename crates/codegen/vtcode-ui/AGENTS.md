# vtcode-ui

[Root AGENTS.md](../AGENTS.md) | Unified UI: design system, theme registry, TUI framework. Consolidated from `vtcode-design` and `vtcode-theme`.

## Modules

| Area | Path |
|---|---|
| Design system | `design/` — color conversion, style bridging, layout, diff, panel primitives |
| Theme registry | `theme/` — ThemeStyles, runtime state, syntax theme resolution |
| TUI framework | `tui/` — session, widgets, runner, markdown rendering, config |

## Rules

- `design` and `theme` are re-exported at crate root (`pub use design::*; pub use theme::*`) for backward compatibility with the old standalone crates.
- `publish = false` — internal crate, not published to crates.io.
- `tui/core_tui/` owns the full terminal session lifecycle; `tui/core_tui/app/session/task_panel.rs` owns compact TODO-panel wrapping/height/header helpers; `tui/ui/` has reusable widgets (markdown, interactive list).
- `tui/config/constants/` holds TUI-specific defaults — keep them here, not in `vtcode-config`; snapshot tests live in `tui/core_tui/widgets/snapshots/`.

## Gotchas

- `vtcode-commons` provides `anstyle_utils` gated behind a `tui` feature — the style bridging in `design/` depends on it.
- The `crossterm` dependency enables `event-stream` and `osc52` features; do not duplicate these in downstream crates.
- Standalone and core session defaults are inline; callers that need alternate-screen rendering must opt in explicitly.
- Floating approval/list overlays own mouse input only inside `modal_list_area`; wheel events outside that hitbox must pass through to the transcript so long plan markdown remains scrollable.
- Floating overlays reuse one bottom-half rectangle for popup rendering and transcript clipping; keep `transcript_area` as the source of truth for scroll metrics and hit-testing.
- `ActivityState` is the authoritative global busy/idle signal for fresh-thread handoffs; use it for input and mode-switch guards even when animation is disabled.
- PTY/tool reflow must preserve explicit status color on the `•` prefix; apply action/tool styling only to the verb so success, failure, and warning remain visually distinct.
- Tool and PTY blocks reserve at least one blank line above and below; shell syntax highlighting is accepted only when it produces distinct token colors, otherwise semantic token styles are the fallback.
- Task-panel tree rows use the shared hanging-prefix wrapper in `session/text_utils.rs`; keep panel row heights derived from wrapped content so transcript and docked panel stay aligned.
- `toggle_tool_display_mode` is a rebindable session action (default `Alt+T`); dispatch it before the legacy `Alt+T` text-edit shortcut and invalidate transcript caches after toggling.
