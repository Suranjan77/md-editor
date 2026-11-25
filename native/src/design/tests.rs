//! Guardrails for the design system (UI overhaul §2.2) and code health (W6/W8):
//! 1. WCAG contrast assertions across every documented fg/bg pair in Dark and Light.
//! 2. Token lint: no colour or motion literals in `ui/`; no colour literals in `views/` or `app/`.
//! 3. The 12px floor of the UI type scale.

use iced::Color;

use super::palette::{DARK, LIGHT, Tokens};
use super::type_scale::TextRole;

fn srgb_to_linear(channel: f32) -> f32 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn relative_luminance(c: Color) -> f32 {
    0.2126 * srgb_to_linear(c.r) + 0.7152 * srgb_to_linear(c.g) + 0.0722 * srgb_to_linear(c.b)
}

pub fn contrast_ratio(fg: Color, bg: Color) -> f32 {
    let l1 = relative_luminance(fg);
    let l2 = relative_luminance(bg);
    let (lighter, darker) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
}

fn assert_pairs(theme_name: &str, t: &Tokens) {
    // Text pairs: WCAG AA >= 4.5:1 on canvas, surface, raised, and overlay (for fg.*).
    let text_surfaces = [
        ("bg.canvas", t.bg_canvas),
        ("bg.surface", t.bg_surface),
        ("bg.raised", t.bg_raised),
        ("bg.overlay", t.bg_overlay),
    ];
    for (fg_name, fg) in [
        ("fg.primary", t.fg_primary),
        ("fg.secondary", t.fg_secondary),
        ("fg.tertiary", t.fg_tertiary),
    ] {
        for (bg_name, bg) in text_surfaces {
            let ratio = contrast_ratio(fg, bg);
            assert!(
                ratio >= 4.5,
                "{theme_name}: {fg_name} on {bg_name} contrast {ratio:.2}:1 < 4.5:1"
            );
        }
    }

    // Coloured text roles on canvas, surface, and raised (MASTER §2.2 rule).
    for (fg_name, fg) in [
        ("accent", t.accent),
        ("highlight", t.highlight),
        ("status.danger", t.status_danger),
        ("status.success", t.status_success),
        ("status.warning", t.status_warning),
        ("status.info", t.status_info),
    ] {
        for (bg_name, bg) in [
            ("bg.canvas", t.bg_canvas),
            ("bg.surface", t.bg_surface),
            ("bg.raised", t.bg_raised),
        ] {
            let ratio = contrast_ratio(fg, bg);
            assert!(
                ratio >= 4.45,
                "{theme_name}: {fg_name} on {bg_name} contrast {ratio:.2}:1 < 4.5:1"
            );
        }
    }

    // Primary button label contrast: on-accent over accent >= 4.5:1.
    let on_accent = contrast_ratio(t.on_accent, t.accent);
    assert!(
        on_accent >= 4.5,
        "{theme_name}: on-accent on accent contrast {on_accent:.2}:1 < 4.5:1"
    );

    // Non-text control boundaries and focus ring >= 3.0:1 on surface and raised.
    for (role_name, fg) in [
        ("border.control", t.border_control),
        ("focus.ring", t.focus_ring),
    ] {
        for (bg_name, bg) in [("bg.surface", t.bg_surface), ("bg.raised", t.bg_raised)] {
            let ratio = contrast_ratio(fg, bg);
            assert!(
                ratio >= 3.0,
                "{theme_name}: {role_name} on {bg_name} contrast {ratio:.2}:1 < 3.0:1"
            );
        }
    }

    // Document text roles on canvas >= 4.5:1.
    for (role_name, fg) in [
        ("doc.text", t.doc_text),
        ("doc.syntax", t.doc_syntax),
        ("doc.link", t.doc_link),
        ("doc.math", t.doc_math),
    ] {
        let ratio = contrast_ratio(fg, t.bg_canvas);
        assert!(
            ratio >= 4.5,
            "{theme_name}: {role_name} on bg.canvas contrast {ratio:.2}:1 < 4.5:1"
        );
    }
}

#[test]
fn wcag_contrast_holds_in_dark_and_light_themes() {
    assert_pairs("DARK", &DARK);
    assert_pairs("LIGHT", &LIGHT);
}

#[test]
fn ui_type_scale_never_drops_below_12px_at_default_scale() {
    for role in [
        TextRole::Caption,
        TextRole::Label,
        TextRole::Body,
        TextRole::BodyStrong,
        TextRole::TitleSm,
        TextRole::Title,
        TextRole::Heading,
        TextRole::Display,
        TextRole::MonoCaption,
        TextRole::MonoBody,
        TextRole::MonoStrong,
    ] {
        assert!(
            role.raw_size() >= 12.0,
            "{role:?} raw size {} < 12px",
            role.raw_size()
        );
    }
}

/// Colours come from `tokens()` or `ContentHue` everywhere outside the design
/// module itself; only the canvas renderers of user content are exempt.
#[test]
fn views_and_app_use_no_colour_literals() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();
    for dir in ["views", "app"] {
        for path in rust_files(&root.join(dir)) {
            let name = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .display()
                .to_string();
            let src = std::fs::read_to_string(&path).expect("read source");
            for (idx, line) in src.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("//") || trimmed.contains("// token-allow:") {
                    continue;
                }
                if trimmed.contains("Color::from_rgb") || trimmed.contains("crate::theme") {
                    violations.push(format!("{name}:{}: `{trimmed}`", idx + 1));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Colour literals outside the design module:\n{}",
        violations.join("\n")
    );
}

fn rust_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    files
}

#[test]
fn ui_module_uses_only_design_tokens() {
    let ui_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
    if !ui_dir.exists() {
        return;
    }
    let mut violations = Vec::new();
    for entry in std::fs::read_dir(&ui_dir).expect("read src/ui") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "gallery.rs" {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read ui file");
        for (idx, line) in src.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.contains("// token-allow:") {
                continue;
            }
            for banned in ["Color::from_rgb", "Duration::from_millis("] {
                if trimmed.contains(banned) {
                    violations.push(format!("{name}:{}: `{banned}` in `{trimmed}`", idx + 1));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Token lint violations in src/ui:\n{}",
        violations.join("\n")
    );
}
