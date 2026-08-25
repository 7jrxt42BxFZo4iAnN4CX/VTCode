#!/usr/bin/env bash
set -euo pipefail

# Measure process startup without provider credentials. Use a release binary
# when available; VTCODE_BENCH_INTERACTIVE=1 enables the manual first-frame run.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${VTCODE_BIN:-${repo_root}/target/release/vtcode}"
runs="${VTCODE_BENCH_RUNS:-5}"

if [[ ! -x "$binary" ]]; then
    cargo build --release --locked --manifest-path "${repo_root}/Cargo.toml"
fi

measure() {
    local label="$1"; shift
    local i start end
    for ((i = 1; i <= runs; i++)); do
        start="$(python3 -c 'import time; print(time.perf_counter_ns())')"
        VTCODE_STARTUP_TRACE="${VTCODE_STARTUP_TRACE:-0}" "$binary" "$@" >/dev/null
        end="$(python3 -c 'import time; print(time.perf_counter_ns())')"
        awk -v l="$label" -v n="$i" -v s="$start" -v e="$end" \
            'BEGIN { printf "%s run=%d elapsed_ms=%.3f\n", l, n, (e-s)/1000000 }'
    done
}

echo "binary=$binary runs=$runs"
measure version --version
measure help --help

if [[ "${VTCODE_BENCH_INTERACTIVE:-0}" == "1" ]]; then
    echo "interactive: stop after phase=first_ui_render"
    VTCODE_STARTUP_TRACE=1 "$binary"
fi
