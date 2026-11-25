//! Unit tests for `relink_note`: wikilinks and Markdown links across file and folder moves.

use super::*;

/// A vault of `files` before the move; wikilinks resolve by exact path (with
/// `.md` implied), then by basename.
fn check(files: &[&str], mv: Move<'_>, note_old: &str, content: &str) -> Option<String> {
    let exists = |rel: &str| files.contains(&rel);
    let resolve = |target: &str, note: &str| -> Option<String> {
        let base = if target.starts_with('.') {
            parent(note)
        } else {
            ""
        };
        let mut rel = normalize(&join(base, target))?;
        if !crate::vault::is_supported_vault_path(std::path::Path::new(&rel)) {
            rel.push_str(".md");
        }
        if files.contains(&rel.as_str()) {
            return Some(rel);
        }
        let name = rel.rsplit('/').next()?.to_lowercase();
        files
            .iter()
            .filter(|f| f.ends_with(".md"))
            .filter(|f| f.rsplit('/').next().map(str::to_lowercase) == Some(name.clone()))
            .min_by_key(|f| f.len())
            .map(|f| f.to_string())
            .or(Some(rel))
    };
    let old = OldVault {
        exists: &exists,
        resolve_wikilink: &resolve,
    };
    let note_new = mv.map(note_old).unwrap_or_else(|| note_old.to_string());
    relink_note(content, note_old, &note_new, mv, &old)
}

const FOLDER: Move<'static> = Move {
    old: "dir",
    new: "archive/dir2",
};

#[test]
fn folder_move_rewrites_path_qualified_wikilinks_only() {
    let files = ["index.md", "dir/note.md"];
    let out = check(
        &files,
        FOLDER,
        "index.md",
        "[[dir/note#Intro|the note]] and [[note]] and [[missing/x]]",
    );
    assert_eq!(
        out.as_deref(),
        Some("[[archive/dir2/note#Intro|the note]] and [[note]] and [[missing/x]]")
    );
}

#[test]
fn folder_move_fixes_relative_links_out_of_the_moved_note() {
    let files = ["index.md", "dir/note.md", "img/a.png"];
    let out = check(
        &files,
        FOLDER,
        "dir/note.md",
        "[[../index]] ![a](../img/a.png)",
    );
    assert_eq!(out.as_deref(), Some("[[index]] ![a](../../img/a.png)"));
}

#[test]
fn links_between_notes_moving_together_are_kept() {
    let files = ["dir/a.md", "dir/b.png"];
    assert_eq!(
        check(&files, FOLDER, "dir/a.md", "![b](b.png) ![c](./b.png)"),
        None
    );
}

#[test]
fn image_rename_rewrites_markdown_images_and_wikilinks() {
    let files = ["notes/a.md", "notes/diagram.png", "sketch.ink"];
    let mv = Move {
        old: "notes/diagram.png",
        new: "notes/figures/diagram v2.png",
    };
    let out = check(
        &files,
        mv,
        "notes/a.md",
        "![d](diagram.png \"Title\") [[notes/diagram.png]]",
    );
    assert_eq!(
        out.as_deref(),
        Some("![d](figures/diagram%20v2.png \"Title\") [[notes/figures/diagram v2.png]]")
    );
}

#[test]
fn ink_rename_rewrites_wikilinks() {
    let files = ["a.md", "sketch.ink"];
    let mv = Move {
        old: "sketch.ink",
        new: "ink/sketch.ink",
    };
    assert_eq!(
        check(&files, mv, "a.md", "see [[sketch.ink]]").as_deref(),
        Some("see [[ink/sketch.ink]]")
    );
}

#[test]
fn vault_relative_and_bracketed_markdown_links() {
    let files = ["a.md", "docs/My Paper.pdf"];
    let mv = Move {
        old: "docs",
        new: "papers",
    };
    let out = check(
        &files,
        mv,
        "a.md",
        "[p](<docs/My Paper.pdf#page=2>) [q](docs/My%20Paper.pdf) [r](/docs/My%20Paper.pdf)",
    );
    assert_eq!(
        out.as_deref(),
        Some(
            "[p](<papers/My Paper.pdf#page=2>) [q](papers/My%20Paper.pdf) [r](/papers/My%20Paper.pdf)"
        )
    );
}

#[test]
fn urls_anchors_and_code_are_left_alone() {
    let files = ["a.md", "dir/x.png"];
    let content =
        "[u](https://x.org/dir/x.png) [h](#top) `![x](dir/x.png)`\n```\n[[dir/x.png]]\n```\n";
    assert_eq!(check(&files, FOLDER, "a.md", content), None);
}

#[test]
fn file_rename_rewrites_bare_wikilinks_to_the_new_name() {
    let files = ["a.md", "notes/B.md"];
    let mv = Move {
        old: "notes/B.md",
        new: "notes/C.md",
    };
    assert_eq!(
        check(&files, mv, "a.md", "[[B]] [[b|alias]]").as_deref(),
        Some("[[notes/C]] [[notes/C|alias]]")
    );
}

#[test]
fn move_map_matches_whole_components() {
    assert_eq!(FOLDER.map("dir"), Some("archive/dir2".into()));
    assert_eq!(FOLDER.map("dir/x/y.md"), Some("archive/dir2/x/y.md".into()));
    assert_eq!(FOLDER.map("directory/y.md"), None);
}
