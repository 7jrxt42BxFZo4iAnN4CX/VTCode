# WebMCP bridge development guide

VT Code's WebMCP bridge lets a browser editor inspect a workspace and submit structured change proposals without giving the browser direct filesystem access. The bridge is opt-in and is implemented in the `vtcode-webmcp` crate.

## Boundaries

The browser uses CodeMirror draft buffers. `Cmd/Ctrl+S` opens review and never writes directly. A proposal contains workspace-relative paths, the `sha256:` digest observed when each file was read, and complete UTF-8 content:

```json
{
  "changes": [
    { "path": "src/greeting.js", "base_digest": "sha256:...", "content": "..." }
  ]
}
```

The adapter re-reads every base file before staging and applying. The headless filesystem adapter rejects absolute paths, parent components, sensitive paths, symlink components, hard-linked files, oversized files, stale digests, duplicate changes, and writes outside its canonical allowed roots. On Unix platforms with directory-handle support, reads and writes traverse from a bound root descriptor with `O_NOFOLLOW`; compare-and-replace locks and validates the opened file before writing, so a pathname swap cannot redirect a stale proposal. Platforms without that directory-handle primitive reject filesystem adapter construction rather than falling back to a pathname race. Revert requires the last change identity and verifies that the current file still matches the applied snapshot. Staged proposals have bounded count and memory budgets.

The browser-generated diff is a review preview, not an authorization or agent
input source. An active `turn.request` may include the `proposal_id` returned by
`patch.propose`; the active adapter revalidates every stored base snapshot and
hands the existing runtime a bounded prompt containing the server-generated
authoritative diff. A stale proposal is rejected before it can enter the TUI,
and the proposal is never applied automatically by the handoff.

## WebMCP API versus the VT Code bridge

The example implements two related but separate directions:

1. The page implements the standard browser WebMCP provider surface through
   `document.modelContext.registerTool()`. When supported by the browser, it
   registers bounded file inspection tools plus page-only editor actions. Tool
   definitions include `title`, JSON Schema input, annotations, and an
   abort-aware `execute(input, { signal })` callback. The registration uses an
   `AbortSignal` to unregister tools and observes `toolchange` notifications.
   Browser or in-page agents discover and invoke these tools through the
   WebMCP API.
2. The VT Code editor connection uses the authenticated `/webmcp` WebSocket.
   It is a VT Code-specific adapter, not a WebMCP wire protocol: browser
   requests go to VT Code and VT Code returns responses and canonical
   `VersionedThreadEvent` notifications. This is the path used by **VT CODE
   TURN**, terminal approval, workspace proposals, and runtime event updates.

The WebMCP specification defines the page `ModelContext` API and browser-agent
observation/invocation. It does not define a server-to-page WebSocket or a way
for a Rust terminal agent to call `document.modelContext.executeTool()`
directly. A future VT Code-to-page tool-call relay would therefore need an
explicit, separately documented bridge extension and must retain terminal
approval; it must not be presented as native WebMCP.

WebMCP calls also require the page to remain open in a supported browser
browsing context. Current Chrome gates the page API on origin isolation and the
`tools` Permissions Policy. The demo exposes the observed prerequisites through
`get_editor_state.webmcp_context` and shows a recovery message when the context
is not eligible; its in-memory editor fallback remains available. The
imperative API is used because this editor has stateful search, selection, draft,
and review actions rather than a standard HTML-form submission surface.

## Browser tool contracts and evals

The example keeps its browser tool surface intentionally small: eight tools for
inspection, navigation, and draft review. It does not expose apply, write, or
revert as WebMCP tools. Each input is checked against its JSON Schema at runtime
before the application callback runs, and every result is bounded to 1,500
characters, following Chrome's current WebMCP security guidance. File and diff
results include truncation flags; collection results include an omitted count.
This keeps model context useful without turning a file or workspace listing into
an unbounded data channel.

`examples/webmcp-challenge/evals/webmcp-evals.ts` is the checked-in eval corpus.
It includes direct intents, open-ended output-dependent tool selection, a
multi-step review journey, and an invalid-argument failure case. Each case
defines the user goal, initial editor state, boundaries, expected UI effects,
success criteria, and recovery guidance. `get_editor_state` exposes the live
workflow state and bounded recommended next tools so an agent can recover its
place after a refresh or a failed step. Browser errors include the next safe
action, such as rereading a stale file or selecting an allowed panel.
`test/webmcp-evals.test.ts` validates tool names, metadata budgets, input errors,
and the browser-only authority boundary. These are deterministic contract
checks. Probabilistic selection and end-to-end model behavior must additionally
be tested in Chrome's Model Context Tool Inspector or another WebMCP-compatible
agent using the prompts in the example guide.

The registration intentionally omits WebMCP's `exposedTo` option. The demo has
no trusted cross-origin embedder, so exposing tools to another origin would add
authority without a defined trust relationship.

