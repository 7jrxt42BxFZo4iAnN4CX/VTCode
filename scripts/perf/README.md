# Local Performance Scripts

These scripts provide a repeatable local performance workflow for VT Code.

## Commands

```bash
# Capture metrics + raw logs
./scripts/perf/baseline.sh baseline
./scripts/perf/baseline.sh latest

# Compare two captured runs
./scripts/perf/compare.sh \
  .vtcode/perf/baseline.json \
  .vtcode/perf/latest.json

# Build release binary for profiling (line tables + frame pointers)
./scripts/perf/profile.sh

# Local-only host-tuned build/run
./scripts/perf/native-build.sh
./scripts/perf/native-run.sh -- --version
```

The baseline captures `tool_pipeline_bench_ms` and the `vtcode-core`
`agent_harness_bench_ms`. The harness benchmark includes deterministic cases
for prompt-resource cache hits, few-shot selection, tool-catalog sorting, and
warm indexed file-search scoring. These measurements are for local comparison;
they are not a CI performance gate.

## Outputs

All artifacts are written to `.vtcode/perf/`:

- `baseline.json` / `latest.json`: captured metrics
- `*-cargo_check.log`: cargo check output
- `*-bench_tool_pipeline.log`: `vtcode-core` tool-pipeline bench output
- `*-bench_agent_harness.log`: `vtcode-core` interactive harness and optimization bench output
- `*-startup.json` (if `hyperfine` installed)
- `diff.md`: markdown comparison report

## Notes

- Cargo steps clear `RUSTC_WRAPPER` and `CARGO_BUILD_RUSTC_WRAPPER` by default so the scripts still work when the environment or `.cargo/config.toml` points at a blocked `sccache`.
- Set `PERF_KEEP_RUSTC_WRAPPER=1` if you explicitly want the perf run to keep the configured wrapper.
- `startup_ms` measures the built `target/debug/vtcode` binary, not `cargo run`, so it tracks process startup instead of compile time.
- When `hyperfine` is unavailable, startup falls back to a 10-run Python mean and writes the raw sample summary to `*-startup.log`.
