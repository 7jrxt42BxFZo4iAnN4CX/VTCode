#!/usr/bin/env bash

# Scan first-party implementation sources for actionable debt markers.
# Deliberately excludes generated, vendored, fixture, template, sample, and
# task-panel content because those files contain marker-shaped data by design.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if ! command -v rg >/dev/null 2>&1; then
    echo "first-party debt scan requires ripgrep (rg)" >&2
    exit 2
fi

exclude_globs=(
    '--glob=!**/vendor/**'
    '--glob=!**/generated/**'
    '--glob=!**/fixture/**'
    '--glob=!**/fixtures/**'
    '--glob=!**/task-panel/**'
    '--glob=!**/task_panel/**'
    '--glob=!**/*task-panel*'
    '--glob=!**/*task_panel*'
    '--glob=!**/templates/**'
    '--glob=!**/assets/samples/**'
)

markers="$(rg --line-number --no-heading --color=never '\b(TODO|FIXME|HACK|XXX)[[:space:]]*:' src crates scripts "${exclude_globs[@]}" || true)"

if [[ -n "$markers" ]]; then
    echo "Actionable first-party debt markers found:" >&2
    echo "$markers" >&2
    exit 1
fi

echo "No actionable first-party TODO/FIXME/HACK/XXX markers found."
