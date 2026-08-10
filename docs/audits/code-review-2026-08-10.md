# Code Review + Fixes — 2026-08-10

Scope: `vtcode-auth` production code, with the existing sandbox/exec audit
changes preserved. The review prioritized credential lifecycle correctness,
storage coupling, error handling, and the largest security-sensitive module.

## Findings and disposition

| Severity | Finding | Root cause | Fix |
|---|---|---|---|
| High | OpenRouter logout could report success after a storage deletion failure | `clear_oauth_token` and `Auto` mode discarded backend errors | Shared storage now propagates deletion failures and clears both new backends plus the legacy file when requested |
| Medium | OpenAI and OpenRouter duplicated keyring, encrypted-file, and serialization logic | Each OAuth module owned a private storage implementation despite the existing `CredentialStorage` pattern used by MCP OAuth | Added typed `OpenAiSessionStorage` and `OpenRouterTokenStorage` boundaries over `CredentialStorage`; added shared JSON methods |
| Medium | Refresh classification was coupled to credential deletion | `classify_refresh_status_error` parsed a response and mutated storage in the same function | Added pure `openai_refresh_policy` classification returning an explicit clear/preserve action; orchestration applies the action |
| Low | Unused refresh helper constructed a partial session with blank token fields | Private deprecated helper had no callers | Removed the dead helper |

## Compatibility and false-positive filtering

- Existing `openai_chatgpt.json` and `openrouter.json` encrypted files remain
  readable and are migrated after a successful load.
- New file storage uses the shared salted AES-256-GCM credential backend.
- Existing token `Debug` redaction and response-body redaction were retained.
- No changes were made to public OAuth client IDs; they are public PKCE
  identifiers, not client secrets.
- Test-only bulk restructuring was intentionally deferred; the review target
  was production coupling and behavior.

## Refactoring guard rails

- OAuth orchestration cannot call raw keyring or file backends.
- Storage adapters expose typed save/load/clear operations and keep legacy
  format handling local to the adapter.
- Provider operations that promise selected-backend scope use an exact-backend
  boundary; the generic credential API retains fallback behavior for callers
  that explicitly use that policy.
- Refresh policy has no storage dependency; only the orchestration layer can
  delete a session.
- Legacy migration is covered for both providers.

## Verification

```text
cargo fmt --all -- --check                                      PASS
cargo clippy --locked -p vtcode-auth --all-targets -- -D warnings PASS
cargo nextest run --locked -p vtcode-auth                         138 passed
./scripts/check-dev.sh --changed                              2298 passed
cargo nextest run --locked -p vtcode-safety                       245 passed
cargo audit --file Cargo.lock                              PASS (3 allowed
                                                           unmaintained warnings)
git diff --check                                                   PASS
python3 scripts/check_docs_links.py                              PASS
bash scripts/check_workflow_security.sh                          PASS
```
