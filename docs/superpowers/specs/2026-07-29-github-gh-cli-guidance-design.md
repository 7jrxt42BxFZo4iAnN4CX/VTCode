# Authenticated GitHub CLI Guidance

## Goal

Add one concise root-level instruction requiring VT Code agents to prefer the
authenticated GitHub CLI for GitHub interactions.

## Design

Add the following bullet to the `AGENTS.md` Rules section:

> For GitHub operations, prefer the authenticated `gh` CLI. Run `gh auth status`
> first and use the authenticated account with access to the target repository.
> Fall back to GitHub connectors or HTTP only when `gh` lacks the required
> capability. Never print tokens or credential contents.

This keeps the default workflow explicit while preserving a fallback for
operations the CLI cannot perform. Authentication checks remain read-only, and
the credential-safety sentence prevents agents from exposing token values while
diagnosing access.

## Verification

- Confirm `AGENTS.md` remains under 150 lines.
- Confirm the new rule appears once in the Rules section.
- Run `git diff --check`.
