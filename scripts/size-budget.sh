#!/usr/bin/env bash
# Enforce module-size ratchets from budgets.toml: every workspace .rs file must
# stay under its per-file ceiling (or the hard limit if unlisted).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

budget_file="budgets.toml"
hard_limit="$(sed -n 's/^hard_limit *= *\([0-9][0-9]*\).*/\1/p' "$budget_file")"
if [[ -z "$hard_limit" ]]; then
    echo "error: missing hard_limit in $budget_file" >&2
    exit 1
fi

declare -A ceilings=()
in_section=0
while IFS= read -r line; do
    case "$line" in
        "[file_budgets]") in_section=1; continue ;;
        \[*) in_section=0; continue ;;
    esac
    (( in_section )) || continue
    [[ "$line" =~ ^\"(.+)\"[[:space:]]*=[[:space:]]*([0-9]+) ]] || continue
    ceilings["${BASH_REMATCH[1]}"]="${BASH_REMATCH[2]}"
done <"$budget_file"

failures=0
while IFS= read -r -d '' file; do
    file="${file#./}" # normalise to match budgets.toml keys (no leading ./)
    lines="$(wc -l <"$file")"
    ceiling="${ceilings[$file]:-$hard_limit}"
    if (( lines > ceiling )); then
        printf 'error: %s has %d lines; ceiling is %d\n' "$file" "$lines" "$ceiling" >&2
        failures=$((failures + 1))
    fi
done < <(find . -path './target' -prune -o -name '*.rs' -type f -print0)

for file in "${!ceilings[@]}"; do
    if [[ ! -f "$file" ]]; then
        echo "error: budgeted file missing: $file" >&2
        failures=$((failures + 1))
    fi
done

if (( failures > 0 )); then
    echo "size budget failed: $failures violation(s)" >&2
    exit 1
fi

echo "size budget passed (hard limit: $hard_limit lines)"
