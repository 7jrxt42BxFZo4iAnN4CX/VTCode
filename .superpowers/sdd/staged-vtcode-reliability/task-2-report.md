# Task 2: apply_patch safety report

## Scope delivered

- Added apply-time resolved workspace containment for every parsed patch target before planning or mutation:
  - add target;
  - delete target;
  - update source; and
  - move destination.
- The shared `vtcode_commons::paths::ensure_path_within_workspace_resolved` helper is the strict check used for each target. The check is placed in `Patch::apply`, the common endpoint used by registry execution and intercepted shell `apply_patch` execution.
- Preserved existing parser validation for absolute and lexical-traversal paths; no parser checks were weakened or replaced.
- Updated model-facing freeform, JSON-schema, registry, and pack descriptions to require workspace-relative paths and forbid absolute paths, `..`, and traversal-like forms.
- Updated `docs/tools/TOOL_SPECS.md` with the same runtime containment contract.

## Tests

### RED

Command:

```text
cargo nextest run -p vtcode-core --test apply_patch_containment
```

Result before the containment change:

```text
2 passed, 1 failed
symlink_escape_is_rejected_before_any_patch_mutation
symlink escape must be rejected: ["[1/2] Added file: created.txt (15 bytes)", "[2/2] Deleted file: escape/victim.txt"]
```

This demonstrated the original vulnerability: a workspace-local symlink to an external directory allowed the second operation to mutate the external file after an earlier add had already mutated the workspace.

### GREEN

Controller verification command:

```text
cargo nextest run -p vtcode-core --test apply_patch_containment
```

Controller result:

```text
passed 3/3
parser_rejects_absolute_and_lexically_traversing_patch_paths
symlink_escape_is_rejected_before_any_patch_mutation
applies_a_valid_workspace_relative_path
```

The symlink regression includes an earlier in-workspace add operation and asserts that both that file and the external target remain unchanged when preflight rejects the patch.

## Additional verification

- `git diff --check` completed successfully with no output.
- `cargo fmt --check` was requested separately but did not complete: it was interrupted after 305.7 seconds by the controller. No formatting-pass claim is made in this report.

## Limitation

Only the requested focused test was run. No broad suite, clippy run, or changed-crate check was run. The formatter timeout remains the sole incomplete requested verification item.

## Fix round 1: intercepted symlink cwd

The shell-interception path now validates its supplied `cwd` against the
session workspace with `ensure_path_within_workspace_resolved` before it
constructs an `ApplyPatchRequest`. The existing `ApplyPatchError::ParseError`
message shape is unchanged.

Regression command:

```text
cargo nextest run -p vtcode-core intercept_rejects_symlink_escaped_cwd_before_outside_mutation
```

Result:

```text
Starting 1 test across 37 binaries (3427 tests skipped)
PASS vtcode-core tools::handlers::intercept_apply_patch::tests::intercept_rejects_symlink_escaped_cwd_before_outside_mutation
Summary: 1 test run: 1 passed, 3427 skipped
```

The regression passes a workspace symlink to an outside directory as the
intercepted cwd, expects the existing parse-rejection type, and verifies that
the attempted outside file was not created.

`git diff --check` completed successfully with no output. Per the fix-round
scope, no formatter, broad suite, or changed-crate check was run.
