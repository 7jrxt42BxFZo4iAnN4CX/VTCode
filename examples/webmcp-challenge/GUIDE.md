# WebMCP Demo Guide

This directory contains the VT Code WebMCP browser editor.
The demo starts in a safe fallback mode.
It never writes directly to the local filesystem.

## Do I need to launch VT Code?

It depends on what you want to try.

- Fallback editing needs only the Vite development server.
- Real workspace inspection needs the standalone `webmcp serve` command.
- A real **VT CODE TURN** needs an interactive VT Code session running in the
  workspace.

To inspect a real workspace, launch the WebMCP bridge.
Give it an explicit origin and workspace root:

```sh
# Run this from the VT Code repository checkout:
cd /path/to/vtcode
cargo run --locked -- webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /absolute/path/to/workspace
```

This standalone bridge provides workspace operations only.
It does not run an LLM turn or connect to an interactive VT Code session.
For real agent turns, use the active-session workflow below instead of this
command.

## Prerequisites

- Node.js and npm for the browser demo.
- VT Code installed, or a VT Code source checkout, for connected workspace mode.

## Fallback mode

Fallback mode is browser-only.
Edits, proposals, checks, and reverts affect only the page's in-memory project.

### Start the demo

From this directory, run:

```sh
npm ci
npm run dev -- --host localhost --port 5173 --strictPort
```

