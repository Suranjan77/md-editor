#!/usr/bin/env bash
# Enforces the ratchet budgets in budgets.toml (pass/fail, unlike the
# directional warnings in architecture-metrics.sh).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

failures=0

echo "Checking file line budgets (budgets.toml [file_budgets])..."
in_section=0
while IFS= read -r line; do
    case "$line" in
        "[file_budgets]") in_section=1; continue ;;
        \[*) in_section=0; continue ;;
    esac
    (( in_section )) || continue
    [[ "$line" =~ ^\"(.+)\"[[:space:]]*=[[:space:]]*([0-9]+) ]] || continue
    file="${BASH_REMATCH[1]}"
    budget="${BASH_REMATCH[2]}"
    if [[ ! -f "$file" ]]; then
        # File dissolved by a refactor: that is the budget reaching zero.
        printf '%-46s %8s %8d  (deleted — remove entry)\n' "$file" "-" "$budget"
        continue
    fi
    lines="$(wc -l <"$file")"
    printf '%-46s %8d %8d' "$file" "$lines" "$budget"
    if (( lines > budget )); then
        printf '  OVER BUDGET\n'
        failures=$((failures + 1))
    else
        printf '\n'
    fi
done <budgets.toml

echo
echo "Checking raw color ratchet..."
raw_color_ceiling="$(sed -n 's/^raw_color_ceiling *= *\([0-9]*\).*/\1/p' budgets.toml)"
if [[ -n "$raw_color_ceiling" ]]; then
    raw_color_count="$(
        find native/src -name '*.rs' -not -path 'native/src/design/*' -print0 \
            | xargs -0 grep -c 'Color::from_rgb' 2>/dev/null \
            | awk -F: '{sum += $NF} END {print sum + 0}'
    )"
    echo "raw Color::from_rgb literals outside design/: $raw_color_count (ceiling: $raw_color_ceiling)"
    if (( raw_color_count > raw_color_ceiling )); then
        echo "error: raw color ratchet exceeded — use design tokens (native/src/design)" >&2
        failures=$((failures + 1))
    fi
fi

echo
echo "Checking dependency budget..."
DEP_COUNT=$(cargo tree --workspace 2>/dev/null | wc -l)
echo "dependency tree lines: $DEP_COUNT (warning threshold 300)"
if [ "$DEP_COUNT" -gt 300 ]; then
    echo "::warning::Dependency count ($DEP_COUNT) exceeds budget of 300!"
fi

echo "Checking file budget..."
FILE_COUNT=$(find native/src core/src -name "*.rs" | wc -l)
echo "rust file count: $FILE_COUNT (warning threshold 150)"
if [ "$FILE_COUNT" -gt 150 ]; then
    echo "::warning::File count ($FILE_COUNT) exceeds budget of 150!"
fi

if (( failures > 0 )); then
    echo "Budget check failed: $failures file(s) over their ratchet ceiling." >&2
    echo "Budgets only go down — split the file or revert the growth." >&2
    exit 1
fi
echo "Budget checks complete."
