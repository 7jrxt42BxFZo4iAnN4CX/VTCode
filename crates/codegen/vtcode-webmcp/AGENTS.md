# vtcode-webmcp

[Root AGENTS.md](../../../AGENTS.md) | Authenticated browser bridge and safe workspace adapter.

## Modules

- `protocol` — versioned browser/server messages.
- `pairing` — expiring one-time codes, sessions, origin binding, revocation.
- `event_hub` — bounded replay and slow-client handling for runtime events.
- `runtime` — adapter traits and result types used by active and headless sessions.
- `filesystem` — canonicalized, digest-checked headless workspace adapter.
- `server` — Axum WebSocket transport and request dispatch.

## Rules

- Never put pairing tokens in URLs, logs, or persistent storage.
- `VersionedThreadEvent` is the only runtime event payload accepted by the hub.
- Patch mutations require adapter authorization and a matching current digest; check authorization is a separate full-auto `exec_command` decision routed through `vtcode-safety`.
- Commands are argv-based and allowlisted; only bounded check invocations may run, and never invoke a shell.
- Browser listings and reads must apply the central sensitive-file exclusions. Filesystem access must use bound directory handles with no-follow traversal where supported; compare-and-replace must operate on the opened handle and fail closed when that guarantee is unavailable.
- The listener is loopback-only; remote access requires a TLS-terminating reverse proxy.
- Multi-file mutations use serialized compare-and-rollback; preserve fail-closed behavior when rollback itself fails. Keep proposal count/byte budgets and subscriber count/byte budgets bounded.
