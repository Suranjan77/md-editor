//! iced keyboard events → kernel [`Chord`] normalization. This is the *only*
//! place toolkit key types appear; everything past here speaks the kernel's
//! layout-independent model. One event produces at most one [`KeyEvent`],
//! which the app routes through `Workspace::handle_key` — the single
//! keystroke entry point (BUG-A discipline).

use md_kernel::input::{Chord, Key, Mods};

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
    let iced::keyboard::Event::KeyPressed {
        key,
        modifiers,
        text,
        ..
    } = event
    else {
        return None;
    };
    from_parts(key, *modifiers, text.as_deref())
}

/// The event's meaningful parts → [`KeyEvent`]; split from [`normalize`] so
/// tests don't have to construct a full toolkit event.
fn from_parts(
    key: &iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
    text: Option<&str>,
) -> Option<KeyEvent> {
    use iced::keyboard::key::Named;

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
                (Some(' '), None) => Some(Key::Space),
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

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::Key as IcedKey;
    use iced::keyboard::Modifiers;
    use iced::keyboard::key::Named;

    fn chord(key: IcedKey, modifiers: Modifiers) -> Option<Chord> {
        from_parts(&key, modifiers, None).and_then(|ev| ev.chord)
    }

    #[test]
    fn ctrl_shift_p_normalizes_to_lowercase_chord() {
        // With shift held the toolkit reports the shifted character; the
        // kernel form is lowercase char + shift mod.
        let got = chord(
            IcedKey::Character("P".into()),
            Modifiers::CTRL | Modifiers::SHIFT,
        );
        assert_eq!(got, Chord::parse("ctrl+shift+p").ok());
    }

    #[test]
    fn plain_character_has_no_mods() {
        let got = chord(IcedKey::Character("z".into()), Modifiers::default());
        assert_eq!(got, Chord::parse("z").ok());
    }

    #[test]
    fn named_keys_map_to_kernel_names() {
        for (named, expect) in [
            (Named::Escape, "escape"),
            (Named::Enter, "enter"),
            (Named::Tab, "tab"),
            (Named::PageDown, "pagedown"),
            (Named::F5, "f5"),
        ] {
            let got = chord(IcedKey::Named(named), Modifiers::default());
            assert_eq!(got, Chord::parse(expect).ok(), "named key {named:?}");
        }
    }

    #[test]
    fn space_arrives_as_character_or_named_identically() {
        let as_char = chord(IcedKey::Character(" ".into()), Modifiers::default());
        let as_named = chord(IcedKey::Named(Named::Space), Modifiers::default());
        assert_eq!(as_char, as_named);
        assert_eq!(as_char, Chord::parse("space").ok());
    }

    #[test]
    fn shifted_text_stays_verbatim_but_the_chord_lowercases() {
        let ev = from_parts(&IcedKey::Character("A".into()), Modifiers::SHIFT, Some("A"));
        let ev = match ev {
            Some(ev) => ev,
            None => panic!("shift+a must normalize"),
        };
        assert_eq!(ev.chord, Chord::parse("shift+a").ok());
        assert_eq!(ev.text.as_deref(), Some("A"), "insertion text is verbatim");
    }

    #[test]
    fn control_text_is_dropped_so_enter_travels_only_as_a_chord() {
        let ev = from_parts(
            &IcedKey::Named(Named::Enter),
            Modifiers::default(),
            Some("\r"),
        );
        let ev = match ev {
            Some(ev) => ev,
            None => panic!("enter must normalize"),
        };
        assert_eq!(ev.chord, Chord::parse("enter").ok());
        assert_eq!(ev.text, None);
    }

    #[test]
    fn ctrl_suppresses_insertion_text() {
        // ctrl+v reports text on some platforms; a command-grade chord must
        // never double as typing.
        let ev = from_parts(&IcedKey::Character("v".into()), Modifiers::CTRL, Some("v"));
        let ev = match ev {
            Some(ev) => ev,
            None => panic!("ctrl+v must normalize"),
        };
        assert_eq!(ev.chord, Chord::parse("ctrl+v").ok());
        assert_eq!(ev.text, None);
    }

    #[test]
    fn meta_is_the_logo_key_and_unknown_keys_are_not_chords() {
        let got = chord(IcedKey::Character("k".into()), Modifiers::LOGO);
        assert_eq!(got, Chord::parse("meta+k").ok());
        assert_eq!(
            chord(IcedKey::Named(Named::CapsLock), Modifiers::default()),
            None
        );
        assert_eq!(
            from_parts(&IcedKey::Unidentified, Modifiers::CTRL, None).and_then(|ev| ev.chord),
            None
        );
    }
}
