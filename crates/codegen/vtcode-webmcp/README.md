# vtcode-webmcp

`vtcode-webmcp` provides the authenticated WebMCP bridge used by the VT Code
browser editor. It is deliberately independent of the TUI so an active session
can supply its own runtime adapter while `vtcode webmcp serve` uses the safe
filesystem adapter.

The bridge is opt-in, binds to loopback by default, requires an expiring
one-time pairing code, validates the browser `Origin` header, and keeps pairing
tokens in process memory. The configured TTL is the inactivity lease for an
authenticated session; authenticated requests refresh it. Browser patch requests are proposals: the adapter
must authorize the mutation and the base digest must still match before any
file is changed. The listener does not terminate TLS; remote access must go
through a TLS-terminating reverse proxy.

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
