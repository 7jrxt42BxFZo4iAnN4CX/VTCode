# vtcode-safety

[Root AGENTS.md](../AGENTS.md) | Command safety detection, execution policies, and sandboxing. Layer 1 crate — depends on vtcode-commons.

## Module Groups

| Area | Modules |
|---|---|
| Command Safety | `command_safety/` — dangerous command detection, shell parsing |
| Execution Policy | `exec_policy/` — policy management, approval workflows, command validation |
| Sandboxing | `sandboxing/` — sandbox policy, permissions, execution environments |

## Rules

- `exec_policy::manager` imports `command_safety::command_might_be_dangerous` and `sandboxing::SandboxPolicy` — these form a tightly coupled safety subsystem.
- Re-export facades in vtcode-core (`command_safety/mod.rs`, `exec_policy/mod.rs`, `sandboxing/mod.rs`) must stay in sync.
- The `BashParser` singleton (`once_cell::Lazy`) is safe across crates — read-after-init pattern.

## Gotchas

- `exec_policy/parser.rs` imports `vtcode_commons::fs::{parse_json_with_context, read_file_with_context}`.
- `exec_policy/command_validation.rs` imports `vtcode_commons::paths::{canonicalize_workspace, normalize_path}`; its workspace containment delegates to `vtcode_commons::paths::ensure_path_within_workspace_resolved` (symlink-aware walk lives in commons, tests included).
- `sandboxing/` uses tree-sitter for Bash AST analysis — pinned to specific versions.
- `command_safety::shell_parser` must extract nested simple commands from loops/conditionals so safety checks and approval caching see loop bodies, not just top-level shell syntax.
- `command_safety::shell_parser` owns dynamic-shell-syntax detection; `find` expansion must fail closed before preflight or learned approval.
- Sandboxed pipe/PTy and MCP stdio launches filter sensitive environment names after overrides; macOS hostname allowlists reject unenforceable policies, and unsupported Windows restrictions fail closed.
- Keep `exec_policy_command_validation` fuzzing and traversal/symlink regression cases aligned with changes to workspace containment or command validation.
