#!/usr/bin/env bash
# Ratchet check: count of unwrap()/expect( in production code may only go down.
# Production code = .rs files under core/src and native/src, excluding:
#   - anything after the first `#[cfg(test)]` line in a file
#   - files whose path contains "tests"
#   - lines carrying a `// INVARIANT:` justification (documented escape hatch)
# Ceiling lives in budgets.toml ([ratchets] unwrap_ceiling).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

ceiling="$(sed -n 's/^unwrap_ceiling *= *\([0-9]*\).*/\1/p' budgets.toml)"
if [[ -z "$ceiling" ]]; then
    echo "error: unwrap_ceiling not found in budgets.toml" >&2
    exit 1
fi

count=0
while IFS= read -r source_file; do
    case "$source_file" in
        *tests*) continue ;;
    esac
    file_count="$(
        awk '/^#\[cfg\(test\)\]/{exit} {print}' "$source_file" \
            | grep -v '// INVARIANT:' \
            | grep -c '\.unwrap()\|\.expect(' || true
    )"
    count=$((count + file_count))
done < <(find core/src native/src -name '*.rs' | sort)

echo "unwrap()/expect( in production code: $count (ceiling: $ceiling)"
if (( count > ceiling )); then
    echo "error: unwrap budget exceeded — remove unwraps or document an // INVARIANT: and keep the count at or below $ceiling" >&2
    exit 1
fi
if (( count < ceiling )); then
    echo "note: count is below ceiling — ratchet budgets.toml down to $count"
fi
