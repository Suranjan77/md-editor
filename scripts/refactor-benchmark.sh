#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run_timed() {
    local label="$1"
    shift

    printf '\n== %s ==\n' "$label"
    command time -f 'elapsed=%e user=%U system=%S max_rss_kb=%M' "$@"
}

run_timed "release build" cargo build --workspace --release
run_timed "10k-line markdown parse" \
    cargo test -p md-editor-native --release \
    editor::parser::tests::large_document_highlight_preserves_line_count_and_block_ids -- --exact
run_timed "editor height lookup" \
    cargo test -p md-editor-native --release \
    editor::layout_tree::tests::large_height_tree_queries_stay_logarithmic -- --exact
run_timed "1000-page PDF layout lookup" \
    cargo test -p md-editor-native --release \
    features::pdf::view_model::tests::large_pdf_layout_page_lookup_stays_logarithmic -- --exact
run_timed "PDF viewport scheduling" \
    cargo test -p md-editor-native --release \
    app::model::tests::pdf_viewport_render_range_uses_visible_pages_plus_small_preload -- --exact
run_timed "vault unified search" \
    cargo test -p md-editor-core --release \
    vault::search::tests::test_search_vault_unified -- --exact
