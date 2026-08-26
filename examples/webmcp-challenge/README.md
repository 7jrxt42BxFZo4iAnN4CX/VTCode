# VT Code WebMCP Challenge demo

This is a dependency-free static browser demo of a human-approved coding workflow. It is self-contained: the sample project and all state live in page memory. No arbitrary code, shell commands, network requests, filesystem access, or user-supplied JavaScript are executed.

The example is covered by the repository's [MIT OR Apache-2.0 license](../../LICENSE).

## Run locally

From this directory, use any static server. For example:

```sh
python3 -m http.server 8000
```

Open `http://localhost:8000/`. If you already have `npx serve` installed, `npx serve .` works too; this project does not install or require it.

## WebMCP behavior

When `document.modelContext.registerTool` is available in a secure context, the page registers seven narrow tools. Browsers without WebMCP show a clear “WebMCP unavailable” status; every tool remains exercisable through the normal UI. No `exposedTo` origins are configured, so the tools retain the API's same-origin default.

| Tool | Input | Behavior |
| --- | --- | --- |
| `list_project_files` | none | Returns the three in-memory paths; read-only. |
| `search_code` | `{ query: string }` | Returns matching file/line records; read-only. |
| `read_file` | `{ path: string }` | Returns one known file; read-only. |
| `propose_patch` | none | Stages a fixed greeting patch; does not mutate files. |
| `run_checks` | none | Runs two deterministic content assertions in-browser; read-only. |
| `apply_approved_patch` | none | Applies only the staged patch. The UI approval dialog is required first. |
| `revert_last_change` | none | Restores the previous content only after a confirmation dialog. |

Tool handlers reject unknown paths, reject unexpected input keys, cap user-controlled string lengths, and do not accept executable content. `read_file` and `search_code` mark returned source as untrusted; read-only tools use `readOnlyHint`. The mutation handlers require short-lived UI confirmation tokens, so direct agent calls cannot bypass the approval dialogs. The sample project is intentionally bounded to three files. There is no persistence, network, filesystem, or command execution.

## Three-minute demo script

1. Open the page and point out the browser support status, sandbox note, file tree, read-only viewer, and live audit log.
2. Search for `greeting`, open the matching source result, and explain that inspection tools cannot mutate state.
3. Click **Propose greeting update**. Review the staged diff and the “Awaiting approval” state.
4. Click **Approve & apply**, confirm the keyboard-friendly dialog, then run local checks.
5. Click **Revert last change**, confirm, and show the original source restored.
6. Click **Run self-check** to demonstrate the complete proposal → approval → apply → verify → revert path. In a WebMCP-capable browser, explain that the same bounded handlers are registered as tools.
