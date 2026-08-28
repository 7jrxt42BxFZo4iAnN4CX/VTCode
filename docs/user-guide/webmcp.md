# WebMCP Browser Bridge User Guide

VT Code ships a first-class, opt-in WebMCP browser bridge. It connects a
browser editor to either the current VT Code session or a bounded standalone
workspace bridge:

The bridge is part of the main `vtcode` binary and the published
`vtcode-webmcp` crate; it is not an example-only feature. The repository's
`examples/webmcp-challenge` Vite project is the maintained reference browser
client used to exercise the bridge and the browser's native WebMCP API.

```text
inspect → edit a draft → review a diff → propose → apply → check → revert
```

It has three modes:

| Mode | Start | Write boundary |
| --- | --- | --- |
| Reference editor | Open the browser client without pairing | Page memory only |
| Headless connected | Pair `vtcode webmcp serve` | Selected workspace; full-auto policy |
| Active connected | `/webmcp pair <origin>` in `vtcode chat` | Session workspace; terminal policy |

The browser never writes to the filesystem directly.

For a real agent turn, pairing has two sides:

1. **VT Code TUI:** run `/webmcp pair <origin>`. VT Code starts the active
   bridge and prints the WebSocket URL and one-time pairing code.
2. **Web app:** open **Settings → Connect or re-pair a VT Code bridge**, paste both values, and select
   **Pair with VT Code**.

The browser cannot start or approve the bridge. Keep the TUI running while the
browser is connected.

Browser WebMCP is available only while the reference editor is open in a supported browser
tab or webview. Chrome gates the page API on origin isolation and the `tools`
Permissions Policy; the reference editor reports the observed values in
`get_editor_state.webmcp_context` and keeps its normal fallback active when the
API is unavailable. The authenticated VT Code bridge is separate and can still
be used for workspace operations when explicitly paired.

## Two integration paths

The browser client has two interfaces that are easy to confuse:

- **Browser WebMCP:** When the browser provides `document.modelContext`, the
  page registers eight bounded tools for browser/in-page agents. They can list
  and read visible files, search buffers, inspect editor state, open files, stage
  one exact edit in a clean browser draft, review a draft, and switch panels.
  Draft edits update browser memory only; they cannot approve or apply a
  filesystem change.
- **VT Code bridge:** The paired editor uses VT Code's authenticated custom
  WebSocket protocol. The browser sends workspace, patch, check, and turn
  requests; VT Code sends responses and runtime events back. This is the path
  for **VT CODE TURN** and terminal policy.

The WebMCP specification covers the first interface (`ModelContext` and
browser-agent tool invocation), not the second. A browser WebMCP agent cannot
automatically call the Rust VT Code process, and VT Code cannot directly call
`document.modelContext.executeTool()` without a separate bridge extension.
Keeping the interfaces separate preserves the terminal approval boundary.

## Evaluate the browser tool surface

