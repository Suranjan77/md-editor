#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

failures=0

# Fallback shim: this script must not silently pass when ripgrep is absent
# (every probe is wrapped in `|| true`, so a missing rg means a green lie).
if ! command -v rg >/dev/null 2>&1; then
    rg() {
        local mode="search" quiet=0 args=() paths=()
        while (( $# )); do
            case "$1" in
                --files) mode="files" ;;
                -g) shift ;; # glob filter; the find below already targets *.rs
                -q) quiet=1 ;;
                -n|-l) ;;
                -*) ;;
                *) args+=("$1") ;;
            esac
            shift
        done
        if [[ "$mode" == "files" ]]; then
            find "${args[@]}" -type f -name '*.rs'
            return
        fi
        local pattern="${args[0]}"
        paths=("${args[@]:1}")
        if (( ${#paths[@]} == 0 )); then
            if (( quiet )); then grep -qE "$pattern"; else grep -nE "$pattern"; fi
        else
            if (( quiet )); then
                grep -rqE "$pattern" "${paths[@]}"
            else
                grep -rnE --include='*' "$pattern" "${paths[@]}"
            fi
        fi
    }
fi

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
while IFS= read -r source_file; do
    awk '/^#\[cfg\(test\)\]/{exit} {print}' "$source_file" >>"$renderer_production"
done < <(rg --files native/src/editor/renderer -g '*.rs')
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

check_no_matches \
    'native must use core persistence APIs instead of rusqlite' \
    '\brusqlite::|extern crate rusqlite' \
    native/src native/Cargo.toml

check_no_matches \
    'AppState infrastructure fields must not be public' \
    'pub (db|vault_root|file_index|pdf_state|pdf_renderer):' \
    core/src/state.rs

check_no_matches \
    'AppState must delegate persistence to database repositories' \
    '\brusqlite::|\b(SELECT|INSERT INTO|UPDATE|DELETE FROM)\b' \
    core/src/state.rs

check_no_matches \
    'core must not import winit' \
    '(^|[^[:alnum:]_])winit::|extern crate winit' \
    core/src core/Cargo.toml

check_no_matches \
    'native must not invoke pdfium directly — go through core PdfService' \
    '(^|[^[:alnum:]_])pdfium_render::|extern crate pdfium_render' \
    native/src native/Cargo.toml

# Cross-feature import ban (docs/ARCHITECTURE_RULES.md): features/X must not
# import crate::features::Y for X != Y. Existing violations are allowlisted
# below; the list may only SHRINK (Phase 3 removes them via AppEvent routing).
cross_feature_allowlist=(
    'native/src/features/pdf/update.rs:use crate::features::shell::ActivePanel;'
    'native/src/features/search.rs:use crate::features::pdf::search::PdfSearchState;'
    'native/src/features/workspace.rs:use crate::features::pdf::navigation::NavigationHistory;'
    'native/src/features/workspace.rs:crate::features::pdf::navigation::NavigationTarget'
)
while IFS= read -r match; do
    file="${match%%:*}"
    rest="${match#*:}"
    line_content="${rest#*:}"
    feature_dir="$(printf '%s' "$file" | sed -E 's|native/src/features/([a-z_]+).*|\1|')"
    target="$(printf '%s' "$line_content" | sed -E 's|.*crate::features::([a-z_]+).*|\1|')"
    [[ "$feature_dir" == "$target" ]] && continue
    allowed=0
    for entry in "${cross_feature_allowlist[@]}"; do
        entry_file="${entry%%:*}"
        entry_frag="${entry#*:}"
        if [[ "$file" == "$entry_file" && "$line_content" == *"$entry_frag"* ]]; then
            allowed=1
            break
        fi
    done
    if (( ! allowed )); then
        printf '%s\n' "$match" >&2
        fail "cross-feature import: features/$feature_dir must not import features/$target (route via AppEvent)"
    fi
done < <(rg -n 'crate::features::[a-z_]+' native/src/features -g '*.rs' || true)

check_no_matches \
    'design system must not depend on features or views' \
    'crate::(features|views|app)::' \
    native/src/design

if (( failures > 0 )); then
    printf 'Architecture checks failed: %d\n' "$failures" >&2
    exit 1
fi

printf 'Architecture checks passed.\n'
