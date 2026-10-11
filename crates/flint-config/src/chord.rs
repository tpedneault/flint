//! A global hotkey as written in `settings.toml`: `"Alt+Space"`,
//! `"Ctrl+Shift+K"`, `"F12"`.
//!
//! Its own type rather than `fenix_keymap::KeyPress`, which models keys
//! *inside* an app: it folds Shift into the character and has no function
//! keys, while an OS hotkey needs Shift as a modifier and F-keys are
//! common choices. Parsing lives here, beside the setting, so a bad chord
//! is reported like any other bad value; `flint-platform` maps a parsed
//! chord onto what the OS registers.

use std::fmt;
use std::str::FromStr;

/// The key a chord ends with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// `A`–`Z`, stored uppercase.
    Letter(char),
    /// `0`–`9` on the main row.
    Digit(u8),
    /// `F1`–`F24`.
    F(u8),
    /// `Space`.
    Space,
    /// `Tab`.
    Tab,
    /// `Enter`.
    Enter,
    /// `Escape`.
    Escape,
    /// `Backspace`.
    Backspace,
    /// `Insert`.
    Insert,
    /// `Delete`.
    Delete,
    /// `Home`.
    Home,
    /// `End`.
    End,
    /// `PageUp`.
    PageUp,
    /// `PageDown`.
    PageDown,
    /// `Up`.
    Up,
    /// `Down`.
    Down,
    /// `Left`.
    Left,
    /// `Right`.
    Right,
    /// One of `` ` - = [ ] \ ; ' , . / `` on a US layout.
    Punct(char),
}

/// Punctuation keys a chord may end with, by their US-layout character.
const PUNCT: &str = "`-=[]\\;',./";

/// A modifier set and a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    /// Ctrl.
    pub ctrl: bool,
    /// Alt.
    pub alt: bool,
    /// Shift.
    pub shift: bool,
    /// The Windows / Super / Meta key.
    pub super_: bool,
    /// The key pressed with them.
    pub key: Key,
}

impl Chord {
    /// `Alt+Space`, the default.
    pub const ALT_SPACE: Chord = Chord {
        ctrl: false,
        alt: true,
        shift: false,
        super_: false,
        key: Key::Space,
    };
}

impl FromStr for Chord {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let mut chord = Chord {
            ctrl: false,
            alt: false,
            shift: false,
            super_: false,
            key: Key::Space,
        };
        let mut key = None;
        for part in text.split('+').map(str::trim) {
            if part.is_empty() {
                return Err(format!(
                    "`{text}` has an empty part; write it like `Alt+Space`"
                ));
            }
            let flag = match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => Some(&mut chord.ctrl),
                "alt" => Some(&mut chord.alt),
                "shift" => Some(&mut chord.shift),
                "super" | "win" | "meta" => Some(&mut chord.super_),
                _ => None,
            };
            match flag {
                Some(flag) if !*flag => *flag = true,
                Some(_) => return Err(format!("`{text}` names `{part}` twice")),
                None if key.is_none() => key = Some(parse_key(part)?),
                None => return Err(format!("`{text}` has more than one key besides modifiers")),
            }
        }
        chord.key = key.ok_or_else(|| format!("`{text}` has no key besides modifiers"))?;
        Ok(chord)
    }
}

fn parse_key(part: &str) -> Result<Key, String> {
    let lower = part.to_ascii_lowercase();
    let mut chars = part.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphabetic() {
            return Ok(Key::Letter(c.to_ascii_uppercase()));
        }
        if let Some(d) = c.to_digit(10) {
            return Ok(Key::Digit(d as u8));
        }
        if PUNCT.contains(c) {
            return Ok(Key::Punct(c));
        }
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        if (1..=24).contains(&n) {
            return Ok(Key::F(n));
        }
    }
    Ok(match lower.as_str() {
        "space" => Key::Space,
        "tab" => Key::Tab,
        "enter" | "return" => Key::Enter,
        "escape" | "esc" => Key::Escape,
        "backspace" => Key::Backspace,
        "insert" => Key::Insert,
        "delete" | "del" => Key::Delete,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        _ => return Err(format!("`{part}` isn't a key Flint knows")),
    })
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in [
            (self.ctrl, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
            (self.super_, "Super+"),
        ] {
            if on {
                f.write_str(name)?;
            }
        }
        match self.key {
            Key::Letter(c) | Key::Punct(c) => write!(f, "{c}"),
            Key::Digit(d) => write!(f, "{d}"),
            Key::F(n) => write!(f, "F{n}"),
            other => write!(f, "{other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> Chord {
        text.parse().unwrap_or_else(|e| panic!("{text}: {e}"))
    }

    #[test]
    fn the_default_parses_in_any_case_and_spacing() {
        assert_eq!(chord("Alt+Space"), Chord::ALT_SPACE);
        assert_eq!(chord("alt + space"), Chord::ALT_SPACE);
        assert_eq!(chord("SPACE+ALT"), Chord::ALT_SPACE);
    }

    #[test]
    fn display_is_canonical_and_parses_back() {
        for text in [
            "Ctrl+Shift+K",
            "Super+F12",
            "Ctrl+Alt+Shift+Super+PageDown",
            "Alt+;",
            "Ctrl+1",
            "F1",
        ] {
            let c = chord(text);
            assert_eq!(c.to_string(), text);
            assert_eq!(chord(&c.to_string()), c);
        }
        assert_eq!(chord("control+win+k").to_string(), "Ctrl+Super+K");
    }

    #[test]
    fn mistakes_are_explained() {
        for (text, says) in [
            ("Alt+", "empty part"),
            ("Alt+Alt+K", "twice"),
            ("Alt+K+J", "more than one key"),
            ("Ctrl+Shift", "no key"),
            ("Alt+Spacebar", "`Spacebar` isn't a key"),
            ("F25", "`F25` isn't a key"),
            ("", "empty part"),
        ] {
            let err = text.parse::<Chord>().unwrap_err();
            assert!(err.contains(says), "{text}: {err}");
        }
    }
}