For a real browser-agent pass, use a supported WebMCP browser, enable
`chrome://flags/#enable-webmcp-testing`, relaunch Chrome, and open the deployed
editor. The [Chrome WebMCP documentation](https://developer.chrome.com/docs/ai/webmcp)
links to the Model Context Tool Inspector for listing tools, executing JSON
inputs, and inspecting structured output or errors.

Use these prompts to cover direct selection, an output-dependent sequence, and
a review journey:

1. “What files are available in this workspace?” → `list_project_files`.
2. “Find the file that defines the greeting and open it in the editor.” →
   `search_code`, then `open_file` with the path returned by the search.
3. “Change Hello to Hi in the greeting and prepare the draft for my review.” →
   `read_file`, `stage_text_edit` with the returned digest, then `review_draft`.
4. “Show me the current draft diff, then open the changes panel.” →
   `review_draft`, then `open_panel` with `changes`.

Check that long file/search results are explicitly truncated, untrusted file
content is marked with `untrustedContentHint`, `get_editor_state` reports the
workflow and `webmcp_context`, page-only actions update the editor, and no
browser tool can approve, apply, or revert a filesystem change.
The deterministic corpus and contract checks live in
`examples/webmcp-challenge/evals/webmcp-evals.ts` and run with `bun run test`.
Model tool selection remains probabilistic and needs the real browser-agent pass.

### Capture real Chrome or ChatGPT evidence

Open **Evidence** in the reference editor, select **Chrome WebMCP Tool
Inspector** or **ChatGPT in-app browser**, and choose **Start new run** before
the external client begins. Use Chrome with the WebMCP testing flag (or a
valid origin-trial token), or open the deployed page in ChatGPT's in-app
browser when that client exposes WebMCP. The recorder wraps the registered
callbacks, so each actual discovery and tool invocation is captured with
bounded metadata, errors, elapsed time, and a sanitized editor-state snapshot.

After the run, use **Copy JSON** or **Download JSON** and keep the export with
the client tool-inspector screenshot or screen recording. The export omits
file contents, diffs, prompts, pairing codes, session tokens, and sensitive
fields. The selected client name is a human attestation; the JSON demonstrates
that the page callbacks ran and should be reviewed together with the client
capture. **Run self-check** is a deterministic fallback test and is not a
substitute for this external-client evidence.

### Optional Chrome origin trial

Chrome offers WebMCP through a [time-limited origin trial](https://developer.chrome.com/blog/ai-webmcp-origin-trial).
To test the deployed reference editor without the testing flag, request a token for the exact origin
`https://vinhnx.github.io` and set the repository Actions variable
`WEBMCP_ORIGIN_TRIAL_TOKEN`. The Pages workflow passes that value to the Vite
build as `VITE_WEBMCP_ORIGIN_TRIAL_TOKEN`, which injects the token into the
document head before the application accesses `document.modelContext`.

No token is committed to the repository. If the variable is unset, use
`chrome://flags/#enable-webmcp-testing` in a supported Chrome version or use the normal
WebMCP-unavailable fallback. A token is tied to its registered origin, so a
production token does not enable the local Vite origin.

## Run the reference editor without a bridge

This is the quickest way to try the editor and does not require VT Code, Rust,
an API key, or a network connection.

From the repository root:

```sh
cd examples/webmcp-challenge
./start.sh
```

The launcher installs browser dependencies if needed and starts the local
server at `http://localhost:5173`. The header should show **Fallback mode**.
If your terminal is already in `examples/webmcp-challenge`, skip the `cd`
command.

The launcher can also start a connected workflow. Use `--headless` for a
workspace-only bridge or `--active` for an interactive VT Code session:

```sh
./start.sh --headless --workspace /absolute/path/to/workspace
./start.sh --active --workspace /absolute/path/to/workspace
```

In active mode, enter `/webmcp pair http://localhost:5173` when the TUI is
ready, then paste the printed URL and pairing code into the browser. Use
`--port 5174` consistently in the launcher, bridge origin, and pairing command
if port `5173` is already occupied. See the reference client's
[GUIDE.md](../../examples/webmcp-challenge/GUIDE.md) for the complete
one-command workflow.

### Complete the fallback walkthrough

1. Select `src/config.js` in the workspace tree.
2. Change `WebMCP` to another value, such as `browser`.
3. Select **Review changes** or press `Cmd+S` on macOS / `Ctrl+S` on Windows and Linux.
4. Inspect the unified diff. The edit is still only a draft.
5. Select **Approve patch**, confirm the browser dialog, then select **Apply approved patch**.
6. Select **Run checks**. The fallback runs deterministic checks in the browser.
7. Select **Revert last change** if you want to restore the original project.

Fallback approvals and changes are deliberately simulated. The browser keeps
the fallback project, drafts, open tabs, and selected file across a page
refresh. A new Vite app instance clears that browser state. Real bridge
credentials are never stored, so a refreshed page must be paired again before
it can access a real workspace.

Other useful controls:

- **Reload** reads the selected file again from the current backend.
- **Discard draft** removes an unsubmitted edit from the selected file.
- **Filter files** narrows the hierarchical explorer by file name or path;
  `Cmd/Ctrl+K` focuses it. It does not load the whole project to search file
  contents.
- **Run self-check** performs an edit, review, approval, apply, check, and
  revert cycle. Start with a clean workspace and an open file.
- **Request turn** explains that fallback mode has no VT Code runtime; it does not call an LLM.

### Open a workspace from the web app

Select **Settings** in the header, open **Choose a workspace and setup mode**,
and enter the absolute path to the workspace. The settings dialog generates
two safe, copyable choices:

1. **Active session · VT CODE TURN** starts `vtcode chat` for that workspace.
   After it opens, run `/webmcp pair <origin>` in the VT Code TUI.
2. **Workspace only** starts `vtcode webmcp serve` with the selected
   `--allowed-root`. This provides workspace inspection and proposals, but no
   agent turns.

The browser cannot launch a local process or grant filesystem access. Run the
copied command in a terminal, keep that process running, then select **Open
pairing settings** and paste the URL and one-time code printed by VT Code.

### Manage settings

Use the header's **Settings** button for all setup and pairing controls. The
dialog contains the browser origin, workspace path, generated terminal
commands, WebSocket URL, and one-time pairing code. The URL and workspace path
are remembered for this Vite app instance to make reconnecting easier; the
pairing code and authenticated session token are never stored.

After pairing, **Bridge status** is synchronized from the VT Code `status`
response and heartbeat. It shows the authenticated workspace, runtime mode,
listener, pairing lease, and request limits. These are read-only values: the
active VT Code TUI and its `[webmcp]` configuration remain authoritative. To
change origins, roots, policy, or the active session, use the VT Code TUI and
pair again with the newly printed values.

The reference editor uses an IDE-style layout rather than a page of file previews. The
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
./start.sh
```

Open `http://localhost:5173` and leave this terminal running. The reference client uses
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

The pairing code expires after five minutes and can be used once. After a
successful pair, the browser sends a small authenticated heartbeat and the
session lease is refreshed while the page remains connected. Keep the bridge
terminal open while using the editor. Press `Ctrl+C` there to stop the server
and revoke its in-memory sessions.

Use the URL and code printed by the same, currently running bridge process.
Every restart may choose a different port and creates a new one-time code.
If the browser reports a WebSocket connection failure, it is usually using an
old URL, a stopped bridge, or a code from an earlier restart.

### 3. Pair the browser

1. Open **Settings** and expand **Connect or re-pair a VT Code bridge**.
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

For fallback mode, the prompt composer includes the browser-reviewed diff within
the bridge prompt limit. In active mode, the browser sends the staged
`proposal_id`; VT Code revalidates the files and supplies its own bounded,
authoritative unified diff to the TUI. The handoff never applies the proposal
automatically. The headless filesystem adapter rejects the request with an
**agent turns unavailable** message.

## Send a draft to an active VT Code session

Use this workflow for the **VT CODE TURN** button.
The browser bridge and interactive TUI must be started by the same VT Code
process.

After starting the browser as described above, start VT Code in the target
workspace. Run this command from the VT Code repository checkout:

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

The TUI prints the values the browser needs:

```text
Active WebMCP bridge started.
WebSocket: ws://127.0.0.1:<port>/webmcp
Pairing code: <one-time-code> (expires in <seconds> seconds)
In the WebMCP editor, open **Settings → Connect or re-pair a VT Code bridge** and paste the WebSocket URL and pairing code above.
```

In the browser, open **Settings → Connect or re-pair a VT Code bridge**, paste the
WebSocket URL and pairing code into the two fields, and select **Pair with VT
Code**. Do not use the URL or code from a separate `vtcode webmcp serve`
process.

In the browser, review the draft, select **Stage for VT Code turn**, write the
instruction, and select **Request VT Code turn**. From the prompt composer,
`Cmd/Ctrl+Enter` performs the same action. After staging an active-session
proposal, the editor switches to **VT CODE** and focuses the prompt composer.
The prompt and server-authoritative diff arrive in the active TUI as a normal
agent turn. The browser diff remains a local review preview; the adapter
revalidates the proposal ID before enqueueing the handoff.
Terminal tool permissions remain authoritative.
The active bridge keeps direct browser apply, check, and revert disabled; ask
VT Code to perform those actions and reload the browser afterward.

### Optional: enable apply and checks for a disposable workspace

Only do this with a workspace you are comfortable changing automatically.
Create a small Rust workspace so the built-in connected check has a
`Cargo.toml` and lockfile:

```sh
cargo new --bin /tmp/vtcode-webmcp-workspace
cd /tmp/vtcode-webmcp-workspace
```

Add this to `/tmp/vtcode-webmcp-workspace/vtcode.toml`:

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
  --allowed-root /tmp/vtcode-webmcp-workspace
```

For a source checkout, build or run the repository binary instead, for
source checkout from the VT Code repository. Pass `--workspace-dir` so VT Code loads
the disposable workspace configuration:

```sh
cargo run --locked -- --workspace-dir /tmp/vtcode-webmcp-workspace --full-auto webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /tmp/vtcode-webmcp-workspace
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

The GitHub Pages build is a static copy of the reference editor and starts in
fallback mode. It is useful for evaluating browser WebMCP read-only tools, but
it cannot access a local workspace by itself. The production bridge is the
Rust component shipped with the VT Code binary; it does not depend on GitHub
Pages.

To connect a deployed HTTPS page to a bridge, use a TLS-terminating reverse
proxy and a `wss://` public URL. The bridge remains bound to loopback; direct
remote binding is rejected. Configure the exact deployed page origin in the
bridge allowlist. For local real-workspace use, use the Vite
development URL described above.

## Troubleshooting

### “WebSocket connection failed”

Confirm that the bridge is still running and that the browser uses the exact
WebSocket URL printed by VT Code, including its port and `/webmcp` path. For
active-session mode, the URL must come from the TUI where you ran
`/webmcp pair`; a standalone `webmcp serve` URL cannot receive agent turns.

### “WebMCP session is not authorized”

The browser lost its in-memory session token, the bridge was restarted, or the
page was suspended longer than the session lease. For an active session, run
`/webmcp pair http://localhost:5173` again in the TUI. For a standalone bridge,
restart `vtcode webmcp serve`. Then paste the new URL and one-time code. An open
page normally renews the lease automatically.

### “Enter the WebMCP WebSocket URL and the terminal pairing code”

The browser form is missing one or both pairing values. Paste the URL into
**WebSocket URL** and the code into **One-time pairing code**. The code is not
part of the URL.

### “Origin rejected”

Use the browser's actual origin in `--origin`. Check the hostname, port,
scheme, and trailing slash. `localhost` and `127.0.0.1` are not interchangeable.

### “Pairing code is invalid” or expired

Restart `vtcode webmcp serve` and enter the newly printed code. A code is
one-time and is not reusable after a successful pairing.

### The TUI prints only “Active WebMCP bridge started.”

Restart VT Code from the current source checkout, then run
`/webmcp pair http://localhost:5173` again. A current build prints the
WebSocket URL, pairing code, and browser instruction immediately below the
status line.

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
bun install --frozen-lockfile
bun run typecheck
bun run test
bun run build
```

The implementation details, protocol boundaries, configuration, and security
model are documented in the [WebMCP bridge development guide](../development/webmcp.md).