Open the local URL printed by Vite.
It is normally [`http://localhost:5173`](http://localhost:5173).

### Edit and apply a change

1. Use **Filter files** to narrow the explorer, expand a directory, and open a
   file. Only the selected file is loaded into CodeMirror.
2. Edit the file. The explorer and tab show `*` while its browser draft is
   different from the backend snapshot.
3. Select **Review changes**, or press `Cmd/Ctrl+S`.
4. Inspect the unified diff in the bottom **CHANGES** panel.
5. Select **Approve patch**.
6. Select **Apply approved patch**.
7. Select **Run checks** in the bottom **TERMINAL** panel to run the
   deterministic browser checks.
8. Select **Revert last change** to restore the original content.

The editor, explorer, and terminal panel have independent scroll areas. Long
files, paths, diffs, and check output do not expand the whole page. Open files
are kept in tabs, while unopened files remain metadata-only until selected.

Refreshing the page removes all fallback changes.
**Pair editor first** opens the connection panel.
Fallback mode is not an LLM runtime and cannot execute agent turns.

## Connected workspace mode (headless)

Use two terminals.
The browser connects to the bridge over a local WebSocket.

### 1. Start the browser editor

From this directory, run:

```sh
npm ci
npm run dev -- --host localhost --port 5173 --strictPort
```

Keep the terminal running.
Open `http://localhost:5173` in your browser.

### 2. Start the WebMCP bridge

From the VT Code repository, run:

```sh
cd /path/to/vtcode
cargo run --locked -- webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /absolute/path/to/workspace
```

After `cargo build --locked`, the equivalent repository binary command is:

```sh
./target/debug/vtcode webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /absolute/path/to/workspace
```

An installed `vtcode` must be a build that includes the `webmcp` command.
If it reports `unrecognized subcommand 'serve'`, use one of the repository
commands above or update the installed binary.
Replace `/absolute/path/to/workspace` with the workspace root for the browser.
Avoid `--allowed-root .` unless you intentionally want the bridge terminal's
current directory as the workspace root.
The bridge prints a WebSocket address and a one-time pairing code.

### 3. Pair the browser

1. Expand **Connect this editor to a local VT Code WebMCP bridge**.
2. Paste the printed `ws://.../webmcp` address into **WebSocket URL**.
3. Paste the pairing code into **One-time pairing code**.
4. Select **Pair with VT Code**.

Use the URL and code printed by the same bridge process. Every bridge restart
can choose a new port and always creates a new one-time code. Keep that
terminal running; a URL or code from a process stopped with `Ctrl+C` cannot be
used again.
The workspace tree now reads from the selected root.
The pairing code and session token remain in browser memory only.

The configured origin must match exactly.
`http://localhost:5173` and `http://127.0.0.1:5173` are different origins.

## Active VT Code session mode (real agent turns)

Use this mode when the **VT CODE TURN** button should submit a prompt to VT
Code.
The interactive TUI and the browser bridge must belong to the same VT Code
process.
Starting `webmcp serve` in one terminal and `vtcode chat` in another does not
attach them together.

### 1. Start the browser editor

Run the browser steps from the headless workflow above and keep Vite running.
Use the exact browser origin, normally `http://localhost:5173`. The demo
uses a strict port so Vite fails if `5173` is already occupied instead of
silently switching to a different origin.

### 2. Start interactive VT Code in the target workspace

If the target workspace is the VT Code checkout, run this from its root:

```sh
cd /path/to/vtcode
cargo run --locked -- chat
```

For another workspace, pass it explicitly:

```sh
cd /path/to/vtcode
cargo run --locked -- --workspace /absolute/path/to/workspace chat
```

An installed binary can use the same `chat` command.
The session needs the normal provider authentication required by VT Code.

### 3. Start the bridge from the TUI

In the interactive VT Code prompt, enter:

```text
/webmcp pair http://localhost:5173
```

VT Code prints an active-session WebSocket URL and one-time pairing code.
Paste those values into the browser connection panel.
Do not use the URL or code from a separate `vtcode webmcp serve` process.

### 4. Send the browser draft to VT Code

1. Edit a file; a `*` marker indicates an unsent browser draft.
2. Select **Review changes** to create the unified diff.
3. Select **Stage for VT Code turn** to validate the proposal in the bridge.
4. Enter an instruction in the prompt composer.
5. Select **Request VT Code turn**.

The prompt and reviewed diff are inserted into the active TUI as a normal
agent turn.
Approve any file or command tool requests in the VT Code terminal.
The browser never authorizes or writes the file itself.

The active bridge intentionally does not expose direct browser apply, check,
or revert operations.
Ask VT Code to perform those actions through its normal tools and then use
**Reload** in the browser to inspect the resulting workspace.

## Connected edit workflow

1. Edit a file; a `*` marker indicates an unsent browser draft.
2. Select **Review changes** to create a client-side unified diff.
3. In headless mode, select **Stage proposal** to send the structured
   proposal with base file digests.
4. In active-session mode, select **Stage for VT Code turn**, then use the
   prompt composer to ask VT Code to apply the change.
5. Use **Reload** after VT Code finishes to inspect clean files.

The default standalone bridge is headless.
It supports workspace inspection and proposal staging.
It does not open an interactive terminal approval prompt.
Filesystem apply and checks require explicit full-auto permissions.
Browser confirmation is not authorization.

The current standalone bridge also does not execute agent turns.
The **VT CODE TURN** response panel reports that turns are unavailable.
It does not imply that an LLM reviewed the diff.
A real model response requires the active-session workflow above.

## Useful controls

- **Reload** refreshes a clean file from the current backend.
- **Discard draft** removes the selected unsent draft.
- **Filter files** narrows the tree by file name or path; it does not load the
  entire project to search file contents.
- The bottom **TERMINAL**, **CHANGES**, and **VT CODE** tabs keep activity,
  diffs, prompts, and responses in one compact workspace panel.
- **Run self-check** validates the fallback edit, apply, check, and revert flow.
- External changes conflict with dirty drafts; discard before reloading.

## Build and test

From this directory, run:

```sh
npm test
npm run build
```

The production build uses a relative base path.
The `dist/` directory can be hosted below a GitHub Pages project path.

## Security notes

The browser sends proposals, not filesystem writes.
The bridge validates the origin and pairing code.
It also validates relative paths, file digests, size limits, and backend policy.

Do not expose the bridge directly to the public internet.
Do not reuse a pairing code.

## Troubleshooting

### WebSocket connection failed

Make sure the bridge terminal still shows `WebMCP listening locally` and has
not returned to a shell prompt. Copy the newest WebSocket URL and pairing code
from that terminal. A stopped bridge, an old port, or a code from an earlier
restart produces this error.

Open the Vite URL using the same origin configured for the bridge. For this
guide, use `http://localhost:5173`, then configure
`--origin http://localhost:5173`. Do not substitute
`http://127.0.0.1:5173` unless you also change `--origin` and restart the
bridge.

If `vtcode webmcp serve` is rejected as an unknown command, the installed
`vtcode` is older than the source checkout. Run `cargo run --locked -- webmcp
serve ...` from the repository root or use `./target/debug/vtcode` after
building it.

If the shell prompt already ends in `examples/webmcp-challenge`, do not run
`cd examples/webmcp-challenge` again. Run the npm commands from that directory.
