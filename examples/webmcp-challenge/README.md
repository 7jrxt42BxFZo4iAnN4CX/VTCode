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

From the VT Code repository root:

```sh
cd examples/webmcp-challenge
./start.sh
```

The launcher installs dependencies if needed and starts Vite at
`http://localhost:5173`. The Vite base is relative, so the generated site also
works beneath a GitHub Pages project path.

On startup, an unpaired browser opens **Settings** so you can paste the VT Code
WebSocket URL and one-time pairing code. Close it to use fallback mode.

The browser preserves the fallback project, drafts, open tabs, and selected
file across a browser refresh. A new Vite app instance clears that browser
state. Real bridge credentials are never stored, so a refreshed page must be
paired again before it can access a real workspace.

If your terminal is already in `examples/webmcp-challenge`, skip the `cd`
command.

Start a connected workflow with the same launcher:

```sh
./start.sh --headless --workspace /absolute/path/to/project
./start.sh --active --workspace /absolute/path/to/project
```

Use `--headless` for workspace-only access. Use `--active` for **VT CODE TURN**;
when the TUI is ready, run `/webmcp pair http://localhost:5173` and paste its
WebSocket URL and pairing code into the browser. Add `--port 5174` when port
`5173` is already occupied. See [GUIDE.md](GUIDE.md) for the complete workflow.

Build and test the example with:

```sh
npm test
npm run build
```

## Pair with VT Code

Pairing is a two-step handoff:

1. **VT Code TUI:** run `/webmcp pair http://localhost:5173` in the same
   interactive session that should receive browser prompts.
2. **Web app:** open **Settings → Connect or re-pair a VT Code bridge**, paste the
   WebSocket URL and one-time pairing code printed by the TUI, and select
   **Pair with VT Code**.

In the TUI, `/webmcp` shows the current bridge details and available command
arguments. Use `/webmcp help` to show only the command guide.

The TUI prints the values in this order:

```text
WebSocket: ws://127.0.0.1:<port>/webmcp
Pairing code: <one-time-code>
```

Keep the TUI running. Do not use a URL or code from an old session or from a
separate headless `webmcp serve` process when you want **VT CODE TURN**.

If the TUI says WebMCP is already listening, it prints the active WebSocket
URL, pairing code, and expiry. To disconnect that browser and issue a fresh
pairing, run `/webmcp pair --replace http://localhost:5173`, then confirm
**Disconnect and re-pair**. `/webmcp unpair` also asks for confirmation before
disconnecting without starting a replacement bridge.

The app's **Settings** dialog contains the workspace path, browser origin,
generated active-session and workspace-only setup commands, WebSocket URL, and
pairing code. Enter the absolute workspace path there and copy the setup
command you need. The browser cannot launch VT Code itself; run the copied
command in a terminal, then enter the printed URL and code under **Connect or
re-pair a VT Code bridge**.

After pairing, the **Bridge status** section mirrors the terminal-owned
workspace, runtime mode, listener, pairing lease, and request limits. The
workspace path and WebSocket URL are remembered for the current Vite app
instance, but pairing codes and session tokens are never persisted. Use the
VT Code TUI to change origins, roots, or policy, then pair again.

### Workspace-only pairing

For workspace inspection without agent turns, start the headless bridge from
the repository checkout with an explicit origin allowlist:

```sh
cd /path/to/vtcode
cargo run --locked -- webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /absolute/path/to/workspace
```

Enter the newest printed `ws://.../webmcp` address and matching one-time
pairing code in the editor's Settings dialog. Keep the bridge terminal
running. A restart can change the port and always invalidates the previous
code. The pairing code and returned session token are held only in JavaScript
memory; they are not put in query parameters, browser storage, or logs. The
connected browser sends a small authenticated heartbeat so an open session can
remain paired while it is idle. If the browser is suspended longer than the
session lease, restart the standalone bridge and enter its new URL and code. In
the active-session workflow, run `/webmcp pair ...` again in the TUI instead.
The terminal remains the write authority. A default server started without
explicit full-auto permission can inspect and stage proposals but rejects
filesystem mutation requests; checks also require `exec_command` (or `*`) in
the explicit full-auto allowlist.

If an installed `vtcode` says `unrecognized subcommand 'serve'`, update it or
run `cargo run --locked -- webmcp serve ...` from the source checkout. The
configured origin must match the page exactly: `localhost:5173` and
`127.0.0.1:5173` are different origins.

The standalone `serve` command is headless: it does not open an interactive
terminal approval prompt or execute agent turns. For the complete active-session
workflow, see the [demo user guide](../../docs/user-guide/webmcp-demo.md).

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

After **Stage for VT Code turn**, the active-session editor opens the **VT CODE**
panel and focuses the prompt. Press `Cmd/Ctrl+Enter` in the prompt to request
the turn without clicking the button.

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

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| `Cmd/Ctrl+Shift+P` | Open the quick-action command palette. |
| `Cmd/Ctrl+,` | Open Settings. |
| `?` | Open or close the help popup when you are not typing. |
| `Cmd/Ctrl+K` | Focus the explorer file filter. |
| `Cmd/Ctrl+S` | Review the current draft instead of writing immediately. |
| `Cmd/Ctrl+Enter` | Request a VT Code turn from the prompt composer. |
| `Esc` | Close the active popup. |

The **Actions** and **?** buttons in the header provide the same entry points.
The command palette filters actions as you type and disables actions that are
not available for the current editor state or backend.

## WebMCP browser API

When the browser exposes `document.modelContext.registerTool`, the page
registers seven standard WebMCP tools: bounded file listing, current-file
reading, code search, editor-state inspection, file opening, draft review, and
panel navigation. The last three change only the browser view or draft-review
state; they do not approve, apply, or revert filesystem changes. Each tool has
a display title, JSON Schema input, read-only/untrusted-content annotations,
and an abort-aware callback. The registration listens for `toolchange` and can
be unregistered with its `AbortSignal`.

This is the browser-agent direction described by WebMCP: a browser-integrated
or in-page agent discovers tools registered by the page and invokes them in the
page's execution context. The normal UI remains usable when that API is
unavailable. Browser WebMCP tools do not bypass the VT Code WebSocket pairing
or terminal approval boundary.

The VT Code connection is a separate authenticated bridge. The browser sends
workspace, patch, check, and turn requests over that WebSocket; VT Code sends
responses and canonical runtime events back. WebMCP does not define that
remote VT Code protocol, so this demo does not claim that the WebSocket itself
is the WebMCP API.

## GitHub Pages

The repository workflow at `../../.github/workflows/webmcp-demo.yml` runs
`npm ci`, builds the Vite app, and publishes `dist/` when the demo branch is
pushed or the workflow is started manually. Configure Pages to use **GitHub
Actions**.

The example is covered by the repository's [MIT OR Apache-2.0 license](../../LICENSE).
