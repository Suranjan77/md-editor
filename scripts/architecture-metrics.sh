#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

printf '%-42s %10s %10s\n' 'File' 'Lines' 'Budget'
printf '%-42s %10s %10s\n' '----' '-----' '------'

report_path() {
    local file="$1"
    local budget="$2"
    local lines
    if [[ -d "$file" ]]; then
        lines="$(
            find "$file" -type f -name '*.rs' -print0 \
                | sort -z \
                | xargs -0 cat \
                | wc -l
        )"
    else
        lines="$(wc -l <"$file")"
    fi
    printf '%-42s %10d %10d' "$file" "$lines" "$budget"
    if (( lines > budget )); then
        printf '  warning'
    fi
    printf '\n'
}

# Budgets are directional warning thresholds, not pass/fail gates.
report_path native/src/app 10000
report_path native/src/editor/renderer 4800
report_path native/src/editor/parser 2300
report_path native/src/editor/buffer 2200
report_path core/src/domain/pdf 800
report_path core/src/infrastructure/pdfium 1750
report_path core/src/vault 1200
report_path core/src/vault.rs 800
report_path core/src/state.rs 800
report_path native/src/messages.rs 350
report_path native/src/main.rs 250

printf '\nMigration counters\n'
native_sql_matches="$(rg -l '\brusqlite::' native/src -g '*.rs' || true)"
if [[ -n "$native_sql_matches" ]]; then
    native_sql_count="$(printf '%s\n' "$native_sql_matches" | wc -l)"
else
    native_sql_count=0
fi

public_field_matches="$(
    rg -n 'pub (db|vault_root|file_index|pdf_state|pdf_renderer):' \
        core/src/state.rs || true
)"
if [[ -n "$public_field_matches" ]]; then
    public_field_count="$(printf '%s\n' "$public_field_matches" | wc -l)"
else
    public_field_count=0
fi

printf 'native rusqlite files: %s\n' "$native_sql_count"
printf 'public AppState infrastructure fields: %s\n' "$public_field_count"
