#!/usr/bin/env bash
# Clippy count ratchet: fails if the workspace has more clippy warnings than
# the committed baseline in `clippy-baseline.txt`. When the count drops, lower
# the baseline in the same change (`--update` does it) so it only goes down.
set -euo pipefail
cd "$(dirname "$0")/.."

baseline_file=clippy-baseline.txt
baseline=$(tr -dc '0-9' < "$baseline_file")

output=$(cargo clippy --workspace --all-targets --message-format=short 2>&1) || {
    echo "$output"
    echo "clippy failed to run" >&2
    exit 1
}
# Count unique source-located warnings (a lib and its test target report the
# same warning twice).
warnings=$(echo "$output" | grep -E '^[^ ]+\.rs:[0-9]+:[0-9]+: warning' | sort -u || true)
count=$(echo -n "$warnings" | grep -c . || true)

echo "clippy warnings: $count (baseline $baseline)"
if [[ "${1:-}" == "--update" ]]; then
    echo "$count" > "$baseline_file"
    echo "baseline set to $count"
    exit 0
fi
if (( count > baseline )); then
    echo "$warnings"
    echo "clippy warnings grew from $baseline to $count — fix the new ones" >&2
    exit 1
fi
if (( count < baseline )); then
    echo "warnings dropped below the baseline: run scripts/clippy-ratchet.sh --update and commit clippy-baseline.txt" >&2
    exit 1
fi
