//! Toolkit-event → kernel-chord normalization. The kernel is toolkit-agnostic
//! (ADR-0100); this module is the single place iced keyboard types are
//! translated into [`md3_kernel::Chord`]s. Characters are lowercased here
//! because the kernel stores layout-independent lowercase keys and carries
//! shift in [`md3_kernel::Mods`].

use iced::keyboard::key::Named;
use iced::keyboard::{Key as IcedKey, Modifiers};
use md3_kernel::{Chord, Key, Mods};

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
