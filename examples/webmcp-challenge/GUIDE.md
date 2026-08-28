# VT Code WebMCP Demo

This is a browser code editor with a safe in-memory fallback. In a real VT Code
session, the browser sends reviewed proposals to VT Code. VT Code performs the
terminal-approved file changes. The browser never writes to the filesystem
directly.

## Start the demo

Run these commands from the VT Code repository root:

```sh
cd examples/webmcp-challenge
./start.sh
```

Open <http://localhost:5173>. This starts fallback mode; no VT Code process is
required. Because no bridge is paired yet, **Settings** opens automatically.
Close it to use the fallback editor, or pair a VT Code bridge there.
Press `?` after closing Settings for the short in-app guide and shortcuts.

For a real workspace and agent turns, use the recommended active mode:

```sh
cd examples/webmcp-challenge
./start.sh --active --workspace /absolute/path/to/project
```

The launcher starts Vite and an interactive VT Code session. In the VT Code TUI,
run:

```text
/webmcp pair http://localhost:5173
```

Open **Settings** in the browser and paste the printed WebSocket URL and pairing
code. Use the values from this same TUI session. Do not start a separate
`webmcp serve` process for active agent turns.

For workspace browsing without agent turns, use:

```sh
./start.sh --headless --workspace /absolute/path/to/project
```

Paste the URL and code printed by the bridge into **Settings**. Headless mode is
useful for reading and proposing changes; terminal approval and VT CODE TURN
require active mode.

Use `--port 5174` if port `5173` is already in use. Use the same port in the
browser URL and `/webmcp pair` command.

## Edit and apply a change

1. Open a file in **EXPLORER** and edit it. A `*` marks a dirty draft.
2. Select **Review changes** or press `Cmd/Ctrl+S`.
3. In **CHANGES**, inspect the browser preview and stage it. Active mode sends
   the proposal ID; VT Code rechecks the files and generates the authoritative
   diff for the turn.
4. In **VT CODE**, enter a prompt and select **Request VT Code turn**, or press
   `Cmd/Ctrl+Enter` in the prompt.
5. Approve requests in the VT Code terminal.
6. Select **Reload** after VT Code finishes.

In fallback mode, use **Approve patch**, **Apply approved patch**, **Run checks**,
and **Revert last change**. Fallback changes remain in memory and are not
written to disk.

## Pairing and reconnecting

- `/webmcp` shows the active listener and available commands in the TUI.
- `/webmcp pair --replace http://localhost:5173` disconnects the current bridge
  after confirmation and creates a new pairing.
- `/webmcp unpair` disconnects the active bridge.
- Pairing codes are one-time and expire after five minutes. A restarted bridge
  needs a new URL and code.
- If the session is unauthorized after an idle period, pair again with the newest values.

The browser preserves fallback drafts, tabs, selection, and explorer state across
refreshes for the current Vite app instance. It does not store pairing codes or
session tokens. Restarting Vite clears the fallback workspace.

## Useful controls

| Shortcut | Action |
| --- | --- |
| `Cmd/Ctrl+Shift+P` | Open the action palette |
| `Cmd/Ctrl+,` | Open Settings |
| `Cmd/Ctrl+K` | Focus the file filter |
| `Cmd/Ctrl+S` | Review the current draft |
| `Cmd/Ctrl+Enter` | Request a VT Code turn |
| `?` | Open help when focus is not in a form field |
| `Esc` | Close the active popup |

Long file lists, editor content, diffs, and terminal output have independent
scroll areas. Files are loaded when opened, so the explorer does not flatten the
entire workspace into the editor.

## Troubleshooting

### Port `5173` is busy

Reuse the existing demo at <http://localhost:5173>, or start another instance:

```sh
./start.sh --port 5174
```

Then use `http://localhost:5174` consistently in the browser and pairing
command.

### WebSocket connection failed

Make sure the bridge or active VT Code session is still running. Paste the newest
WebSocket URL and one-time code from that same process. Its origin must exactly
match the browser origin; `localhost` and `127.0.0.1` are different origins.

### The TUI prints no URL or pairing code

Run the demo from the current VT Code checkout, stop old VT Code processes, and pair again:

```sh
cd /path/to/vtcode
cargo run --locked -- chat
```

Then enter `/webmcp pair http://localhost:5173` in that TUI.

### `serve` is an unknown subcommand

Use `./start.sh --headless --workspace /absolute/path/to/project`, or run the
command from a current VT Code checkout with
`cargo run --locked -- webmcp serve ...`.

## Test the demo

From `examples/webmcp-challenge`:

```sh
npm test
npm run build
```

See the [WebMCP demo user guide](../../docs/user-guide/webmcp-demo.md) for the
full protocol, configuration, and security model.
