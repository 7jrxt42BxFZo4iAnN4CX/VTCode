# Task 2 Report: Share discussion-first behavior between Duck and Plan

## Implementation

- Added `DISCUSSION_FIRST_GUIDANCE` and composed both built-in primary prompts from it plus role-specific fragments.
- Plan retains repository-grounded read-only discovery, emits exactly one `<proposed_plan>`, never writes plan artifacts manually, and waits for explicit approval before implementation.
- Duck remains rubber-duck-only, read-only, and directs edit requests to the Build agent; it does not gain plan persistence semantics.
- Added prompt regression assertions for shared guidance, Plan-only behavior, and Duck-only boundaries.

## TDD verification

RED command:

```text
cargo nextest run -p vtcode-config builtin_plan_agent
```

Output: 3 tests run; 1 passed, 2 failed. The new shared-guidance and Plan-contract assertions failed against the old prompts; the existing request-user-input test passed.

GREEN command:

```text
cargo nextest run -p vtcode-config builtin_plan_agent
```

Output: 3 tests run; 3 passed, 398 skipped.

Additional verification:

```text
cargo fmt -- --check
cargo nextest run -p vtcode-config
```

Output: formatting clean; 401 tests run, 401 passed, 0 skipped.

## Files changed

- `crates/codegen/vtcode-config/src/subagents.rs`
- `.superpowers/sdd/2026-08-23-plan-mode-reliability/task-2-report.md`

## Self-review

- Prompt composition is limited to the two built-in primary agents in scope.
- Plan permissions/tools are unchanged, including read-only shell enforcement and `request_user_input`.
- Duck receives no planning tools, persistence instructions, or `<proposed_plan>` contract.
- `git diff --check` passed.

## Concerns

None identified.

## Fix round 1

Changed the Plan role contract from one `<proposed_plan>` block to exactly one final `<proposed_plan>` block, and updated `builtin_plan_agent_prompt_requires_grounded_discovery_and_approval` to assert the final-block requirement.

RED:

```text
cargo nextest run -p vtcode-config builtin_plan_agent
```

Output: 3 tests run; 2 passed, 1 failed. The Plan contract test failed because the prompt did not contain `exactly one final <proposed_plan> block`.

GREEN and formatting:

```text
cargo nextest run -p vtcode-config builtin_plan_agent
cargo fmt -- --check
```

Output: 3 tests run; 3 passed, 398 skipped; formatting clean. Covering tests include the shared Plan/Duck guidance test, the Plan grounded-discovery/final-plan/approval test, and the existing request-user-input test.
