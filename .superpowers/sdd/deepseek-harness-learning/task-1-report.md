# Task 1 report: privacy-preserving harness trace analyzer

## Scope

Implemented the analyzer slice in `crates/codegen/vtcode-eval` only. The analyzer has no dependency on runtime crates or manifest changes and parses JSONL through `serde_json::Value`, allowing it to accept both DeepSeek-style records and VT Code `ThreadEvent` wire shapes without duplicating the runtime event contract.

## Behavior delivered

- Added `trace_analyzer` and re-exported its public summary and text/file entry points from `lib.rs`.
- Added serde-serializable `HarnessTraceSummary`, `TokenUsage`, and `LatencyStatistics` types.
- Tracks turns, steps, tool calls, per-tool counts, repeated calls, error categories, latency aggregates/percentiles, output byte totals, token/cache usage, malformed lines, and unrecognized lines.
- Counts only aggregate facts. Command arguments, file contents, tool output text, and free-form error messages are not retained; free-form errors are reduced to bounded categories.
- Continues after malformed JSON and valid-but-unrecognized records.
- Added synchronous `analyze_jsonl_file` with contextual filesystem errors.
- Added a compact generated fixture reproducing 453 steps, 468 calls, and 20 errors without copying the supplied log.

## TDD evidence

### RED

Command:

```text
cargo nextest run --locked -p vtcode-eval summarizes_deepseek_baseline_without_retaining_raw_text
```

Result: failed as expected because `HarnessTraceSummary` and `analyze_jsonl` did not yet exist.

### GREEN

Focused command:

```text
CARGO_TARGET_DIR=/tmp/vtcode-task1-target cargo nextest run --locked -p vtcode-eval trace_analyzer::
```

Result: 4/4 analyzer tests passed.

Final package command:

```text
CARGO_TARGET_DIR=/tmp/vtcode-task1-target cargo nextest run --locked -p vtcode-eval
```

Result: 14/14 tests passed.

Additional checks:

```text
cargo fmt -p vtcode-eval -- --check
CARGO_TARGET_DIR=/tmp/vtcode-task1-target cargo clippy --locked -p vtcode-eval --all-targets -- -D warnings
git diff --check
```

All completed successfully.

## Files changed

- `crates/codegen/vtcode-eval/src/trace_analyzer.rs`
- `crates/codegen/vtcode-eval/src/lib.rs`
- `crates/codegen/vtcode-eval/AGENTS.md` (module map and privacy invariant)
- `.superpowers/sdd/deepseek-harness-learning/task-1-report.md`

An unrelated concurrent modification in `crates/codegen/vtcode-core/src/prompts/guidelines.rs` was detected and left untouched; it is not part of the Task 1 commit.
