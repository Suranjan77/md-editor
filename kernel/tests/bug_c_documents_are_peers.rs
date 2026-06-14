//! BUG-C regression suite (M1 gate, plan §5).
//!
//! v2 symptom: a PDF could only ever be viewed in split view — opening one
//! hardcoded `split_view_active = true` and the view tree special-cased
//! "editor, optionally with a second pane".
//!
//! v3 contract (pillar 2, "documents are peers"): any document opens in any
//! pane as a tab; split is an explicit layout choice, never a requirement.

use md_kernel::input::EditorKind;
use md_kernel::pane::PaneError;
use md_kernel::{SplitAxis, TabId, Workspace};

fn ok<T>(r: Result<T, PaneError>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("unexpected pane error: {e}"),
    }
}

#[test]
fn pdf_opens_standalone_in_a_single_pane() {
    let mut ws = Workspace::new();
    let tab = ok(ws.open("papers/paper.pdf", EditorKind::Pdf));
    assert_eq!(
        ws.panes.pane_count(),
        1,
        "opening a PDF must not create a split"
    );
    assert_eq!(ws.focused_tab(), Some(tab));
    assert_eq!(ws.focused_editor_kind(), Some(EditorKind::Pdf));
}

#[test]
fn pdf_and_markdown_tab_side_by_side_in_one_pane() {
    // The v2-impossible state: a PDF as just another tab next to a note.
    let mut ws = Workspace::new();
    let md = ok(ws.open("notes/research.md", EditorKind::Markdown));
    let pdf = ok(ws.open("papers/paper.pdf", EditorKind::Pdf));
    assert_eq!(ws.panes.pane_count(), 1);

    let pane = match ws.focused_pane().and_then(|p| ws.panes.pane(p)) {
        Some(p) => p,
        None => panic!("focused pane must exist"),
    };
    assert_eq!(pane.tabs().len(), 2);

    // Tab switching swaps document kinds with no layout/mode change.
    ok(ws.focus_tab(md));
    assert_eq!(ws.focused_editor_kind(), Some(EditorKind::Markdown));
    ok(ws.focus_tab(pdf));
    assert_eq!(ws.focused_editor_kind(), Some(EditorKind::Pdf));
    assert_eq!(ws.panes.pane_count(), 1);
}

#[test]
fn split_is_an_explicit_choice_and_collapses_cleanly() {
    let mut ws = Workspace::new();
    let md = ok(ws.open("notes/research.md", EditorKind::Markdown));
    let pdf = ok(ws.open_in_new_split("papers/paper.pdf", EditorKind::Pdf, SplitAxis::Horizontal));
    assert_eq!(
        ws.panes.pane_count(),
        2,
        "split happened because it was asked for"
    );

    // Closing the PDF tab collapses the split; focus returns to the note.
    ok(ws.close_tab(pdf));
    assert_eq!(ws.panes.pane_count(), 1);
    assert_eq!(ws.focused_tab(), Some(md));
}

#[test]
fn closing_everything_leaves_a_clean_empty_workspace() {
    let mut ws = Workspace::new();
    let pdf = ok(ws.open("papers/paper.pdf", EditorKind::Pdf));
    ok(ws.close_tab(pdf));
    assert_eq!(ws.panes.pane_count(), 1);
    assert_eq!(ws.focused_tab(), None::<TabId>);
    assert!(
        ws.docs.is_empty(),
        "document store garbage-collects unreferenced documents"
    );
}

#[test]
fn no_mode_flags_exist_scope_stack_is_derived() {
    // The five v2 flags (split_view_active / showing_pdf / active_panel /
    // two active_paths) have no equivalent: layout lives in the PaneTree,
    // focus in the FocusModel, and everything else is derived per call.
    let mut ws = Workspace::new();
    let stack_empty = ws.scope_stack();
    let pdf = ok(ws.open("papers/paper.pdf", EditorKind::Pdf));
    let stack_pdf = ws.scope_stack();
    ok(ws.close_tab(pdf));
    let stack_after = ws.scope_stack();

    assert_ne!(stack_empty, stack_pdf);
    assert_eq!(
        stack_empty, stack_after,
        "state fully unwinds — nothing to hand-sync"
    );
}
