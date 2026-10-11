//! Turns winit's key events into [`Input`]s. Pure, so the mapping (and
//! its awkward corners, like AltGr) is tested without a window.

use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::launcher::Input;

/// The inputs one key press produces: usually zero or one, more when an
/// input method commits several characters at once.
pub fn inputs(key: &Key, mods: ModifiersState) -> Vec<Input> {
    let ctrl = mods.control_key();
    let alt = mods.alt_key();
    match key {
        Key::Named(named) => named_input(*named, ctrl).into_iter().collect(),
        Key::Character(text) => {
            // AltGr arrives as Ctrl+Alt on Windows, and is how many
            // layouts type `@`, `{` or `€`: that is text, not a shortcut.
            if ctrl && !alt {
                let mut chars = text.chars();
                return match (chars.next().map(|c| c.to_ascii_lowercase()), chars.next()) {
                    (Some('n'), None) => vec![Input::Down],
                    (Some('p'), None) => vec![Input::Up],
                    (Some('w'), None) => vec![Input::DeleteWord],
                    (Some('u'), None) => vec![Input::Clear],
                    _ => Vec::new(),
                };
            }
            if (alt && !ctrl) || mods.super_key() {
                return Vec::new();
            }
            text.chars()
                .filter(|c| !c.is_control())
                .map(Input::Char)
                .collect()
        }
        _ => Vec::new(),
    }
}

fn named_input(key: NamedKey, ctrl: bool) -> Option<Input> {
    Some(match key {
        NamedKey::Escape => Input::Escape,
        NamedKey::Enter => Input::Submit,
        NamedKey::Backspace if ctrl => Input::DeleteWord,
        NamedKey::Backspace => Input::Backspace,
        NamedKey::ArrowUp => Input::Up,
        NamedKey::ArrowDown => Input::Down,
        NamedKey::Space if !ctrl => Input::Char(' '),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(s: &str, mods: ModifiersState) -> Vec<Input> {
        inputs(&Key::Character(s.into()), mods)
    }

    #[test]
    fn plain_text_is_typed() {
        assert_eq!(ch("a", ModifiersState::empty()), [Input::Char('a')]);
        assert_eq!(ch("A", ModifiersState::SHIFT), [Input::Char('A')]);
        assert_eq!(
            ch("é€", ModifiersState::empty()),
            [Input::Char('é'), Input::Char('€')]
        );
        assert_eq!(
            inputs(&Key::Named(NamedKey::Space), ModifiersState::empty()),
            [Input::Char(' ')]
        );
    }

    #[test]
    fn insert_mode_control_keys() {
        let ctrl = ModifiersState::CONTROL;
        assert_eq!(ch("n", ctrl), [Input::Down]);
        assert_eq!(ch("p", ctrl), [Input::Up]);
        assert_eq!(ch("w", ctrl), [Input::DeleteWord]);
        assert_eq!(ch("u", ctrl), [Input::Clear]);
        assert_eq!(ch("N", ctrl | ModifiersState::SHIFT), [Input::Down]);
        assert!(ch("k", ctrl).is_empty());
        assert_eq!(
            inputs(&Key::Named(NamedKey::Backspace), ctrl),
            [Input::DeleteWord]
        );
    }

    #[test]
    fn altgr_types_rather_than_triggering_shortcuts() {
        let altgr = ModifiersState::CONTROL | ModifiersState::ALT;
        assert_eq!(ch("@", altgr), [Input::Char('@')]);
        assert!(ch("x", ModifiersState::ALT).is_empty());
        assert!(ch("x", ModifiersState::SUPER).is_empty());
    }

    #[test]
    fn named_keys() {
        let none = ModifiersState::empty();
        for (named, input) in [
            (NamedKey::Escape, Input::Escape),
            (NamedKey::Enter, Input::Submit),
            (NamedKey::ArrowUp, Input::Up),
            (NamedKey::ArrowDown, Input::Down),
            (NamedKey::Backspace, Input::Backspace),
        ] {
            assert_eq!(inputs(&Key::Named(named), none), [input]);
        }
        assert!(inputs(&Key::Named(NamedKey::Tab), none).is_empty());
    }
}
