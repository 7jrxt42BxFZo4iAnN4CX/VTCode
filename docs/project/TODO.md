# VT Code TODO Triage

> Source: [Codeprobe audit](https://quinn.inkboxwire.com/r/vinhnx-vtcode-msllbbtq) — "79 TODOs"
> Triage date: 2026-08-09

## Summary

The audit's "79 TODOs" is a **false positive**. An automated scan counted every
occurrence of the literal word "TODO" — including the TODO **task panel**
feature name, test fixtures that use `docs/project/TODO.md` as a path, and grep
patterns that search for "TODO" as content.

### Actual code-debt markers in first-party source

| Category | Count | Status |
| --- | --- | --- |
| `TODO:` comments | **1** | Triaged below |
| `FIXME:` comments | **0** | — |
| `HACK:`/`XXX:` comments | **0** | — |
| `todo!()` macros | **0** | — |
| `unimplemented!()` macros | **0** | — |
| `unreachable!()` macros | 13 | All correct invariant guards (see below) |

### The 1 real TODO

**File**: `src/agent/runloop/unified/tool_pipeline/status.rs:74`
**Original**: `// TODO: Progress variant planned for streaming tool progress updates`
**Status**: Converted to a precise planned-feature comment with implementation
notes. Adding a `Progress { chunk: String }` variant to `ToolExecutionStatus`
requires updating all match arms (`display_status()`, `error()`, UI renderers).
This is a feature enhancement, not debt — no correctness or security impact.

### `unreachable!()` macro audit (13 occurrences)

All 13 are correct invariant guards, not unimplemented code:

- `AuthCredentialsStoreMode::Auto => unreachable!("effective_mode() resolves Auto")`
  (×5 in `vtcode-auth`) — `Auto` is always resolved before these match arms.
- `ToolCallStatus::InProgress => unreachable!("InProgress status passed to completion event")`
  (×1 in `vtcode-exec-events`) — enforced by the event contract.
- `_ => unreachable!("unexpected built-in primary agent")` (×2) — enum exhaustiveness.
- `nibble must be in 0..=15` (×1 in `vtcode-commons`) — mathematical invariant.
- Others (×4) — similar invariant guards in match arms.

These follow the AGENTS.md guidance: "Use `assert!`/`debug_assert!` for
invariants that always hold." None are debt.

## Audit security findings — all false positives

| Finding | File | Why it's safe |
| --- | --- | --- |
| Generic hardcoded secret | `sanitizer.rs` | Secret-*scrubbing* module: regex patterns + AWS doc example key as test fixtures |
| AWS access key id | `sanitizer.rs` | `AKIAIOSFODNN7EXAMPLE` — AWS's well-known documentation example |
| Generic hardcoded secret | `openai_chatgpt_oauth.rs` | OAuth 2.1 PKCE *public* client ID (not a secret by design) |
| Generic hardcoded secret | `openrouter_oauth.rs` | `sk-test-key-12345` test fixture |
| Generic hardcoded secret | `openai/provider/tests.rs` | Test fixtures asserting Debug impls don't leak tokens |
| `eval()` usage (×7) | `extensions/` | All in `node_modules/` vendored dependencies, not VT Code code |
| Empty catch blocks (×1) | `extensions/` | Not found in first-party code |

**Real-secret heuristic scan** (private keys, `AKIA…`, `sk-proj-…`, `ghp_…`,
`xox…` across all non-vendored source): **zero hits**.

## Improvements made from this audit

1. **Secret scanning CI** (`.github/workflows/secret-scan.yml` + `.gitleaks.toml`)
   — gitleaks on every push/PR + weekly full-history scan. Closes audit
   recommendation #2 ("Add automated secret scanning").

2. **Windows sandbox fail-closed** (`vtcode-safety/sandboxing/`) —
   `SandboxType::WindowsRestrictedToken.is_available()` now returns `false`
   (the sandbox isn't implemented) and `transform_windows` returns
   `UnavailableSandboxType` instead of silently passing through unsandboxed.
   This prevents a user who configures `ReadOnly`/`WorkspaceWrite` policy on
   Windows from getting zero sandboxing without an error. See
   [docs/guides/security.md](../guides/security.md) § Process sandbox boundaries.

3. **TODO triage** (this file) — the "79 TODOs" is 1 real planned-feature
   comment; the rest are false positives. Closes audit recommendation #4
   ("Reduce TODO backlog").

## Audit hardening follow-up

The audit hardening pass also resolved the implementation debt behind the
remaining boundary findings:

- Active pipe and PTY sessions now sanitize inherited and override-supplied
  credential, token, cloud, linker, and dynamic-loader variables.
- MCP stdio clients inherit `McpSandboxContext` through pools and reconnects;
  the canonical sandbox transform is used and unsupported restrictive policies
  fail closed. macOS hostname allowlists are rejected rather than widened.
- Provider error streams are capped at 16 KiB before parsing; diagnostics are
  then bounded, UTF-8-safe, and secret-redacted before reaching errors, logs,
  or custom auth-command messages.
- Provider-owned subprocesses now filter inherited API keys, cloud
  credentials, tokens, linker overrides, and dynamic-loader variables. Copilot
  and its optional `gh` status probe retain only their documented GitHub auth
  variables; local model helpers and custom auth commands receive no unrelated
  provider credentials.
- The global lint migration is enforced with `-D warnings`; intentional
  suppressions carry reasons and the stale result/slice/cast debt is removed.
- ACP permission-flow tests cover allow, deny, cancel, unknown-option, and
  request-failure outcomes over a duplex connection harness.
- `scripts/first-party-debt-scan.sh` blocks new actionable markers while
  excluding vendored, generated, fixture, template, sample, and task-panel
  content.

Verification:

```bash
./scripts/first-party-debt-scan.sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --locked --workspace
```
