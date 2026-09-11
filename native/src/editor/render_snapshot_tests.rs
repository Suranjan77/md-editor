//! Characterization harness for the editor renderer.
//!
//! Drives the `Editor` widget only through its public `Widget` surface (layout,
//! draw, update, mouse_interaction) against a feature-rich document, records
//! every draw call, published message and interaction, and compares the
//! transcript to a golden file. Used to prove a refactor is behavior-neutral.
//!
//! Run with `RENDER_SNAPSHOT=<path> cargo test -p md-editor-native render_snapshot -- --ignored`.
//! Set `RENDER_SNAPSHOT_WRITE=1` to (re)write the golden file.

use std::fmt::Write as _;
use std::ops::{Deref, DerefMut};

use iced::advanced::graphics::core::event::Event;
use iced::advanced::image;
use iced::advanced::widget::Widget;
use iced::keyboard::{self, Key, key};
use iced::mouse;
use iced::{Point, Rectangle, Size};

use std::collections::HashMap;

use crate::editor::buffer::EditorCommand;
use crate::editor::highlight::highlight_markdown;
use crate::editor::renderer::testing::{Recorder, View};
use crate::editor::renderer::{Editor, ImageCache, MathCache, MathRender, line_visual_y};

type Real = iced::Renderer;

/// Round every float literal in a debug string so harmless reassociation of
/// float arithmetic during a refactor doesn't register as a difference.
fn round_floats(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let starts_num = c.is_ascii_digit()
            || (c == b'-' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit());
        let prev_is_ident = i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if starts_num && !prev_is_ident {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_digit()
                    || bytes[i] == b'.'
                    || bytes[i] == b'e'
                    || (bytes[i] == b'-' && bytes[i - 1] == b'e'))
            {
                i += 1;
            }
            let tok = &s[start..i];
            if tok.contains('.') || tok.contains('e') {
                match tok.parse::<f64>() {
                    Ok(v) => {
                        let r = (v * 1000.0).round() / 1000.0;
                        let _ = write!(out, "{:.3}", if r == 0.0 { 0.0 } else { r });
                    }
                    Err(_) => out.push_str(tok),
                }
            } else {
                out.push_str(tok);
            }
        } else {
            let ch = s[i..].chars().next().expect("char boundary");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

const DOC: &str = r#"# Heading One with **bold** inside
## Second level heading
Plain paragraph line.
alpha **bold text** and *italic words* and `inline code` then [a link](http://example.com) and [[Wiki Note|alias]] end
A very long paragraph that keeps going and going so that it must wrap across several visual rows in narrower layouts, with some **strong emphasis in the middle of the wrap** and trailing words to finish it off nicely.
https://example.com/averyveryveryveryveryveryveryveryveryveryveryveryveryveryveryverylongtokenwithoutanyspacesatall
- [ ] unchecked task item
- [x] checked task item with *style*
1. numbered item
- bullet item
> quoted line one with **bold**
> quoted line two

```rust
fn main() {
    let a_really_long_identifier_name_to_force_horizontal_scrolling = "some string value that is long enough";
}
```
```
no language block
```
| Name | Value | Description | Extra column one | Extra column two | Extra column three |
|---|---|---|---|---|---|
| alpha | 1 | first row with **bold** | x | y | z |
| beta | 2 | second row | x | y | z |
| gamma | 3 | third | x | y | z |
after table
$$
E = mc^2
$$
$$
\uncached_long_equation_source_line_that_is_quite_wide + \alpha^2 + \beta^2 + \gamma^2 = \delta
$$
$$x^2$$
Inline $a^2$ math and $\uncached$ inline and more text
![Figure alt](img/cached.png)
![Missing alt](img/missing.png)
---

Trailing paragraph after an empty line.
"#;

fn fake_handle(seed: u8) -> image::Handle {
    image::Handle::from_rgba(2, 2, vec![seed; 16])
}

fn caches() -> (ImageCache, MathCache) {
    let mut images = HashMap::new();
    images.insert(
        "img/cached.png".to_string(),
        (fake_handle(1), 1200.0, 500.0),
    );
    let mut math = HashMap::new();
    for (i, (tex, w, h)) in [
        ("E = mc^2", 140.0, 30.0),
        ("x^2", 30.0, 24.0),
        ("a^2", 28.0, 40.0),
    ]
    .into_iter()
    .enumerate()
    {
        math.insert(
            tex.to_string(),
            MathRender {
                inline_handle: fake_handle(10 + i as u8),
                block_handle: fake_handle(20 + i as u8),
                width: w,
                height: h,
            },
        );
    }
    (images, math)
}

/// A [`View`] that writes everything it does to a transcript.
struct Harness<'a> {
    out: &'a mut String,
    view: View,
}

impl Deref for Harness<'_> {
    type Target = View;
    fn deref(&self) -> &View {
        &self.view
    }
}

