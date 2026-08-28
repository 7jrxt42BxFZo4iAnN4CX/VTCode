# WebMCP Demo User Guide

The WebMCP demo is a browser code editor for exploring the workflow:

```text
inspect → edit a draft → review a diff → propose → apply → check → revert
```

It has three modes:

| Mode | Start | Write boundary |
| --- | --- | --- |
| Fallback | Open the demo without pairing | Page memory only |
| Headless connected | Pair `vtcode webmcp serve` | Selected workspace; full-auto policy |
| Active connected | `/webmcp pair <origin>` in `vtcode chat` | Session workspace; terminal policy |

The browser never writes to the filesystem directly.

## Run the fallback demo

This is the quickest way to try the editor and does not require VT Code, Rust,
an API key, or a network connection.

From the repository root:

```sh
cd examples/webmcp-challenge
npm ci
npm run dev
```

Open the local URL printed by Vite, normally `http://localhost:5173`. The
header should show **Fallback mode**.

### Complete the fallback walkthrough

1. Select `src/config.js` in the workspace tree.
2. Change `WebMCP` to another value, such as `browser`.
3. Select **Review changes** or press `Cmd+S` on macOS / `Ctrl+S` on Windows and Linux.
4. Inspect the unified diff. The edit is still only a draft.
5. Select **Approve patch**, confirm the browser dialog, then select **Apply approved patch**.
6. Select **Run checks**. The fallback runs deterministic checks in the browser.
7. Select **Revert last change** if you want to restore the original project.

Fallback approvals and changes are deliberately simulated. Closing or
refreshing the page removes them.

Other useful controls:

- **Reload** reads the selected file again from the current backend.
- **Discard draft** removes an unsubmitted edit from the selected file.
- **Filter files** narrows the hierarchical explorer by file name or path;
  `Cmd/Ctrl+K` focuses it. It does not load the whole project to search file
  contents.
- **Run self-check** performs an edit, review, approval, apply, check, and
  revert cycle. Start with a clean workspace and an open file.
- **Request turn** explains that fallback mode has no VT Code runtime; it does not call an LLM.

The demo uses an IDE-style layout rather than a page of file previews. The
explorer keeps directories collapsed, unopened files remain metadata-only, and
the selected file is loaded into CodeMirror on demand. Open files appear as
tabs. The bottom panel keeps **TERMINAL**, **CHANGES**, and **VT CODE** output
inside bounded scroll areas, so long files and command output do not overflow
the page.

## Connect to a real workspace

Use the local development server when pairing with a local bridge. This avoids
the browser security restrictions that commonly apply to a deployed HTTPS
page connecting to a plain `ws://` endpoint.

### 1. Start the browser editor

In one terminal:

```sh
cd examples/webmcp-challenge
npm ci
npm run dev -- --host localhost --port 5173 --strictPort
```

Open `http://localhost:5173` and leave this terminal running. The example uses
a strict port so it fails clearly if `5173` is occupied instead of switching to
an origin that does not match the bridge allowlist.

### 2. Start the bridge

In a second terminal, run the bridge from the VT Code repository root:

```sh
cargo run --locked -- webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /absolute/path/to/workspace
```

Replace `/absolute/path/to/workspace` with the project you want the browser to
inspect. For an already-built source checkout, use:

```sh
./target/debug/vtcode webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /absolute/path/to/workspace
```

The command above assumes the repository binary has already been built with
`cargo build --locked`. A globally installed binary may predate the WebMCP
command; if it reports `unrecognized subcommand 'serve'`, use the `cargo run`
command above or build and run `./target/debug/vtcode` from the repository.

The server binds to loopback and normally chooses an available port. Copy the
WebSocket URL and one-time pairing code printed by the command. The URL will
look like:

```text
ws://127.0.0.1:<port>/webmcp
```

The pairing code expires after five minutes and can be used once. Keep the
bridge terminal open while using the editor. Press `Ctrl+C` there to stop the
server and revoke its in-memory sessions.

Use the URL and code printed by the same, currently running bridge process.
Every restart may choose a different port and creates a new one-time code.
If the browser reports a WebSocket connection failure, it is usually using an
old URL, a stopped bridge, or a code from an earlier restart.

### 3. Pair the browser

1. Expand **Connect this editor to a local VT Code WebMCP bridge**.
2. Paste the printed WebSocket URL into **WebSocket URL**.
3. Paste the printed code into **One-time pairing code**.
4. Select **Pair with VT Code**.

The editor should change to **VT Code connected**, and the workspace tree will
show files from the selected root instead of the fallback files. The browser
keeps the pairing code and session token only in memory.

The origin must match exactly. For example, `http://localhost:5173` and
`http://127.0.0.1:5173` are different origins, and a trailing slash is not
accepted in the configured origin.

## Edit, review, and apply in headless mode

The editing steps are the same as fallback mode:

1. Filter the explorer if needed, expand a directory, and open a file. Only
   the selected file is loaded into the editor.
2. Edit its draft buffer; a `*` marker identifies unsent changes.
3. Select **Review changes** or press `Cmd/Ctrl+S`, then inspect the diff in
   the bottom **CHANGES** panel.
4. Select **Request VT Code approval** to send the structured proposal with
   the base file digests.
