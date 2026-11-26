//! README media: drives the real app through a demo research vault and renders the screenshots and GIF frames in `images/` (`#[ignore]`d; run by `scripts/readme-media.sh`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced_program::Program;

use super::flows::wait_for;
use super::{Session, Vault, boot};
use crate::app::MdEditor;
use crate::design::ThemeMode;
use crate::editor::buffer::{EditorCommand, Movement};
use crate::messages::{
    EditorMessage, GraphMessage, Message, PdfMessage, SettingsMessage, ShellMessage, Shortcut,
    VaultMessage,
};

/// Device pixels per logical pixel for the README stills (1200×800 → 1920×1280).
const STILL_SCALE: f32 = 1.6;

const PAPER: &str = "papers/Windowed Sparse Attention.pdf";
const NOTE: &str = "notes/Sparse attention.md";

const SPARSE_NOTE: &str = r"# Sparse attention

Notes on [[papers/Windowed Sparse Attention.pdf|the windowed attention paper]] for the [[Long context]] chapter.

> [!claim] A local window plus a few global tokens gets within 0.2 points of dense attention at a quarter of the memory.

The cost drops from quadratic to linear in $n$ for a fixed window $w$:

$$
C(n) = O(n\,w\,d)
$$

![The attention pattern](<pdf://papers/Windowed Sparse Attention.pdf?page=2&crop=235,62,150,140>)

| Model | Window | Accuracy | Memory |
|---|---|---|---|
| Dense | – | 84.1 | 38.2 GB |
| Windowed + global | 256 | 83.9 | 9.8 GB |

> [!question] Does the window hurt multi-hop retrieval?

- [x] Read sections 1–3
- [ ] Re-derive the cost in [[Memory budget]]
- [ ] ~~Benchmark the dense baseline~~ numbers are in the paper

supports:: [[Long context]]
";

/// A small research vault: linked notes, a paper, a bibliography, a daily note.
fn demo_vault() -> (Vault, PathBuf) {
    // The vault folder's name shows in the status bar, so give it a real one.
    let parent = std::env::temp_dir().join(format!(
        "md-editor-showcase-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let vault = Vault {
        root: parent.join("Research"),
    };
    std::fs::create_dir_all(&vault.root).expect("create vault");
    let vault = vault
        .file(
            "Reading list.md",
            "# Reading list\n\nThis term: efficient attention for long documents.\n\n- [x] [[Sparse attention]]\n- [ ] [[Kernels]]\n- [ ] [[Memory budget]]\n\nOpen threads live in [[Open questions]].\n",
        )
        .file(NOTE, SPARSE_NOTE)
        .file(
            "notes/Long context.md",
            "# Long context\n\nWhy documents past 8k tokens need something other than dense attention.\n\n- [[Sparse attention]]: a local window plus global tokens\n- [[Kernels]]: fused attention keeps the dense layer cheaper\n\ncontradicts:: [[Memory budget]]\n",
        )
        .file(
            "notes/Memory budget.md",
            "# Memory budget\n\nPeak memory at $n = 8192$ for a 12-layer model.\n\nderives:: [[Sparse attention]]\n",
        )
        .file(
            "notes/Kernels.md",
            "# Kernels\n\nA static mask composes with fused attention kernels.\n\nSee [[Long context]].\n",
        )
        .file(
            "notes/Open questions.md",
            "# Open questions\n\n> [!question] Can one dense layer every few blocks recover multi-hop accuracy?\n\nFrom [[Sparse attention]] and [[Long context]].\n",
        )
        .file(
            "daily/2026-11-25.md",
            "# 2026-11-25\n\n## Tasks\n- [x] Read the windowed attention paper\n- [ ] Re-derive the cost in [[Memory budget]]\n- [ ] Draft the long-context chapter outline\n",
        )
        .file(
            "refs.bib",
            "@article{reader2026windowed,\n  title = {Windowed Sparse Attention for Long Documents},\n  author = {Reader, A. and Writer, B.},\n  year = {2026}\n}\n",
        )
        .fixture(PAPER, "showcase/sparse-attention.pdf");
    (vault, parent)
}

fn shortcut(s: Shortcut) -> Message {
    Message::KeyboardShortcut(s)
}

fn edit(command: EditorCommand) -> Message {
    Message::Editor(EditorMessage::Command(command))
}

fn theme(mode: ThemeMode) -> Message {
    Message::Settings(SettingsMessage::ThemeModeSelected(mode))
}

fn images_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/readme-media")
}

/// GIF frames, numbered in order, each with how long it stays on screen.
struct Frames {
    dir: PathBuf,
    list: Vec<(PathBuf, Duration)>,
}

impl Frames {
    fn new(dir: PathBuf) -> Self {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create frames dir");
        Self {
            dir,
            list: Vec::new(),
        }
    }

    fn grab<P>(&mut self, app: &mut Session<P>, hold: Duration)
    where
        P: Program<State = MdEditor, Message = Message> + 'static,
    {
        let path = self.dir.join(format!("{:03}.png", self.list.len()));
        app.screenshot_to(&path, 1.0);
        self.list.push((path, hold));
    }

    /// An ffmpeg concat list (`ffmpeg -f concat -i frames.txt`).
    fn write_list(&self) {
        let mut out = String::from("ffconcat version 1.0\n");
        for (path, hold) in &self.list {
            out.push_str(&format!(
                "file '{}'\nduration {:.3}\n",
                path.display(),
                hold.as_secs_f32()
            ));
        }
        // The concat demuxer ignores the last duration unless the file repeats.
        if let Some((last, _)) = self.list.last() {
            out.push_str(&format!("file '{}'\n", last.display()));
        }
        std::fs::write(self.dir.join("frames.txt"), out).expect("write frame list");
    }
}

fn still<P>(app: &mut Session<P>, name: &str)
where
    P: Program<State = MdEditor, Message = Message> + 'static,
{
    app.screenshot_to(&images_dir().join(format!("{name}.png")), STILL_SCALE);
}

fn wait_for_paper<P>(app: &mut Session<P>)
where
    P: Program<State = MdEditor, Message = Message> + 'static,
{
    wait_for(app, "the paper's pages and text", |p| {
        p.pdf_rendered >= 2 && p.pdf_texts_loaded > 0 && p.pdf_document_ready
    });
}

#[test]
#[ignore = "renders the README media; run scripts/readme-media.sh"]
fn readme_media() {
    let (vault, parent) = demo_vault();
    std::fs::create_dir_all(images_dir()).expect("create media dir");
    let mut gif = Frames::new(images_dir().join("gif-frames"));
    let quick = Duration::from_millis(90);
    let hold = Duration::from_millis(1800);

    // 1. Writing: the note with its live preview, then a wikilink typed in a
    // short note, so the completion popup is on screen.
    let mut app = boot("showcase", &vault, Some(NOTE));
    let window = app.probe().main_window.expect("the shell window is open");
    app.send(Message::Shell(ShellMessage::WindowRescaled(
        window,
        STILL_SCALE,
    )));
    app.wait(Duration::from_millis(500));
    app.settle();
    gif.grab(&mut app, hold);
    still(&mut app, "editor");

    app.send(Message::Vault(VaultMessage::SidebarFileClicked(
        "notes/Open questions.md".to_string(),
    )));
    app.send(edit(EditorCommand::MoveCursor {
        movement: Movement::DocEnd,
        extend: false,
    }));
    gif.grab(&mut app, Duration::from_millis(900));
    for chunk in ["\n", "Related: ", "[[", "Ke", "r"] {
        app.send(edit(EditorCommand::InsertText(chunk.to_string())));
        // The popup sits at the caret, which a frame reports.
        app.redraw();
        gif.grab(&mut app, quick * 3);
    }
    gif.grab(&mut app, Duration::from_millis(1200));
    assert!(!app.probe().completion.is_empty(), "no completion popup");
    still(&mut app, "completion");
    app.send(Message::Editor(EditorMessage::CompletionAccept));
    gif.grab(&mut app, Duration::from_millis(1000));
    app.send(Message::Vault(VaultMessage::SidebarFileClicked(
        NOTE.to_string(),
    )));

    // 2. Reading: the paper beside the note, a highlight on it.
    app.send(Message::Vault(VaultMessage::OpenToSide(PAPER.to_string())));
    wait_for_paper(&mut app);
    app.wait(Duration::from_millis(300));
    app.settle();
    gif.grab(&mut app, hold);
    // The abstract's first sentence.
    app.send(Message::Pdf(PdfMessage::SelectionFinished(0, 160, 240)));
    gif.grab(&mut app, Duration::from_millis(700));
    app.send(Message::Pdf(PdfMessage::QuickHighlight));
    app.settle();
    gif.grab(&mut app, hold);
    still(&mut app, "split-dark");

    app.send(theme(ThemeMode::Light));
    app.settle();
    gif.grab(&mut app, hold);
    still(&mut app, "split-light");
    app.send(theme(ThemeMode::Dark));
    app.send(shortcut(Shortcut::Unsplit));
    app.settle();

    // 3. Connecting: the knowledge graph with typed links.
    app.send(Message::Graph(GraphMessage::Toggle));
    app.wait(Duration::from_millis(800));
    app.settle();
    let probe = app.probe();
    assert!(probe.graph_visible && probe.graph_nodes >= 6, "{probe:?}");
    gif.grab(&mut app, hold);
    still(&mut app, "graph");
    app.send(Message::Graph(GraphMessage::Toggle));
    app.send(shortcut(Shortcut::Escape));

    // 4. Finding: the command palette over files and commands.
    app.send(Message::Shell(ShellMessage::CommandPaletteOpen));
    app.send(Message::Shell(ShellMessage::CommandPaletteQueryChanged(
        "split".to_string(),
    )));
    gif.grab(&mut app, Duration::from_millis(1400));
    still(&mut app, "palette");
    app.send(shortcut(Shortcut::Escape));

    // 5. Keeping going: today's Daily Desk note, then the study tracker.
    app.send(shortcut(Shortcut::DailyNote));
    app.settle();
    gif.grab(&mut app, hold);
    still(&mut app, "daily");

    app.send(Message::Shell(ShellMessage::TrackerToggle));
    for (date, hours, notes) in [
        ("2026-11-19", "1.5", "Dense attention baseline"),
        ("2026-11-21", "2", "Long context survey"),
        ("2026-11-23", "1.25", "Kernels"),
        ("2026-11-24", "2.5", "Windowed attention paper"),
        ("2026-11-25", "1.75", "Memory budget"),
    ] {
        for message in [
            ShellMessage::TrackerManualDateChanged(date.to_string()),
            ShellMessage::TrackerManualHoursChanged(hours.to_string()),
            ShellMessage::TrackerManualNotesChanged(notes.to_string()),
            ShellMessage::TrackerManualAdd,
        ] {
            app.send_quick(Message::Shell(message));
        }
    }
    app.settle();
    assert_eq!(app.probe().tracker_sessions, 5);
    gif.grab(&mut app, hold);
    still(&mut app, "tracker");
    app.finish();

    gif.write_list();
    drop(vault);
    let _ = std::fs::remove_dir_all(parent);
}
