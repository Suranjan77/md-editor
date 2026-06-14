#!/usr/bin/env bash
# Architecture boundary checks (ADR-0100). The engine crates — kernel, editor,
# vault, pdf — are toolkit-agnostic and must never reference a GUI toolkit
# (iced/winit) in code or manifests. Only the shell crate composes the engines
# with iced. This script fails CI if a boundary is crossed.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

failures=0

# Fallback shim so a missing ripgrep cannot turn every probe into a green lie.
if ! command -v rg >/dev/null 2>&1; then
    rg() {
        local quiet=0 args=()
        while (( $# )); do
            case "$1" in
                -q) quiet=1 ;;
                -n) ;;
                -*) ;;
                *) args+=("$1") ;;
            esac
            shift
        done
        local pattern="${args[0]}"
        local paths=("${args[@]:1}")
        if (( quiet )); then
            grep -rqE "$pattern" "${paths[@]}"
        else
            grep -rnE "$pattern" "${paths[@]}"
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

# Engine crates must not name a GUI toolkit anywhere — imports or manifest deps.
for crate in kernel editor vault pdf; do
    check_no_matches \
        "$crate must stay toolkit-free (ADR-0100): no iced/winit" \
        '(use (iced|winit)\b|\b(iced|winit)::|^[[:space:]]*(iced|winit)[[:space:]]*=)' \
        "$crate/src" "$crate/Cargo.toml"
done

# Engines never depend on each other in production code: composition happens
# only in the shell. (pdf has a dev-only md-vault dependency for the FTS-bridge
# test; that is allowed because it lives under [dev-dependencies].)
for crate in kernel editor vault pdf; do
    check_no_matches \
        "$crate production code must not import sibling engine crates (compose in the shell)" \
        '\bmd_(kernel|editor|vault|pdf|shell)::' \
        "$crate/src"
done

if (( failures > 0 )); then
    printf 'Architecture checks failed: %d\n' "$failures" >&2
    exit 1
fi

printf 'Architecture checks passed.\n'