5. Select **Apply after terminal approval** only after the backend has accepted
   the proposal.
6. Select **TERMINAL**, run checks, and inspect **CHECK OUTPUT**.
7. Use **Revert last change** when the change identity is still current.

The standalone `vtcode webmcp serve` path is headless. Its default policy
allows inspection and proposal staging, but rejects real mutation and check
execution unless the corresponding tools are explicitly enabled by full-auto
policy. It does not display an interactive terminal approval prompt or execute
agent turns. The editor reports this explicitly in the prompt panel instead of
showing a successful no-op.

The prompt composer includes the reviewed unified diff, bounded to the bridge
prompt limit. A real active runtime can review the browser draft; the headless
filesystem adapter rejects the request with an **agent turns unavailable**
message.

## Send a draft to an active VT Code session

Use this workflow for the **VT CODE TURN** button.
The browser bridge and interactive TUI must be started by the same VT Code
process.

After starting the browser as described above, start VT Code in the target
workspace:

```sh
cd /path/to/vtcode
cargo run --locked -- chat
```

For a different workspace, pass it explicitly:

```sh
cargo run --locked -- --workspace /absolute/path/to/workspace chat
```

In the VT Code TUI, enter:

```text
/webmcp pair http://localhost:5173
```

Paste the printed WebSocket URL and one-time code into the browser.
Do not use the URL or code from a separate `vtcode webmcp serve` process.

In the browser, review the draft, select **Stage for VT Code turn**, write the
instruction, and select **Request VT Code turn**.
The prompt and diff arrive in the active TUI as a normal agent turn.
Terminal tool permissions remain authoritative.
The active bridge keeps direct browser apply, check, and revert disabled; ask
VT Code to perform those actions and reload the browser afterward.

### Optional: enable apply and checks for a disposable demo workspace

Only do this with a workspace you are comfortable changing automatically.
Create a small Rust workspace so the built-in connected check has a
`Cargo.toml` and lockfile:

```sh
cargo new --bin /tmp/vtcode-webmcp-demo
cd /tmp/vtcode-webmcp-demo
```

Add this to `/tmp/vtcode-webmcp-demo/vtcode.toml`:

```toml
[automation.full_auto]
enabled = true
allowed_tools = ["apply_patch", "exec_command"]
require_profile_ack = false
```

Start the server from that workspace with the full-auto flag:

```sh
vtcode --full-auto webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /tmp/vtcode-webmcp-demo
```

For a source checkout, build or run the repository binary instead, for
example from the VT Code repository. Pass `--workspace-dir` so VT Code loads
the disposable workspace configuration:

```sh
cargo run --locked -- --workspace-dir /tmp/vtcode-webmcp-demo --full-auto webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /tmp/vtcode-webmcp-demo
```

In this mode `apply_patch` permits the real file mutation and `exec_command`
permits the connected `cargo check --locked` action. The browser confirmation
is only a UI step; the server-side full-auto policy is the authority.

## Drafts and conflicts

Draft buffers are separate from backend snapshots:

- A `*` beside a file means the browser has unsent edits.
- **Review changes** creates a diff but does not write a file.
- **Reload** refreshes a clean buffer from the backend.
- If a dirty file changed outside the editor, reload stops with an
  external-change conflict instead of overwriting the draft.
- Use **Discard draft** when the external version should win, then reload if necessary.
- A stale proposal is rejected when its base digest no longer matches the current file.
- Revert is also fail-closed if a file changed after the bridge applied it.

## GitHub Pages and deployed copies

The GitHub Pages build is a static copy of the editor and starts in fallback
mode. It is useful for demonstrating editing, diff review, and browser WebMCP
read-only tools, but it cannot access a local workspace by itself.

To connect a deployed HTTPS page to a bridge, use a TLS-terminating reverse
proxy and a `wss://` public URL. The bridge remains bound to loopback; direct
remote binding is rejected. Configure the exact deployed page origin in the
bridge allowlist. For local real-workspace demonstrations, use the Vite
development URL described above.

## Troubleshooting

### “WebSocket connection failed”

Confirm that the bridge is still running and that the browser uses the exact
WebSocket URL printed by VT Code, including its port and `/webmcp` path.

### “Origin rejected”

Use the browser's actual origin in `--origin`. Check the hostname, port,
scheme, and trailing slash. `localhost` and `127.0.0.1` are not interchangeable.

### “Pairing code is invalid” or expired

Restart `vtcode webmcp serve` and enter the newly printed code. A code is
one-time and is not reusable after a successful pairing.

### Apply or checks are rejected

This is expected for the default headless policy. For a disposable workspace,
enable `[automation.full_auto]`, include `apply_patch` and/or `exec_command` in
`allowed_tools`, and start the command with `--full-auto`. Check output uses
`cargo check --locked` in connected mode.

### A file shows an external-change conflict

Another process changed the file after the browser loaded it. Copy any needed
draft text, choose **Discard draft**, and reload the file before creating a new
proposal.

## Developer commands

From `examples/webmcp-challenge`:

```sh
npm test
npm run build
```

The implementation details, protocol boundaries, configuration, and security
model are documented in the [WebMCP bridge development guide](../development/webmcp.md).
