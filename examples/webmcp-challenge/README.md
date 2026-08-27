# VT Code WebMCP editor

This Vite example is a real CodeMirror 6 browser editor for a VT Code
workspace. It supports editable drafts, syntax highlighting, tabs, line
numbers, dirty state, unified diff review, checks, prompt requests, and
backend events.

For the click-by-click setup and fallback/connected workflow, see the
[local demo guide](GUIDE.md) or the [repository WebMCP demo user
guide](../../docs/user-guide/webmcp-demo.md).

The browser never writes to the local filesystem. With no bridge, the page
starts in a deterministic `InMemoryBackend`; edits, proposals, checks, and
reverts are confined to page memory. When paired with `vtcode webmcp serve`,
the same UI uses an authenticated WebSocket backend. VT Code verifies base
digests, owns the authoritative diff, applies terminal or full-auto policy,
and validates current files before reverts.

The interface is intentionally IDE-shaped. The explorer keeps directories
collapsed, lists file metadata without loading every file, and reads content
only when a file is opened. The bottom panel separates `TERMINAL`, `CHANGES`,
and `VT CODE` output so large files and long command output stay inside their
own scroll areas.

## Run locally

From this directory:

```sh
npm ci
npm run dev -- --host localhost --port 5173 --strictPort
```

Open the URL printed by Vite. The Vite base is relative, so the generated site
also works beneath a GitHub Pages project path.

Build and test the example with:

```sh
npm test
npm run build
```

## Pair with VT Code

Start the bridge from the repository checkout with an explicit origin
allowlist. Using the checkout avoids accidentally invoking an older globally
installed `vtcode` binary:

```sh
cd /path/to/vtcode
cargo run --locked -- webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /absolute/path/to/workspace
```

Enter the newest printed `ws://.../webmcp` address and matching one-time
pairing code in the editor's connection panel. Keep the bridge terminal
running. A restart can change the port and always invalidates the previous
code. The pairing code and returned session token are held only in JavaScript
memory; they are not put in query parameters, browser storage, or logs. The
terminal remains the write authority. A default server started without
explicit full-auto permission can inspect and stage proposals but rejects
filesystem mutation requests; checks also require `exec_command` (or `*`) in
the explicit full-auto allowlist.

If an installed `vtcode` says `unrecognized subcommand 'serve'`, update it or
run `cargo run --locked -- webmcp serve ...` from the source checkout. The
configured origin must match the page exactly: `localhost:5173` and
`127.0.0.1:5173` are different origins.

The standalone `serve` command is headless: it does not open an interactive
terminal approval prompt or execute agent turns. For a real **VT CODE TURN**,
start `vtcode chat` in the target workspace and enter
`/webmcp pair http://localhost:5173` in that same TUI session. See the user
guide for the complete active-session workflow.

## Editor workflow

1. Filter the workspace with **Filter files**, expand a directory, and open a
   file. Only the selected file is loaded into the editor.
2. Edit the file in its draft buffer. Open another file from the explorer or
   tabs without losing the draft.
3. Use `Cmd/Ctrl+S` or **Review changes** to create a client-side unified diff.
4. Select **CHANGES** in the bottom panel and review the diff. **Approve
   patch** sends structured changes with their `sha256:` base digests. In
   fallback mode this is an in-memory approval; in connected mode it requests
   VT Code/terminal approval.
5. Apply only after the backend accepts the proposal. Stale files and
   external changes fail closed.
6. Select **TERMINAL** to inspect activity and checks, or **VT CODE** to
   compose an agent turn.

The prompt composer attaches the reviewed diff to a VT Code turn request. The
static fallback and the standalone headless bridge do not execute an agent
turn: the editor reports that limitation in the response panel. The active
TUI bridge started with `/webmcp pair <origin>` is the runtime path for a real
model response.

Reload compares a clean draft with the latest backend snapshot. A dirty buffer
is marked as an external-change conflict instead of being silently overwritten.
File paths, content sizes, proposal counts, commands, and WebSocket frames are
bounded by the backend. The file filter narrows the explorer by name or path;
it is not a full-content search.

## WebMCP browser API

When the browser exposes `document.modelContext.registerTool`, the page
registers bounded read-only inspection tools. The normal UI remains usable when
that API is unavailable. Browser WebMCP tools do not bypass the VT Code
WebSocket pairing or terminal approval boundary.

## GitHub Pages

The repository workflow at `../../.github/workflows/webmcp-demo.yml` runs
`npm ci`, builds the Vite app, and publishes `dist/` when the demo branch is
pushed or the workflow is started manually. Configure Pages to use **GitHub
Actions**.

The example is covered by the repository's [MIT OR Apache-2.0 license](../../LICENSE).
