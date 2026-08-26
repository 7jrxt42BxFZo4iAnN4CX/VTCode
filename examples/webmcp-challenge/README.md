# VT Code WebMCP Challenge demo

This is a dependency-free static browser demo of a human-approved coding workflow. It is self-contained: the sample project and all state live in page memory. No arbitrary code, shell commands, network requests, filesystem access, or user-supplied JavaScript are executed.

The example is covered by the repository's [MIT OR Apache-2.0 license](../../LICENSE).

## Run locally

From this directory, use any static server. For example:

```sh
python3 -m http.server 8000
```

Open `http://localhost:8000/`. If you already have `npx serve` installed, `npx serve .` works too; this project does not install or require it.

## Deploy to GitHub Pages

The repository workflow at `../../.github/workflows/webmcp-demo.yml` publishes this directory when `feat/webmcp-challenge-demo` is pushed, or when the workflow is run manually. In the repository settings, set **Pages → Build and deployment → Source** to **GitHub Actions** if Pages is not enabled yet.

```sh
git push --set-upstream origin feat/webmcp-challenge-demo
```

For this repository, the expected project URL is `https://vinhnx.github.io/VTCode/` after the first successful deployment. Verify the deployed URL in the workflow's `github-pages` environment before adding it to Devpost.

## WebMCP behavior

When `document.modelContext.registerTool` is available in a secure context, the page dynamically registers a safe baseline and exposes mutation tools only for the lifecycle step where they are usable. Browsers without WebMCP show a clear “WebMCP unavailable” status; every action remains exercisable through the normal UI. No `exposedTo` origins are configured, so the tools retain the API's same-origin default.

| Tool | Input | Behavior |
| --- | --- | --- |
| `list_project_files` | none | Returns the three in-memory paths; read-only. |
| `search_code` | `{ query: string }` | Returns matching file/line records; read-only. |
| `read_file` | `{ path: string }` | Returns one known file; read-only. |
| `propose_patch` | none | Stages a fixed greeting patch; does not mutate files. |
| `run_checks` | none | Runs two deterministic content assertions in-browser; read-only. |
| `apply_approved_patch` | none | Registered only after explicit UI approval; applies only the still-staged patch when approval is present, consumes approval, then is unregistered. |
| `revert_last_change` | none | Registered after apply; restores the previous content only after a confirmation dialog, then is unregistered. |

Tool handlers reject unknown paths, reject unexpected input keys, cap user-controlled string lengths, and do not accept executable content. `read_file` and `search_code` mark returned source as untrusted; read-only tools use `readOnlyHint`. Registration uses abort signals to unregister tools as their lifecycle window closes, and partial registrations are cleaned up on failure. Approval is a separate UI action: it records permission while leaving the patch staged, then enables and registers `apply_approved_patch`. Applying consumes that approval; direct agent calls remain fail-closed without it. The sample project is intentionally bounded to three files. There is no persistence, network, filesystem, or command execution.

## Three-minute demo script

1. Open the page and point out the browser support status, sandbox note, file tree, read-only viewer, and live audit log.
2. Search for `greeting`, open the matching source result, and explain that inspection tools cannot mutate state.
3. Click **Stage greeting update**. Review the staged diff and the “Awaiting approval” state.
4. Click **Approve patch** and confirm. The patch remains staged, and **Apply approved patch** is enabled.
5. Click **Apply approved patch**, confirm the keyboard-friendly dialog, then open `src/greeting.js` to see the highlighted modified line and the “Modified in memory” editor state before running local checks. An agent can invoke the dynamically registered apply tool at this point as well.
6. Click **Revert last change**, confirm, and show the original source restored.
7. Click **Run self-check** to demonstrate the complete proposal → approval → apply → verify → revert path. In a WebMCP-capable browser, explain that the same bounded handlers are registered as tools.
