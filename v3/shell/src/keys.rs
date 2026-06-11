//! Toolkit-event → kernel-chord normalization. The kernel is toolkit-agnostic
//! (ADR-0100); this module is the single place iced keyboard types are
//! translated into [`md3_kernel::Chord`]s. Characters are lowercased here
//! because the kernel stores layout-independent lowercase keys and carries
//! shift in [`md3_kernel::Mods`].

use iced::keyboard::key::Named;
use iced::keyboard::{Key as IcedKey, Modifiers};
use md3_kernel::{Chord, Key, Mods};

/// One normalized key press: the chord form (lowercased, for keymap
/// resolution) *and* the text the press produced (case/layout-preserved, for
/// raw insertion into a buffer or overlay). Either may be absent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyPress {
    pub chord: Option<Chord>,
    pub text: Option<String>,
}

impl KeyPress {
    pub fn chord(chord: Chord) -> KeyPress {
        KeyPress {
            chord: Some(chord),
            text: None,
        }
    }
}

/// Normalize a toolkit key-press event. `produced` is the text the toolkit
/// says the press generates; control characters are dropped (Enter/Backspace
/// travel as chords, never as text).
pub fn press(key: IcedKey, modifiers: Modifiers, produced: Option<&str>) -> KeyPress {
    let text = produced
        .filter(|t| !t.is_empty() && t.chars().all(|c| !c.is_control()))
        .map(str::to_string);
    KeyPress {
        chord: chord(key, modifiers),
        text,
    }
}

/// Normalize one key press. `None` means "no chord" — a bare modifier press
/// or a key the kernel has no name for; such events are never commands.
pub fn chord(key: IcedKey, modifiers: Modifiers) -> Option<Chord> {
    let mods = Mods {
        ctrl: modifiers.control(),
        shift: modifiers.shift(),
        alt: modifiers.alt(),
        meta: modifiers.logo(),
    };
    Some(Chord::new(mods, kernel_key(key)?))
}

fn kernel_key(key: IcedKey) -> Option<Key> {
    match key {
        IcedKey::Character(s) => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(' '), None) => Some(Key::Space),
                (Some(c), None) => Some(Key::Char(c.to_ascii_lowercase())),
                _ => None,
            }
        }
        IcedKey::Named(named) => named_key(named),
        IcedKey::Unidentified => None,
    }
}

fn named_key(named: Named) -> Option<Key> {
    let key = match named {
        Named::Enter => Key::Enter,
        Named::Escape => Key::Escape,
        Named::Tab => Key::Tab,
        Named::Backspace => Key::Backspace,
        Named::Delete => Key::Delete,
        Named::Space => Key::Space,
        Named::ArrowUp => Key::Up,
        Named::ArrowDown => Key::Down,
        Named::ArrowLeft => Key::Left,
        Named::ArrowRight => Key::Right,
        Named::Home => Key::Home,
        Named::End => Key::End,
        Named::PageUp => Key::PageUp,
        Named::PageDown => Key::PageDown,
        Named::F1 => Key::F(1),
        Named::F2 => Key::F(2),
        Named::F3 => Key::F(3),
        Named::F4 => Key::F(4),
        Named::F5 => Key::F(5),
        Named::F6 => Key::F(6),
        Named::F7 => Key::F(7),
        Named::F8 => Key::F(8),
        Named::F9 => Key::F(9),
        Named::F10 => Key::F(10),
        Named::F11 => Key::F(11),
        Named::F12 => Key::F(12),
        _ => return None,
    };
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn press_keeps_case_in_text_but_lowercases_the_chord() {
        let p = press(IcedKey::Character("A".into()), Modifiers::SHIFT, Some("A"));
        assert_eq!(p.chord, Chord::parse("shift+a").ok());
        assert_eq!(p.text.as_deref(), Some("A"), "insertion text is verbatim");
    }

    #[test]
    fn press_drops_control_text_so_enter_travels_only_as_a_chord() {
        let p = press(
            IcedKey::Named(Named::Enter),
            Modifiers::default(),
            Some("\r"),
        );
        assert_eq!(p.chord, Chord::parse("enter").ok());
        assert_eq!(p.text, None);
    }

    #[test]
    fn meta_is_the_logo_key_and_unknown_keys_are_not_chords() {
        let got = chord(IcedKey::Character("k".into()), Modifiers::LOGO);
        assert_eq!(got, Chord::parse("meta+k").ok());
        assert_eq!(
            chord(IcedKey::Named(Named::CapsLock), Modifiers::default()),
            None
        );
        assert_eq!(chord(IcedKey::Unidentified, Modifiers::CTRL), None);
    }
}