impl DerefMut for Harness<'_> {
    fn deref_mut(&mut self) -> &mut View {
        &mut self.view
    }
}

impl Harness<'_> {
    fn frame(&mut self, label: &str, viewport: Option<Rectangle>) {
        let bounds = self.view.layout().bounds();
        let viewport_used = viewport.unwrap_or(bounds);
        let mut calls: Vec<String> = self
            .view
            .draw_in(viewport)
            .iter()
            .map(|call| call.describe().to_string())
            .collect();
        // Block chrome is emitted in HashMap order; compare as a multiset.
        calls.sort();
        let _ = writeln!(
            self.out,
            "== frame {label} w={} cursor=({},{}) sel={:?} bounds={:?} vp={:?}",
            self.view.width,
            self.view.buffer.cursor_line,
            self.view.buffer.cursor_col,
            self.view.buffer.selection,
            bounds,
            viewport_used
        );
        for call in calls {
            let _ = writeln!(self.out, "  {}", round_floats(&call));
        }
    }

    fn log_event(
        &mut self,
        label: &str,
        cursor: Option<Point>,
        outcome: crate::editor::renderer::testing::EventOutcome,
    ) {
        let _ = writeln!(
            self.out,
            "{}",
            round_floats(&format!(
                "-- event {label} at={cursor:?} captured={} msgs={:?} clip={:?}",
                outcome.captured, outcome.messages, outcome.clipboard_writes
            ))
        );
    }

    fn event(&mut self, label: &str, event: Event, cursor: Option<Point>) {
        let outcome = self.view.event(event, cursor);
        self.log_event(label, cursor, outcome);
    }

    fn event_on(
        &mut self,
        label: &str,
        event: Event,
        cursor: Option<Point>,
        node: iced::advanced::layout::Node,
    ) {
        let outcome = self.view.event_on(event, cursor, node);
        self.log_event(label, cursor, outcome);
    }

    fn click(&mut self, label: &str, p: Point) {
        self.event(
            &format!("{label} press"),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Some(p),
        );
        self.event(
            &format!("{label} release"),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Some(p),
        );
    }

    fn key(&mut self, label: &str, k: Key, modifiers: keyboard::Modifiers, text: Option<&str>) {
        let outcome = self.view.key(k, modifiers, text);
        self.log_event(label, None, outcome);
    }

    fn modifiers(&mut self, m: keyboard::Modifiers) {
        let outcome = self.view.modifiers(m);
        self.log_event("modifiers", None, outcome);
    }
}

