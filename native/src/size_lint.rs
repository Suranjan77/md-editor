//! Code-health guardrail (code-health plan W6): no non-test module over ~800 lines and no function over 150 lines, in core and native, with a ratchet list for today's offenders that may only shrink.

use std::path::{Path, PathBuf};

/// Lines of non-test code a module may have.
const MODULE_LIMIT: usize = 800;
/// Lines a single function may span, signature to closing brace.
const FUNCTION_LIMIT: usize = 150;

/// Modules over the limit today, with the size they may not grow past. Split
/// one and lower (or remove) its entry; the test fails if an entry is stale.
const MODULE_RATCHET: &[(&str, usize)] = &[("native/src/ink/mod.rs", 879)];

/// Functions over the limit today, by `path::name`, with their ceiling.
const FUNCTION_RATCHET: &[(&str, usize)] = &[
    ("native/src/editor/highlight/inline.rs::highlight_line", 161),
    (
        "native/src/editor/highlight/inline.rs::parse_inline_spans",
        231,
    ),
    (
        "native/src/editor/highlight/reference.rs::highlight_markdown",
        344,
    ),
    ("native/src/editor/highlight.rs::highlight_with_state", 187),
    ("native/src/ink/canvas.rs::draw", 183),
    ("native/src/slides/text.rs::layout_paragraph", 185),
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            // Test-only trees.
            if name != "e2e" && name != "tests" {
                sources(&path, out);
            }
        } else if name.ends_with(".rs")
            && name != "tests.rs"
            && !name.ends_with("_tests.rs")
            && !matches!(name, "properties.rs" | "testing.rs" | "gallery.rs")
        {
            out.push(path);
        }
    }
}

/// The file's lines up to its first `#[cfg(test)]` module.
fn non_test_lines(src: &str) -> Vec<&str> {
    let lines: Vec<&str> = src.lines().collect();
    let end = lines
        .windows(2)
        .position(|pair| {
            pair[0].trim() == "#[cfg(test)]" && pair[1].trim_start().starts_with("mod ")
        })
        .unwrap_or(lines.len());
    lines[..end].to_vec()
}

/// `(name, first_line, length)` of every function: from a line that starts an
/// `fn` item to the closing brace at the same indentation.
fn functions(lines: &[&str]) -> Vec<(String, usize, usize)> {
    let mut found = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let item = trimmed
            .trim_start_matches("pub(crate) ")
            .trim_start_matches("pub(super) ")
            .trim_start_matches("pub ")
            .trim_start_matches("const ")
            .trim_start_matches("async ");
        let Some(rest) = item.strip_prefix("fn ") else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let indent = &line[..line.len() - trimmed.len()];
        let close = format!("{indent}}}");
        if let Some(offset) = lines[i..].iter().position(|l| *l == close) {
            found.push((name, i + 1, offset + 1));
        }
    }
    found
}

#[test]
fn modules_and_functions_stay_small() {
    let root = workspace_root();
    let mut files = Vec::new();
    sources(&root.join("core/src"), &mut files);
    sources(&root.join("native/src"), &mut files);
    files.sort();

    let mut problems = Vec::new();
    let mut seen_modules = Vec::new();
    let mut seen_functions = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        let src = std::fs::read_to_string(&path).expect("read source");
        let lines = non_test_lines(&src);

        let limit = MODULE_RATCHET
            .iter()
            .find(|(p, _)| *p == rel)
            .map(|&(p, ceiling)| {
                seen_modules.push(p);
                ceiling
            })
            .unwrap_or(MODULE_LIMIT);
        if lines.len() > limit {
            problems.push(format!(
                "{rel}: {} non-test lines (limit {limit}); split it",
                lines.len()
            ));
        }

        for (name, line, length) in functions(&lines) {
            let key = format!("{rel}::{name}");
            let limit = FUNCTION_RATCHET
                .iter()
                .find(|(k, _)| *k == key)
                .map(|&(k, ceiling)| {
                    seen_functions.push(k);
                    ceiling
                })
                .unwrap_or(FUNCTION_LIMIT);
            if length > limit {
                problems.push(format!(
                    "{rel}:{line}: fn {name} is {length} lines (limit {limit}); split it"
                ));
            }
        }
    }
    for (path, _) in MODULE_RATCHET {
        if !seen_modules.contains(path) {
            problems.push(format!("ratchet entry {path} no longer exists; remove it"));
        }
    }
    for (key, _) in FUNCTION_RATCHET {
        if !seen_functions.contains(key) {
            problems.push(format!("ratchet entry {key} no longer exists; remove it"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn function_scanner_measures_to_the_matching_brace() {
    let src = "fn a() {\n    if x {\n    }\n}\n\npub(super) fn b(\n    y: u8,\n) {\n}\n";
    let lines: Vec<&str> = src.lines().collect();
    assert_eq!(
        functions(&lines),
        vec![("a".to_string(), 1, 4), ("b".to_string(), 6, 4)]
    );
}
