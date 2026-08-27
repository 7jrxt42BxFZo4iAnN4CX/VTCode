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

## Transport and pairing

`WebmcpServer` exposes one WebSocket endpoint at `/webmcp`. Every connection must send an allowed `Origin` header. The first JSON message consumes a short-lived one-time pairing code. The response returns an in-memory session token; subsequent messages include that token and a request ID. Tokens are never written to URLs, logs, or persistent storage.

The protocol uses `VersionedThreadEvent` for runtime events. `WebmcpEventHub` adds a bridge sequence, retains a bounded replay window for reconnects, reports sequence gaps, and removes clients whose bounded queue is full. Lifecycle events are not silently dropped for a slow client.

The server rejects malformed JSON, binary frames, oversized frames, requests over the in-flight limit, disallowed origins, expired/revoked sessions, unsupported adapter operations, and unauthorised mutation requests. Mutation responses remain pending until the adapter reports a result so a transport timeout cannot be mistaken for a failed write. Adapter errors are returned as a generic runtime failure so filesystem details do not become a transport side channel; an intentionally unsupported operation uses the `unsupported` error code.

## Running the headless bridge

Start it only when a browser connection is intended, with an exact origin allowlist:

```sh
vtcode webmcp serve --origin http://localhost:5173
```

The default bind is loopback with an OS-selected port. `--allowed-root` explicitly bounds the workspace roots available to a headless bridge. Patch application requires `apply_patch` in the existing explicit full-auto allowlist; checks separately require `exec_command` (or the wildcard entry). If either capability would be enabled, the selected workspace must also pass the existing full-auto workspace-trust gate. Checks resolve an allowlisted executable from trusted installation locations, clear the inherited environment, and run through the canonical `vtcode-safety` workspace sandbox with a bounded timeout and output cap. The bridge does not terminate TLS or bind a remote interface: `--allow-remote --public-url wss://...` is accepted only for a TLS-terminating reverse proxy that forwards to the loopback listener. Direct non-loopback binding is rejected.

The slash command `/webmcp` reports the command family and security boundary
inside an active session. `/webmcp pair <origin>` starts an authenticated
bridge owned by that interactive TUI session, prints its one-time pairing code,
and routes `turn.request` prompts into the existing interaction loop. The
browser still cannot authorize writes; normal VT Code terminal permissions and
tool policy remain authoritative. `/webmcp unpair` stops that session bridge
and revokes its in-memory sessions.

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
only write path. Bridge prompts use a prompt-only inline event, so text beginning
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
node --check examples/webmcp-challenge/src/main.js
npm run build --prefix examples/webmcp-challenge
```

Keep adversarial coverage for path traversal, symlink escapes, stale changes, command injection, environment leakage, pairing reuse/expiry, origin rejection, malformed frames, sequence gaps, and slow clients.
