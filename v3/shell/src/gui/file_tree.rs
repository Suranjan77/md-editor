use std::collections::BTreeSet;

use iced::widget::{button, column, container, mouse_area, text};
use iced::{Background, Border, Element, Fill, Padding};
use md3_kernel::CommandId;

use super::{Message, tokens};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub label: String,
    pub rel_path: String,
    pub is_dir: bool,
    pub depth: u16,
}

/// Flatten the visible portion of the tree given the expanded set.
pub fn visible_rows(files: &[String], expanded: &BTreeSet<String>) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    append_level(files, "", 0, expanded, &mut rows);
    rows
}

fn append_level(
    files: &[String],
    prefix: &str,
    depth: u16,
    expanded: &BTreeSet<String>,
    out: &mut Vec<TreeRow>,
) {
    let mut immediate_children = Vec::new();
    let mut seen = BTreeSet::new();

    for file in files {
        if !file.starts_with(prefix) || file == prefix {
            continue;
        }

        let relative = if prefix.is_empty() {
            file.as_str()
        } else {
            let strip = &file[prefix.len()..];
            strip.strip_prefix('/').unwrap_or(strip)
        };

        let first_part = match relative.split('/').next() {
            Some(part) if !part.is_empty() => part,
            _ => continue,
        };

        if seen.contains(first_part) {
            continue;
        }
        seen.insert(first_part);

        let is_dir = relative.contains('/');
        let child_path = if prefix.is_empty() {
            first_part.to_string()
        } else {
            format!("{}/{}", prefix, first_part)
        };

        immediate_children.push((first_part.to_string(), child_path, is_dir));
    }

    // Sort immediate children: dirs before files, then case-insensitive alphabetically.
    immediate_children.sort_by(|a, b| {
        if a.2 == b.2 {
            a.0.to_lowercase().cmp(&b.0.to_lowercase())
        } else if a.2 {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });

    for (name, path, is_dir) in immediate_children {
        out.push(TreeRow {
            label: name,
            rel_path: path.clone(),
            is_dir,
            depth,
        });

        if is_dir && expanded.contains(&path) {
            append_level(files, &path, depth + 1, expanded, out);
        }
    }
}

pub fn context_popover(tree_width: f32, is_dir: bool) -> Element<'static, Message> {
    let backdrop = mouse_area(
        container(iced::widget::Space::new())
            .width(Fill)
            .height(Fill),
    )
    .on_press(Message::TreeContextClosed);
    let mut items = column![].spacing(1).padding(5);
    if !is_dir {
        items = items
            .push(
                button(text("Open").size(13))
                    .width(190)
                    .style(button::text)
                    .on_press(Message::TreeContextOpen { split: false }),
            )
            .push(
                button(text("Open in Split").size(13))
                    .width(190)
                    .style(button::text)
                    .on_press(Message::TreeContextOpen { split: true }),
            );
    }
    for (label, command) in [
        ("Rename", CommandId("file.rename")),
        ("Delete", CommandId("file.delete")),
        ("New Note Here", CommandId("file.new-note")),
        ("New Folder Here", CommandId("file.new-folder")),
    ] {
        items = items.push(
            button(text(label).size(13))
                .width(190)
                .style(button::text)
                .on_press(Message::TreeContextCommand(command)),
        );
    }
    let card = container(items).style(|_| container::Style {
        background: Some(Background::Color(tokens::dark().bg_secondary)),
        border: Border {
            color: tokens::dark().border,
            width: 1.0,
            radius: 5.0.into(),
        },
        ..container::Style::default()
    });
    let positioned = container(card).width(Fill).height(Fill).padding(Padding {
        top: 78.0,
        right: 0.0,
        bottom: 0.0,
        left: (tree_width - 8.0).max(0.0),
    });
    iced::widget::stack![backdrop, positioned].into()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_vault_yields_empty_rows() {
        let files = vec![];
        let expanded = BTreeSet::new();
        let rows = visible_rows(&files, &expanded);
        assert!(rows.is_empty());
    }

    #[test]
    fn empty_directory_marker_stays_visible() {
        let files = vec!["empty/".to_string()];
        let rows = visible_rows(&files, &BTreeSet::new());
        assert_eq!(
            rows,
            vec![TreeRow {
                label: "empty".to_string(),
                rel_path: "empty".to_string(),
                is_dir: true,
                depth: 0,
            }]
        );
    }

    #[test]
    fn nesting_expansion_ordering() {
        let files = vec![
            "README.md".to_string(),
            "docs/PLAN.md".to_string(),
            "docs/V3.md".to_string(),
            "src/main.rs".to_string(),
            "docs/img/logo.png".to_string(),
        ];

        // 1. Collapsed docs and src
        let mut expanded = BTreeSet::new();
        let rows = visible_rows(&files, &expanded);
        assert_eq!(
            rows,
            vec![
                TreeRow {
                    label: "docs".to_string(),
                    rel_path: "docs".to_string(),
                    is_dir: true,
                    depth: 0
                },
                TreeRow {
                    label: "src".to_string(),
                    rel_path: "src".to_string(),
                    is_dir: true,
                    depth: 0
                },
                TreeRow {
                    label: "README.md".to_string(),
                    rel_path: "README.md".to_string(),
                    is_dir: false,
                    depth: 0
                },
            ]
        );

        // 2. Expand docs (docs/img is collapsed)
        expanded.insert("docs".to_string());
        let rows = visible_rows(&files, &expanded);
        assert_eq!(
            rows,
            vec![
                TreeRow {
                    label: "docs".to_string(),
                    rel_path: "docs".to_string(),
                    is_dir: true,
                    depth: 0
                },
                TreeRow {
                    label: "img".to_string(),
                    rel_path: "docs/img".to_string(),
                    is_dir: true,
                    depth: 1
                },
                TreeRow {
                    label: "PLAN.md".to_string(),
                    rel_path: "docs/PLAN.md".to_string(),
                    is_dir: false,
                    depth: 1
                },
                TreeRow {
                    label: "V3.md".to_string(),
                    rel_path: "docs/V3.md".to_string(),
                    is_dir: false,
                    depth: 1
                },
                TreeRow {
                    label: "src".to_string(),
                    rel_path: "src".to_string(),
                    is_dir: true,
                    depth: 0
                },
                TreeRow {
                    label: "README.md".to_string(),
                    rel_path: "README.md".to_string(),
                    is_dir: false,
                    depth: 0
                },
            ]
        );

        // 3. Expand docs/img too
        expanded.insert("docs/img".to_string());
        let rows = visible_rows(&files, &expanded);
        assert_eq!(
            rows,
            vec![
                TreeRow {
                    label: "docs".to_string(),
                    rel_path: "docs".to_string(),
                    is_dir: true,
                    depth: 0
                },
                TreeRow {
                    label: "img".to_string(),
                    rel_path: "docs/img".to_string(),
                    is_dir: true,
                    depth: 1
                },
                TreeRow {
                    label: "logo.png".to_string(),
                    rel_path: "docs/img/logo.png".to_string(),
                    is_dir: false,
                    depth: 2
                },
                TreeRow {
                    label: "PLAN.md".to_string(),
                    rel_path: "docs/PLAN.md".to_string(),
                    is_dir: false,
                    depth: 1
                },
                TreeRow {
                    label: "V3.md".to_string(),
                    rel_path: "docs/V3.md".to_string(),
                    is_dir: false,
                    depth: 1
                },
                TreeRow {
                    label: "src".to_string(),
                    rel_path: "src".to_string(),
                    is_dir: true,
                    depth: 0
                },
                TreeRow {
                    label: "README.md".to_string(),
                    rel_path: "README.md".to_string(),
                    is_dir: false,
                    depth: 0
                },
            ]
        );
    }
}
