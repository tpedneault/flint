//! Turns winit's key events into [`Press`]es. Pure, so the mapping (and
//! its awkward corners, like AltGr) is tested without a window. What a
//! press means depends on the mode, which is the launcher's business.

use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::launcher::Press;

/// The presses one key event produces: usually zero or one, more when an
/// input method commits several characters at once.
pub fn presses(key: &Key, mods: ModifiersState) -> Vec<Press> {
    let ctrl = mods.control_key();
    let alt = mods.alt_key();
    match key {
        Key::Named(named) => named_press(*named, ctrl).into_iter().collect(),
        Key::Character(text) => {
            // AltGr arrives as Ctrl+Alt on Windows, and is how many
            // layouts type `@`, `{` or `€`: that is text, not a shortcut.
            if ctrl && !alt {
                let mut chars = text.chars();
                return match (chars.next(), chars.next()) {
                    (Some(c), None) if c.is_ascii_alphabetic() => {
                        vec![Press::Ctrl(c.to_ascii_lowercase())]
                    }
                    _ => Vec::new(),
                };
            }
            if (alt && !ctrl) || mods.super_key() {
                return Vec::new();
            }
            text.chars()
                .filter(|c| !c.is_control())
                .map(Press::Char)
                .collect()
        }
        _ => Vec::new(),
    }
}

fn named_press(key: NamedKey, ctrl: bool) -> Option<Press> {
    Some(match key {
        NamedKey::Escape => Press::Escape,
        NamedKey::Enter => Press::Enter,
        NamedKey::Backspace if ctrl => Press::CtrlBackspace,
        NamedKey::Backspace => Press::Backspace,
        NamedKey::ArrowUp => Press::Up,
        NamedKey::ArrowDown => Press::Down,
        NamedKey::Tab => Press::Tab,
        NamedKey::Space if !ctrl => Press::Char(' '),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(s: &str, mods: ModifiersState) -> Vec<Press> {
        presses(&Key::Character(s.into()), mods)
    }

    #[test]
    fn plain_text_is_typed() {
        assert_eq!(ch("a", ModifiersState::empty()), [Press::Char('a')]);
        assert_eq!(ch("G", ModifiersState::SHIFT), [Press::Char('G')]);
        assert_eq!(
            ch("é€", ModifiersState::empty()),
            [Press::Char('é'), Press::Char('€')]
        );
        assert_eq!(
            presses(&Key::Named(NamedKey::Space), ModifiersState::empty()),
            [Press::Char(' ')]
        );
    }

    #[test]
    fn control_letters_are_lowercased() {
        let ctrl = ModifiersState::CONTROL;
        assert_eq!(ch("n", ctrl), [Press::Ctrl('n')]);
        assert_eq!(ch("N", ctrl | ModifiersState::SHIFT), [Press::Ctrl('n')]);
        assert!(ch("1", ctrl).is_empty());
        assert_eq!(
            presses(&Key::Named(NamedKey::Backspace), ctrl),
            [Press::CtrlBackspace]
        );
    }

    #[test]
    fn altgr_types_rather_than_triggering_shortcuts() {
        let altgr = ModifiersState::CONTROL | ModifiersState::ALT;
        assert_eq!(ch("@", altgr), [Press::Char('@')]);
        assert!(ch("x", ModifiersState::ALT).is_empty());
        assert!(ch("x", ModifiersState::SUPER).is_empty());
    }

    #[test]
    fn named_keys() {
        let none = ModifiersState::empty();
        for (named, press) in [
            (NamedKey::Escape, Press::Escape),
            (NamedKey::Enter, Press::Enter),
            (NamedKey::ArrowUp, Press::Up),
            (NamedKey::ArrowDown, Press::Down),
            (NamedKey::Backspace, Press::Backspace),
            (NamedKey::Tab, Press::Tab),
        ] {
            assert_eq!(presses(&Key::Named(named), none), [press]);
        }
        assert!(presses(&Key::Named(NamedKey::F1), none).is_empty());
    }
}