fn run_scenarios(out: &mut String) {
    let (images, math) = caches();
    let lines = highlight_markdown(DOC);
    let line_count = lines.len();

    // Fine width sweep of line offsets, so small wrapping changes surface.
    for width in (180..1400).step_by(3) {
        let width = width as f32;
        let mut row = String::new();
        for &(focused, al, ac) in &[(false, 0, 0), (true, 31, 8)] {
            for target in 0..=line_count {
                let y =
                    line_visual_y::<Real>(&lines, &images, &math, width, al, ac, target, focused);
                let _ = write!(row, "{y:.2},");
            }
        }
        let _ = writeln!(out, "sweep w={width} {row}");
    }

    for &width in &[300.0_f32, 760.0, 1300.0] {
        let mut view = View::new(DOC, width);
        view.images = images.clone();
        view.math = math.clone();
        let mut h = Harness { out, view };

        let size = <Editor<'_, String> as Widget<String, iced::Theme, Recorder>>::size(&h.editor());
        let _ = writeln!(h.out, "#### width {width} size={size:?}");

        // Vertical movement before any layout has run (rebuilds the tree).
        let raw_starts: Vec<(usize, usize)> = (0..line_count)
            .flat_map(|l| [(l, 0), (l, 7)])
            .chain([(3, 40), (4, 120)])
            .collect();
        for &(line, col) in &raw_starts {
            h.reset_tree();
            h.set_cursor(line.min(line_count - 1), col);
            let node = iced::advanced::layout::Node::new(Size::new(width, 4000.0));
            let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
            h.event_on(
                "raw focus",
                press,
                Some(Point::new(90.0, 30.0)),
                node.clone(),
            );
            for (name, k) in [("down", key::Named::ArrowDown), ("up", key::Named::ArrowUp)] {
                let ev = Event::Keyboard(keyboard::Event::KeyPressed {
                    key: Key::Named(k),
                    modified_key: Key::Named(k),
                    physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
                    location: keyboard::Location::Standard,
                    modifiers: keyboard::Modifiers::empty(),
                    text: None,
                    repeat: false,
                });
                h.event_on(
                    &format!("raw {name} from {line}:{col}"),
                    ev,
                    None,
                    node.clone(),
                );
            }
        }
        h.reset_tree();
        h.set_cursor(0, 0);
        for target in 0..=line_count {
            for &(focused, al, ac) in &[(false, 0, 0), (true, 4, 10), (true, 22, 3), (true, 30, 0)]
            {
                let y =
                    line_visual_y::<Real>(&lines, &images, &math, width, al, ac, target, focused);
                let _ = writeln!(
                    h.out,
                    "visual_y t={target} f={focused} a=({al},{ac}) {y:.2}"
                );
            }
        }

        // Unfocused, full and partial viewports.
        h.frame("unfocused", None);
        let total_h = h.layout().bounds().height;
        for k in 0..6 {
            let vp = Rectangle {
                x: 0.0,
                y: total_h * k as f32 / 6.0,
                width,
                height: 300.0,
            };
            h.frame(&format!("unfocused vp{k}"), Some(vp));
        }

        // Pointer grid: interactions and clicks (unfocused and focused).
        let step_y = 19.0;
        let mut ys = Vec::new();
        let mut y = 1.0;
        while y < total_h {
            ys.push(y);
            y += step_y;
        }
        let xs: Vec<f32> = (0..((width / 29.0) as usize))
            .map(|i| 3.0 + i as f32 * 29.0)
            .collect();
        for &ctrl in &[false, true] {
            h.modifiers(if ctrl {
                keyboard::Modifiers::CTRL
            } else {
                keyboard::Modifiers::empty()
            });
            let mut row = String::new();
            for &py in &ys {
                row.clear();
                for &px in &xs {
                    let _ = write!(row, "{:?},", h.interaction(Point::new(px, py)));
                }
                let _ = writeln!(h.out, "interaction ctrl={ctrl} y={py} {row}");
            }
        }
        h.modifiers(keyboard::Modifiers::empty());
        for px in [70.0, 150.0] {
            let mut runs = String::new();
            let mut last = String::new();
            let mut y = 0.0;
            while y < total_h {
                let it = format!("{:?}", h.interaction(Point::new(px, y)));
                if it != last {
                    let _ = write!(runs, "{y}:{it} ");
                    last = it;
                }
                y += 0.5;
            }
            let _ = writeln!(h.out, "interaction column x={px} {runs}");
        }
        for &py in &ys {
            for &px in xs.iter().step_by(2) {
                h.click(&format!("grid({px},{py})"), Point::new(px, py));
            }
        }
        h.modifiers(keyboard::Modifiers::CTRL);
        for &py in &ys {
            for &px in xs.iter().step_by(5) {
                h.click(&format!("ctrlgrid({px},{py})"), Point::new(px, py));
            }
        }
        h.modifiers(keyboard::Modifiers::empty());

        // Focus the editor, then walk the cursor over every line.
        h.click("focus", Point::new(100.0, 40.0));
        for line in 0..line_count {
            let len = h.buffer.line_text(line).chars().count();
            let mut cols = vec![0, len / 2, len];
            if line == 3 {
                cols.extend([8, 12, 25, 40, 70, 90]);
            }
            cols.dedup();
            for col in cols {
                h.set_cursor(line, col);
                h.frame(&format!("cursor {line}:{col}"), None);
                for (name, k) in [("down", key::Named::ArrowDown), ("up", key::Named::ArrowUp)] {
                    for shift in [false, true] {
                        let m = if shift {
                            keyboard::Modifiers::SHIFT
                        } else {
                            keyboard::Modifiers::empty()
                        };
                        h.key(&format!("{name} shift={shift}"), Key::Named(k), m, None);
                    }
                }
            }
        }

        // Partial viewport while focused inside blocks.
        for &(line, col) in &[(15, 4), (22, 5), (28, 2), (31, 3), (34, 1)] {
            h.set_cursor(line.min(line_count - 1), col);
            for k in 0..4 {
                let vp = Rectangle {
                    x: 0.0,
                    y: total_h * k as f32 / 4.0,
                    width,
                    height: 420.0,
                };
                h.frame(&format!("focused vp{k} at {line}:{col}"), Some(vp));
            }
        }

        // Keyboard commands.
        h.set_cursor(3, 10);
        let named = [
            key::Named::Backspace,
            key::Named::Delete,
            key::Named::Enter,
            key::Named::ArrowLeft,
            key::Named::ArrowRight,
            key::Named::Home,
            key::Named::End,
            key::Named::Tab,
        ];
        for k in named {
            for shift in [false, true] {
                let m = if shift {
                    keyboard::Modifiers::SHIFT
                } else {
                    keyboard::Modifiers::empty()
                };
                h.key(&format!("{k:?} shift={shift}"), Key::Named(k), m, None);
            }
        }
        for c in ["z", "y", "a", "b", "i", "e", "k", "c", "x", "v", "q"] {
            for m in [keyboard::Modifiers::CTRL, keyboard::Modifiers::LOGO] {
                h.key(
                    &format!("mod {c} {m:?}"),
                    Key::Character(c.into()),
                    m,
                    Some(c),
                );
            }
        }
        for t in ["a", "(", "\"", "`", "é", "ab", "\u{7}"] {
            h.key(
                &format!("type {t:?}"),
                Key::Character(t.into()),
                keyboard::Modifiers::empty(),
                Some(t),
            );
        }

        // Drag selection within and across lines, then copy it.
        for &((x0, y0), (x1, y1)) in &[
            ((80.0, 110.0), (240.0, 110.0)),
            ((90.0, 60.0), (200.0, 180.0)),
            ((70.0, 180.0), (150.0, 40.0)),
        ] {
            h.event(
                "drag press",
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Some(Point::new(x0, y0)),
            );
            h.event(
                "drag move",
                Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(x1, y1),
                }),
                Some(Point::new(x1, y1)),
            );
            h.frame("dragging", None);
            h.event(
                "drag release",
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                Some(Point::new(x1, y1)),
            );
            h.key(
                "copy after drag",
                Key::Character("c".into()),
                keyboard::Modifiers::CTRL,
                Some("c"),
            );
        }

        // Buffer-owned selection.
        h.click("refocus", Point::new(100.0, 40.0));
        h.buffer.execute(EditorCommand::SetSelection {
            anchor_line: 2,
            anchor_col: 3,
            focus_line: 5,
            focus_col: 12,
        });
        h.frame("buffer selection", None);
        h.buffer
            .execute(EditorCommand::SetCursor { line: 2, col: 0 });

        // Search highlights.
        h.search = ("line", false, false, Some((10, 9)));
        h.frame("search plain", None);
        h.search = ("[a-z]+ing", true, true, None);
        h.frame("search regex", None);
        h.search = ("", false, false, None);

        // Horizontal scrolling of blocks: wheel (shift + vertical, and native
        // horizontal), then scrollbar drags.
        for &py in &ys {
            for delta in [
                mouse::ScrollDelta::Lines { x: 0.0, y: -0.25 },
                mouse::ScrollDelta::Pixels { x: 7.0, y: 0.0 },
                mouse::ScrollDelta::Lines { x: 0.0, y: 2.0 },
            ] {
                h.modifiers(keyboard::Modifiers::SHIFT);
                h.event(
                    &format!("wheel {delta:?}"),
                    Event::Mouse(mouse::Event::WheelScrolled { delta }),
                    Some(Point::new(200.0, py)),
                );
                // Record the offset before the next event can saturate it.
                h.frame(
                    &format!("wheel at {py}"),
                    Some(Rectangle {
                        x: 0.0,
                        y: py - 30.0,
                        width,
                        height: 60.0,
                    }),
                );
            }
        }
        h.modifiers(keyboard::Modifiers::empty());
        h.frame("after wheel", None);
        for &py in &ys {
            h.event(
                "sb press",
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Some(Point::new(150.0, py)),
            );
            h.event(
                "sb move",
                Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(260.0, py),
                }),
                Some(Point::new(260.0, py)),
            );
            h.event(
                "sb release",
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                Some(Point::new(260.0, py)),
            );
        }
        h.frame("after scrollbar drags", None);

        // Click outside the content column blurs.
        h.click("outside", Point::new(width + 50.0, 20.0));
        h.frame("blurred", None);
    }
}

#[test]
#[ignore = "characterization harness; run explicitly with RENDER_SNAPSHOT set"]
fn render_snapshot() {
    let Ok(path) = std::env::var("RENDER_SNAPSHOT") else {
        panic!("set RENDER_SNAPSHOT to the golden file path");
    };
    let mut out = String::new();
    run_scenarios(&mut out);

    if std::env::var("RENDER_SNAPSHOT_WRITE").is_ok() {
        std::fs::write(&path, &out).expect("write golden");
        return;
    }
    let golden = std::fs::read_to_string(&path).expect("read golden");
    if golden != out {
        let actual = format!("{path}.actual");
        std::fs::write(&actual, &out).expect("write actual");
        let first_diff = golden
            .lines()
            .zip(out.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| golden.lines().count().min(out.lines().count()));
        panic!(
            "render transcript differs from golden (first differing line {}); see {actual}",
            first_diff + 1
        );
    }
}
