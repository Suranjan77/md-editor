//! iced keyboard events → kernel [`Chord`] normalization. This is the *only*
//! place toolkit key types appear; everything past here speaks the kernel's
//! layout-independent model. One event produces at most one [`KeyEvent`],
//! which the app routes through `Workspace::handle_key` — the single
//! keystroke entry point (BUG-A discipline).

use md3_kernel::input::{Chord, Key, Mods};

/// A normalized keystroke: the chord for the keymap, plus whatever text the
/// key would insert if no command claims it.
#[derive(Debug, Clone)]
pub struct KeyEvent {
    pub chord: Option<Chord>,
    /// Printable text for the raw-input fallback (`None` for pure-modifier
    /// or non-printing keys). Carries the *shifted* character ("A", "*").
    pub text: Option<String>,
}

pub fn normalize(event: &iced::keyboard::Event) -> Option<KeyEvent> {
    use iced::keyboard::key::Named;
    let iced::keyboard::Event::KeyPressed {
        key,
        modifiers,
        text,
        ..
    } = event
    else {
        return None;
    };

    let mods = Mods {
        ctrl: modifiers.control(),
        shift: modifiers.shift(),
        alt: modifiers.alt(),
        meta: modifiers.logo(),
    };

    let kernel_key = match key {
        iced::keyboard::Key::Character(s) => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Some(Key::Char(c.to_ascii_lowercase())),
                _ => None,
            }
        }
        iced::keyboard::Key::Named(named) => match named {
            Named::Enter => Some(Key::Enter),
            Named::Escape => Some(Key::Escape),
            Named::Tab => Some(Key::Tab),
            Named::Backspace => Some(Key::Backspace),
            Named::Delete => Some(Key::Delete),
            Named::Space => Some(Key::Space),
            Named::ArrowUp => Some(Key::Up),
            Named::ArrowDown => Some(Key::Down),
            Named::ArrowLeft => Some(Key::Left),
            Named::ArrowRight => Some(Key::Right),
            Named::Home => Some(Key::Home),
            Named::End => Some(Key::End),
            Named::PageUp => Some(Key::PageUp),
            Named::PageDown => Some(Key::PageDown),
            Named::F1 => Some(Key::F(1)),
            Named::F2 => Some(Key::F(2)),
            Named::F3 => Some(Key::F(3)),
            Named::F4 => Some(Key::F(4)),
            Named::F5 => Some(Key::F(5)),
            Named::F6 => Some(Key::F(6)),
            Named::F7 => Some(Key::F(7)),
            Named::F8 => Some(Key::F(8)),
            Named::F9 => Some(Key::F(9)),
            Named::F10 => Some(Key::F(10)),
            Named::F11 => Some(Key::F(11)),
            Named::F12 => Some(Key::F(12)),
            _ => None,
        },
        _ => None,
    };

    // Insertable text only when no command-grade modifier is held — ctrl/alt/
    // meta combos are chords, never typing. The space key produces " ".
    let insert_text = if mods.ctrl || mods.alt || mods.meta {
        None
    } else {
        match (key, text) {
            (iced::keyboard::Key::Named(Named::Space), _) => Some(" ".to_string()),
            (_, Some(t)) if t.chars().any(|c| !c.is_control()) => Some(t.to_string()),
            _ => None,
        }
    };

    if kernel_key.is_none() && insert_text.is_none() {
        return None;
    }
    Some(KeyEvent {
        chord: kernel_key.map(|key| Chord::new(mods, key)),
        text: insert_text,
    })
}