The example also supports [Chrome's optional origin trial](https://developer.chrome.com/blog/ai-webmcp-origin-trial)
without making a token part of source control. Set `VITE_WEBMCP_ORIGIN_TRIAL_TOKEN` during the Vite
build to prepend an `origin-trial` meta tag before the module loads. The Pages
workflow maps the optional repository Actions variable
`WEBMCP_ORIGIN_TRIAL_TOKEN` to that build variable. The token must be registered
for the exact page origin; when it is absent, feature detection, the Chrome
testing flag, and the in-memory fallback remain the supported paths.

## Transport and pairing

`WebmcpServer` exposes one WebSocket endpoint at `/webmcp`. Every connection must send an allowed `Origin` header. The first JSON message consumes a short-lived one-time pairing code. The response returns an in-memory session token; subsequent messages include that token and a request ID. The configured pairing TTL is the session inactivity lease: authenticated requests refresh it, while an idle session expires. Tokens are never written to URLs, logs, or persistent storage.

The protocol uses `VersionedThreadEvent` for runtime events. `WebmcpEventHub` adds a bridge sequence, retains a bounded replay window for reconnects, reports sequence gaps, and removes clients whose bounded queue is full. Lifecycle events are not silently dropped for a slow client.

The server rejects malformed JSON, binary frames, oversized frames, requests over the in-flight limit, disallowed origins, expired/revoked sessions, unsupported adapter operations, and unauthorised mutation requests. Mutation responses remain pending until the adapter reports a result so a transport timeout cannot be mistaken for a failed write. Adapter errors are returned as a generic runtime failure so filesystem details do not become a transport side channel; an intentionally unsupported operation uses the `unsupported` error code.

An authenticated `status` response includes a `settings` object containing the
non-secret listener host/port, pairing lease, frame limit, in-flight limit, and
remote-proxy flag, plus the exact authenticated browser origin. It also
includes the adapter's canonical workspace root and runtime capabilities. The
example editor renders these values in its Settings dialog and refreshes them
from the authenticated heartbeat, so the TUI configuration is the source of
truth. Pairing codes, session tokens, and other credentials are never returned
as settings or persisted by the browser.

## Running the headless bridge

Start it only when a browser connection is intended, with an exact origin allowlist:

```sh
vtcode webmcp serve --origin http://localhost:5173
```

The default bind is loopback with an OS-selected port. `--allowed-root` explicitly bounds the workspace roots available to a headless bridge. Patch application requires `apply_patch` in the existing explicit full-auto allowlist; checks separately require `exec_command` (or the wildcard entry). If either capability would be enabled, the selected workspace must also pass the existing full-auto workspace-trust gate. Checks resolve an allowlisted executable from trusted installation locations, clear the inherited environment, and run through the canonical `vtcode-safety` workspace sandbox with a bounded timeout and output cap. The bridge does not terminate TLS or bind a remote interface: `--allow-remote --public-url wss://...` is accepted only for a TLS-terminating reverse proxy that forwards to the loopback listener. Direct non-loopback binding is rejected.

The slash command `/webmcp` reports the command family and security boundary
inside an active session. `/webmcp pair <origin>` starts an authenticated
bridge owned by that interactive TUI session, prints its WebSocket URL,
one-time pairing code, and browser next-step instruction, then routes
`turn.request` prompts into the existing interaction loop. The browser pastes
the URL and code into **Settings → Connect or re-pair a VT Code bridge**. The
browser still cannot authorize writes; normal VT Code terminal permissions and
tool policy remain authoritative. If the bridge is already running, the pair
command prints its current endpoint, one-time code, and expiry. Use
`/webmcp pair --replace <origin>` to open a terminal confirmation before
disconnecting the current browser and issuing a fresh pairing code. `/webmcp unpair`
also requires terminal confirmation before it stops the bridge and revokes its
in-memory sessions.

`/webmcp` and `/webmcp help` also print the supported command arguments and
the active-session pairing boundary directly in the TUI.

The standalone `vtcode webmcp serve` adapter is intentionally headless. It
serves one root per process, reports `turns_available: false`, and rejects
`turn.request` because it has no active agent runtime. Multiple configured roots
are rejected until an explicit root-selection flow is wired through terminal
approval.

## Runtime adapters

`RuntimeAdapter` is the seam for an active TUI session. The active adapter
routes proposals and reads through the bounded workspace adapter, queues agent
turn prompts into the idle interaction loop, and publishes canonical runtime
events through the existing harness emitter. Direct browser apply/check/revert
operations remain denied so the model's normal terminal permission flow is the
only write path. When a proposal is attached to a turn, the adapter sends its
identity and authoritative unified diff through that prompt-only path; it does
not call the apply operation. Bridge prompts use a prompt-only inline event, so text beginning
with `/` is sent to the model and cannot invoke a TUI slash command. A headless
adapter must keep the explicit full-auto policy, workspace-trust gate, and
allowed-root restrictions.

Do not add a parallel event enum. Wrap runtime `ThreadEvent` values in `VersionedThreadEvent` at the pipeline boundary, then publish the versioned value through `WebmcpEventHub::publish`; preserve bridge sequence ordering when adding replay or client-side event handling.

## Configuration and verification

The `[webmcp]` table is separate from `[mcp]` and disabled by default:

```toml
[webmcp]
enabled = false
host = "127.0.0.1"
port = 0
allowed_origins = ["http://localhost:5173"]
allowed_roots = []
pairing_ttl_secs = 300
max_frame_bytes = 1048576
max_in_flight_requests = 8
```

Run the focused checks with:

```sh
RUSTFLAGS='-D warnings' cargo check --locked -p vtcode-webmcp -p vtcode
cargo nextest run -p vtcode-webmcp -p vtcode-core -p vtcode
cd examples/webmcp-challenge
bun install --frozen-lockfile
bun run typecheck
bun run test
bun run build
```

Keep adversarial coverage for path traversal, symlink escapes, stale changes, command injection, environment leakage, pairing reuse/expiry, origin rejection, malformed frames, sequence gaps, and slow clients.
