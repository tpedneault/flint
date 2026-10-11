//! The global hotkey, through the `global-hotkey` crate: `RegisterHotKey`
//! on Windows (no admin rights needed), an X11 key grab on X11.
//!
//! Wayland gives ordinary applications no global hotkeys at all, so there
//! this reports [`HotkeyError::Unsupported`] and the user binds
//! `flint toggle` in the desktop's shortcut settings (docs/KDE.md). The
//! xdg-desktop-portal GlobalShortcuts portal is a later improvement.
//!
//! On Windows the registration belongs to the thread that made it, and the
//! hotkey arrives as a message on that thread's queue, so a
//! [`SystemHotkey`] must be created and used on the thread running the
//! window's event loop. The daemon does that; nothing here spawns a thread.
//!
//! The [`Hotkey`] trait is what the daemon's logic talks to, so changing
//! the chord on reload, and reporting failure, are tested with a fake.

use flint_config::{Chord, Key};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

/// Why a chord isn't active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    /// This session can't have global hotkeys (Wayland, no display).
    Unsupported(String),
    /// The OS refused the chord, usually because another app owns it.
    Failed(String),
}

impl std::fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HotkeyError::Unsupported(why) | HotkeyError::Failed(why) => f.write_str(why),
        }
    }
}

/// Something that can hold one global hotkey.
pub trait Hotkey {
    /// Makes `chord` the hotkey, replacing the previous one. On failure
    /// no hotkey is active.
    fn set(&mut self, chord: Chord) -> Result<(), HotkeyError>;
}

/// The OS's global hotkey.
pub struct SystemHotkey {
    manager: Result<GlobalHotKeyManager, HotkeyError>,
    current: Option<HotKey>,
}

impl SystemHotkey {
    /// Prepares to register, calling `on_press` each time the hotkey is
    /// pressed. Must be called on the event-loop thread (see the module
    /// comment); `on_press` runs on whatever thread delivers the event.
    pub fn new(on_press: impl Fn() + Send + Sync + 'static) -> Self {
        let manager = if crate::display::is_wayland() {
            Err(HotkeyError::Unsupported(
                "Wayland has no global hotkeys; bind `flint toggle` to a shortcut \
                 in the desktop's settings"
                    .into(),
            ))
        } else {
            GlobalHotKeyManager::new().map_err(|e| {
                HotkeyError::Unsupported(format!("global hotkeys aren't available: {e}"))
            })
        };
        if manager.is_ok() {
            GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
                if event.state() == HotKeyState::Pressed {
                    on_press();
                }
            }));
        }
        SystemHotkey {
            manager,
            current: None,
        }
    }
}

impl Hotkey for SystemHotkey {
    fn set(&mut self, chord: Chord) -> Result<(), HotkeyError> {
        let manager = self.manager.as_ref().map_err(Clone::clone)?;
        if let Some(old) = self.current.take() {
            let _ = manager.unregister(old);
        }
        let hotkey = to_hotkey(chord);
        manager.register(hotkey).map_err(|e| {
            HotkeyError::Failed(match e {
                global_hotkey::Error::AlreadyRegistered(_) => {
                    format!("{chord} is already taken by another application")
                }
                other => format!("{chord} couldn't be registered: {other}"),
            })
        })?;
        self.current = Some(hotkey);
        Ok(())
    }
}

/// The `global-hotkey` form of a chord.
fn to_hotkey(chord: Chord) -> HotKey {
    let mut mods = Modifiers::empty();
    for (on, flag) in [
        (chord.ctrl, Modifiers::CONTROL),
        (chord.alt, Modifiers::ALT),
        (chord.shift, Modifiers::SHIFT),
        (chord.super_, Modifiers::SUPER),
    ] {
        if on {
            mods |= flag;
        }
    }
    HotKey::new(Some(mods).filter(|m| !m.is_empty()), code(chord.key))
}

