# vtcode-webmcp

`vtcode-webmcp` is the production WebMCP bridge shipped with VT Code. It
provides the authenticated transport, pairing, protocol, and bounded workspace
adapter used by the VT Code browser integration. It is deliberately
independent of the TUI so an active session can supply its own runtime adapter
while `vtcode webmcp serve` uses the safe filesystem adapter. The Vite app
under `apps/webmcp` is the browser application, not the
bridge's authority boundary.

The bridge is opt-in, binds to loopback by default, requires an expiring
one-time pairing code, validates the browser `Origin` header, and keeps pairing
tokens in process memory. The configured TTL is the inactivity lease for an
authenticated session; authenticated requests refresh it. Browser patch requests are proposals: the adapter
must authorize the mutation and the base digest must still match before any
file is changed. The listener does not terminate TLS; remote access must go
through a TLS-terminating reverse proxy.

## OpenAI-compatible remote MCP

`vtcode webmcp serve` has an independent, opt-in read-only MCP surface. It
mounts modern Streamable HTTP at `/mcp` and legacy HTTP+SSE at `/sse/`, with
session messages at `/messages/{session_id}`. The only advertised tools are:

- `search({ query })` → `{ results: [{ id, title, url }] }`
- `fetch({ id })` → `{ id, title, text, url, metadata? }`

The handlers use the same `RuntimeAdapter::list_files` and
`RuntimeAdapter::read_file` boundary as the bridge. Search is deterministic and
case-insensitive, with defaults of 20 results, 256 scanned files, and 16 MiB
of scanned UTF-8 content. Both tools return structured content and matching JSON
text content and are marked read-only, non-destructive, idempotent, and closed
world. Citation URLs are empty unless a prefix is configured; the crate never
serves workspace files over HTTP.

The listener remains loopback-only. An external TLS proxy or identity provider
must validate the public OAuth bearer, remove it, and inject an internal bearer
token from the environment variable configured by `proxy_token_env`. VT Code
validates that internal token and exposes protected-resource metadata at
`/.well-known/oauth-protected-resource`; it does not implement an OAuth,
token, or JWKS server. The nested MCP Origin allowlist is separate from the
browser pairing allowlist, and missing MCP `Origin` is accepted.

Example:

```sh
export VTCODE_WEBMCP_MCP_PROXY_TOKEN='proxy-injected-internal-token'
vtcode webmcp serve --mcp \
  --mcp-public-url https://mcp.example.com/sse/ \
  --mcp-authorization-server https://login.example.com \
  --mcp-proxy-token-env VTCODE_WEBMCP_MCP_PROXY_TOKEN \
  --allowed-root /absolute/path/to/workspace
```

See the [WebMCP development guide](../../../docs/development/webmcp.md) for
configuration, security boundaries, and transport tests.

The optional live Responses API smoke test is ignored by default. Set
`OPENAI_API_KEY` and `VTCODE_WEBMCP_LIVE_SSE_URL` to a reachable public HTTPS
`/sse/` URL, then run `cargo nextest run -p vtcode-webmcp --locked
--run-ignored all -E 'test(live_openai_responses_api_smoke)'`.

The browser diff is a review preview only. For an active turn, the browser
sends the staged proposal ID; the adapter revalidates its stored snapshots and
hands VT Code a bounded prompt with the authoritative unified diff. The
proposal remains unapplied until the normal VT Code tools and terminal policy
authorize the change.

Filesystem listings remain bounded and skip common dependency, cache,
generated-output, and credential directories such as `node_modules`, `target`,
`.git`, `dist`, and `.ssh`. Sensitive basenames are excluded from both listings
and reads. On supported platforms, file access traverses from a bound workspace
directory handle with no-follow operations; compare-and-replace uses the opened
file handle so an external change cannot be overwritten by a stale proposal.

```rust,no_run
use std::sync::Arc;
use vtcode_webmcp::{FilesystemWorkspace, WebmcpServer, WebmcpServerConfig};

# async fn run() -> vtcode_webmcp::Result<()> {
let workspace = FilesystemWorkspace::new(".", [], false).await?;
let mut config = WebmcpServerConfig::default();
config.allowed_origins = vec!["http://localhost:5173".to_string()];
let server = WebmcpServer::new(Arc::new(workspace), config)?;
let pairing = server.begin_pairing();
println!("Pair in the terminal: {}", pairing.code());
server.serve().await?;
# Ok(())
# }
```

The public protocol types are in [`protocol`], and active VT Code sessions can
publish canonical runtime events through [`WebmcpEventHub`]. The standalone
filesystem adapter reports `turns_available: false` and rejects agent-turn
requests until an active runtime adapter is attached; it never fabricates a
successful turn.

After pairing, an authenticated `status` response includes the adapter's
workspace/runtime capabilities and a non-secret `BridgeSettings` snapshot of
the listener, limits, pairing lease, and remote-proxy mode. Browser clients may
display and refresh this snapshot, but the active TUI remains authoritative for
origins, roots, permissions, and writes. Pairing codes and session tokens are
not part of the settings payload.

In an active TUI session, `/webmcp pair <origin>` reports the existing
listener, endpoint, one-time code, and expiry when a bridge is already
running. `/webmcp pair --replace <origin>` asks for terminal confirmation,
revokes current browser sessions, and issues a fresh pairing code on the same
listener. `/webmcp unpair` uses terminal confirmation before stopping the
listener.
