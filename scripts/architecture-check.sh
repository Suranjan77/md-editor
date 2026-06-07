#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

failures=0

fail() {
    printf 'error: %s\n' "$1" >&2
    failures=$((failures + 1))
}

check_no_matches() {
    local description="$1"
    local pattern="$2"
    shift 2

    local matches
    matches="$(rg -n "$pattern" "$@" 2>/dev/null || true)"
    if [[ -n "$matches" ]]; then
        printf '%s\n' "$matches" >&2
        fail "$description"
    fi
}

printf 'Checking workspace dependency graph...\n'
cargo metadata --no-deps --format-version 1 >/dev/null
core_dependencies="$(cargo tree -p md-editor-core --depth 1 --prefix none)"
if printf '%s\n' "$core_dependencies" | tail -n +2 | rg -q '^md-editor-native '; then
    fail 'md-editor-core must not depend on md-editor-native'
fi

check_no_matches \
    'core must not import native UI dependencies' \
    '(^|[^[:alnum:]_])(iced|md_editor_native)::|extern crate (iced|md_editor_native)' \
    core/src core/Cargo.toml

check_no_matches \
    'views must not import SQLite or PDFium' \
    '(^|[^[:alnum:]_])(rusqlite|pdfium_render)::|extern crate (rusqlite|pdfium_render)' \
    native/src/views

renderer_production="$(mktemp)"
trap 'rm -f "$renderer_production"' EXIT
awk '/^#\[cfg\(test\)\]/{exit} {print}' native/src/editor/renderer.rs >"$renderer_production"
check_no_matches \
    'renderer production code must consume parser output, not invoke parser implementations' \
    '\b(highlight_markdown|parse_markdown)\b|(pulldown_cmark|comrak|markdown|syntect|ratex_parser)::' \
    "$renderer_production"

while IFS= read -r source_file; do
    production_source="$(mktemp)"
    awk '/^#\[cfg\(test\)\]/{exit} {print}' "$source_file" >"$production_source"
    if rg -n '\.set_text\(' "$production_source" >/dev/null; then
        rg -n '\.set_text\(' "$production_source" >&2
        fail "direct buffer set_text in production code: $source_file"
    fi
    rm -f "$production_source"
done < <(rg --files native/src -g '*.rs')

printf 'Reporting migration debt (warning-only)...\n'
native_sql_files="$(rg -l '\brusqlite::' native/src -g '*.rs' || true)"
if [[ -n "$native_sql_files" ]]; then
    printf 'warning: native direct SQLite remains in:\n%s\n' "$native_sql_files" >&2
fi

public_infrastructure="$(rg -n 'pub (db|vault_root|file_index|pdf_state|pdf_renderer):' core/src/state.rs || true)"
if [[ -n "$public_infrastructure" ]]; then
    printf 'warning: AppState still exposes infrastructure fields:\n%s\n' \
        "$public_infrastructure" >&2
fi

if (( failures > 0 )); then
    printf 'Architecture checks failed: %d\n' "$failures" >&2
    exit 1
fi

printf 'Architecture checks passed.\n'