fn code(key: Key) -> Code {
    const LETTERS: [Code; 26] = [
        Code::KeyA,
        Code::KeyB,
        Code::KeyC,
        Code::KeyD,
        Code::KeyE,
        Code::KeyF,
        Code::KeyG,
        Code::KeyH,
        Code::KeyI,
        Code::KeyJ,
        Code::KeyK,
        Code::KeyL,
        Code::KeyM,
        Code::KeyN,
        Code::KeyO,
        Code::KeyP,
        Code::KeyQ,
        Code::KeyR,
        Code::KeyS,
        Code::KeyT,
        Code::KeyU,
        Code::KeyV,
        Code::KeyW,
        Code::KeyX,
        Code::KeyY,
        Code::KeyZ,
    ];
    const DIGITS: [Code; 10] = [
        Code::Digit0,
        Code::Digit1,
        Code::Digit2,
        Code::Digit3,
        Code::Digit4,
        Code::Digit5,
        Code::Digit6,
        Code::Digit7,
        Code::Digit8,
        Code::Digit9,
    ];
    const F: [Code; 24] = [
        Code::F1,
        Code::F2,
        Code::F3,
        Code::F4,
        Code::F5,
        Code::F6,
        Code::F7,
        Code::F8,
        Code::F9,
        Code::F10,
        Code::F11,
        Code::F12,
        Code::F13,
        Code::F14,
        Code::F15,
        Code::F16,
        Code::F17,
        Code::F18,
        Code::F19,
        Code::F20,
        Code::F21,
        Code::F22,
        Code::F23,
        Code::F24,
    ];
    match key {
        // `Chord` parsing guarantees the ranges.
        Key::Letter(c) => LETTERS[(c as u8 - b'A') as usize],
        Key::Digit(d) => DIGITS[d as usize],
        Key::F(n) => F[(n - 1) as usize],
        Key::Space => Code::Space,
        Key::Tab => Code::Tab,
        Key::Enter => Code::Enter,
        Key::Escape => Code::Escape,
        Key::Backspace => Code::Backspace,
        Key::Insert => Code::Insert,
        Key::Delete => Code::Delete,
        Key::Home => Code::Home,
        Key::End => Code::End,
        Key::PageUp => Code::PageUp,
        Key::PageDown => Code::PageDown,
        Key::Up => Code::ArrowUp,
        Key::Down => Code::ArrowDown,
        Key::Left => Code::ArrowLeft,
        Key::Right => Code::ArrowRight,
        Key::Punct(c) => match c {
            '`' => Code::Backquote,
            '-' => Code::Minus,
            '=' => Code::Equal,
            '[' => Code::BracketLeft,
            ']' => Code::BracketRight,
            '\\' => Code::Backslash,
            ';' => Code::Semicolon,
            '\'' => Code::Quote,
            ',' => Code::Comma,
            '.' => Code::Period,
            _ => Code::Slash,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hotkey(text: &str) -> HotKey {
        to_hotkey(text.parse().unwrap())
    }

    #[test]
    fn chords_map_onto_the_same_keys_global_hotkey_parses() {
        for (ours, theirs) in [
            ("Alt+Space", "alt+space"),
            ("Ctrl+Shift+K", "control+shift+KeyK"),
            ("Super+F12", "super+F12"),
            ("Ctrl+Alt+7", "control+alt+Digit7"),
            ("Alt+;", "alt+Semicolon"),
            ("Ctrl+/", "control+Slash"),
            ("Shift+PageDown", "shift+PageDown"),
        ] {
            let expected: HotKey = theirs.parse().unwrap();
            assert_eq!(hotkey(ours), expected, "{ours}");
        }
    }

    #[test]
    fn every_letter_digit_and_function_key_maps() {
        for c in 'A'..='Z' {
            hotkey(&format!("Alt+{c}"));
        }
        for d in 0..=9 {
            hotkey(&format!("Alt+{d}"));
        }
        for n in 1..=24 {
            hotkey(&format!("F{n}"));
        }
        let punct = "`-=[]\\;',./";
        let codes: std::collections::HashSet<Code> = punct
            .chars()
            .map(|p| hotkey(&format!("Alt+{p}")).key)
            .collect();
        assert_eq!(
            codes.len(),
            punct.len(),
            "each punctuation key is its own code"
        );
    }
}
